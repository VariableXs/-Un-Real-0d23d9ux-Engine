//! UNX-F11662 argv[0]=未注册名降级主行为判据
//!
//! busybox 是单文件多调用二进制。除了 `busybox <name> …` 这条**直接调用**
//! 路，它还有第二条路：把 `/bin/ls`、`/bin/cat` 做成指向 busybox 的符号
//! 链接（或硬链接、或改名的副本），由内核把 `argv[0]` 填成链接名，busybox
//! 自己从 `argv[0]` 反推要跑哪个 applet。`CONFIG_NO_INDIVIDUAL_APPLET=y`
//! 时这条路的形状又变一次：个体名不再单独成表，而是按组
//! （coreutils / editors / archive / findutils / procps …）编成
//! `busybox <group> <applet> …`，链接名要先进组表、再进成员表。
//!
//! 本卡判的就是**第二条路的降级主行为**：`argv[0]` 是个未注册名（断链、
//! 打错的链接名、被裁掉又留下链接的个体名、指向组的链接却没带成员名……）
//! 时，真机不是"打一行错就退"，而是**降级回主二进制自身的主行为**：
//! 先往 stdout 打完整 usage 横幅（banner），再往 stderr 打
//! `busybox: 'foo': applet not found`（errline），最后追加一行指路
//! （hint：`busybox --list` 或 `busybox coreutils ls`），进程以 127 结束。
//! 三件套**缺一件就不算主行为齐备**，本卡把"齐备位"命名 `ready`——
//! 它不是"跑通了"，而是"降级面完整"。这就是本卡错误码 AV_EDEFAULT 的真身，
//! 也是它和 F11661 最实的分界：F11661 的锚是一条孤零零的 notfound 行，
//! 本卡的锚是一整块输出（banner + errline + hint + 双流去向 + 127）。
//!
//! # 闸门次序
//!
//! `argv0 → base → name → table → exec → banner → stream → ready`，八段单调，
//! 后段不得在前段短路后被"事后补记"。次序不可倒的理由：
//!
//! 1. `argv0` 最先——内核可能给一个空 argv（`execve` 传空表），此时
//!    `argv[0]` 根本不存在，程序名无从谈起，现场判死在不可推档。
//! 2. `base` 次之——链接路径的名字要过 `last_component(argv[0])`；尾斜杠
//!    （`/usr/sbin/`）基名为空，个体可执行体路径也推不出来，这一段的失败
//!    与"名字查不到"是两类事，绝不能合并归档。
//! 3. `name` 第三——`@` 改形（`bb_pop_arg`/retitle 约定：首字符 `@` 时
//!    程序名取 `argv[1]`，`argv[0]` 只当占位）；剥完仍为空就是坏现场。
//!    **本卡不在这一段做形态体检**：真机对 `argv[0]` 不设长度上限、不折
//!    大小写、不认 `..`；超长名只是查不到，落 `table` 档，不落 `name` 档。
//! 4. `table` 第四——组表 + 成员表两跳，答复分五档：
//!    `Registered / Alias / Group / Trimmed / Unregistered`。
//!    `Group` 档是本卡独有：名字是个**组名**（coreutils），链接到它等于
//!    "进了组却没给成员"，真机打组清单退 1，不算降级失败。
//! 5. `exec` 第五——`NO_INDIVIDUAL` 下个体名要 `execv("/proc/self/exe")`
//!    重启进组；`CONFIG_NOMMU` 时改名副本与 shebang 形态重启不了，判死在
//!    这一段（退 126，`can't re-exec`），根本不进降级输出面。
//! 6. `banner` 第六——降级主行为的第一件：usage 横幅。档位四取
//!    `Full / Groups / GroupOnly / Suppressed`，只有 `Full` 算齐备。
//! 7. `stream` 第七——双流去向：`Two`（横幅进 stdout、错行进 stderr）才是
//!    真机；串成一条流（`Merged`）、只剩一条（`OnlyStdout`/`OnlyStderr`）、
//!    全丢（`Suppressed`）都要判死在本段。
//! 8. `ready` 收尾——三件套齐备 + 127 + ENOENT 才置齐备位。
//!
//! # 关键差异（本卡深度所在）
//!
//! - **本卡 vs UNX-F11661**：F11661 判 `busybox <name>` **直接调用**的报错
//!   面——键取自 `argv[1]`、原样进表、不做 basename、不降级；现场只有一条
//!   notfound 行加一个码档，七闸里没有 banner/stream，也没有组表、没有
//!   re-exec。本卡判 `argv[0]` 自身是未注册名时的**降级主行为**——键取自
//!   `argv[0]` 的基名，要过 `@` 改形，要进组表，答复不是"一行错"而是
//!   "一整块输出"。同一条 `busybox: 'foo': applet not found` 在两卡里位置
//!   不同：F11661 里它是唯一现场，本卡里它只是横幅的尾行。
//!   分工的硬例子：`busybox foo`（`argv[0]` 就是 busybox，无需降级）由
//!   F11661 判；`/bin/foo -> busybox`（`argv[0]="foo"`，必须降级）由本卡判。
//!   本卡的 lex 类失败只覆盖"名字不可推"（空 argv / 尾斜杠 / 裸 `@`）；
//!   带 `/`、纯点、超长这类形态畸形在链接路上**不会**停在形态段——
//!   `applet_name(argv[0])` 先把基名剥掉了，剩下的名字即使 200 字节也照样
//!   进表、照样被判成 Unknown。这条不可照抄的理由写在闸三注释里。
//! - **本卡 vs UNX-F11663**：F11663 判"表里本来有这个名字、但被 config
//!   裁掉"这一内部维度，并主张其用户面与未注册名等同。本卡把 `Trimmed`
//!   与 `Unregistered` **分开归档**，但只在 `hint` 维上分开（前者指路到组
//!   调用，后者指路到 `--list`），退出码与 errno 必须相同；一旦被裁件的
//!   码档也与未注册名不同，说明某个构建偷偷把内部维度做进了用户面。
//!   换句话说：F11663 判"等不等"，本卡判"降级块完整不完整"。
//! - **本卡的 `ready` 不是"跑通"**：判据里最容易被误读的一维。`ready` 是
//!   **降级主行为齐备位**（banner + errline + hint + 双流 + 127 五者同真），
//!   真机对未注册名恰恰是"打完整主用法再退 127"，所以 `ready=false` 的
//!   127 现场比 `ready=true` 的更差。状态维走 `estate` 档（与成功档
//!   互斥的 `Outcome::ok()` 才是"跑通"），`Claim::success()` 用
//!   `stage==Ready && code==0`，故齐备位绝不会被当成成功申报。
//!
//! 判序：状态 → 阶段 → errno → 退出码 → 注解 → 齐备位。
//!
//! 语料 14 槽：8 槽申报正确（放行 3、降级 3、半备 1、未定 1），6 槽各挂
//! 一处错误申报（状态/阶段/码/值/注解/齐备位）。账本 14 格 8 成 6 败，
//! 冻结后重放同指纹。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，`audit` 逐格
//! 对账，冻结后 `replay` 重建同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址（F11662）。
pub const SEG_BASE: u32 = 0x6d110;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 语料槽数。
pub const CORPUS: usize = 14;

/// 申报正确的槽数。
pub const ACCEPT: usize = 8;

/// 申报错误的槽数。
pub const REJECT: usize = 6;

/// 档位数（放行 / 降级 / 半备 / 未定）。
pub const TIER_COUNT: usize = 4;

// ---------------------------------------------------------------------------
// busybox 现场常量
// ---------------------------------------------------------------------------

/// 组表大小（`NO_INDIVIDUAL_APPLET` 的家族数）。
pub const GROUP_COUNT: usize = 11;

/// 成员表条数（本构建的个体名 + 别名）。
pub const TABLE_COUNT: usize = 27;

/// 表名上限。真机**不校验**它，本卡只在 `--list-full` 的列宽里用它，
/// 并把超长名照样放行到 table 档——这条不可照抄 F11661 的 lex 闸。
pub const APPLET_NAME_MAX: usize = 30;

/// busybox 自身可执行文件的名字集合大小。
pub const SELF_NAME_COUNT: usize = 4;

/// 放行：applet 正常返回 0。
pub const EXIT_RUN_OK: u16 = 0;

/// 组名自调用（进组没给成员）：打组清单退 1。
pub const EXIT_GROUP_USAGE: u16 = 1;

/// 裸 `busybox`（无参）：打完整 usage 退 1。
pub const EXIT_USAGE: u16 = 1;

/// 降级主行为的退出码——shell 对 "command not found" 的约定档。
pub const EXIT_DEGRADE: u16 = 127;

/// 找到名字但重启不了（改名副本 / shebang 在无 NOMMU 之外）。
pub const EXIT_EXEC: u16 = 126;

/// applet 自身判定失败（`return EXIT_FAILURE`）。
pub const EXIT_APPLET_FAIL: u16 = 1;

// ---------------------------------------------------------------------------
// errno
// ---------------------------------------------------------------------------

/// 名字在表里不存在——降级的正牌 errno。
pub const ERR_ENOENT: i32 = 2;

/// 功能未编译进来 / 重启不了（`-ENOSYS` 家族）。
pub const ERR_ENOSYS: i32 = 38;

/// 名字形态不合法（空名、裸 `@`）。
pub const ERR_EINVAL: i32 = 22;

/// 现场不可推（空 argv，`argv[0]` 根本不存在）。
pub const ERR_EBADARG: i32 = -1;

/// 成功——errno 恒零。
pub const ERR_NONE: i32 = 0;

// ---------------------------------------------------------------------------
// 错误码（13 个）
// ---------------------------------------------------------------------------

/// 阶段申报不符（八闸位）。
pub const AV_ESTAGE: u32 = SEG_BASE + 0x01;
/// errno 申报不符。
pub const AV_ECODE: u32 = SEG_BASE + 0x02;
/// 退出码申报不符。
pub const AV_EVALUE: u32 = SEG_BASE + 0x03;
/// 注解申报不符。
pub const AV_ENOTE: u32 = SEG_BASE + 0x04;
/// 降级主行为齐备位申报不符。
pub const AV_EDEFAULT: u32 = SEG_BASE + 0x05;
/// 链接形态申报不符。
pub const AV_ELINK: u32 = SEG_BASE + 0x06;
/// 状态矛盾（成败互报）。
pub const AV_ESTATE: u32 = SEG_BASE + 0x07;
/// 计数不符。
pub const AV_ECOUNT: u32 = SEG_BASE + 0x08;
/// 槽位非法。
pub const AV_ESLOT: u32 = SEG_BASE + 0x09;
/// 指纹不符。
pub const AV_ESEAL: u32 = SEG_BASE + 0x0a;
/// 无观测。
pub const AV_ENOBS: u32 = SEG_BASE + 0x0b;
/// 矩阵不平。
pub const AV_EMATRIX: u32 = SEG_BASE + 0x0c;
/// 账本不平。
pub const AV_EBAL: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// `argv[0]` 在场性判定。
pub const OP_ARGV0: u8 = 0x01;
/// 基名提取。
pub const OP_BASE: u8 = 0x02;
/// 定名（`@` 改形）。
pub const OP_NAME: u8 = 0x03;
/// 组表 + 成员表查表。
pub const OP_TABLE: u8 = 0x04;
/// 重启可行性。
pub const OP_EXEC: u8 = 0x05;
/// 横幅与双流成形。
pub const OP_OUTPUT: u8 = 0x06;
/// 语料全跑。
pub const OP_CORPUS: u8 = 0x07;
/// 重放。
pub const OP_REPLAY: u8 = 0x08;

// ---------------------------------------------------------------------------
// 哈希
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

/// 折入一个字（大端序逐字节）。
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

/// FNV 终值——把折叠链的当前值当作指纹输出。
///
/// 本卡的 `seal()` 需要"折完即得"的裸值，与 [`name_hash`] 的 ELF 风格
/// 散列分开：一个管现场指纹，一个只管名字。
fn fold_end(h: u64) -> u64 {
    h
}

/// ELF 风格名字散列——busybox 的 applet 表在 `applet_tables` 里带一张
/// `applet_hash[]`，就是用它把 strcmp 从全表降到一个桶。本函数照抄
/// 真算法（移位 + 异或 + 补码），用来做"同桶不同名"体检：桶号相同而
/// 名字不同，说明表里存在散列冲突；桶号相同而名字也相同，才是重复注册。
pub fn name_hash(name: &str) -> u32 {
    let bytes = name.as_bytes();
    let mut h = 0u32;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h = (h << 4) + (*b as u32);
            let g = h & 0xf000_0000;
            if g != 0 {
                h = (h ^ (g >> 24)) & 0x0fff_ffff;
            }
        }
        i += 1;
    }
    h
}

/// 名字是否落在全表里（散列体检用：只重名才算冲突，同桶不算）。
pub fn hash_bucket(name: &str, bits: u32) -> u32 {
    let mask = (1u32 << bits) - 1;
    name_hash(name) & mask
}

// ---------------------------------------------------------------------------
// 文本
// ---------------------------------------------------------------------------

/// 十进制文本。
fn dec_text(v: u32) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 10];
    let mut n = v;
    let mut i = 10usize;
    while n > 0 && i > 0 {
        i -= 1;
        buf[i] = b'0' + ((n % 10) as u8);
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

/// u32 文本。
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
// 字节串基元（no_std 下的子串操作全部手写，判据不能靠切片语法）
// ---------------------------------------------------------------------------

/// 两串是否逐字相等。
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

/// 串长度。
pub fn text_len(s: &str) -> usize {
    s.as_bytes().len()
}

/// 串里是否含某字节。
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

/// 从 `from` 起在 `hay` 里找 `needle`（手写窗口匹配，一次越界都不许有）。
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

/// 出现次数（`message()` 里"同一个名字被打了几遍"是本卡的实测维）。
pub fn count_occurrences(hay: &str, needle: &str) -> usize {
    let mut n = 0usize;
    let mut from = 0usize;
    loop {
        match find_from(hay, needle, from) {
            Some(p) => {
                n += 1;
                from = p.saturating_add(needle.len());
            }
            None => break,
        }
    }
    n
}

/// busybox 的 `last_component(argv[0])`：取最后一个 `/` 之后的子串。
pub fn last_component(s: &'static str) -> &'static str {
    match s.rsplit_once('/') {
        Some((_, tail)) => tail,
        None => s,
    }
}

/// 取最后一个 `/` 之前的前缀（不含 `/`）。
pub fn dir_prefix(s: &'static str) -> Option<&'static str> {
    match s.rsplit_once('/') {
        Some((head, _)) => Some(head),
        None => None,
    }
}

/// 是否含 `/`，以及是否以 `/` 结尾。
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

/// 是否含控制字符（< 0x20 或 0x7f）。
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

/// 是否全部由 `.` 组成。
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

/// 首字符是否为 `@`（busybox 的 retitle / NOMMU `bb_re_exec` 约定）。
pub fn has_at_prefix(s: &str) -> bool {
    match s.as_bytes().get(0) {
        Some(c) => *c == b'@',
        None => false,
    }
}

/// 剥掉一层 `@` 前缀；没有前缀时原样返回。
pub fn strip_at(s: &'static str) -> &'static str {
    match s.strip_prefix('@') {
        Some(rest) => rest,
        None => s,
    }
}

/// 把 `,` 还原成 `/`（`bb_re_exec` 用 `,` 传输路径，本卡反向还原）。
pub fn commas_to_slashes(s: &str) -> String {
    let mut out = String::new();
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if let Some(c) = b.get(i) {
            if *c == b',' {
                out.push('/');
            } else {
                out.push(*c as char);
            }
        }
        i += 1;
    }
    out
}

/// `--list` 那行的拼接器：按分隔符把名字串成一行（无尾分隔符）。
pub fn join_names(items: &[&'static str], sep: &str) -> String {
    let mut out = String::new();
    let mut i = 0usize;
    while i < items.len() {
        if let Some(n) = items.get(i) {
            if !out.is_empty() {
                out.push_str(sep);
            }
            out.push_str(n);
        }
        i += 1;
    }
    out
}

/// 分隔符整项成员判定：`"ping"` 绝不能因为含 `"in"` 而被判成在清单里。
pub fn list_has(list: &str, sep: &str, name: &str) -> bool {
    if sep.is_empty() || name.is_empty() {
        return false;
    }
    let mut p = String::new();
    p.push_str(sep);
    p.push_str(list);
    p.push_str(sep);
    let mut w = String::new();
    w.push_str(sep);
    w.push_str(name);
    w.push_str(sep);
    find_from(p.as_str(), w.as_str(), 0).is_some()
}

/// 清单项数（空清单为 0，其余按分隔符计数）。
pub fn list_items(list: &str, sep: &str) -> usize {
    if list.is_empty() {
        return 0;
    }
    if sep.is_empty() {
        return 1;
    }
    let mut n = 1usize;
    let mut from = 0usize;
    loop {
        match find_from(list, sep, from) {
            Some(p) => {
                n += 1;
                from = p.saturating_add(sep.len());
            }
            None => break,
        }
    }
    n
}

/// 换行计数：渲染文本的行数必须自己数出来。
pub fn nl_count(s: &str) -> usize {
    let b = s.as_bytes();
    let mut n = 0usize;
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
// 链接形态：argv[0] 是怎么变成现在这个样子的
// ---------------------------------------------------------------------------

/// `argv[0]` 的来源形态——本卡的第一条纵线，F11661 完全没有这一维。
///
/// busybox 的 `applet_name` 取自 `argv[0]`，而 `argv[0]` 由**内核**填：
/// 符号链接、硬链接、改名副本、`execv("/proc/self/exe")` 重启、shebang
/// 包装，五种来源给出的 `argv[0]` 形状不同，能推出来的东西也不同。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Link {
    /// 相对符号链接：`ln -s /bin/busybox foo`，`argv[0]="foo"`。
    SymlinkRel,
    /// 绝对符号链接：`/bin/ls -> /bin/busybox`，`argv[0]` 带目录。
    SymlinkAbs,
    /// 硬链接：`ln /bin/busybox /bin/ls`，与绝对符号链接同形，
    /// 但**没有**链接目标可回查，故个体名只能靠 basename。
    Hardlink,
    /// 改名副本：`cp /bin/busybox /tmp/myapp`，路径是真的，
    /// 但在 NOMMU 下这类副本重启不了（没有 `/proc/self/exe`）。
    RenamedCopy,
    /// 进程自己 `execv("/proc/self/exe")` 重启：`argv[0]` 被写成个体名。
    /// `NO_INDIVIDUAL_APPLET` 的正常形态，也是最容易被漏判的一条。
    ExecArgv0,
    /// shebang 包装：`#!/bin/busybox applet`——内核把 `argv[0]` 填成
    /// 脚本路径，busybox 看到的名字永远不是 applet 名。
    Shebang,
}

impl Link {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Link::SymlinkRel => "symlink-rel",
            Link::SymlinkAbs => "symlink-abs",
            Link::Hardlink => "hardlink",
            Link::RenamedCopy => "relinked-copy",
            Link::ExecArgv0 => "exec-argv0",
            Link::Shebang => "shebang",
        }
    }

    /// 次序（`--list-full` 的展示序，与表序无关，是固定的形态纵线）。
    pub fn order(&self) -> u8 {
        match self {
            Link::SymlinkRel => 0,
            Link::SymlinkAbs => 1,
            Link::Hardlink => 2,
            Link::RenamedCopy => 3,
            Link::ExecArgv0 => 4,
            Link::Shebang => 5,
        }
    }

    /// `argv[0]` 是否带目录（决定基名要不要过 `last_component`）。
    pub fn keeps_dirpath(&self) -> bool {
        match self {
            Link::SymlinkAbs | Link::RenamedCopy | Link::Shebang => true,
            Link::SymlinkRel | Link::Hardlink | Link::ExecArgv0 => false,
        }
    }

    /// 内核是否可能把 `argv[0]` 留空（只有 shebang 之外才有意义）。
    pub fn may_have_no_name(&self) -> bool {
        match self {
            Link::ExecArgv0 | Link::Shebang => true,
            _ => false,
        }
    }

    /// 是否"程序名可控"的形态——`@` 改形只对这几式有意义：
    /// 这几式的 `argv[0]` 是安装工具或进程自己写的，允许以 `@` 起头
    /// 声明"真正的程序名在 `argv[1]`"。
    pub fn retitle_capable(&self) -> bool {
        match self {
            Link::SymlinkAbs | Link::SymlinkRel | Link::Hardlink | Link::ExecArgv0 => true,
            Link::RenamedCopy | Link::Shebang => false,
        }
    }

    /// 是否指向 busybox 本尊（硬链接/改名副本不指向，断链才像本尊）。
    pub fn targets_busybox(&self) -> bool {
        match self {
            Link::SymlinkRel | Link::SymlinkAbs | Link::ExecArgv0 => true,
            Link::Hardlink | Link::RenamedCopy | Link::Shebang => false,
        }
    }

    /// 该形态下个体名能否被 `execv("/proc/self/exe")` 重启进组。
    ///
    /// 真规则：`NO_INDIVIDUAL_APPLET` 的组外名字要靠重启进组；NOMMU 没有
    /// `fork`，shebang 与改名副本两条路的可执行体路径不可信，重启即判死。
    pub fn reexec_allowed(&self, nommu: bool) -> bool {
        match self {
            Link::Shebang => false,
            Link::RenamedCopy => !nommu,
            _ => true,
        }
    }
}

/// busybox 自身可执行文件的名字集合（`applet_name` 认这几个为"本尊"）。
pub const SELF_NAMES: [&str; SELF_NAME_COUNT] = ["busybox", "@busybox", "/bin/busybox", "init"];

/// 名字是否属于 busybox 本尊集合。
pub fn is_self_name(name: &str) -> bool {
    let mut i = 0usize;
    while i < SELF_NAMES.len() {
        if let Some(s) = SELF_NAMES.get(i) {
            if same_text(s, name) {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 链接形态表（六式，`enums_ok` 逐式核名字与规则）。
pub const LINKS: [Link; 6] = [
    Link::SymlinkRel,
    Link::SymlinkAbs,
    Link::Hardlink,
    Link::RenamedCopy,
    Link::ExecArgv0,
    Link::Shebang,
];

// ---------------------------------------------------------------------------
// 定名：从 argv[0] 到 applet_name
// ---------------------------------------------------------------------------

/// 一次定名的结果。
///
/// busybox 的定名只有两小步：取 `argv[0]`，若首字符是 `@` 则把
/// `argv[1]` 当程序名并丢下标 0，然后取 basename。长度、点、控制字符
/// **一概不查**——这与 F11661 的 `lex` 闸正好相反，本卡的 `Named` 档
/// 允许畸形名一路进表去查（查不中才降级）。查不到和名字畸形在真机里
/// 是同一条路，所以这里只有"有没有拿到名字"三档，没有"名字好不好"档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resolved {
    /// 现场根本没有 `argv[0]`。
    Absent,
    /// 定名结果是空串。
    Empty,
    /// 正常名字（现场表项都是 `'static`，故不借用输入）。
    Named(&'static str),
}

impl Resolved {
    /// 是否拿到可用名字（空串不算可用——真机拿它查表也是不中）。
    pub fn usable(&self) -> bool {
        match self {
            Resolved::Named(_) => true,
            _ => false,
        }
    }

    /// 文本档名。
    pub fn name(&self) -> &'static str {
        match self {
            Resolved::Absent => "absent",
            Resolved::Empty => "empty",
            Resolved::Named(_) => "named",
        }
    }

    /// 拿到的名字本体；`Absent`/`Empty` 给空串（与真机 `applet_name` 初值一致）。
    pub fn text(&self) -> &'static str {
        match self {
            Resolved::Named(n) => n,
            _ => "",
        }
    }

    /// 是否"现场就不成立"（无 `argv[0]`），它决定报错行进哪条流。
    pub fn sceneless(&self) -> bool {
        match self {
            Resolved::Absent => true,
            _ => false,
        }
    }
}

/// busybox 的定名：`applet_name = argv[0]`（必要时走 `@` 改形），取 basename。
pub fn applet_name(
    argv0: &'static str,
    argv1: &'static str,
    argc: u8,
    absent: bool,
    link: Link,
) -> Resolved {
    if absent || argc == 0 {
        return Resolved::Absent;
    }
    let raw = if has_at_prefix(argv0) && link.retitle_capable() && argc > 1 {
        argv1
    } else {
        argv0
    };
    if raw.is_empty() {
        return Resolved::Empty;
    }
    let base = last_component(raw);
    if base.is_empty() {
        return Resolved::Empty;
    }
    Resolved::Named(base)
}

/// 定名用的程序名前缀（报错行的第一个字段）。
///
/// 与 [`applet_name`] 的分工是**本卡最容易判死自己的一维**：`bb_error_msg`
/// 打印的"程序名"取的是 `last_component(argv[0])`（**不剥 `@`**），而分发
/// 用的名字才走 `@` 改形与 `argv[1]`。于是 `"@zzz"`（`argv[1]="zzz"`）的现场是
/// `@zzz: 'zzz': applet not found`——前缀带着 `@`，名字不带。
///
/// 前缀**只**从 `argv[0]` 推：`argv[0]` 是空串时前缀就是空串（现场行长成
/// `: '': applet not found`）。这里**绝不**在空名现场借用 `argv[1]`：
/// `argv[1]` 在那种现场里只是第一个参数（可能是 `ls`、也可能是 `-l`），
/// 借它当程序名会让一个彻底不成立的现场打印出一条看起来合理的报错行，
/// 而本卡判的恰恰是"名字不可推"这一维。
pub fn prog_prefix(argv0: &'static str, argv1: &'static str, argc: u8, absent: bool) -> &'static str {
    // 形参留着是给签名对齐 [`applet_name`]，但**一次都不读**——读它就等于编名字。
    let _ = argv1;
    if absent || argc == 0 {
        return "";
    }
    if argv0.is_empty() {
        return "";
    }
    last_component(argv0)
}

/// 可执行体路径的推断（NOMMU 重启与 `--list-full` 的对照面）。
pub fn exec_path(argv0: &'static str, absent: bool, link: Link) -> Option<&'static str> {
    if absent || argv0.is_empty() {
        return None;
    }
    match link.keeps_dirpath() {
        true => {
            if slash_shape(argv0).0 {
                Some(argv0)
            } else {
                None
            }
        }
        false => None,
    }
}

// ---------------------------------------------------------------------------
// 组表：NO_INDIVIDUAL_APPLET 的家族清单
// ---------------------------------------------------------------------------

/// 一个组（`busybox <group>` 的 group）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Group {
    /// 组名（真 busybox 的 section 名）。
    pub name: &'static str,
    /// Kconfig 开关。
    pub kconfig: &'static str,
    /// 组内**主名**行数（含未注册件；别名不计，它跟着主名走）。
    pub members: u8,
    /// 组内主名里可按个体名直调的个数（`registered && individual`）。
    pub individuals: u8,
    /// 是否默认 `NO_INDIVIDUAL_APPLET=y`。
    pub merged_by_default: bool,
}

/// 手写组表 11 条（取自真 busybox 的 applet section）。
pub fn groups() -> [Group; GROUP_COUNT] {
    [
        Group {
            name: "coreutils",
            kconfig: "CONFIG_FEATURE_COREUTILS_INDIVIDUAL",
            members: 10,
            individuals: 9,
            merged_by_default: false,
        },
        Group {
            name: "editors",
            kconfig: "CONFIG_FEATURE_EDITORS_INDIVIDUAL",
            members: 4,
            individuals: 2,
            merged_by_default: true,
        },
        Group {
            name: "archive",
            kconfig: "CONFIG_FEATURE_ARCHIVES_INDIVIDUAL",
            members: 5,
            individuals: 5,
            merged_by_default: true,
        },
        Group {
            name: "findutils",
            kconfig: "CONFIG_FEATURE_FINDUTILS_INDIVIDUAL",
            members: 2,
            individuals: 2,
            merged_by_default: true,
        },
        Group {
            name: "procps",
            kconfig: "CONFIG_FEATURE_PROCPS_INDIVIDUAL",
            members: 3,
            individuals: 3,
            merged_by_default: true,
        },
        Group {
            name: "shellutils",
            kconfig: "CONFIG_FEATURE_SHELLUTILS_INDIVIDUAL",
            members: 1,
            individuals: 1,
            merged_by_default: true,
        },
        Group {
            name: "fsck",
            kconfig: "CONFIG_FEATURE_FSCK_INDIVIDUAL",
            members: 0,
            individuals: 0,
            merged_by_default: true,
        },
        Group {
            name: "logging",
            kconfig: "CONFIG_FEATURE_LOGGING_INDIVIDUAL",
            members: 0,
            individuals: 0,
            merged_by_default: true,
        },
        Group {
            name: "networking",
            kconfig: "CONFIG_FEATURE_NETWORKING_INDIVIDUAL",
            members: 0,
            individuals: 0,
            merged_by_default: true,
        },
        Group {
            name: "sysadm",
            kconfig: "CONFIG_FEATURE_SYSADM_INDIVIDUAL",
            members: 0,
            individuals: 0,
            merged_by_default: true,
        },
        Group {
            name: "crypto",
            kconfig: "CONFIG_FEATURE_CRYPTO_INDIVIDUAL",
            members: 0,
            individuals: 0,
            merged_by_default: true,
        },
    ]
}

/// 组表十一行（`groups()` 的常量镜像，`table_ok` 逐行对拍）。
pub const GROUPS: [Group; GROUP_COUNT] = [
        Group {
            name: "coreutils",
            kconfig: "CONFIG_FEATURE_COREUTILS_INDIVIDUAL",
            members: 10,
            individuals: 9,
            merged_by_default: false,
        },
        Group {
            name: "editors",
            kconfig: "CONFIG_FEATURE_EDITORS_INDIVIDUAL",
            members: 4,
            individuals: 2,
            merged_by_default: true,
        },
        Group {
            name: "archive",
            kconfig: "CONFIG_FEATURE_ARCHIVES_INDIVIDUAL",
            members: 5,
            individuals: 5,
            merged_by_default: true,
        },
        Group {
            name: "findutils",
            kconfig: "CONFIG_FEATURE_FINDUTILS_INDIVIDUAL",
            members: 2,
            individuals: 2,
            merged_by_default: true,
        },
        Group {
            name: "procps",
            kconfig: "CONFIG_FEATURE_PROCPS_INDIVIDUAL",
            members: 3,
            individuals: 3,
            merged_by_default: true,
        },
        Group {
            name: "shellutils",
            kconfig: "CONFIG_FEATURE_SHELLUTILS_INDIVIDUAL",
            members: 1,
            individuals: 1,
            merged_by_default: true,
        },
        Group {
            name: "fsck",
            kconfig: "CONFIG_FEATURE_FSCK_INDIVIDUAL",
            members: 0,
            individuals: 0,
            merged_by_default: true,
        },
        Group {
            name: "logging",
            kconfig: "CONFIG_FEATURE_LOGGING_INDIVIDUAL",
            members: 0,
            individuals: 0,
            merged_by_default: true,
        },
        Group {
            name: "networking",
            kconfig: "CONFIG_FEATURE_NETWORKING_INDIVIDUAL",
            members: 0,
            individuals: 0,
            merged_by_default: true,
        },
        Group {
            name: "sysadm",
            kconfig: "CONFIG_FEATURE_SYSADM_INDIVIDUAL",
            members: 0,
            individuals: 0,
            merged_by_default: true,
        },
        Group {
            name: "crypto",
            kconfig: "CONFIG_FEATURE_CRYPTO_INDIVIDUAL",
            members: 0,
            individuals: 0,
            merged_by_default: true,
        },
];

/// 组名查表。
pub fn group_at(name: &str) -> Option<Group> {
    let g = groups();
    let mut i = 0usize;
    while i < g.len() {
        if let Some(x) = g.get(i) {
            if same_text(x.name, name) {
                return Some(*x);
            }
        }
        i += 1;
    }
    None
}

/// 组名序号（`--list` 的展示序；未命中返回 `None`，绝不返 0 冒充命中）。
pub fn group_index(name: &str) -> Option<usize> {
    let g = groups();
    let mut i = 0usize;
    while i < g.len() {
        if let Some(x) = g.get(i) {
            if same_text(x.name, name) {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// 组内**主名**清单（`busybox <group>` 的用法行走的就是它）。
///
/// `sep` 由调用方给：横幅用 `", "`，`--list` 用 `'\n'` 前的裸名表。
/// 未注册件（`registered=false`）不进这份清单——真机里它压根没进
/// `applet_name[]`，列出来就是骗人。
pub fn group_member_list(group: &str, sep: &str) -> String {
    let mut out = String::new();
    let mut first = true;
    let r = table();
    let mut i = 0usize;
    while i < r.len() {
        if let Some(x) = r.get(i) {
            if x.is_primary() && same_text(x.group, group) && x.registered {
                if !first {
                    out.push_str(sep);
                }
                first = false;
                out.push_str(x.name);
            }
        }
        i += 1;
    }
    out
}

/// 组内主名行数（含未注册件——降级指路要把它们算进来）。
pub fn group_primary_count(group: &str) -> usize {
    let mut n = 0usize;
    let r = table();
    let mut i = 0usize;
    while i < r.len() {
        if let Some(x) = r.get(i) {
            if x.is_primary() && same_text(x.group, group) {
                n += 1;
            }
        }
        i += 1;
    }
    n
}

/// 组内已注册主名行数。
pub fn group_registered_count(group: &str) -> usize {
    let mut n = 0usize;
    let r = table();
    let mut i = 0usize;
    while i < r.len() {
        if let Some(x) = r.get(i) {
            if x.is_primary() && same_text(x.group, group) && x.registered {
                n += 1;
            }
        }
        i += 1;
    }
    n
}

/// 组内可按个体名直调的主名行数（`Group.individuals` 的对账口）。
pub fn group_individual_count(group: &str) -> usize {
    let mut n = 0usize;
    let r = table();
    let mut i = 0usize;
    while i < r.len() {
        if let Some(x) = r.get(i) {
            if x.is_primary() && same_text(x.group, group) && x.dispatchable() {
                n += 1;
            }
        }
        i += 1;
    }
    n
}

// ---------------------------------------------------------------------------
// 成员表：个体名 / 别名
// ---------------------------------------------------------------------------

/// 一行成员表项。
///
/// `applet_name[]` / `applet_main[]` 在 `NO_INDIVIDUAL_APPLET` 下按组切开，
/// 于是"名字在不在表里"变成两问：在不在**本组**的表里（`registered`），
/// 以及能不能**按个体名**直调（`individual`）。F11663 判的正是后者的用户面
/// 后果；本卡把 `individual=false` 的名字判成 `Group` 档——它要求走
/// `busybox <group> <name>`，而不是报 not found。
#[derive(Clone, Copy,Debug,PartialEq,Eq)]
pub struct Member {
    /// 表里的名字。
    pub name: &'static str,
    /// 所属组（空串 = 主二进制自带，不属于任何组）。
    pub group: &'static str,
    /// 别名指向的主名；空串表示本身是主名。
    pub alias_of: &'static str,
    /// 注册序（表下标）。
    pub prio: u8,
    /// 是否在本构建的表里。
    pub registered: bool,
    /// 是否可按个体名直调（`CONFIG_*_INDIVIDUAL`）。
    pub individual: bool,
}

impl Member {
    /// 空白行。
    pub const fn blank() -> Member {
        Member {
            name: "",
            group: "",
            alias_of: "",
            prio: 0,
            registered: false,
            individual: false,
        }
    }

    /// 是否别名。
    pub fn is_alias(&self) -> bool {
        !self.alias_of.is_empty()
    }

    /// 是否主名。
    pub fn is_primary(&self) -> bool {
        self.alias_of.is_empty()
    }

    /// 是否属于某个组。
    pub fn in_group(&self) -> bool {
        !self.group.is_empty()
    }

    /// 答复是否"可以按这个名字跑起来"。
    pub fn dispatchable(&self) -> bool {
        self.registered && self.individual
    }

    /// 该行在 `--list-full` 里的完整名字：`<group> <name>`；无组时为裸名。
    pub fn qualified(&self) -> String {
        let mut s = String::new();
        if self.in_group() {
            s.push_str(self.group);
            s.push(' ');
        }
        s.push_str(self.name);
        s
    }

    /// 行指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_str(h, self.name);
        h = fold_str(h, self.group);
        h = fold_str(h, self.alias_of);
        h = fold_word(h, self.prio as u64);
        h = fold_word(h, self.registered as u64);
        h = fold_word(h, self.individual as u64);
        fold_end(h)
    }

    /// 渲染一行（`--list-full` 风格）。
    pub fn render(&self) -> String {
        let mut s = self.qualified();
        if self.is_alias() {
            s.push_str(" -> ");
            s.push_str(self.alias_of);
        }
        if !self.registered {
            s.push_str(" [trimmed]");
        } else if !self.individual {
            s.push_str(" [group-only]");
        }
        s.push('\n');
        s
    }
}

/// 手写成员表 27 行：主名 25、别名 2（`ash`/`hush` 指向 `sh`）。
///
/// 全部取自真 busybox 的 applet 名；`vi` 是 editors 组里
/// `DEFAULT_NO_INDIVIDUAL_APPLET=y` 的对应物（`individual=false`，
/// 故按个体名调用会落 `Group` 档）；`ed` 被裁掉（`registered=false`）
/// 是给 F11663 留的同源样本；`fsck/logging/networking/sysadm/crypto`
/// 五个组在本构建里一个成员都没编进来，只有组名——`--list` 只列成员，
/// 于是它们"有组无人"，这正是 `Groups` 档横幅与清单条数对不上的地方。
pub fn table() -> [Member; TABLE_COUNT] {
    [
        Member {
            name: "ls",
            group: "coreutils",
            alias_of: "",
            prio: 0,
            registered: true,
            individual: true,
        },
        Member {
            name: "cp",
            group: "coreutils",
            alias_of: "",
            prio: 1,
            registered: true,
            individual: true,
        },
        Member {
            name: "mv",
            group: "coreutils",
            alias_of: "",
            prio: 2,
            registered: true,
            individual: true,
        },
        Member {
            name: "rm",
            group: "coreutils",
            alias_of: "",
            prio: 3,
            registered: true,
            individual: true,
        },
        Member {
            name: "cat",
            group: "coreutils",
            alias_of: "",
            prio: 4,
            registered: true,
            individual: true,
        },
        Member {
            name: "echo",
            group: "coreutils",
            alias_of: "",
            prio: 5,
            registered: true,
            individual: true,
        },
        Member {
            name: "dd",
            group: "coreutils",
            alias_of: "",
            prio: 6,
            registered: true,
            individual: true,
        },
        Member {
            name: "true",
            group: "coreutils",
            alias_of: "",
            prio: 7,
            registered: true,
            individual: true,
        },
        Member {
            name: "printf",
            group: "coreutils",
            alias_of: "",
            prio: 8,
            registered: true,
            individual: true,
        },
        Member {
            name: "test",
            group: "coreutils",
            alias_of: "",
            prio: 9,
            registered: true,
            individual: false,
        },
        Member {
            name: "awk",
            group: "editors",
            alias_of: "",
            prio: 10,
            registered: true,
            individual: true,
        },
        Member {
            name: "sed",
            group: "editors",
            alias_of: "",
            prio: 11,
            registered: true,
            individual: true,
        },
        Member {
            name: "ed",
            group: "editors",
            alias_of: "",
            prio: 12,
            registered: false,
            individual: false,
        },
        Member {
            name: "vi",
            group: "editors",
            alias_of: "",
            prio: 13,
            registered: true,
            individual: false,
        },
        Member {
            name: "tar",
            group: "archive",
            alias_of: "",
            prio: 14,
            registered: true,
            individual: true,
        },
        Member {
            name: "gzip",
            group: "archive",
            alias_of: "",
            prio: 15,
            registered: true,
            individual: true,
        },
        Member {
            name: "bzip2",
            group: "archive",
            alias_of: "",
            prio: 16,
            registered: true,
            individual: true,
        },
        Member {
            name: "cpio",
            group: "archive",
            alias_of: "",
            prio: 17,
            registered: true,
            individual: true,
        },
        Member {
            name: "xz",
            group: "archive",
            alias_of: "",
            prio: 18,
            registered: true,
            individual: true,
        },
        Member {
            name: "find",
            group: "findutils",
            alias_of: "",
            prio: 19,
            registered: true,
            individual: true,
        },
        Member {
            name: "xargs",
            group: "findutils",
            alias_of: "",
            prio: 20,
            registered: true,
            individual: true,
        },
        Member {
            name: "ps",
            group: "procps",
            alias_of: "",
            prio: 21,
            registered: true,
            individual: true,
        },
        Member {
            name: "top",
            group: "procps",
            alias_of: "",
            prio: 22,
            registered: true,
            individual: true,
        },
        Member {
            name: "uptime",
            group: "procps",
            alias_of: "",
            prio: 23,
            registered: true,
            individual: true,
        },
        Member {
            name: "sh",
            group: "shellutils",
            alias_of: "",
            prio: 24,
            registered: true,
            individual: true,
        },
        Member {
            name: "ash",
            group: "shellutils",
            alias_of: "sh",
            prio: 25,
            registered: true,
            individual: true,
        },
        Member {
            name: "hush",
            group: "shellutils",
            alias_of: "sh",
            prio: 26,
            registered: true,
            individual: true,
        },
    ]
}

/// 按名查成员表（精确 strcmp，真 `find_applet_by_name` 的语义）。
pub fn row_at(name: &str) -> Option<Member> {
    let r = table();
    let mut i = 0usize;
    while i < r.len() {
        if let Some(x) = r.get(i) {
            if same_text(x.name, name) {
                return Some(*x);
            }
        }
        i += 1;
    }
    None
}

/// 成员表序号（未命中 `None`，绝不返 0 冒充命中）。
pub fn row_index(name: &str) -> Option<usize> {
    let r = table();
    let mut i = 0usize;
    while i < r.len() {
        if let Some(x) = r.get(i) {
            if same_text(x.name, name) {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// 别名行数（`alias_of` 非空的成员表行）。
pub fn alias_count() -> usize {
    let mut n = 0usize;
    let r = table();
    let mut i = 0usize;
    while i < r.len() {
        if let Some(x) = r.get(i) {
            if x.is_alias() {
                n += 1;
            }
        }
        i += 1;
    }
    n
}

/// 未注册行数（`registered=false`，即被裁件）。
pub fn trimmed_count() -> usize {
    let mut n = 0usize;
    let r = table();
    let mut i = 0usize;
    while i < r.len() {
        if let Some(x) = r.get(i) {
            if !x.registered {
                n += 1;
            }
        }
        i += 1;
    }
    n
}

/// 只进组、不能按个体名直调的行数（`registered && !individual`）。
pub fn group_only_count() -> usize {
    let mut n = 0usize;
    let r = table();
    let mut i = 0usize;
    while i < r.len() {
        if let Some(x) = r.get(i) {
            if x.registered && !x.individual {
                n += 1;
            }
        }
        i += 1;
    }
    n
}

/// 可直调行数（`registered && individual`，含别名）。
pub fn dispatchable_count() -> usize {
    let mut n = 0usize;
    let r = table();
    let mut i = 0usize;
    while i < r.len() {
        if let Some(x) = r.get(i) {
            if x.dispatchable() {
                n += 1;
            }
        }
        i += 1;
    }
    n
}

/// 主名行数（`alias_of` 为空）。
pub fn primary_count() -> usize {
    let mut n = 0usize;
    let r = table();
    let mut i = 0usize;
    while i < r.len() {
        if let Some(x) = r.get(i) {
            if x.is_primary() {
                n += 1;
            }
        }
        i += 1;
    }
    n
}

/// 组名清单（`--list` 之外的第二条指路面：`busybox <group>`）。
pub fn group_name_list(sep: &str) -> String {
    let mut out = String::new();
    let mut first = true;
    let g = groups();
    let mut i = 0usize;
    while i < g.len() {
        if let Some(x) = g.get(i) {
            if !first {
                out.push_str(sep);
            }
            first = false;
            out.push_str(x.name);
        }
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// 八闸阶段
// ---------------------------------------------------------------------------

/// 一次调用的结束阶段——本卡的纵线，八段。
///
/// 与 F11661 的七闸比，本卡多了 `exec`（重启进组）并把 `text` 换成
/// `banner` + `stream` 两段：降级主行为的现场是**一整块输出**，横幅缺件
/// 与流向前错是两类完全不同的缺陷，合成一段就判不出是哪一件坏了。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// 闸一：`argv[0]` 在场性。空 argv 判死在这里。
    Argv0,
    /// 闸二：基名提取。尾斜杠（`/usr/sbin/`）在这里判死。
    Base,
    /// 闸三：定名与 `@` 改形。裸 `@` 在这里判死；名字是本尊也在这里归位。
    Name,
    /// 闸四：组表 + 成员表两跳。答复五档在本段定型。
    Table,
    /// 闸五：重启进组。NOMMU / 改名副本 / shebang 在这里判死（退 126）。
    Exec,
    /// 闸六：usage 横幅成形。档位四取。
    Banner,
    /// 闸七：双流去向。只有 `Two` 是真机。
    Stream,
    /// 闸八：降级主行为齐备（收尾段，也覆盖"跑通"的正路收尾）。
    Ready,
}

impl Stage {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Stage::Argv0 => "argv0",
            Stage::Base => "base",
            Stage::Name => "name",
            Stage::Table => "table",
            Stage::Exec => "exec",
            Stage::Banner => "banner",
            Stage::Stream => "stream",
            Stage::Ready => "ready",
        }
    }

    /// 次序（0..7，`Gate` 的单调依据）。
    pub fn order(&self) -> u8 {
        match self {
            Stage::Argv0 => 0,
            Stage::Base => 1,
            Stage::Name => 2,
            Stage::Table => 3,
            Stage::Exec => 4,
            Stage::Banner => 5,
            Stage::Stream => 6,
            Stage::Ready => 7,
        }
    }

    /// 是否已进入输出面（闸六之后）。
    ///
    /// 这一刀是本卡的红档依据：`ready` 齐备位只在输出面里才有意义，
    /// 判死在 `exec` 之前的现场压根没资格谈"三件套"。
    pub fn on_surface(&self) -> bool {
        self.order() >= Stage::Banner.order()
    }

    /// 是否属于"现场不可推"档（闸一二三）。
    pub fn sceneless(&self) -> bool {
        self.order() <= Stage::Name.order()
    }
}

/// 八闸纵线（`enums_ok` 逐段核名字与次序）。
pub const STAGES: [Stage; 8] = [
    Stage::Argv0,
    Stage::Base,
    Stage::Name,
    Stage::Table,
    Stage::Exec,
    Stage::Banner,
    Stage::Stream,
    Stage::Ready,
];

// ---------------------------------------------------------------------------
// 查表答复五档
// ---------------------------------------------------------------------------

/// 组表 + 成员表两跳的答复档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    /// 主名在成员表里、且可按个体名直调。
    Registered,
    /// 别名在成员表里（`ash -> sh`）——跑的是主名的实现。
    Alias,
    /// 名字**是个组名**或"在表里但只能进组调用"（`vi`）。
    ///
    /// 本卡独有档：真机对它打的是组清单加退 1，不是 `applet not found`。
    Group,
    /// 名字在源里、被 config 裁掉（`ed`）——用户面与未注册名同形。
    Trimmed,
    /// 名字根本不存在。
    Unregistered,
}

impl Hit {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Hit::Registered => "registered",
            Hit::Alias => "alias",
            Hit::Group => "group",
            Hit::Trimmed => "trimmed",
            Hit::Unregistered => "unregistered",
        }
    }

    /// 次序。
    pub fn order(&self) -> u8 {
        match self {
            Hit::Registered => 0,
            Hit::Alias => 1,
            Hit::Group => 2,
            Hit::Trimmed => 3,
            Hit::Unregistered => 4,
        }
    }

    /// 是否"这名字能直接把 applet 跑起来"。
    pub fn runnable(&self) -> bool {
        match self {
            Hit::Registered | Hit::Alias => true,
            _ => false,
        }
    }

    /// 是否走降级主行为（三件套）。
    ///
    /// `Trimmed` 与 `Unregistered` 同在此列——与 F11663 的分界就落在这
    /// 一行：内档不同，用户面同形。
    pub fn degrades(&self) -> bool {
        match self {
            Hit::Trimmed | Hit::Unregistered => true,
            _ => false,
        }
    }

    /// 是否"进组才有戏"档。
    pub fn group_face(&self) -> bool {
        match self {
            Hit::Group => true,
            _ => false,
        }
    }

    /// 是否内部维度档（被裁件——用户面不该看出来的那一维）。
    pub fn internal(&self) -> bool {
        match self {
            Hit::Trimmed => true,
            _ => false,
        }
    }
}

/// 五档纵线。
pub const HITS: [Hit; 5] = [Hit::Registered, Hit::Alias, Hit::Group, Hit::Trimmed, Hit::Unregistered];

// ---------------------------------------------------------------------------
// 输出面三维：横幅 / 双流 / 指路
// ---------------------------------------------------------------------------

/// usage 横幅档位——降级三件套的第一件。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Banner {
    /// 完整主用法（头 + 用法式 + "Currently defined functions:" + 清单）。
    Full,
    /// 组清单（`busybox coreutils` 这类进组没给成员的现场）。
    Groups,
    /// 单组清单（组内个体名只能进组调用时的短面）。
    GroupOnly,
    /// 没打横幅（`CONFIG_SHOW_USAGE=n` 或全被重定向丢掉）。
    Suppressed,
}

impl Banner {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Banner::Full => "full",
            Banner::Groups => "groups",
            Banner::GroupOnly => "group-only",
            Banner::Suppressed => "suppressed",
        }
    }

    /// 次序。
    pub fn order(&self) -> u8 {
        match self {
            Banner::Full => 0,
            Banner::Groups => 1,
            Banner::GroupOnly => 2,
            Banner::Suppressed => 3,
        }
    }

    /// 是否齐备件（只有完整主用法算齐备）。
    pub fn complete(&self) -> bool {
        match self {
            Banner::Full => true,
            _ => false,
        }
    }

    /// 是否打了任何清单（区分"没横幅"与"横幅窄"）。
    pub fn shows_list(&self) -> bool {
        match self {
            Banner::Suppressed => false,
            _ => true,
        }
    }

    /// 该档的出口流：横幅永远走 stdout，`Suppressed` 无出口。
    pub fn stream(&self) -> &'static str {
        match self {
            Banner::Suppressed => "none",
            _ => "stdout",
        }
    }
}

/// 双流去向档位——降级三件套的第二件（错行去哪）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flows {
    /// 横幅走 stdout、错行走 stderr——真机档。
    Two,
    /// 只剩 stdout 一条（stderr 被 `2>/dev/null` 吞了）。
    OnlyStdout,
    /// 只剩 stderr 一条（横幅被关或被并走）。
    OnlyStderr,
    /// 两条并被一条（`&>` 同文件；顺序信息丢失）。
    Merged,
    /// 两条都丢。
    Suppressed,
}

impl Flows {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Flows::Two => "two",
            Flows::OnlyStdout => "only-stdout",
            Flows::OnlyStderr => "only-stderr",
            Flows::Merged => "merged",
            Flows::Suppressed => "suppressed",
        }
    }

    /// 次序。
    pub fn order(&self) -> u8 {
        match self {
            Flows::Two => 0,
            Flows::OnlyStdout => 1,
            Flows::OnlyStderr => 2,
            Flows::Merged => 3,
            Flows::Suppressed => 4,
        }
    }

    /// 是否真机档（两条流各走各的）。
    pub fn two(&self) -> bool {
        match self {
            Flows::Two => true,
            _ => false,
        }
    }

    /// 错行是否还能被人看见。
    pub fn err_visible(&self) -> bool {
        match self {
            Flows::Two | Flows::OnlyStderr | Flows::Merged => true,
            _ => false,
        }
    }

    /// 横幅是否还能被人看见。
    pub fn banner_visible(&self) -> bool {
        match self {
            Flows::Two | Flows::OnlyStdout | Flows::Merged => true,
            _ => false,
        }
    }

    /// 是否整块输出都丢了。
    pub fn lost(&self) -> bool {
        match self {
            Flows::Suppressed => true,
            _ => false,
        }
    }
}

/// 双流五档纵线。
pub const FLOWS: [Flows; 5] =
    [Flows::Two, Flows::OnlyStdout, Flows::OnlyStderr, Flows::Merged, Flows::Suppressed];

/// 指路行档位——降级三件套的第三件。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hint {
    /// 没指路（跑通、判死在输出面之前、或指路特性被裁）。
    None,
    /// `Run 'busybox --list' …`——未注册名的正牌指路。
    TryList,
    /// `Run 'busybox --list-full' …`——带目录路径的链接名（可能撞组名）。
    TryListFull,
    /// `Run 'busybox <group> <name>' …`——被裁件与组内件走组调用。
    TryGroup,
}

impl Hint {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Hint::None => "none",
            Hint::TryList => "try-list",
            Hint::TryListFull => "try-list-full",
            Hint::TryGroup => "try-group",
        }
    }

    /// 次序。
    pub fn order(&self) -> u8 {
        match self {
            Hint::None => 0,
            Hint::TryList => 1,
            Hint::TryGroup => 2,
            Hint::TryListFull => 3,
        }
    }

    /// 是否齐备件（指路行必须存在）。
    pub fn present(&self) -> bool {
        match self {
            Hint::None => false,
            _ => true,
        }
    }

    /// 是否指到组调用（内部维度外溢的那一档）。
    pub fn points_to_group(&self) -> bool {
        match self {
            Hint::TryGroup => true,
            _ => false,
        }
    }
}

/// 指路四档纵线。
pub const HINTS: [Hint; 4] = [Hint::None, Hint::TryList, Hint::TryGroup, Hint::TryListFull];

// ---------------------------------------------------------------------------
// 注解
// ---------------------------------------------------------------------------

/// 结果注解——本卡的"为什么"档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Note {
    /// 个体名直调跑通（链接名就是注册主名）。
    Ran,
    /// 别名跑通（跑的是主名实现，注解要如实标出来）。
    AliasRan,
    /// 重启进组后跑通（`NO_INDIVIDUAL` 的正路）。
    ReExecRun,
    /// 进了组没给成员：打组清单退 1。
    GroupUsage,
    /// 完整降级主行为（banner + errline + hint 三件套齐）。
    Degraded,
    /// 降级但横幅缺件（半备）。
    Bannerless,
    /// 降级但两条流被并/被吞（半备）。
    FlowsOff,
    /// 降级但指路行被裁（半备）。
    Hintless,
    /// 重启判死（NOMMU 下的改名副本 / shebang），退 126。
    NoReexec,
    /// 空 argv：程序名无从谈起。
    Sceneless,
    /// 基名不可推（尾斜杠）。
    Baseless,
    /// 裸 `@` 占位：改形后没有名字。
    AtBare,
    /// `argv[0]` 就是 busybox 本尊——该现场归 F11661，本卡不判。
    MainPath,
}

impl Note {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Note::Ran => "ran",
            Note::AliasRan => "alias-ran",
            Note::ReExecRun => "re-exec-run",
            Note::GroupUsage => "group-usage",
            Note::Degraded => "degraded",
            Note::Bannerless => "bannerless",
            Note::FlowsOff => "flows-off",
            Note::Hintless => "hintless",
            Note::NoReexec => "no-re-exec",
            Note::Sceneless => "sceneless",
            Note::Baseless => "baseless",
            Note::AtBare => "at-bare",
            Note::MainPath => "main-path",
        }
    }

    /// 次序。
    pub fn order(&self) -> u8 {
        match self {
            Note::Ran => 0,
            Note::AliasRan => 1,
            Note::ReExecRun => 2,
            Note::GroupUsage => 3,
            Note::Degraded => 4,
            Note::Bannerless => 5,
            Note::FlowsOff => 6,
            Note::Hintless => 7,
            Note::NoReexec => 8,
            Note::Sceneless => 9,
            Note::Baseless => 10,
            Note::AtBare => 11,
            Note::MainPath => 12,
        }
    }

    /// 是否"跑通"面注解。
    pub fn run_face(&self) -> bool {
        match self {
            Note::Ran | Note::AliasRan | Note::ReExecRun => true,
            _ => false,
        }
    }

    /// 是否降级面注解（三件套的各种完整度）。
    pub fn degrade_face(&self) -> bool {
        match self {
            Note::Degraded | Note::Bannerless | Note::FlowsOff | Note::Hintless => true,
            _ => false,
        }
    }

    /// 是否"现场本身不成立"注解。
    pub fn no_scene(&self) -> bool {
        match self {
            Note::Sceneless | Note::Baseless | Note::AtBare => true,
            _ => false,
        }
    }

    /// 是否退出码 127 面（shell 的 not-found 约定）。
    pub fn exit_127_face(&self) -> bool {
        match self {
            Note::Degraded | Note::Bannerless | Note::FlowsOff | Note::Hintless => true,
            _ => false,
        }
    }
}

/// 十三档注解纵线。
pub const NOTES: [Note; 13] = [
    Note::Ran,
    Note::AliasRan,
    Note::ReExecRun,
    Note::GroupUsage,
    Note::Degraded,
    Note::Bannerless,
    Note::FlowsOff,
    Note::Hintless,
    Note::NoReexec,
    Note::Sceneless,
    Note::Baseless,
    Note::AtBare,
    Note::MainPath,
];

// ---------------------------------------------------------------------------
// 档位
// ---------------------------------------------------------------------------

/// 结果档位——矩阵的行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// 跑通：走到收尾段且退出码 0。
    Pass,
    /// 完整降级：三件套齐 + ENOENT + 127。本卡的正牌红档面。
    Degrade,
    /// 半备：到了输出面尽头但三件套不齐（缺横幅/流向前错/组用法）。
    Partial,
    /// 未定：判死在输出面之前，齐备位无从谈起。
    Undef,
}

impl Tier {
    /// 由实际结果投影。
    ///
    /// 只看 `Outcome`，不看 `Claim`——与 F11661 同律：六格复刻现场都落
    /// degrade 档，覆盖面看着满，红档却挤在同一行。
    pub fn of(o: &Outcome) -> Tier {
        if o.stage != Stage::Ready {
            return Tier::Undef;
        }
        if o.value == EXIT_RUN_OK {
            return Tier::Pass;
        }
        if o.default {
            Tier::Degrade
        } else {
            Tier::Partial
        }
    }

    /// 次序。
    pub fn index(&self) -> usize {
        match self {
            Tier::Pass => 0,
            Tier::Degrade => 1,
            Tier::Partial => 2,
            Tier::Undef => 3,
        }
    }

    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Tier::Pass => "pass",
            Tier::Degrade => "degrade",
            Tier::Partial => "partial",
            Tier::Undef => "undef",
        }
    }

    /// 是否成功档（只有跑通算成功）。
    pub fn success(&self) -> bool {
        match self {
            Tier::Pass => true,
            _ => false,
        }
    }

    /// 是否降级面（本卡四档里两档是降级面：完整与半备）。
    pub fn degrade_face(&self) -> bool {
        match self {
            Tier::Degrade | Tier::Partial => true,
            _ => false,
        }
    }

    /// 该档是否允许 `ready` 齐备位置真。
    pub fn ready_allowed(&self) -> bool {
        match self {
            Tier::Degrade => true,
            _ => false,
        }
    }
}

/// 四档纵线。
pub const TIERS: [Tier; TIER_COUNT] = [Tier::Pass, Tier::Degrade, Tier::Partial, Tier::Undef];

// ---------------------------------------------------------------------------
// 缺陷
// ---------------------------------------------------------------------------

/// 申报缺陷。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// 一致。
    Ok,
    /// 阶段错（八闸位报歪）。
    Stage,
    /// errno 错。
    Code,
    /// 退出码错。
    Value,
    /// 注解错。
    Note,
    /// 降级主行为齐备位错。
    Default,
    /// 链接形态错（本卡独有维，申报器不许改口）。
    Link,
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
            Fault::Stage => AV_ESTAGE,
            Fault::Code => AV_ECODE,
            Fault::Value => AV_EVALUE,
            Fault::Note => AV_ENOTE,
            Fault::Default => AV_EDEFAULT,
            Fault::Link => AV_ELINK,
            Fault::Estate => AV_ESTATE,
            Fault::Count => AV_ECOUNT,
            Fault::Slot => AV_ESLOT,
            Fault::Seal => AV_ESEAL,
            Fault::Nobs => AV_ENOBS,
            Fault::Matrix => AV_EMATRIX,
            Fault::Bal => AV_EBAL,
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
        Fault::Link => "link",
        Fault::Estate => "estate",
        Fault::Count => "count",
        Fault::Slot => "slot",
        Fault::Seal => "seal",
        Fault::Nobs => "nobs",
        Fault::Matrix => "matrix",
        Fault::Bal => "bal",
    }
}

/// 缺陷纵线（与 13 个错误码一一对齐的那 13 档）。
pub const FAULTS: [Fault; 14] = [
    Fault::Ok,
    Fault::Stage,
    Fault::Code,
    Fault::Value,
    Fault::Note,
    Fault::Default,
    Fault::Link,
    Fault::Estate,
    Fault::Count,
    Fault::Slot,
    Fault::Seal,
    Fault::Nobs,
    Fault::Matrix,
    Fault::Bal,
];

// ---------------------------------------------------------------------------
// 现场形状
// ---------------------------------------------------------------------------

/// 一次调用的现场形状。
///
/// 现场给的是**内核交给进程的东西**（`argv`、链接形态、config 开关、重定向
/// 面），不是结果。结果一律由 [`define`] 走八闸推出来——这条纪律让申报器
/// 无从"看着答案编现场"。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shape {
    /// 语料标签。
    pub tag: &'static str,
    /// `argv[0]`。
    pub argv0: &'static str,
    /// `argv[1]`（`@` 改形时才是程序名）。
    pub argv1: &'static str,
    /// 参数个数（0 与"没有 `argv[0]`"是两个档，故单独一维）。
    pub argc: u8,
    /// `argv` 表本身为空——内核给的现场不成立。
    pub absent: bool,
    /// 链接形态。
    pub link: Link,
    /// `CONFIG_NOMMU`。
    pub nommu: bool,
    /// `CONFIG_SHOW_USAGE=n`：降级时不打横幅。
    pub banner_off: bool,
    /// 指路行被裁（`CONFIG_FEATURE_APPLET_HINTS=n`）。
    pub hint_off: bool,
    /// stdout 与 stderr 被并到同一处（`&>`）。
    pub merged_out: bool,
    /// 两条流都被丢进黑洞（`>/dev/null 2>&1`）。
    pub dropped_out: bool,
}

impl Shape {
    /// 空白现场（`argc=1`，一个不存在的名字，绝对符号链接）。
    pub const fn blank() -> Shape {
        Shape {
            tag: "blank",
            argv0: "/bin/blank",
            argv1: "",
            argc: 1,
            absent: false,
            link: Link::SymlinkAbs,
            nommu: false,
            banner_off: false,
            hint_off: false,
            merged_out: false,
            dropped_out: false,
        }
    }

    /// 定名结果。
    pub fn resolves(&self) -> Resolved {
        applet_name(self.argv0, self.argv1, self.argc, self.absent, self.link)
    }

    /// 进表查的键（`applet_name`）；定名不成立时是空串。
    pub fn key(&self) -> &'static str {
        self.resolves().text()
    }

    /// 报错行的程序名前缀（`bb_error_msg` 用的那一份）。
    pub fn prog(&self) -> &'static str {
        prog_prefix(self.argv0, self.argv1, self.argc, self.absent)
    }

    /// 是否真的走了 `@` 改形（闸三的开关位）。
    pub fn retitled(&self) -> bool {
        !self.absent
            && self.argc > 1
            && has_at_prefix(self.argv0)
            && self.link.retitle_capable()
    }

    /// 键是否带目录路径（决定指路行进 `--list` 还是 `--list-full`）。
    pub fn keyed_with_dir(&self) -> bool {
        self.link.keeps_dirpath() && slash_shape(self.argv0).0
    }

    /// 该现场的可执行体路径。
    pub fn path(&self) -> Option<&'static str> {
        exec_path(self.argv0, self.absent, self.link)
    }

    /// 双流去向（重定向面的规则，与 config 面合成后才有最终档）。
    pub fn flows_plan(&self) -> Flows {
        if self.dropped_out {
            return Flows::Suppressed;
        }
        if self.merged_out {
            return Flows::Merged;
        }
        if self.banner_off {
            return Flows::OnlyStderr;
        }
        Flows::Two
    }

    /// 是否属于"本尊自调用"（归 F11661 的现场，本卡只判它该被归位）。
    pub fn self_named(&self) -> bool {
        is_self_name(self.argv0) || is_self_name(self.key())
    }

    /// 现场指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_str(h, self.tag);
        h = fold_str(h, self.argv0);
        h = fold_str(h, self.argv1);
        h = fold_word(h, self.argc as u64);
        h = fold_word(h, self.absent as u64);
        h = fold_word(h, self.link.order() as u64);
        h = fold_word(h, self.nommu as u64);
        h = fold_word(h, self.banner_off as u64);
        h = fold_word(h, self.hint_off as u64);
        h = fold_word(h, self.merged_out as u64);
        fold_word(h, self.dropped_out as u64)
    }
}

// ---------------------------------------------------------------------------
// 结果
// ---------------------------------------------------------------------------

/// 八闸走出来的结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// 结束阶段。
    pub stage: Stage,
    /// errno。
    pub code: i32,
    /// 退出码。
    pub value: u16,
    /// 注解。
    pub note: Note,
    /// 查表答复档。
    pub hit: Hit,
    /// 链接形态（原样带出，供 `AV_ELINK` 那条纵线核）。
    pub link: Link,
    /// 横幅档。
    pub banner: Banner,
    /// 双流档。
    pub flows: Flows,
    /// 指路档。
    pub hint: Hint,
    /// 降级主行为齐备位（banner + errline + hint + 双流 + 127 五者同真）。
    pub default: bool,
    /// 定名结果。
    pub name: &'static str,
    /// 命中组名（未命中为空串）。
    pub group: &'static str,
    /// 可执行体路径（推不出为 `None`）。
    pub path: Option<&'static str>,
}

impl Outcome {
    /// 空结果（未定档的极小值）。
    pub const fn blank() -> Outcome {
        Outcome {
            stage: Stage::Argv0,
            code: ERR_EBADARG,
            value: EXIT_USAGE,
            note: Note::Sceneless,
            hit: Hit::Unregistered,
            link: Link::SymlinkAbs,
            banner: Banner::Suppressed,
            flows: Flows::Suppressed,
            hint: Hint::None,
            default: false,
            name: "",
            group: "",
            path: None,
        }
    }

    /// 是否"跑通"。
    ///
    /// 与 `Claim::success()` 同律（收尾段 + errno 零），这是 `Estate`
    /// 能判的前提。注意 `group-usage` 也满足它——进了组打清单确实"没出错"，
    /// 它只是没跑 applet；那一维由 `value` 与 `note` 分开报。
    pub fn ok(&self) -> bool {
        self.stage == Stage::Ready && self.code == ERR_NONE
    }

    /// 是否跑通了某个 applet（退出码 0）。
    pub fn ran(&self) -> bool {
        self.stage == Stage::Ready && self.value == EXIT_RUN_OK
    }

    /// 是否落降级面（未注册名/被裁件的出口）。
    pub fn degraded(&self) -> bool {
        self.stage == Stage::Ready && self.code == ERR_ENOENT && self.value == EXIT_DEGRADE
    }

    /// 是否三件套齐备。
    pub fn complete(&self) -> bool {
        self.default
    }

    /// 是否降级但缺件（半备）。
    pub fn half(&self) -> bool {
        self.stage == Stage::Ready && self.degrade_face() && !self.default
    }

    /// 是否降级面结果（齐备与否不论）。
    pub fn degrade_face(&self) -> bool {
        self.note.degrade_face()
    }

    /// 是否判死在输出面之前。
    pub fn undef(&self) -> bool {
        !self.stage.on_surface()
    }

    /// 是否带指路行。
    pub fn has_hint(&self) -> bool {
        self.hint.present()
    }

    /// 该结果的档位。
    pub fn tier(&self) -> Tier {
        Tier::of(self)
    }

    /// 结果指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.stage.order() as u64);
        h = fold_word(h, self.code as u64);
        h = fold_word(h, self.value as u64);
        h = fold_word(h, self.note.order() as u64);
        h = fold_word(h, self.hit.order() as u64);
        h = fold_word(h, self.link.order() as u64);
        h = fold_word(h, self.banner.order() as u64);
        h = fold_word(h, self.flows.order() as u64);
        h = fold_word(h, self.hint.order() as u64);
        h = fold_word(h, self.default as u64);
        h = fold_str(h, self.name);
        h = fold_str(h, self.group);
        match self.path {
            Some(p) => fold_str(h, p),
            None => fold_word(h, 0),
        }
    }
}

// ---------------------------------------------------------------------------
// 八闸推进
// ---------------------------------------------------------------------------

/// 走八闸，出一结果。
///
/// 闸序不可倒，每闸只做自己那一段的判据：
/// 闸一只管 `argv` 在场，闸二只管基名可推，闸三只管改形与本尊归位，
/// 闸四两跳定档，闸五定重启生死，闸六七八把降级面成形。
pub fn define(sh: &Shape) -> Outcome {
    let mut o = Outcome {
        stage: Stage::Argv0,
        code: ERR_EBADARG,
        value: EXIT_USAGE,
        note: Note::Sceneless,
        hit: Hit::Unregistered,
        link: sh.link,
        banner: Banner::Suppressed,
        flows: Flows::Suppressed,
        hint: Hint::None,
        default: false,
        name: "",
        group: "",
        path: None,
    };

    // 闸一：argv[0] 在场性。
    if sh.absent || sh.argc == 0 {
        return o;
    }

    // 闸二：基名可推。
    let r = sh.resolves();
    if !r.usable() {
        o.stage = Stage::Base;
        o.code = ERR_EINVAL;
        o.value = EXIT_USAGE;
        o.note = Note::Baseless;
        o.flows = Flows::OnlyStderr;
        return o;
    }
    let name = r.text();
    o.name = name;
    o.path = sh.path();

    // 闸三：定名与本尊归位。
    if has_at_prefix(name) && strip_at(name).is_empty() {
        o.stage = Stage::Name;
        o.code = ERR_EINVAL;
        o.value = EXIT_USAGE;
        o.note = Note::AtBare;
        o.flows = Flows::OnlyStderr;
        return o;
    }
    if sh.self_named() {
        o.stage = Stage::Name;
        o.code = ERR_NONE;
        o.value = EXIT_RUN_OK;
        o.note = Note::MainPath;
        o.banner = Banner::Full;
        o.flows = Flows::OnlyStdout;
        return o;
    }
    o.stage = Stage::Table;

    // 闸四：组表 + 成员表两跳。
    let mut member_only = false;
    match row_at(name) {
        Some(row) => {
            o.group = row.group;
            if !row.registered {
                o.hit = Hit::Trimmed;
            } else if row.is_alias() {
                o.hit = Hit::Alias;
            } else if row.individual {
                o.hit = Hit::Registered;
            } else {
                o.hit = Hit::Group;
                member_only = true;
            }
        }
        None => match group_at(name) {
            Some(g) => {
                o.hit = Hit::Group;
                o.group = g.name;
            }
            None => o.hit = Hit::Unregistered,
        },
    }
    o.stage = Stage::Exec;

    // 直通：个体名或别名就在表里且可直调。
    if o.hit.runnable() {
        o.stage = Stage::Ready;
        o.code = ERR_NONE;
        o.value = EXIT_RUN_OK;
        o.note = match o.hit {
            Hit::Alias => Note::AliasRan,
            _ => Note::Ran,
        };
        o.banner = Banner::Suppressed;
        o.flows = Flows::OnlyStdout;
        o.hint = Hint::None;
        return o;
    }

    // 闸五：组内件。组默认合并才有组调用入口，才谈得上重启进组。
    if o.hit.group_face() {
        let merged = match group_at(o.group) {
            Some(g) => g.merged_by_default,
            None => false,
        };
        if member_only && !merged {
            // 组入口不存在（coreutils 类默认拆开的组里被单独关掉个体名）：
            // 只能打该组的单组清单退 1，本卡给它 Banner::GroupOnly 档。
            o.stage = Stage::Ready;
            o.code = ERR_NONE;
            o.value = EXIT_GROUP_USAGE;
            o.note = Note::GroupUsage;
            o.banner = if sh.banner_off { Banner::Suppressed } else { Banner::GroupOnly };
            o.flows = sh.flows_plan();
            o.hint = if sh.hint_off { Hint::None } else { Hint::TryListFull };
            return o;
        }
        if !sh.link.reexec_allowed(sh.nommu) {
            o.stage = Stage::Exec;
            o.code = ERR_ENOSYS;
            o.value = EXIT_EXEC;
            o.note = Note::NoReexec;
            o.banner = Banner::Suppressed;
            o.flows = Flows::OnlyStderr;
            o.hint = Hint::None;
            return o;
        }
        if member_only {
            o.stage = Stage::Ready;
            o.code = ERR_NONE;
            o.value = EXIT_RUN_OK;
            o.note = Note::ReExecRun;
            o.banner = Banner::Suppressed;
            o.flows = Flows::OnlyStdout;
            o.hint = Hint::None;
            return o;
        }
        // 组名自调用：进了组没给成员——打组清单退 1，不是降级。
        o.stage = Stage::Ready;
        o.code = ERR_NONE;
        o.value = EXIT_GROUP_USAGE;
        o.note = Note::GroupUsage;
        o.banner = if sh.banner_off { Banner::Suppressed } else { Banner::Groups };
        o.flows = sh.flows_plan();
        o.hint = if sh.hint_off { Hint::None } else { Hint::TryGroup };
        return o;
    }

    // 降级主行为：闸六横幅 → 闸七双流 → 闸八齐备位。
    o.stage = Stage::Banner;
    o.code = ERR_ENOENT;
    o.value = EXIT_DEGRADE;
    o.banner = if sh.banner_off || sh.dropped_out { Banner::Suppressed } else { Banner::Full };
    o.hint = if sh.hint_off {
        Hint::None
    } else if o.hit.internal() {
        Hint::TryGroup
    } else if sh.keyed_with_dir() {
        Hint::TryListFull
    } else {
        Hint::TryList
    };

    o.stage = Stage::Stream;
    o.flows = sh.flows_plan();

    o.note = if o.flows.lost() {
        Note::FlowsOff
    } else if !o.banner.complete() {
        Note::Bannerless
    } else if !o.flows.two() {
        Note::FlowsOff
    } else if !o.hint.present() {
        Note::Hintless
    } else {
        Note::Degraded
    };

    o.stage = Stage::Ready;
    o.default = o.banner.complete() && o.flows.two() && o.hint.present();
    o
}

// ---------------------------------------------------------------------------
// 现场文案（逐字取自真 busybox 的措辞）
// ---------------------------------------------------------------------------

/// 横幅版本行。
const USAGE_VERSION: &str = "BusyBox v1.36.1 multi-call binary.\n";

/// 横幅版权段。
const USAGE_COPYRIGHT: &str = "BusyBox is copyrighted by many authors between 1998-2015.\n\
Licensed under GPLv2. See source distribution for detailed copyright notices.\n\n";

/// 横幅用法式四行（真 busybox 的四条 `or:` 形式）。
const USAGE_SYNOPSES: &str = "Usage: busybox [function [arguments]...]\n\
   or: busybox --list[-full]\n\
   or: busybox --install [-s] [DIR]\n\
   or: function [arguments]...\n\n";

/// 横幅说明段（制表符缩进，逐字照真机）。
const USAGE_BLURB: &str = "\tBusyBox is a multi-call binary that combines many common Unix\n\
\tutilities into a single executable.  Most people will create a\n\
\tlink to this executable for the utility name they want, and\n\
\tinvoking BusyBox will then perform the utility function.\n\n";

/// 清单引导行。
const FUNCTIONS_HEAD: &str = "Currently defined functions:\n";

/// 组清单引导行（`busybox <group>` 的现场用）。
const GROUPS_HEAD: &str = "Currently defined groups:\n";

/// notfound 行的逐字锚：程序名 + 冒号 + 引住的名字 + 固定尾巴，**无句号**。
const ERR_ANCHOR_LIT: &str = "busybox: 'foo': applet not found";

/// `--list` 的清单体：只列已进表的名字（含别名），主名在前。
pub fn list_dump(sep: &str) -> String {
    let mut out = String::new();
    let mut first = true;
    let r = table();
    let mut i = 0usize;
    while i < r.len() {
        if let Some(x) = r.get(i) {
            if x.registered {
                if !first {
                    out.push_str(sep);
                }
                first = false;
                out.push_str(x.name);
            }
        }
        i += 1;
    }
    out
}

/// `--list-full` 的清单体：一行一个 `<group> <name>`，含未注册件的标记。
pub fn list_full_dump() -> String {
    let mut out = String::new();
    let r = table();
    let mut i = 0usize;
    while i < r.len() {
        if let Some(x) = r.get(i) {
            if x.registered {
                out.push_str(x.render().as_str());
            }
        }
        i += 1;
    }
    out
}

/// notfound 行（不带换行的纯文本由 [`errline_of`] 负责，这里给完整一行）。
pub fn msg_applet_not_found(prog: &str, name: &str) -> String {
    let mut s = String::from(prog);
    s.push_str(": '");
    s.push_str(name);
    s.push_str("': applet not found\n");
    s
}

/// 指路行：`--list`。
pub fn msg_try_list() -> String {
    String::from("Run 'busybox --list' to find all built-in applets.\n")
}

/// 指路行：`--list-full`。
pub fn msg_try_list_full() -> String {
    String::from("Run 'busybox --list-full' to see applets with their group prefix.\n")
}

/// 指路行：组调用。
pub fn msg_try_group(group: &str, name: &str) -> String {
    let mut s = String::from("Run 'busybox ");
    s.push_str(group);
    s.push(' ');
    s.push_str(name);
    s.push_str("' to invoke it through its group.\n");
    s
}

/// 重启判死行。真机在无 NOMMU 的重启失败面上打 `can't re-exec`，
/// 后面跟可执行体路径；路径推不出时打 `(unknown)`。
pub fn msg_cant_reexec(prog: &str, path: &str) -> String {
    let mut s = String::from(prog);
    s.push_str(": can't re-exec '");
    if path.is_empty() {
        s.push_str("(unknown)");
    } else {
        s.push_str(path);
    }
    s.push_str("': Operation not supported\n");
    s
}

/// 名字推不出时的现场行（空 argv / 尾斜杠 / 裸 `@` 三式共用）。
pub fn msg_no_name(prog: &str) -> String {
    let mut s = String::from(prog);
    s.push_str(": can't determine applet name\n");
    s
}

/// 指路行的最终文本。
pub fn hint_text(h: Hint, group: &str, name: &str) -> String {
    match h {
        Hint::None => String::new(),
        Hint::TryList => msg_try_list(),
        Hint::TryListFull => msg_try_list_full(),
        Hint::TryGroup => msg_try_group(group, name),
    }
}

/// 完整主横幅（版本 + 版权 + 用法式 + 说明 + 清单）。
pub fn banner_full() -> String {
    let mut s = String::from(USAGE_VERSION);
    s.push_str(USAGE_COPYRIGHT);
    s.push_str(USAGE_SYNOPSES);
    s.push_str(USAGE_BLURB);
    s.push_str(FUNCTIONS_HEAD);
    s.push('\t');
    s.push_str(list_dump(", ").as_str());
    s.push('\n');
    s
}

/// 组清单横幅（组名自调用的现场）。
pub fn banner_groups() -> String {
    let mut s = String::from(USAGE_VERSION);
    s.push_str(USAGE_COPYRIGHT);
    s.push_str(GROUPS_HEAD);
    s.push('\t');
    s.push_str(group_name_list(", ").as_str());
    s.push('\n');
    s
}

/// 单组清单横幅（组入口不存在时的现场）。
pub fn banner_group_only(group: &str) -> String {
    let mut s = String::from(USAGE_VERSION);
    s.push_str("Usage: busybox <applet> [arguments]...\n\n");
    s.push_str(group);
    s.push_str(":\n\t");
    s.push_str(group_member_list(group, ", ").as_str());
    s.push('\n');
    s
}

/// 现场横幅的最终文本（按档位选版面）。
pub fn banner_text(o: &Outcome) -> String {
    if !o.banner.shows_list() {
        return String::new();
    }
    match o.banner {
        Banner::Full => banner_full(),
        Banner::Groups => banner_groups(),
        Banner::GroupOnly => banner_group_only(o.group),
        Banner::Suppressed => String::new(),
    }
}

/// 现场的错行（降级/判死面的那一条）。
pub fn errline_of(sh: &Shape, o: &Outcome) -> String {
    let prog = sh.prog();
    match o.note {
        Note::Degraded | Note::Bannerless | Note::FlowsOff | Note::Hintless => {
            msg_applet_not_found(prog, o.name)
        }
        Note::NoReexec => match o.path {
            Some(p) => msg_cant_reexec(prog, p),
            None => msg_cant_reexec(prog, ""),
        },
        Note::Sceneless | Note::Baseless | Note::AtBare => msg_no_name(prog),
        _ => String::new(),
    }
}

/// 指路行里该填的成员名。
///
/// 组名自调用（`/bin/coreutils -> busybox`）的现场缺的是**成员**，不是名字
/// 本身：把组名自己填回去会长成 `Run 'busybox coreutils coreutils'`，那是
/// 一条把用户引向死路的指路行。真机在这种现场给的是占位成员名，故这里
/// 组档填 `<applet>`；被裁件（`Trimmed`）填真实名字——它要的是"这个名字
/// 在组里怎么调"。
pub fn hint_member(o: &Outcome) -> &'static str {
    match o.hit.group_face() {
        true => "<applet>",
        false => o.name,
    }
}

/// 整块输出面：横幅 + 错行 + 指路，按双流档过滤。
///
/// 这是本卡的"现场"——F11661 的现场是一行，本卡的现场是这一块。
/// 双流档里 `OnlyStdout` 丢错行、`OnlyStderr` 丢横幅、`Merged`/`Suppressed`
/// 按重定向面处理：并流保留顺序（两块都出），全丢则空。
pub fn message(sh: &Shape, o: &Outcome) -> String {
    if o.flows.lost() {
        return String::new();
    }
    let mut s = String::new();
    if o.flows.banner_visible() {
        s.push_str(banner_text(o).as_str());
    }
    if o.flows.err_visible() {
        s.push_str(errline_of(sh, o).as_str());
    }
    if o.hint.present() && !o.flows.lost() {
        s.push_str(hint_text(o.hint, o.group, hint_member(o)).as_str());
    }
    s
}

// ---------------------------------------------------------------------------
// 申报与用例
// ---------------------------------------------------------------------------

/// 一槽申报。
///
/// 申报面只有判序那六维（阶段/errno/退出码/注解/齐备位 + 由前三维推得的
/// 成败）——**链接形态、横幅档、双流档、指路档都不在申报面里**。这是本卡
/// 与 F11661 的又一处分野：那四维是现场属性，申报器无权改口，改了就落
/// `AV_ELINK`/`AV_ESEAL`，由 `Shape::seal` 与 `Outcome::seal` 对拍。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Claim {
    /// 结束阶段。
    pub stage: Stage,
    /// errno。
    pub code: i32,
    /// 退出码。
    pub value: u16,
    /// 注解。
    pub note: Note,
    /// 降级主行为齐备位。
    pub default: bool,
}

impl Claim {
    /// 空白申报（申报器什么都不记得时的初值——跑通档）。
    pub const fn blank() -> Claim {
        Claim { stage: Stage::Ready, code: ERR_NONE, value: EXIT_RUN_OK, note: Note::Ran, default: false }
    }

    /// 是否申报"跑通了"。
    ///
    /// 与 [`Outcome::ok`] 同律（收尾段 + errno 零），`Estate` 判据才成立。
    pub fn success(&self) -> bool {
        self.stage == Stage::Ready && self.code == ERR_NONE
    }
}

/// 一槽用例。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Case {
    /// 现场形状。
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

    /// 现场档位。
    pub fn tier(&self) -> Tier {
        Tier::of(&self.actual())
    }
}

// ---------------------------------------------------------------------------
// 语料（14 槽）
// ---------------------------------------------------------------------------

/// 8 个可正确申报的现场 + 6 个"同一档答复、六种误报"的现场。
///
/// 前 8 槽把 3 档放行（个体名直调 / 别名 / 重启进组）、3 档完整降级
/// （未注册名 / 被裁件 / `@` 改形）、1 档组名自调用、1 档 NOMMU 重启判死
/// 全部走齐。后 6 槽形状互不相同（链接形态占四式、名字各不相同、指路档
/// 分 `--list` 与 `--list-full`），实际结果却逐位相同——这正是本卡的
/// 红档显式：一整套降级主行为能被六条不同的路报歪。
pub const SHAPES: [Shape; CORPUS] = [
    Shape {
        tag: "link-abs-run-ls",
        argv0: "/bin/ls",
        argv1: "-l",
        argc: 2,
        absent: false,
        link: Link::SymlinkAbs,
        nommu: false,
        banner_off: false,
        hint_off: false,
        merged_out: false,
        dropped_out: false,
    },
    Shape {
        tag: "hardlink-run-ash",
        argv0: "ash",
        argv1: "",
        argc: 1,
        absent: false,
        link: Link::Hardlink,
        nommu: false,
        banner_off: false,
        hint_off: false,
        merged_out: false,
        dropped_out: false,
    },
    Shape {
        tag: "reexec-run-vi",
        argv0: "vi",
        argv1: "",
        argc: 1,
        absent: false,
        link: Link::ExecArgv0,
        nommu: false,
        banner_off: false,
        hint_off: false,
        merged_out: false,
        dropped_out: false,
    },
    Shape {
        tag: "link-rel-unreg-foo",
        argv0: "foo",
        argv1: "",
        argc: 1,
        absent: false,
        link: Link::SymlinkRel,
        nommu: false,
        banner_off: false,
        hint_off: false,
        merged_out: false,
        dropped_out: false,
    },
    Shape {
        tag: "link-abs-trimmed-ed",
        argv0: "/bin/ed",
        argv1: "",
        argc: 1,
        absent: false,
        link: Link::SymlinkAbs,
        nommu: false,
        banner_off: false,
        hint_off: false,
        merged_out: false,
        dropped_out: false,
    },
    Shape {
        tag: "retitle-unreg-zzz",
        argv0: "@zzz",
        argv1: "zzz",
        argc: 2,
        absent: false,
        link: Link::ExecArgv0,
        nommu: false,
        banner_off: false,
        hint_off: false,
        merged_out: false,
        dropped_out: false,
    },
    Shape {
        tag: "group-name-usage",
        argv0: "coreutils",
        argv1: "",
        argc: 1,
        absent: false,
        link: Link::SymlinkRel,
        nommu: false,
        banner_off: false,
        hint_off: false,
        merged_out: false,
        dropped_out: false,
    },
    Shape {
        tag: "nommu-copy-noexec",
        argv0: "/tmp/vi",
        argv1: "",
        argc: 1,
        absent: false,
        link: Link::RenamedCopy,
        nommu: true,
        banner_off: false,
        hint_off: false,
        merged_out: false,
        dropped_out: false,
    },
    Shape {
        tag: "estate-swap",
        argv0: "/bin/nope",
        argv1: "-l",
        argc: 2,
        absent: false,
        link: Link::SymlinkAbs,
        nommu: false,
        banner_off: false,
        hint_off: false,
        merged_out: false,
        dropped_out: false,
    },
    Shape {
        tag: "stage-swap",
        argv0: "ghost",
        argv1: "",
        argc: 1,
        absent: false,
        link: Link::SymlinkRel,
        nommu: false,
        banner_off: false,
        hint_off: false,
        merged_out: false,
        dropped_out: false,
    },
    Shape {
        tag: "code-swap",
        argv0: "/usr/sbin/phantom",
        argv1: "",
        argc: 1,
        absent: false,
        link: Link::Hardlink,
        nommu: false,
        banner_off: false,
        hint_off: false,
        merged_out: false,
        dropped_out: false,
    },
    Shape {
        tag: "value-swap",
        argv0: "@gone",
        argv1: "gone",
        argc: 2,
        absent: false,
        link: Link::ExecArgv0,
        nommu: false,
        banner_off: false,
        hint_off: false,
        merged_out: false,
        dropped_out: false,
    },
    Shape {
        tag: "note-swap",
        argv0: "/bin/missing",
        argv1: "",
        argc: 1,
        absent: false,
        link: Link::Shebang,
        nommu: false,
        banner_off: false,
        hint_off: false,
        merged_out: false,
        dropped_out: false,
    },
    Shape {
        tag: "default-swap",
        argv0: "/tmp/copied",
        argv1: "",
        argc: 1,
        absent: false,
        link: Link::RenamedCopy,
        nommu: false,
        banner_off: false,
        hint_off: false,
        merged_out: false,
        dropped_out: false,
    },
];

/// 与 `SHAPES` 一一对应的申报。
pub const CLAIMS: [Claim; CORPUS] = [
    Claim { stage: Stage::Ready, code: ERR_NONE, value: EXIT_RUN_OK, note: Note::Ran, default: false },
    Claim {
        stage: Stage::Ready,
        code: ERR_NONE,
        value: EXIT_RUN_OK,
        note: Note::AliasRan,
        default: false,
    },
    Claim {
        stage: Stage::Ready,
        code: ERR_NONE,
        value: EXIT_RUN_OK,
        note: Note::ReExecRun,
        default: false,
    },
    Claim {
        stage: Stage::Ready,
        code: ERR_ENOENT,
        value: EXIT_DEGRADE,
        note: Note::Degraded,
        default: true,
    },
    Claim {
        stage: Stage::Ready,
        code: ERR_ENOENT,
        value: EXIT_DEGRADE,
        note: Note::Degraded,
        default: true,
    },
    Claim {
        stage: Stage::Ready,
        code: ERR_ENOENT,
        value: EXIT_DEGRADE,
        note: Note::Degraded,
        default: true,
    },
    Claim {
        stage: Stage::Ready,
        code: ERR_NONE,
        value: EXIT_GROUP_USAGE,
        note: Note::GroupUsage,
        default: false,
    },
    Claim {
        stage: Stage::Exec,
        code: ERR_ENOSYS,
        value: EXIT_EXEC,
        note: Note::NoReexec,
        default: false,
    },
    Claim { stage: Stage::Ready, code: ERR_NONE, value: EXIT_RUN_OK, note: Note::Ran, default: false },
    Claim {
        stage: Stage::Table,
        code: ERR_ENOENT,
        value: EXIT_DEGRADE,
        note: Note::Degraded,
        default: true,
    },
    Claim {
        stage: Stage::Ready,
        code: ERR_ENOSYS,
        value: EXIT_DEGRADE,
        note: Note::Degraded,
        default: true,
    },
    Claim {
        stage: Stage::Ready,
        code: ERR_ENOENT,
        value: EXIT_APPLET_FAIL,
        note: Note::Degraded,
        default: true,
    },
    Claim {
        stage: Stage::Ready,
        code: ERR_ENOENT,
        value: EXIT_DEGRADE,
        note: Note::GroupUsage,
        default: true,
    },
    Claim {
        stage: Stage::Ready,
        code: ERR_ENOENT,
        value: EXIT_DEGRADE,
        note: Note::Degraded,
        default: false,
    },
];

/// 后 6 槽的期望缺陷。
pub const REJECT_FAULTS: [Fault; REJECT] =
    [Fault::Estate, Fault::Stage, Fault::Code, Fault::Value, Fault::Note, Fault::Default];

/// 后 6 槽的缺陷码。
pub const REJECT_CODES: [u32; REJECT] = [
    AV_ESTATE,
    AV_ESTAGE,
    AV_ECODE,
    AV_EVALUE,
    AV_ENOTE,
    AV_EDEFAULT,
];

/// 账本 8 个成功格的值（总数、成、败、放行、降级、半备、未定、红档率‰）。
pub const VALS: [u32; ACCEPT] = [14, 8, 6, 3, 3, 1, 1, 929];

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
        return Err(AV_ENOBS);
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
/// 判序固定为 Estate → Stage → Code → Value → Note → Default。
/// Estate 最先的理由与 F11661 同，但本卡更硬：降级主行为的现场是"一整块
/// 输出"，把 127 现场报成跑通是最贵的一类误报——它会让审计方以为链接名
/// 全都装对了。归因链必须先斩这条，才谈得上"哪一段观测歪了"。
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
///
/// 一行写完现场与结果，含本卡独有的四个输出面档（`banner`/`flows`/`hint`
/// /`hit`）；错行原文按引号包住附在行尾，供 `message_ok` 逐字对拍。
pub fn detail(c: &Case) -> String {
    let o = c.actual();
    let err = errline_of(&c.shape, &o);
    let mut s = String::from(c.shape.prog());
    s.push_str("/");
    s.push_str(o.name);
    s.push_str(" stage=");
    s.push_str(o.stage.name());
    s.push_str(" code=");
    s.push_str(i32_text(o.code).as_str());
    s.push_str(" exit=");
    s.push_str(u32_text(o.value as u32).as_str());
    s.push_str(" ready=");
    s.push_str(bool_text(o.default));
    s.push_str(" note=");
    s.push_str(o.note.name());
    s.push_str(" hit=");
    s.push_str(o.hit.name());
    s.push_str(" link=");
    s.push_str(o.link.name());
    s.push_str(" banner=");
    s.push_str(o.banner.name());
    s.push_str(" flows=");
    s.push_str(o.flows.name());
    s.push_str(" hint=");
    s.push_str(o.hint.name());
    if !err.is_empty() {
        s.push_str(" err=\"");
        s.push_str(err.as_str());
        s.push('"');
    }
    s.push('\n');
    s
}

// ---------------------------------------------------------------------------
// 期望表（语料的第二份陈述）
// ---------------------------------------------------------------------------

/// 14 槽的期望结束阶段。
///
/// 这份表把 `SHAPES`/`CLAIMS` 用**另一种形式**再写一遍：申报表说
/// "我声明什么"，期望表说"八闸实际走到哪一步、答复落在哪一档"。任一处
/// 笔误都会在这里露出来。
const WANT_STAGE: [Stage; CORPUS] = [
    Stage::Ready,
    Stage::Ready,
    Stage::Ready,
    Stage::Ready,
    Stage::Ready,
    Stage::Ready,
    Stage::Ready,
    Stage::Exec,
    Stage::Ready,
    Stage::Ready,
    Stage::Ready,
    Stage::Ready,
    Stage::Ready,
    Stage::Ready,
];

/// 14 槽的期望 errno。
const WANT_CODE: [i32; CORPUS] = [
    ERR_NONE,
    ERR_NONE,
    ERR_NONE,
    ERR_ENOENT,
    ERR_ENOENT,
    ERR_ENOENT,
    ERR_NONE,
    ERR_ENOSYS,
    ERR_ENOENT,
    ERR_ENOENT,
    ERR_ENOENT,
    ERR_ENOENT,
    ERR_ENOENT,
    ERR_ENOENT,
];

/// 14 槽的期望退出码。
const WANT_VALUE: [u16; CORPUS] = [
    EXIT_RUN_OK,
    EXIT_RUN_OK,
    EXIT_RUN_OK,
    EXIT_DEGRADE,
    EXIT_DEGRADE,
    EXIT_DEGRADE,
    EXIT_GROUP_USAGE,
    EXIT_EXEC,
    EXIT_DEGRADE,
    EXIT_DEGRADE,
    EXIT_DEGRADE,
    EXIT_DEGRADE,
    EXIT_DEGRADE,
    EXIT_DEGRADE,
];

/// 14 槽的期望注解。
const WANT_NOTE: [Note; CORPUS] = [
    Note::Ran,
    Note::AliasRan,
    Note::ReExecRun,
    Note::Degraded,
    Note::Degraded,
    Note::Degraded,
    Note::GroupUsage,
    Note::NoReexec,
    Note::Degraded,
    Note::Degraded,
    Note::Degraded,
    Note::Degraded,
    Note::Degraded,
    Note::Degraded,
];

/// 14 槽的期望齐备位。
const WANT_DEFAULT: [bool; CORPUS] = [
    false, false, false, true, true, true, false, false, true, true, true, true, true, true,
];

/// 14 槽标签。
const WANT_TAGS: [&str; CORPUS] = [
    "link-abs-run-ls",
    "hardlink-run-ash",
    "reexec-run-vi",
    "link-rel-unreg-foo",
    "link-abs-trimmed-ed",
    "retitle-unreg-zzz",
    "group-name-usage",
    "nommu-copy-noexec",
    "estate-swap",
    "stage-swap",
    "code-swap",
    "value-swap",
    "note-swap",
    "default-swap",
];

/// 14 槽的期望档位。
const WANT_TIER: [Tier; CORPUS] = [
    Tier::Pass,
    Tier::Pass,
    Tier::Pass,
    Tier::Degrade,
    Tier::Degrade,
    Tier::Degrade,
    Tier::Partial,
    Tier::Undef,
    Tier::Degrade,
    Tier::Degrade,
    Tier::Degrade,
    Tier::Degrade,
    Tier::Degrade,
    Tier::Degrade,
];

/// 14 槽的期望查表答复档。
const WANT_HIT: [Hit; CORPUS] = [
    Hit::Registered,
    Hit::Alias,
    Hit::Group,
    Hit::Unregistered,
    Hit::Trimmed,
    Hit::Unregistered,
    Hit::Group,
    Hit::Group,
    Hit::Unregistered,
    Hit::Unregistered,
    Hit::Unregistered,
    Hit::Unregistered,
    Hit::Unregistered,
    Hit::Unregistered,
];

/// 14 槽的期望横幅档。
const WANT_BANNER: [Banner; CORPUS] = [
    Banner::Suppressed,
    Banner::Suppressed,
    Banner::Suppressed,
    Banner::Full,
    Banner::Full,
    Banner::Full,
    Banner::Groups,
    Banner::Suppressed,
    Banner::Full,
    Banner::Full,
    Banner::Full,
    Banner::Full,
    Banner::Full,
    Banner::Full,
];

/// 14 槽的期望双流档。
const WANT_FLOWS: [Flows; CORPUS] = [
    Flows::OnlyStdout,
    Flows::OnlyStdout,
    Flows::OnlyStdout,
    Flows::Two,
    Flows::Two,
    Flows::Two,
    Flows::Two,
    Flows::OnlyStderr,
    Flows::Two,
    Flows::Two,
    Flows::Two,
    Flows::Two,
    Flows::Two,
    Flows::Two,
];

/// 14 槽的期望指路档。
const WANT_HINT: [Hint; CORPUS] = [
    Hint::None,
    Hint::None,
    Hint::None,
    Hint::TryList,
    Hint::TryGroup,
    Hint::TryList,
    Hint::TryGroup,
    Hint::None,
    Hint::TryListFull,
    Hint::TryList,
    Hint::TryList,
    Hint::TryList,
    Hint::TryListFull,
    Hint::TryListFull,
];

/// 14 槽的期望定名。
const WANT_NAME: [&str; CORPUS] = [
    "ls",
    "ash",
    "vi",
    "foo",
    "ed",
    "zzz",
    "coreutils",
    "vi",
    "nope",
    "ghost",
    "phantom",
    "gone",
    "missing",
    "copied",
];

/// 14 槽的期望链接形态。
const WANT_LINK: [Link; CORPUS] = [
    Link::SymlinkAbs,
    Link::Hardlink,
    Link::ExecArgv0,
    Link::SymlinkRel,
    Link::SymlinkAbs,
    Link::ExecArgv0,
    Link::SymlinkRel,
    Link::RenamedCopy,
    Link::SymlinkAbs,
    Link::SymlinkRel,
    Link::Hardlink,
    Link::ExecArgv0,
    Link::Shebang,
    Link::RenamedCopy,
];

/// 八格申报正确的下标。
const ACCEPT_SLOTS: [usize; ACCEPT] = [0usize, 1, 2, 3, 4, 5, 6, 7];

/// 六格申报错误的下标。
const REJECT_SLOTS: [usize; REJECT] = [8usize, 9, 10, 11, 12, 13];

/// 账本 14 格的逐格期望（`build_ledger` 的第二份陈述）。
///
/// 这份表把建账函数用**逐格字面量**再写一遍：前 8 格是 `run()`/注入跑的
/// 统计数（第 8 格固定是红档率 929），后 6 格是六个缺陷码。哪一格与
/// `build_ledger` 对不上，`ledger_ok` 就在哪一格露红——账本只按长度对账
/// 是不够的，逐格才是。
const WANT_CELLS: [Sample; SAMPLES] = [
    Sample { slot: 0, op: OP_CORPUS, ok: true, code: 0, value: 14 },
    Sample { slot: 1, op: OP_CORPUS, ok: true, code: 0, value: 8 },
    Sample { slot: 2, op: OP_CORPUS, ok: true, code: 0, value: 6 },
    Sample { slot: 3, op: OP_EXEC, ok: true, code: 0, value: 3 },
    Sample { slot: 4, op: OP_OUTPUT, ok: true, code: 0, value: 3 },
    Sample { slot: 5, op: OP_TABLE, ok: true, code: 0, value: 1 },
    Sample { slot: 6, op: OP_ARGV0, ok: true, code: 0, value: 1 },
    Sample { slot: 7, op: OP_REPLAY, ok: true, code: 0, value: 929 },
    Sample { slot: 8, op: OP_NAME, ok: false, code: AV_ESTATE, value: 0 },
    Sample { slot: 9, op: OP_NAME, ok: false, code: AV_ESTAGE, value: 0 },
    Sample { slot: 10, op: OP_NAME, ok: false, code: AV_ECODE, value: 0 },
    Sample { slot: 11, op: OP_NAME, ok: false, code: AV_EVALUE, value: 0 },
    Sample { slot: 12, op: OP_NAME, ok: false, code: AV_ENOTE, value: 0 },
    Sample { slot: 13, op: OP_NAME, ok: false, code: AV_EDEFAULT, value: 0 },
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

    /// 该档是否为空行（矩阵不平的依据之一）。
    pub fn empty(&self) -> bool {
        self.total == 0
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
/// 分档只看**实际结果**落在哪一档，申报正确与否单独计。本卡的六格复刻现场
/// 全部落 degrade 档，于是 degrade 行必然出现 `total=9 good=3 red=6`：
/// 覆盖面看着满（三槽真降级 + 六槽假申报降级），红档却挤在同一行——这比
/// F11661 的 anchor 行更极端（那卡 8 槽，本卡 9 槽），理由是"降级主行为
/// 齐备"这一档天然吸走所有误报。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Matrix {
    /// 行（pass/degrade/partial/undef）。
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

    /// 空档数（本卡语料覆盖四档，应为 0）。
    pub fn empty_rows(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(r) = self.rows.get(i) {
                if r.empty() {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 红档总数。
    pub fn reds(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(r) = self.rows.get(i) {
                n += r.red();
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

    /// 红档是否全部压在 degrade 行（本卡的显式判据）。
    pub fn reds_on_degrade_row(&self) -> bool {
        match self.row(Tier::Degrade) {
            Some(r) => r.red() == REJECT && self.reds() == REJECT,
            None => false,
        }
    }

    /// 渲染全矩阵。
    pub fn render(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < TIERS.len() {
            if let Some(t) = TIERS.get(i) {
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Report {
    /// 总槽数。
    pub total: u32,
    /// 申报正确数。
    pub accept: u32,
    /// 申报错误数。
    pub reject: u32,
    /// 放行档正确数。
    pub pass: u32,
    /// 完整降级档正确数。
    pub degrade: u32,
    /// 半备档正确数。
    pub partial: u32,
    /// 未定档正确数。
    pub undef: u32,
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
            Err(AV_ECOUNT)
        }
    }

    /// 成功档合计（申报正确且跑通）。
    pub fn success_cells(&self) -> u32 {
        self.pass
    }

    /// 失败档合计（申报正确但没跑通）。
    pub fn failure_cells(&self) -> u32 {
        self.degrade + self.partial + self.undef
    }

    /// 走到输出面的正确格占比（百分数，整型）——降级 + 半备两档。
    pub fn surface_share(&self) -> u32 {
        if self.accept == 0 {
            return 0;
        }
        ((self.degrade + self.partial) * 100) / self.accept
    }

    /// 完整降级主行为占申报正确格的比例（百分数，整型）。
    pub fn degrade_share(&self) -> u32 {
        if self.accept == 0 {
            return 0;
        }
        (self.degrade * 100) / self.accept
    }

    /// 渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("argv0 corpus=");
        s.push_str(u32_text(self.total).as_str());
        s.push_str(" accept=");
        s.push_str(u32_text(self.accept).as_str());
        s.push_str(" reject=");
        s.push_str(u32_text(self.reject).as_str());
        s.push_str(" pass=");
        s.push_str(u32_text(self.pass).as_str());
        s.push_str(" degrade=");
        s.push_str(u32_text(self.degrade).as_str());
        s.push_str(" partial=");
        s.push_str(u32_text(self.partial).as_str());
        s.push_str(" undef=");
        s.push_str(u32_text(self.undef).as_str());
        s.push_str(" match=");
        s.push_str(u32_text(self.matched).as_str());
        s.push_str(" rate=");
        s.push_str(u32_text(self.rate()).as_str());
        s.push_str("ppm\n");
        s
    }
}

/// 全绿渲染文本（逐字钉住）。
const RENDER_OK: &str =
    "argv0 corpus=14 accept=8 reject=6 pass=3 degrade=3 partial=1 undef=1 match=14 rate=1000ppm\n";

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
            return Err(AV_ESLOT);
        }
    }
    let cs = cases();
    let mut total = 0u32;
    let mut accept = 0u32;
    let mut reject = 0u32;
    let mut pass = 0u32;
    let mut degrade = 0u32;
    let mut partial = 0u32;
    let mut undef = 0u32;
    let mut matched = 0u32;
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return Err(AV_ENOBS),
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
            None => return Err(AV_ENOBS),
        };
        total += 1;
        if f == want {
            matched += 1;
        }
        if f == Fault::Ok {
            accept += 1;
            match Tier::of(&a) {
                Tier::Pass => pass += 1,
                Tier::Degrade => degrade += 1,
                Tier::Partial => partial += 1,
                Tier::Undef => undef += 1,
            }
        } else {
            reject += 1;
        }
        i += 1;
    }
    Ok(Report { total, accept, reject, pass, degrade, partial, undef, matched })
}

// ---------------------------------------------------------------------------
// 阶段闸门
// ---------------------------------------------------------------------------

/// 单调阶段闸门。
///
/// 判定纵线 `argv0 → base → name → table → exec → banner → stream → ready`
/// 八段只许向前。回退（例如已经判死在 `exec` 又声称回到 `table` 复核组名）
/// 说明申报者没有按闸门次序观测，而是事后倒推。本卡对这一条比 F11661 更
/// 敏感：`banner`/`stream` 两段观测的是**输出成形之后**的事实，一旦允许
/// 回退，"先看到 127 再补一条横幅"就成了合法申报。
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
            return Err(AV_ESTATE);
        }
        let o = s.order() as i32;
        if o < self.last {
            return Err(AV_ESTAGE);
        }
        self.last = o;
        self.steps = self.steps.saturating_add(1);
        Ok(())
    }

    /// 关闭，返回段数。
    pub fn close(&mut self) -> Result<u32, u32> {
        if self.closed {
            return Err(AV_ESTATE);
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
    /// 槽位（必须严格等于当前长度才可登记）。
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

    /// 格指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.slot as u64);
        h = fold_word(h, self.op as u64);
        h = fold_word(h, self.ok as u64);
        h = fold_word(h, self.code as u64);
        fold_word(h, self.value as u64)
    }
}

/// 单次观测的槽位上限（超出即视为伪造现场）。
pub const CAP: usize = 6;

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
        Ledger { rows: [Sample::blank(); SAMPLES], len: 0, root: 0, frozen: false, rejects: 0 }
    }

    /// 登记一格；槽位不等于当前长度、越界或已冻结一律拒绝并计数。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(AV_ESLOT);
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

    /// 逐格对账：槽位连续、成格值逐格等于 `VALS`、败格码逐格等于
    /// `REJECT_CODES` 且败格值必须为 0。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != ACCEPT || self.err_count() != REJECT {
            return Err(AV_EBAL);
        }
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(AV_EBAL),
            };
            if s.slot as usize != i {
                return Err(AV_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(AV_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(AV_EVALUE);
                        }
                    }
                    None => return Err(AV_EBAL),
                }
                vi += 1;
            } else {
                if s.value != 0 {
                    return Err(AV_EVALUE);
                }
                match REJECT_CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(AV_EVALUE);
                        }
                    }
                    None => return Err(AV_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != ACCEPT || ci != REJECT {
            return Err(AV_EBAL);
        }
        Ok(())
    }

    /// 全账指纹。
    ///
    /// 只折**账目本身**（逐格五字段 + 格数）。`rejects` 是探针计数，不是证据：
    /// 把它折进来，任何一次"试着多登记一格"的探测都会把冻结指纹弄脏，
    /// 于是 `replay()` 永远复现不出 `root`——重放判据就自相矛盾了。
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
        fold_word(h, self.len as u64)
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(AV_EBAL);
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
            Err(AV_ESEAL)
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

/// 建账本：7 格统计 + 1 格红档率 + 6 格缺陷码。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let r = run();
    let _ = l.register(count_cell(0, OP_CORPUS, r.map(|x| x.total)));
    let _ = l.register(count_cell(1, OP_CORPUS, r.map(|x| x.accept)));
    let _ = l.register(count_cell(2, OP_CORPUS, r.map(|x| x.reject)));
    let _ = l.register(count_cell(3, OP_EXEC, r.map(|x| x.pass)));
    let _ = l.register(count_cell(4, OP_OUTPUT, r.map(|x| x.degrade)));
    let _ = l.register(count_cell(5, OP_TABLE, r.map(|x| x.partial)));
    let _ = l.register(count_cell(6, OP_ARGV0, r.map(|x| x.undef)));

    let red = run_with(Some((0, Fault::Code)));
    let _ = l.register(count_cell(7, OP_REPLAY, red.map(|x| x.rate())));

    let cs = cases();
    let mut i = 0usize;
    while i < REJECT {
        if let Some(c) = cs.get(i + ACCEPT) {
            let f = judge(c);
            let _ = l.register(Sample {
                slot: (ACCEPT + i) as u8,
                op: OP_NAME,
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

/// 四路注入：表路 / 判路 / 渲染路 / 账本路。
///
/// 1. 表路——段基址、枚举、链接形态规则、组表与成员表；
/// 2. 判路——八闸纵线、横幅与双流、对判、矩阵；
/// 3. 渲染路——语料、期望表、红档、文案、释义；
/// 4. 账本路——阶段闸门、逐格对账、冻结重放。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = name_ok() && enums_ok() && link_ok() && table_ok();
    out[1] = define_ok() && banner_ok() && judge_ok() && matrix_ok();
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
// 判据基元
// ---------------------------------------------------------------------------

/// 表路一：段基址、错误码阶梯、基数、现场常量。
fn name_ok() -> bool {
    if SEG_BASE != 0x6d110 {
        return false;
    }
    if AV_ESTAGE != SEG_BASE + 0x01
        || AV_ECODE != SEG_BASE + 0x02
        || AV_EVALUE != SEG_BASE + 0x03
        || AV_ENOTE != SEG_BASE + 0x04
        || AV_EDEFAULT != SEG_BASE + 0x05
    {
        return false;
    }
    if AV_ELINK != SEG_BASE + 0x06
        || AV_ESTATE != SEG_BASE + 0x07
        || AV_ECOUNT != SEG_BASE + 0x08
        || AV_ESLOT != SEG_BASE + 0x09
    {
        return false;
    }
    if AV_ESEAL != SEG_BASE + 0x0a
        || AV_ENOBS != SEG_BASE + 0x0b
        || AV_EMATRIX != SEG_BASE + 0x0c
        || AV_EBAL != SEG_BASE + 0x0d
    {
        return false;
    }
    if SEG_BASE % 16 != 0 {
        return false;
    }
    if SAMPLES != CORPUS || CORPUS != 14 || ACCEPT != 8 || REJECT != 6 {
        return false;
    }
    if ACCEPT + REJECT != CORPUS {
        return false;
    }
    if TIER_COUNT != TIERS.len() {
        return false;
    }
    if SAMPLES < CAP + ACCEPT || CAP as usize != REJECT {
        return false;
    }
    if GROUP_COUNT != GROUPS.len() || TABLE_COUNT != table().len() {
        return false;
    }
    if SELF_NAME_COUNT != SELF_NAMES.len() || APPLET_NAME_MAX < 30 {
        return false;
    }
    if ERR_ENOENT != 2 || ERR_ENOSYS != 38 || ERR_EINVAL != 22 {
        return false;
    }
    if ERR_EBADARG != -1 || ERR_NONE != 0 {
        return false;
    }
    if EXIT_RUN_OK != 0 || EXIT_USAGE != 1 || EXIT_APPLET_FAIL != EXIT_USAGE {
        return false;
    }
    if EXIT_GROUP_USAGE != 1 || EXIT_DEGRADE != 127 || EXIT_EXEC != 126 {
        return false;
    }
    if EXIT_DEGRADE <= EXIT_EXEC || EXIT_EXEC <= EXIT_USAGE {
        return false;
    }
    if OP_ARGV0 != 1 || OP_BASE != 2 || OP_NAME != 3 || OP_TABLE != 4 {
        return false;
    }
    if OP_EXEC != 5 || OP_OUTPUT != 6 || OP_CORPUS != 7 || OP_REPLAY != 8 {
        return false;
    }
    let ops = [OP_ARGV0, OP_BASE, OP_NAME, OP_TABLE, OP_EXEC, OP_OUTPUT, OP_CORPUS, OP_REPLAY];
    let mut i = 0usize;
    while i < ops.len() {
        if let Some(o) = ops.get(i) {
            if *o == 0 {
                return false;
            }
            let mut j = i + 1;
            while j < ops.len() {
                if let Some(p) = ops.get(j) {
                    if p == o {
                        return false;
                    }
                }
                j += 1;
            }
        }
        i += 1;
    }
    let codes = [
        AV_ESTAGE,
        AV_ECODE,
        AV_EVALUE,
        AV_ENOTE,
        AV_EDEFAULT,
        AV_ELINK,
        AV_ESTATE,
        AV_ECOUNT,
        AV_ESLOT,
        AV_ESEAL,
        AV_ENOBS,
        AV_EMATRIX,
        AV_EBAL,
    ];
    let mut i = 0usize;
    while i < codes.len() {
        if let Some(a) = codes.get(i) {
            if *a <= SEG_BASE || *a > SEG_BASE + 0x0d {
                return false;
            }
            let mut j = i + 1;
            while j < codes.len() {
                if let Some(b) = codes.get(j) {
                    if a == b {
                        return false;
                    }
                }
                j += 1;
            }
        }
        i += 1;
    }
    true
}

/// 表路二：六条纵线的名字、次序、互斥位与错误码对齐。
fn enums_ok() -> bool {
    if STAGES.len() != 8 {
        return false;
    }
    let stage_names = [
        "argv0",
        "base",
        "name",
        "table",
        "exec",
        "banner",
        "stream",
        "ready",
    ];
    let mut i = 0usize;
    while i < STAGES.len() {
        let s = match STAGES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if s.order() as usize != i {
            return false;
        }
        match stage_names.get(i) {
            Some(n) => {
                if s.name() != *n {
                    return false;
                }
            }
            None => return false,
        }
        if s.on_surface() != (s.order() >= Stage::Banner.order()) {
            return false;
        }
        if s.sceneless() != (s.order() <= Stage::Name.order()) {
            return false;
        }
        i += 1;
    }
    if Stage::Argv0.on_surface() || !Stage::Banner.on_surface() || !Stage::Ready.on_surface() {
        return false;
    }
    if Stage::Table.sceneless() || Stage::Exec.sceneless() {
        return false;
    }

    if HITS.len() != 5 {
        return false;
    }
    let hit_names = ["registered", "alias", "group", "trimmed", "unregistered"];
    let mut i = 0usize;
    while i < HITS.len() {
        let h = match HITS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if h.order() as usize != i {
            return false;
        }
        match hit_names.get(i) {
            Some(n) => {
                if h.name() != *n {
                    return false;
                }
            }
            None => return false,
        }
        if h.runnable() && h.degrades() {
            return false;
        }
        if h.group_face() && (h.runnable() || h.degrades()) {
            return false;
        }
        if h.internal() && !h.degrades() {
            return false;
        }
        i += 1;
    }
    if !Hit::Trimmed.degrades() || !Hit::Unregistered.degrades() {
        return false;
    }
    if Hit::Trimmed.degrades() == Hit::Registered.degrades() {
        return false;
    }

    let banner_names = ["full", "groups", "group-only", "suppressed"];
    let banners = [Banner::Full, Banner::Groups, Banner::GroupOnly, Banner::Suppressed];
    let mut i = 0usize;
    let mut complete_n = 0usize;
    let mut shown_n = 0usize;
    while i < banners.len() {
        let b = match banners.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if b.order() as usize != i {
            return false;
        }
        match banner_names.get(i) {
            Some(n) => {
                if b.name() != *n {
                    return false;
                }
            }
            None => return false,
        }
        if b.complete() {
            complete_n += 1;
        }
        if b.shows_list() {
            shown_n += 1;
        }
        if b.complete() && !b.shows_list() {
            return false;
        }
        i += 1;
    }
    if complete_n != 1 || shown_n != 3 {
        return false;
    }
    if Banner::Full.stream() != "stdout" || Banner::Suppressed.stream() != "none" {
        return false;
    }

    if FLOWS.len() != 5 {
        return false;
    }
    let flow_names = ["two", "only-stdout", "only-stderr", "merged", "suppressed"];
    let mut i = 0usize;
    while i < FLOWS.len() {
        let f = match FLOWS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if f.order() as usize != i {
            return false;
        }
        match flow_names.get(i) {
            Some(n) => {
                if f.name() != *n {
                    return false;
                }
            }
            None => return false,
        }
        if f.lost() && (f.err_visible() || f.banner_visible()) {
            return false;
        }
        if f.two() != (f == Flows::Two) || f.lost() != (f == Flows::Suppressed) {
            return false;
        }
        i += 1;
    }
    if !(Flows::Merged.err_visible() && Flows::Merged.banner_visible()) {
        return false;
    }
    if Flows::OnlyStdout.err_visible() || Flows::OnlyStderr.banner_visible() {
        return false;
    }

    if HINTS.len() != 4 {
        return false;
    }
    let hint_names = ["none", "try-list", "try-group", "try-list-full"];
    let mut i = 0usize;
    let mut present_n = 0usize;
    let mut group_n = 0usize;
    while i < HINTS.len() {
        let h = match HINTS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if h.present() {
            present_n += 1;
        }
        if h.points_to_group() {
            group_n += 1;
        }
        if h.points_to_group() && !h.present() {
            return false;
        }
        match hint_names.get(i) {
            Some(n) => {
                if h.name() != *n {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    if present_n != 3 || group_n != 1 {
        return false;
    }
    if Hint::None.present() || Hint::TryList.points_to_group() {
        return false;
    }

    if NOTES.len() != 13 {
        return false;
    }
    let note_names = [
        "ran",
        "alias-ran",
        "re-exec-run",
        "group-usage",
        "degraded",
        "bannerless",
        "flows-off",
        "hintless",
        "no-re-exec",
        "sceneless",
        "baseless",
        "at-bare",
        "main-path",
    ];
    let mut i = 0usize;
    let mut run_n = 0usize;
    let mut deg_n = 0usize;
    let mut noscene_n = 0usize;
    let mut e127_n = 0usize;
    while i < NOTES.len() {
        let n = match NOTES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if n.order() as usize != i {
            return false;
        }
        match note_names.get(i) {
            Some(t) => {
                if n.name() != *t {
                    return false;
                }
            }
            None => return false,
        }
        if n.run_face() {
            run_n += 1;
        }
        if n.degrade_face() {
            deg_n += 1;
        }
        if n.no_scene() {
            noscene_n += 1;
        }
        if n.exit_127_face() {
            e127_n += 1;
        }
        if n.run_face() && n.degrade_face() {
            return false;
        }
        if n.degrade_face() != n.exit_127_face() {
            return false;
        }
        i += 1;
    }
    if run_n != 3 || deg_n != 4 || noscene_n != 3 || e127_n != 4 {
        return false;
    }

    let mut i = 0usize;
    while i < TIERS.len() {
        let t = match TIERS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if t.index() != i {
            return false;
        }
        if t.success() != (t == Tier::Pass) {
            return false;
        }
        if t.ready_allowed() != (t == Tier::Degrade) {
            return false;
        }
        if t.degrade_face() && t.success() {
            return false;
        }
        i += 1;
    }
    if TIERS.len() != TIER_COUNT {
        return false;
    }
    let tier_names = ["pass", "degrade", "partial", "undef"];
    let mut i = 0usize;
    while i < tier_names.len() {
        let t = match TIERS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        match tier_names.get(i) {
            Some(n) => {
                if t.name() != *n {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }

    if FAULTS.len() != 14 {
        return false;
    }
    let mut i = 0usize;
    while i < FAULTS.len() {
        let f = match FAULTS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if f.alive() != (f.code() != 0) {
            return false;
        }
        if fault_short(f).is_empty() {
            return false;
        }
        let mut j = i + 1;
        while j < FAULTS.len() {
            if let Some(g) = FAULTS.get(j) {
                if f.alive() && g.alive() && f.code() == g.code() {
                    return false;
                }
                if f != *g && fault_short(f) == fault_short(*g) {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    if Fault::Ok.code() != 0 {
        return false;
    }
    if Fault::Estate.code() != AV_ESTATE
        || Fault::Link.code() != AV_ELINK
        || Fault::Bal.code() != AV_EBAL
    {
        return false;
    }
    if Resolved::Absent.usable() || Resolved::Empty.usable() || !Resolved::Absent.sceneless() {
        return false;
    }
    if Resolved::Empty.sceneless() || Resolved::Absent.name() != "absent" {
        return false;
    }
    if Resolved::Empty.text() != "" || Resolved::Absent.text() != "" {
        return false;
    }
    true
}

/// 表路三：链接形态六式与它们各自的规则。
fn link_ok() -> bool {
    if LINKS.len() != 6 {
        return false;
    }
    let names = ["symlink-rel", "symlink-abs", "hardlink", "relinked-copy", "exec-argv0", "shebang"];
    let mut i = 0usize;
    while i < LINKS.len() {
        let l = match LINKS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if l.order() as usize != i {
            return false;
        }
        match names.get(i) {
            Some(n) => {
                if l.name() != *n {
                    return false;
                }
            }
            None => return false,
        }
        if l.order() > 5 {
            return false;
        }
        let dir = match l {
            Link::SymlinkAbs | Link::RenamedCopy | Link::Shebang => true,
            _ => false,
        };
        if l.keeps_dirpath() != dir {
            return false;
        }
        let may_empty = match l {
            Link::ExecArgv0 | Link::Shebang => true,
            _ => false,
        };
        if l.may_have_no_name() != may_empty {
            return false;
        }
        let retitle = match l {
            Link::RenamedCopy | Link::Shebang => false,
            _ => true,
        };
        if l.retitle_capable() != retitle {
            return false;
        }
        let targets = match l {
            Link::SymlinkRel | Link::SymlinkAbs | Link::ExecArgv0 => true,
            _ => false,
        };
        if l.targets_busybox() != targets {
            return false;
        }
        if l.targets_busybox() && !l.keeps_dirpath() && !l.retitle_capable() {
            return false;
        }
        let mmu_ok = match l {
            Link::Shebang => false,
            Link::RenamedCopy => true,
            _ => true,
        };
        if l.reexec_allowed(false) != mmu_ok {
            return false;
        }
        let nommu_ok = match l {
            Link::Shebang | Link::RenamedCopy => false,
            _ => true,
        };
        if l.reexec_allowed(true) != nommu_ok {
            return false;
        }
        if l.reexec_allowed(true) && !l.reexec_allowed(false) {
            return false;
        }
        i += 1;
    }
    if Link::Shebang.reexec_allowed(false) || Link::RenamedCopy.reexec_allowed(true) {
        return false;
    }
    if !Link::Hardlink.retitle_capable() || Link::Hardlink.targets_busybox() {
        return false;
    }
    if !Link::RenamedCopy.keeps_dirpath() || Link::RenamedCopy.retitle_capable() {
        return false;
    }

    if SELF_NAMES.len() != SELF_NAME_COUNT {
        return false;
    }
    let mut i = 0usize;
    while i < SELF_NAMES.len() {
        if let Some(n) = SELF_NAMES.get(i) {
            if !is_self_name(n) {
                return false;
            }
            if n.is_empty() {
                return false;
            }
        }
        i += 1;
    }
    if is_self_name("ash") || is_self_name("ls") || is_self_name("nope") || is_self_name("") {
        return false;
    }
    if !is_self_name("busybox") || !is_self_name("@busybox") {
        return false;
    }
    if is_self_name("BusyBox") {
        return false;
    }

    let sc = Shape::blank();
    if sc.link != Link::SymlinkAbs || sc.argc != 1 || sc.absent {
        return false;
    }
    if sc.tag != "blank" || sc.nommu || sc.banner_off || sc.hint_off {
        return false;
    }
    if sc.merged_out || sc.dropped_out || sc.retitled() || sc.self_named() {
        return false;
    }
    if sc.flows_plan() != Flows::Two {
        return false;
    }
    let mut off = Shape::blank();
    off.merged_out = true;
    if off.flows_plan() != Flows::Merged {
        return false;
    }
    off.dropped_out = true;
    if off.flows_plan() != Flows::Suppressed {
        return false;
    }
    off.merged_out = false;
    off.dropped_out = false;
    off.banner_off = true;
    if off.flows_plan() != Flows::OnlyStderr {
        return false;
    }
    let mut rel = Shape::blank();
    rel.argv0 = "ls";
    rel.link = Link::SymlinkRel;
    if rel.keyed_with_dir() {
        return false;
    }
    if rel.path().is_some() {
        return false;
    }
    rel.link = Link::SymlinkAbs;
    // 形态说"该带路径"、`argv[0]` 却没带：路径只能是推不出，不能补一个。
    if rel.keyed_with_dir() || rel.path().is_some() {
        return false;
    }
    rel.argv0 = "/bin/ls";
    if rel.path() != Some("/bin/ls") || !rel.keyed_with_dir() {
        return false;
    }
    rel.link = Link::Hardlink;
    if rel.keyed_with_dir() || rel.path().is_some() {
        return false;
    }
    let mut ret = Shape::blank();
    ret.argv0 = "@zzz";
    ret.argv1 = "zzz";
    ret.argc = 2;
    ret.link = Link::ExecArgv0;
    if !ret.retitled() || ret.key() != "zzz" || ret.prog() != "@zzz" {
        return false;
    }
    ret.argc = 1;
    if ret.retitled() || ret.key() != "@zzz" {
        return false;
    }
    ret.link = Link::Shebang;
    ret.argc = 2;
    if ret.retitled() {
        return false;
    }
    let mut noself = Shape::blank();
    noself.argv0 = "busybox";
    if !noself.self_named() {
        return false;
    }
    noself.argv0 = "/bin/busybox";
    if !noself.self_named() || noself.key() != "busybox" {
        return false;
    }
    if define(&noself).note != Note::MainPath {
        return false;
    }
    true
}

/// 表路四：组表十一行、成员表二十七行与两跳查表。
fn table_ok() -> bool {
    let t = table();
    if t.len() != TABLE_COUNT {
        return false;
    }
    if primary_count() + alias_count() != TABLE_COUNT {
        return false;
    }
    if alias_count() != 2 || primary_count() != 25 {
        return false;
    }
    if trimmed_count() != 1 || group_only_count() != 2 || dispatchable_count() != 24 {
        return false;
    }
    let g = groups();
    if g.len() != GROUP_COUNT {
        return false;
    }
    let mut member_sum = 0usize;
    let mut indiv_sum = 0usize;
    let mut merged_n = 0usize;
    let mut empty_n = 0usize;
    let mut i = 0usize;
    while i < g.len() {
        let grp = match g.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if grp.name.is_empty() || grp.kconfig.is_empty() {
            return false;
        }
        if !grp.kconfig.starts_with("CONFIG_FEATURE_") {
            return false;
        }
        if !grp.kconfig.ends_with("_INDIVIDUAL") {
            return false;
        }
        if group_primary_count(grp.name) != grp.members as usize {
            return false;
        }
        if group_individual_count(grp.name) != grp.individuals as usize {
            return false;
        }
        if group_registered_count(grp.name) > grp.members as usize {
            return false;
        }
        if grp.individuals > grp.members {
            return false;
        }
        if grp.members == 0 {
            empty_n += 1;
            if !group_member_list(grp.name, ", ").is_empty() {
                return false;
            }
        }
        if grp.merged_by_default {
            merged_n += 1;
        }
        if !grp.merged_by_default && !same_text(grp.name, "coreutils") {
            return false;
        }
        member_sum = member_sum.saturating_add(grp.members as usize);
        indiv_sum = indiv_sum.saturating_add(grp.individuals as usize);
        if let Some(idx) = group_index(grp.name) {
            if idx != i {
                return false;
            }
        } else {
            return false;
        }
        match group_at(grp.name) {
            Some(x) => {
                if x != grp {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    if member_sum != primary_count() || merged_n != GROUP_COUNT - 1 || empty_n != 5 {
        return false;
    }
    if indiv_sum != 22 {
        return false;
    }
    if group_at("nope").is_some() || group_index("").is_some() {
        return false;
    }

    let mut i = 0usize;
    let mut last_seal = 0u64;
    while i < t.len() {
        let r = match t.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if r.prio as usize != i {
            return false;
        }
        if r.name.is_empty() {
            return false;
        }
        if r.registered && !r.individual && r.is_primary() && !r.in_group() {
            return false;
        }
        if r.is_alias() && r.alias_of.is_empty() {
            return false;
        }
        if r.alias_of == r.name {
            return false;
        }
        if r.is_alias() && row_at(r.alias_of).is_none() {
            return false;
        }
        if !r.dispatchable() && !r.is_primary() && !r.registered {
            return false;
        }
        let s = r.seal();
        if i > 0 && s == last_seal {
            return false;
        }
        last_seal = s;
        if let Some(idx) = row_index(r.name) {
            if idx != i {
                return false;
            }
        } else {
            return false;
        }
        match row_at(r.name) {
            Some(x) => {
                if x != r {
                    return false;
                }
            }
            None => return false,
        }
        if text_len(r.name) > APPLET_NAME_MAX {
            return false;
        }
        if has_control(r.name) || is_dots(r.name) || has_byte(r.name, b'/') {
            return false;
        }
        if r.render().ends_with('\n') != true {
            return false;
        }
        i += 1;
    }
    if row_at("nope").is_some() || row_index("nope").is_some() {
        return false;
    }
    if row_index("").is_some() {
        return false;
    }
    let ls = match row_at("ls") {
        Some(v) => v,
        None => return false,
    };
    if !ls.dispatchable() || !ls.is_primary() || !same_text(ls.group, "coreutils") {
        return false;
    }
    if ls.qualified() != "coreutils ls" || ls.render() != "coreutils ls\n" {
        return false;
    }
    let ash = match row_at("ash") {
        Some(v) => v,
        None => return false,
    };
    if !ash.is_alias() || !same_text(ash.alias_of, "sh") || !ash.dispatchable() {
        return false;
    }
    if ash.qualified() != "shellutils ash" || ash.render() != "shellutils ash -> sh\n" {
        return false;
    }
    let vi = match row_at("vi") {
        Some(v) => v,
        None => return false,
    };
    if vi.dispatchable() || !vi.registered || vi.is_alias() {
        return false;
    }
    if vi.render() != "editors vi [group-only]\n" {
        return false;
    }
    let ed = match row_at("ed") {
        Some(v) => v,
        None => return false,
    };
    if ed.registered || ed.dispatchable() || !same_text(ed.group, "editors") {
        return false;
    }
    if ed.render() != "editors ed [trimmed]\n" {
        return false;
    }
    let test = match row_at("test") {
        Some(v) => v,
        None => return false,
    };
    if test.dispatchable() || !test.registered || !same_text(test.group, "coreutils") {
        return false;
    }
    // `test` 只能组调，但它确实在组里且在表里：组用法清单必须有它。
    if !list_has(group_member_list("coreutils", ", ").as_str(), ", ", "test") {
        return false;
    }
    // 被裁件不进清单（列出来就是骗人）；`sed` 里含 `ed` 两个字母，
    // 所以这一问只能走词界判定，不能走子串。
    if list_has(group_member_list("editors", ", ").as_str(), ", ", "ed") {
        return false;
    }
    if group_member_list("editors", ", ").is_empty() {
        return false;
    }
    if !list_has(group_member_list("editors", ", ").as_str(), ", ", "sed") {
        return false;
    }
    if !list_has(group_member_list("coreutils", ", ").as_str(), ", ", "ls") {
        return false;
    }
    if list_items(group_member_list("coreutils", ", ").as_str(), ", ") != 10 {
        return false;
    }
    if list_items(group_member_list("editors", ", ").as_str(), ", ") != 3 {
        return false;
    }
    if list_items(group_name_list(", ").as_str(), ", ") != GROUP_COUNT {
        return false;
    }
    if group_registered_count("editors") != 3 || group_registered_count("coreutils") != 10 {
        return false;
    }
    if group_primary_count("shellutils") != 1 || group_individual_count("shellutils") != 1 {
        return false;
    }
    if !Member::blank().alias_of.is_empty() || Member::blank().registered {
        return false;
    }
    if Member::blank().seal() == 0 || Member::blank().render() != " [trimmed]\n" {
        return false;
    }
    true
}

/// 判路一：八闸纵线逐段落位。
///
/// 这里把每一段的**唯一出口**写死：闸一只出 `sceneless`，闸二只出
/// `baseless`，闸三出 `at-bare` 与 `main-path`，闸四出直通/组内/降级三支，
/// 闸五出 `no-re-exec` 或 `re-exec-run` 或组用法，闸六七八出降级面的四种
/// 完整度。任何一支走歪（例如查表段就收尾、或者重启判死却打了横幅）都在
/// 这里判红。
fn define_ok() -> bool {
    // 闸一：空 argv。
    let mut a = Shape::blank();
    a.absent = true;
    a.argc = 2;
    let o = define(&a);
    if o.stage != Stage::Argv0
        || o.code != ERR_EBADARG
        || o.value != EXIT_USAGE
        || o.note != Note::Sceneless
        || o.default
        || o.flows != Flows::Suppressed
        || o.banner != Banner::Suppressed
        || o.hint != Hint::None
        || !o.name.is_empty()
        || !o.undef()
        || o.ok() {
        return false;
    }
    if o.tier() != Tier::Undef {
        return false;
    }
    a.absent = false;
    a.argc = 0;
    if define(&a) != o {
        return false;
    }

    // 闸二：基名不可推（尾斜杠、空 argv[0]）。
    let mut b = Shape::blank();
    b.argv0 = "/usr/sbin/";
    b.link = Link::SymlinkAbs;
    let ob = define(&b);
    if ob.stage != Stage::Base
        || ob.code != ERR_EINVAL
        || ob.value != EXIT_USAGE
        || ob.note != Note::Baseless
        || ob.flows != Flows::OnlyStderr
        || ob.hit != Hit::Unregistered
        || !ob.name.is_empty()
        || ob.default {
        return false;
    }
    if b.resolves() != Resolved::Empty || b.key() != "" {
        return false;
    }
    b.argv0 = "";
    if define(&b).stage != Stage::Base {
        return false;
    }

    // 闸三：裸 `@` 与本尊归位。
    let mut c = Shape::blank();
    c.argv0 = "@";
    c.argc = 1;
    c.link = Link::SymlinkAbs;
    let oc = define(&c);
    if oc.stage != Stage::Name
        || oc.code != ERR_EINVAL
        || oc.note != Note::AtBare
        || oc.value != EXIT_USAGE
        || oc.flows != Flows::OnlyStderr {
        return false;
    }
    if c.key() != "@" || !has_at_prefix(c.key()) || !strip_at(c.key()).is_empty() {
        return false;
    }
    c.argv0 = "@busybox";
    c.argv1 = "busybox";
    c.argc = 2;
    if define(&c).note != Note::MainPath {
        return false;
    }
    let mut d = Shape::blank();
    d.argv0 = "busybox";
    d.argv1 = "ls";
    d.argc = 2;
    d.link = Link::ExecArgv0;
    let od = define(&d);
    if od.stage != Stage::Name
        || od.code != ERR_NONE
        || od.value != EXIT_RUN_OK
        || od.note != Note::MainPath
        || od.banner != Banner::Full
        || od.flows != Flows::OnlyStdout
        || od.default {
        return false;
    }
    if od.ok() {
        return false;
    }

    // 闸四直通：个体名。
    let mut e = Shape::blank();
    e.argv0 = "/bin/ls";
    e.argv1 = "-l";
    e.argc = 2;
    let oe = define(&e);
    if oe.stage != Stage::Ready
        || oe.code != ERR_NONE
        || oe.value != EXIT_RUN_OK
        || oe.note != Note::Ran
        || oe.hit != Hit::Registered
        || oe.group != "coreutils"
        || oe.banner != Banner::Suppressed
        || oe.flows != Flows::OnlyStdout
        || oe.hint != Hint::None
        || !oe.ok()
        || !oe.ran()
        || oe.degraded() {
        return false;
    }
    if oe.tier() != Tier::Pass {
        return false;
    }

    // 闸四直通：别名。
    let mut f = Shape::blank();
    f.argv0 = "ash";
    f.link = Link::Hardlink;
    let of_ = define(&f);
    if of_.note != Note::AliasRan || of_.hit != Hit::Alias || of_.stage != Stage::Ready {
        return false;
    }
    if of_.value != EXIT_RUN_OK || !of_.ok() || of_.tier() != Tier::Pass {
        return false;
    }

    // 闸四/闸五：组名自调用。
    let mut g = Shape::blank();
    g.argv0 = "coreutils";
    g.link = Link::SymlinkRel;
    let og = define(&g);
    if og.stage != Stage::Ready
        || og.code != ERR_NONE
        || og.value != EXIT_GROUP_USAGE
        || og.note != Note::GroupUsage
        || og.hit != Hit::Group
        || og.banner != Banner::Groups
        || og.hint != Hint::TryGroup
        || og.flows != Flows::Two
        || og.default
        || !og.ok() {
        return false;
    }
    if og.tier() != Tier::Partial || !og.tier().degrade_face() {
        return false;
    }
    g.argv0 = "editors";
    let og2 = define(&g);
    if og2.note != Note::GroupUsage || og2.group != "editors" || og2.hit != Hit::Group {
        return false;
    }
    g.banner_off = true;
    g.argv0 = "coreutils";
    if define(&g).banner != Banner::Suppressed {
        return false;
    }
    g.banner_off = false;
    g.hint_off = true;
    if define(&g).hint != Hint::None {
        return false;
    }

    // 闸五：组内件——未合并组只有单组清面，合并组看重启。
    let mut h = Shape::blank();
    h.argv0 = "test";
    h.link = Link::SymlinkRel;
    let oh = define(&h);
    if oh.hit != Hit::Group
        || oh.note != Note::GroupUsage
        || oh.banner != Banner::GroupOnly
        || oh.hint != Hint::TryListFull
        || oh.value != EXIT_GROUP_USAGE
        || oh.code != ERR_NONE
        || oh.stage != Stage::Ready
        || oh.default {
        return false;
    }
    if oh.tier() != Tier::Partial {
        return false;
    }
    h.link = Link::ExecArgv0;
    h.argv0 = "vi";
    let oh2 = define(&h);
    if oh2.note != Note::ReExecRun
        || oh2.value != EXIT_RUN_OK
        || oh2.code != ERR_NONE
        || oh2.stage != Stage::Ready
        || oh2.banner != Banner::Suppressed
        || oh2.flows != Flows::OnlyStdout
        || !oh2.ok()
        || oh2.tier() != Tier::Pass {
        return false;
    }
    if oh2.hit != Hit::Group {
        return false;
    }
    let mut k = Shape::blank();
    k.argv0 = "/tmp/vi";
    k.link = Link::RenamedCopy;
    k.nommu = true;
    let ok_ = define(&k);
    if ok_.stage != Stage::Exec
        || ok_.code != ERR_ENOSYS
        || ok_.value != EXIT_EXEC
        || ok_.note != Note::NoReexec
        || ok_.banner != Banner::Suppressed
        || ok_.flows != Flows::OnlyStderr
        || ok_.hint != Hint::None
        || ok_.default
        || !ok_.undef() {
        return false;
    }
    if ok_.tier() != Tier::Undef || ok_.path != Some("/tmp/vi") {
        return false;
    }
    k.nommu = false;
    if define(&k).note != Note::ReExecRun {
        return false;
    }
    k.link = Link::Shebang;
    if define(&k).note != Note::NoReexec {
        return false;
    }

    // 闸四：被裁件——降级面走组调用指路。
    let mut m = Shape::blank();
    m.argv0 = "/bin/ed";
    let om = define(&m);
    if om.hit != Hit::Trimmed
        || om.code != ERR_ENOENT
        || om.value != EXIT_DEGRADE
        || om.note != Note::Degraded
        || om.hint != Hint::TryGroup
        || om.group != "editors"
        || !om.degraded()
        || !om.complete()
        || om.tier() != Tier::Degrade {
        return false;
    }
    if !om.hit.degrades() || !om.hit.internal() || om.ok() {
        return false;
    }

    // 闸四：未注册名——两条指路档按链接形态分。
    let mut n = Shape::blank();
    n.argv0 = "foo";
    n.link = Link::SymlinkRel;
    let on = define(&n);
    if on.hit != Hit::Unregistered || on.hint != Hint::TryList || !on.default {
        return false;
    }
    if on.banner != Banner::Full || on.flows != Flows::Two || on.note != Note::Degraded {
        return false;
    }
    if n.retitled() || n.keyed_with_dir() || n.path().is_some() {
        return false;
    }
    n.argv0 = "/bin/foo";
    n.link = Link::SymlinkAbs;
    if define(&n).hint != Hint::TryListFull {
        return false;
    }
    if !n.keyed_with_dir() || n.path() != Some("/bin/foo") {
        return false;
    }

    // 降级面完整度的四种缺件。
    let mut p = Shape::blank();
    p.argv0 = "foo";
    p.link = Link::SymlinkRel;
    p.banner_off = true;
    let op = define(&p);
    if op.note != Note::Bannerless
        || op.banner != Banner::Suppressed
        || op.flows != Flows::OnlyStderr
        || op.hint != Hint::TryList
        || op.default
        || op.tier() != Tier::Partial
        || !op.half() {
        return false;
    }
    p.banner_off = false;
    p.hint_off = true;
    let op2 = define(&p);
    if op2.note != Note::Hintless || op2.hint != Hint::None || op2.default {
        return false;
    }
    if op2.banner != Banner::Full || op2.flows != Flows::Two || op2.has_hint() {
        return false;
    }
    p.hint_off = false;
    p.merged_out = true;
    let op3 = define(&p);
    if op3.note != Note::FlowsOff || op3.flows != Flows::Merged || op3.default {
        return false;
    }
    if op3.banner != Banner::Full || !op3.half() {
        return false;
    }
    p.merged_out = false;
    p.dropped_out = true;
    let op4 = define(&p);
    if op4.note != Note::FlowsOff
        || op4.flows != Flows::Suppressed
        || op4.banner != Banner::Suppressed
        || op4.default
        || op4.code != ERR_ENOENT
        || op4.value != EXIT_DEGRADE {
        return false;
    }
    if !op4.flows.lost() || message(&p, &op4).len() != 0 {
        return false;
    }

    // 全局不变量：闸四/六/七不是收尾段；齐备位只在收尾段成立。
    let cs = cases();
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let o = c.actual();
        if o.stage == Stage::Table || o.stage == Stage::Banner || o.stage == Stage::Stream {
            return false;
        }
        if o.default && (o.stage != Stage::Ready || !o.degraded() || !o.note.exit_127_face()) {
            return false;
        }
        if o.default != (o.banner.complete() && o.flows.two() && o.hint.present()) {
            return false;
        }
        if o.stage != Stage::Ready && o.default {
            return false;
        }
        if Tier::of(&o) == Tier::Degrade && !o.default {
            return false;
        }
        if o.ran() && o.hit.degrades() {
            return false;
        }
        if o.note.run_face() != (o.value == EXIT_RUN_OK && o.stage == Stage::Ready) {
            return false;
        }
        i += 1;
    }
    if define(&Shape::blank()).seal() == 0 {
        return false;
    }
    if Outcome::blank().stage != Stage::Argv0 || Outcome::blank().default {
        return false;
    }
    if Outcome::blank().ok() || !Outcome::blank().undef() {
        return false;
    }
    true
}

/// 判路二：横幅版面、清单与整块输出的成形。
fn banner_ok() -> bool {
    let full = banner_full();
    if !full.starts_with(USAGE_VERSION) {
        return false;
    }
    if !full.contains(USAGE_COPYRIGHT) || !full.contains(USAGE_SYNOPSES) {
        return false;
    }
    if !full.contains(USAGE_BLURB) || !full.contains(FUNCTIONS_HEAD) {
        return false;
    }
    if nl_count(full.as_str()) != 16 {
        return false;
    }
    if !full.ends_with('\n') || full.contains("--list-full]") {
        return false;
    }
    if !full.contains("or: busybox --list[-full]") {
        return false;
    }
    let groups = banner_groups();
    if !groups.contains(GROUPS_HEAD) || nl_count(groups.as_str()) != 6 {
        return false;
    }
    if groups.contains(FUNCTIONS_HEAD) {
        return false;
    }
    let mut i = 0usize;
    while i < GROUPS.len() {
        if let Some(g) = GROUPS.get(i) {
            if !groups.contains(g.name) {
                return false;
            }
        }
        i += 1;
    }
    let only = banner_group_only("coreutils");
    if nl_count(only.as_str()) != 5 || !only.contains("coreutils:") {
        return false;
    }
    if !only.contains("Usage: busybox <applet> [arguments]...") {
        return false;
    }
    // 组入口不存在时打的单组清单：`test` 只能组调，但它就在这一组里；
    // `printf` 也必须在——清单空了就是骗人。
    if !only.contains("test") || !only.contains("printf") {
        return false;
    }
    if banner_group_only("fsck").contains("fsck:") != true {
        return false;
    }

    let dump = list_dump(", ");
    if list_items(dump.as_str(), ", ") != 26 {
        return false;
    }
    if !list_has(dump.as_str(), ", ", "ash") || !list_has(dump.as_str(), ", ", "ls") {
        return false;
    }
    if list_has(dump.as_str(), ", ", "ed") || list_has(dump.as_str(), ", ", "nope") {
        return false;
    }
    if dump.contains("coreutils ") {
        return false;
    }
    let full_list = list_full_dump();
    if nl_count(full_list.as_str()) != 26 {
        return false;
    }
    if !full_list.contains("coreutils ls\n") {
        return false;
    }
    if !full_list.contains("shellutils ash -> sh\n") {
        return false;
    }
    if full_list.contains("editors ed") || !full_list.contains("editors vi [group-only]") {
        return false;
    }
    if full_list.contains("crypto") {
        return false;
    }

    let mut sh = Shape::blank();
    sh.argv0 = "foo";
    sh.link = Link::SymlinkRel;
    let o = define(&sh);
    let msg = message(&sh, &o);
    if !msg.starts_with(USAGE_VERSION) {
        return false;
    }
    if find_from(msg.as_str(), "foo: 'foo': applet not found", 0).is_none() {
        return false;
    }
    if msg.contains("applet not found.") {
        return false;
    }
    if find_from(msg.as_str(), "Run 'busybox --list' to find all built-in applets.", 0).is_none()
    {
        return false;
    }
    if !msg.ends_with('\n') || nl_count(msg.as_str()) != 18 {
        return false;
    }
    if banner_text(&o) != full {
        return false;
    }

    sh.banner_off = true;
    let o2 = define(&sh);
    let m2 = message(&sh, &o2);
    if !m2.starts_with("foo: 'foo': applet not found") {
        return false;
    }
    if m2.contains(USAGE_VERSION) || nl_count(m2.as_str()) != 2 {
        return false;
    }
    if banner_text(&o2).len() != 0 {
        return false;
    }

    sh.banner_off = false;
    sh.hint_off = true;
    let o3 = define(&sh);
    if nl_count(message(&sh, &o3).as_str()) != 17 {
        return false;
    }
    sh.hint_off = false;
    sh.dropped_out = true;
    let o3b = define(&sh);
    if message(&sh, &o3b).len() != 0 || !o3b.flows.lost() {
        return false;
    }
    sh.dropped_out = false;
    sh.merged_out = true;
    let o4 = define(&sh);
    let m4 = message(&sh, &o4);
    if nl_count(m4.as_str()) != 18 || !m4.contains("applet not found") {
        return false;
    }
    if !o4.flows.banner_visible() || !o4.flows.err_visible() {
        return false;
    }

    let mut grp = Shape::blank();
    grp.argv0 = "coreutils";
    grp.link = Link::SymlinkRel;
    let og = define(&grp);
    let mg = message(&grp, &og);
    if nl_count(mg.as_str()) != 7 {
        return false;
    }
    if mg.contains("applet not found") {
        return false;
    }
    if !mg.contains("Run 'busybox coreutils ") || !mg.contains(GROUPS_HEAD) {
        return false;
    }

    let mut nx = Shape::blank();
    nx.argv0 = "/tmp/vi";
    nx.link = Link::RenamedCopy;
    nx.nommu = true;
    let onx = define(&nx);
    let mnx = message(&nx, &onx);
    if !mnx.starts_with("vi: can't re-exec '/tmp/vi': Operation not supported") {
        return false;
    }
    if mnx.len() != errline_of(&nx, &onx).len() {
        return false;
    }
    let mut unknown = Shape::blank();
    unknown.argv0 = "vi";
    unknown.link = Link::Shebang;
    unknown.argc = 0;
    unknown.absent = true;
    let ou = define(&unknown);
    if ou.note != Note::Sceneless || message(&unknown, &ou).len() != 0 {
        return false;
    }
    unknown.absent = false;
    unknown.argc = 1;
    let ou2 = define(&unknown);
    if ou2.note != Note::NoReexec || ou2.name != "vi" {
        return false;
    }
    if errline_of(&unknown, &ou2) != "vi: can't re-exec '(unknown)': Operation not supported\n" {
        return false;
    }
    if hint_text(Hint::None, "coreutils", "ls").len() != 0 {
        return false;
    }
    if hint_text(Hint::TryList, "coreutils", "ls") != msg_try_list() {
        return false;
    }
    if hint_text(Hint::TryListFull, "coreutils", "ls") != msg_try_list_full() {
        return false;
    }
    if hint_text(Hint::TryGroup, "editors", "ed") != msg_try_group("editors", "ed") {
        return false;
    }
    true
}

/// 判路三：对判纵线与判序。
fn judge_ok() -> bool {
    let cs = cases();
    let mut i = 0usize;
    while i < ACCEPT {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if judge(&c) != Fault::Ok || !c.ok() {
            return false;
        }
        if expected(i) != Some(Fault::Ok) {
            return false;
        }
        if c.actual().ok() != c.claim.success() {
            return false;
        }
        i += 1;
    }
    if i != ACCEPT {
        return false;
    }
    let mut k = 0usize;
    while k < REJECT {
        let c = match cs.get(k + ACCEPT) {
            Some(v) => *v,
            None => return false,
        };
        let want = match REJECT_FAULTS.get(k) {
            Some(v) => *v,
            None => return false,
        };
        if judge(&c) != want || c.ok() {
            return false;
        }
        if expected(k + ACCEPT) != Some(want) {
            return false;
        }
        if want.code() != AV_ESTATE
            && want.code() != AV_ESTAGE
            && want.code() != AV_ECODE
            && want.code() != AV_EVALUE
            && want.code() != AV_ENOTE
            && want.code() != AV_EDEFAULT {
            return false;
        }
        if c.actual().ok() != c.claim.success() && want != Fault::Estate {
            return false;
        }
        k += 1;
    }
    if Case::blank().judge() != Fault::Estate {
        return false;
    }
    if Claim::blank().stage != Stage::Ready || !Claim::blank().success() {
        return false;
    }
    if Claim::blank().default || Claim::blank().code != ERR_NONE {
        return false;
    }

    let live = match case_at(3) {
        Some(v) => v,
        None => return false,
    };
    let mut stage_only = live;
    stage_only.claim.stage = Stage::Table;
    if judge(&stage_only) != Fault::Stage {
        return false;
    }
    let mut code_only = live;
    code_only.claim.code = ERR_ENOSYS;
    if judge(&code_only) != Fault::Code {
        return false;
    }
    let mut value_only = live;
    value_only.claim.value = EXIT_USAGE;
    if judge(&value_only) != Fault::Value {
        return false;
    }
    let mut note_only = live;
    note_only.claim.note = Note::Sceneless;
    if judge(&note_only) != Fault::Note {
        return false;
    }
    let mut default_only = live;
    default_only.claim.default = false;
    if judge(&default_only) != Fault::Default {
        return false;
    }
    let mut estate = live;
    estate.claim.code = ERR_NONE;
    if judge(&estate) != Fault::Estate {
        return false;
    }

    let run_case = match case_at(0) {
        Some(v) => v,
        None => return false,
    };
    let mut deep = run_case;
    deep.claim.stage = Stage::Argv0;
    deep.claim.code = ERR_EBADARG;
    if judge(&deep) != Fault::Estate {
        return false;
    }
    let mut late = run_case;
    late.claim.value = EXIT_DEGRADE;
    if judge(&late) != Fault::Value {
        return false;
    }
    let mut pair = [Fault::Estate, Fault::Stage, Fault::Code, Fault::Value, Fault::Note];
    pair[0] = judge(&estate);
    pair[1] = judge(&stage_only);
    pair[2] = judge(&code_only);
    pair[3] = judge(&value_only);
    pair[4] = judge(&note_only);
    let mut i = 0usize;
    while i < pair.len() {
        let want = match REJECT_FAULTS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if pair[i] != want {
            return false;
        }
        if fault_short(pair[i]) != fault_short(want) {
            return false;
        }
        i += 1;
    }
    if pair.iter().any(|f| !f.alive()) {
        return false;
    }
    true
}

/// 判路四：矩阵分档与统计。
fn matrix_ok() -> bool {
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
    if m.empty_rows() != 0 {
        return false;
    }
    if m.perfect_rows() != 3 {
        return false;
    }
    if !m.reds_on_degrade_row() {
        return false;
    }
    if let Some(r) = m.row(Tier::Pass) {
        if r.total != 3 || r.good != 3 || r.red() != 0 || !r.perfect() {
            return false;
        }
    } else {
        return false;
    }
    if let Some(r) = m.row(Tier::Degrade) {
        if r.total != 9 || r.good != 3 || r.red() != 6 || r.perfect() {
            return false;
        }
    } else {
        return false;
    }
    if let Some(r) = m.row(Tier::Partial) {
        if r.total != 1 || r.good != 1 || r.red() != 0 {
            return false;
        }
    } else {
        return false;
    }
    if let Some(r) = m.row(Tier::Undef) {
        if r.total != 1 || r.good != 1 || r.red() != 0 {
            return false;
        }
    } else {
        return false;
    }
    let txt = m.render();
    if nl_count(txt.as_str()) != TIER_COUNT {
        return false;
    }
    if !txt.starts_with("pass total=3 good=3 red=0\n") {
        return false;
    }
    if !txt.contains("degrade total=9 good=3 red=6\n") {
        return false;
    }
    if !txt.contains("partial total=1 good=1 red=0\n") {
        return false;
    }
    if !txt.contains("undef total=1 good=1 red=0\n") {
        return false;
    }
    let b = Matrix::blank();
    if b.slots() != 0 || b.goods() != 0 || b.covered() || b.perfect_rows() != 0 {
        return false;
    }
    if b.empty_rows() != TIER_COUNT || b.reds() != 0 {
        return false;
    }
    if nl_count(b.render().as_str()) != TIER_COUNT {
        return false;
    }
    if b.row(Tier::Pass).is_none() {
        return false;
    }
    let mut r = Row::blank();
    if r.perfect() || r.red() != 0 || !r.empty() {
        return false;
    }
    r.total = 5;
    if r.perfect() || r.red() != 5 || r.empty() {
        return false;
    }
    r.good = 7;
    if r.red() != 5 || r.perfect() {
        return false;
    }
    r.good = 5;
    if !r.perfect() || r.red() != 0 {
        return false;
    }
    if r.render(Tier::Undef) != "undef total=5 good=5 red=0\n" {
        return false;
    }
    if Row::blank().render(Tier::Partial) != "partial total=0 good=0 red=0\n" {
        return false;
    }
    if (Row { total: 2, good: 3 }).red() != 2 {
        return false;
    }
    let cs = cases();
    let mut i = ACCEPT;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if c.tier() != Tier::Degrade || judge(&c) == Fault::Ok {
            return false;
        }
        if Tier::of(&c.actual()) != WANT_TIER[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// 渲染路一：语料、期望表与渲染逐字。
fn corpus_ok() -> bool {
    if SHAPES.len() != CORPUS || CLAIMS.len() != CORPUS {
        return false;
    }
    if cover(CORPUS) != Ok(()) || cover(CORPUS - 1) != Err(AV_ENOBS) {
        return false;
    }
    if case_at(CORPUS).is_some() || case_at(SAMPLES).is_some() {
        return false;
    }
    if cases().len() != CORPUS {
        return false;
    }
    let cs = cases();
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if let Some(s) = SHAPES.get(i) {
            if c.shape != *s {
                return false;
            }
        } else {
            return false;
        }
        if let Some(k) = CLAIMS.get(i) {
            if c.claim != *k {
                return false;
            }
        } else {
            return false;
        }
        let o = c.actual();
        match WANT_TAGS.get(i) {
            Some(t) => {
                if c.tag() != *t || o.name.is_empty() != (*t == "blank") {
                    return false;
                }
            }
            None => return false,
        }
        match WANT_STAGE.get(i) {
            Some(v) => {
                if o.stage != *v {
                    return false;
                }
            }
            None => return false,
        }
        match WANT_CODE.get(i) {
            Some(v) => {
                if o.code != *v {
                    return false;
                }
            }
            None => return false,
        }
        match WANT_VALUE.get(i) {
            Some(v) => {
                if o.value != *v {
                    return false;
                }
            }
            None => return false,
        }
        match WANT_NOTE.get(i) {
            Some(v) => {
                if o.note != *v {
                    return false;
                }
            }
            None => return false,
        }
        match WANT_DEFAULT.get(i) {
            Some(v) => {
                if o.default != *v {
                    return false;
                }
            }
            None => return false,
        }
        match WANT_TIER.get(i) {
            Some(v) => {
                if Tier::of(&o) != *v {
                    return false;
                }
            }
            None => return false,
        }
        match WANT_HIT.get(i) {
            Some(v) => {
                if o.hit != *v {
                    return false;
                }
            }
            None => return false,
        }
        match WANT_BANNER.get(i) {
            Some(v) => {
                if o.banner != *v {
                    return false;
                }
            }
            None => return false,
        }
        match WANT_FLOWS.get(i) {
            Some(v) => {
                if o.flows != *v {
                    return false;
                }
            }
            None => return false,
        }
        match WANT_HINT.get(i) {
            Some(v) => {
                if o.hint != *v {
                    return false;
                }
            }
            None => return false,
        }
        match WANT_NAME.get(i) {
            Some(v) => {
                if o.name != *v || c.shape.key() != *v {
                    return false;
                }
            }
            None => return false,
        }
        match WANT_LINK.get(i) {
            Some(v) => {
                if o.link != *v || c.shape.link != *v {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    if expected(0) != Some(Fault::Ok) || expected(ACCEPT - 1) != Some(Fault::Ok) {
        return false;
    }
    if expected(CORPUS - 1) != Some(Fault::Default) || expected(CORPUS).is_some() {
        return false;
    }
    if REJECT_FAULTS.len() != REJECT || REJECT_CODES.len() != REJECT {
        return false;
    }
    if VALS.len() != ACCEPT {
        return false;
    }
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if r.render() != RENDER_OK {
        return false;
    }
    if !r.green() || r.gate() != Ok(()) {
        return false;
    }
    if WANT_TAGS[0] != "link-abs-run-ls" || WANT_TAGS[7] != "nommu-copy-noexec" {
        return false;
    }
    if WANT_TAGS[8] != "estate-swap" || WANT_TAGS[13] != "default-swap" {
        return false;
    }
    true
}

/// 渲染路二：八绿六红，六红各挂一维且顺序固定。
fn slots_ok() -> bool {
    let cs = cases();
    let mut i = 0usize;
    while i < ACCEPT {
        if let Some(v) = ACCEPT_SLOTS.get(i) {
            if *v != i {
                return false;
            }
        } else {
            return false;
        }
        if let Some(c) = cs.get(i) {
            if judge(c) != Fault::Ok {
                return false;
            }
            if !c.ok() || expected(i) != Some(Fault::Ok) {
                return false;
            }
            if c.claim.stage != c.actual().stage {
                return false;
            }
            if c.claim.note != c.actual().note {
                return false;
            }
        } else {
            return false;
        }
        i += 1;
    }
    i = 0usize;
    while i < REJECT {
        if let Some(v) = REJECT_SLOTS.get(i) {
            if *v != ACCEPT + i || *v >= CORPUS {
                return false;
            }
        } else {
            return false;
        }
        let c = match cs.get(ACCEPT + i) {
            Some(v) => *v,
            None => return false,
        };
        let a = c.actual();
        let k = c.claim;
        let want = match REJECT_FAULTS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if judge(&c) != want {
            return false;
        }
        let estate_off = a.ok() != k.success();
        let stage_off = a.stage != k.stage;
        let code_off = a.code != k.code;
        let value_off = a.value != k.value;
        let note_off = a.note != k.note;
        let default_off = a.default != k.default;
        let head = if estate_off {
            Fault::Estate
        } else if stage_off {
            Fault::Stage
        } else if code_off {
            Fault::Code
        } else if value_off {
            Fault::Value
        } else if note_off {
            Fault::Note
        } else if default_off {
            Fault::Default
        } else {
            Fault::Ok
        };
        if head != want {
            return false;
        }
        if want == Fault::Estate {
            // Estate 是最粗的一维。申报把降级面认成跑通时，`stage` 反而是
            // 唯一对得上的一维（`success()` 由 `stage`+`code` 派生），其余
            // 四维全错位却被它整个遮蔽——判罚只许报 Estate 一个。
            if !estate_off
                || stage_off
                || !code_off
                || !value_off
                || !note_off
                || !default_off
            {
                return false;
            }
        } else {
            if estate_off {
                return false;
            }
            let offs = [stage_off, code_off, value_off, note_off, default_off];
            let mut n = 0usize;
            let mut j = 0usize;
            while j < offs.len() {
                if let Some(v) = offs.get(j) {
                    if *v {
                        n += 1;
                    }
                }
                j += 1;
            }
            if n != 1 {
                return false;
            }
        }
        i += 1;
    }
    if ACCEPT_SLOTS.len() + REJECT_SLOTS.len() != CORPUS {
        return false;
    }
    true
}

/// 渲染路三：红档显式——注入必须让 `rate` 离开千分位。
fn reds_ok() -> bool {
    if REJECT != 6 || VALS.len() != ACCEPT {
        return false;
    }
    match VALS.get(ACCEPT - 1) {
        Some(v) => {
            if *v != 929 {
                return false;
            }
        }
        None => return false,
    }
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if !r.green() {
        return false;
    }
    if r.total != CORPUS as u32 || r.accept != ACCEPT as u32 || r.reject != REJECT as u32 {
        return false;
    }
    if r.rate() != 1000 {
        return false;
    }
    let red = match run_with(Some((0, Fault::Code))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if red.matched != 13 || red.rate() != 929 || red.green() {
        return false;
    }
    if red.accept != 7 || red.reject != 7 || red.pass != 2 {
        return false;
    }
    if red.gate() != Err(AV_ECOUNT) {
        return false;
    }
    if red.degrade != 3 || red.partial != 1 || red.undef != 1 {
        return false;
    }
    let flip = match run_with(Some((CORPUS - 1, Fault::Ok))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if flip.matched != 13 || flip.rate() != 929 {
        return false;
    }
    if flip.accept != 9 || flip.reject != 5 || flip.degrade != 4 {
        return false;
    }
    if run_with(Some((CORPUS, Fault::Stage))) != Err(AV_ESLOT) {
        return false;
    }
    if run_with(Some((255, Fault::Ok))).is_err() != true {
        return false;
    }
    if run_with(None) != run() {
        return false;
    }
    let m = Matrix::of();
    if !m.reds_on_degrade_row() {
        return false;
    }
    if m.row(Tier::Degrade).is_none() {
        return false;
    }
    if let Some(x) = m.row(Tier::Degrade) {
        if x.red() != REJECT {
            return false;
        }
    }
    true
}

/// 渲染路四：现场文案逐字。
fn message_ok() -> bool {
    if ERR_ANCHOR_LIT != "busybox: 'foo': applet not found" {
        return false;
    }
    if msg_applet_not_found("busybox", "foo") != "busybox: 'foo': applet not found\n" {
        return false;
    }
    if msg_applet_not_found("", "") != ": '': applet not found\n" {
        return false;
    }
    if !msg_applet_not_found("x", "y").ends_with("applet not found\n") {
        return false;
    }
    if msg_applet_not_found("x", "y").contains("applet not found.") {
        return false;
    }
    if msg_try_list() != "Run 'busybox --list' to find all built-in applets.\n" {
        return false;
    }
    if msg_try_list_full() != "Run 'busybox --list-full' to see applets with their group prefix.\n"
    {
        return false;
    }
    if msg_try_group("editors", "ed") != "Run 'busybox editors ed' to invoke it through its group.\n"
    {
        return false;
    }
    if msg_try_group("", "") != "Run 'busybox  ' to invoke it through its group.\n" {
        return false;
    }
    if msg_cant_reexec("vi", "/tmp/vi")
        != "vi: can't re-exec '/tmp/vi': Operation not supported\n"
    {
        return false;
    }
    if !msg_cant_reexec("vi", "").contains("(unknown)") {
        return false;
    }
    if msg_no_name("busybox") != "busybox: can't determine applet name\n" {
        return false;
    }
    if msg_no_name("") != ": can't determine applet name\n" {
        return false;
    }
    if USAGE_VERSION != "BusyBox v1.36.1 multi-call binary.\n" {
        return false;
    }
    if FUNCTIONS_HEAD != "Currently defined functions:\n" {
        return false;
    }
    if GROUPS_HEAD != "Currently defined groups:\n" {
        return false;
    }
    if !USAGE_SYNOPSES.starts_with("Usage: busybox [function [arguments]...]\n") {
        return false;
    }
    if !USAGE_BLURB.starts_with("\tBusyBox is a multi-call binary") {
        return false;
    }
    if !USAGE_COPYRIGHT.contains("Licensed under GPLv2.") {
        return false;
    }
    let cs = cases();
    let mut i = 0usize;
    let mut degrade_n = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let o = c.actual();
        let m = message(&c.shape, &o);
        if o.note.degrade_face() {
            degrade_n += 1;
            if o.default {
                let line = msg_applet_not_found(c.shape.prog(), o.name);
                if !m.contains(line.as_str()) {
                    return false;
                }
                if !m.contains(banner_text(&o).as_str()) {
                    return false;
                }
                if !m.contains(hint_text(o.hint, o.group, o.name).as_str()) {
                    return false;
                }
                if nl_count(m.as_str()) != 18 {
                    return false;
                }
            } else if m.len() == 0 {
                return false;
            }
        } else if o.note.run_face() || o.note == Note::MainPath {
            if m.len() != 0 {
                return false;
            }
        }
        if o.note == Note::GroupUsage {
            if m.contains("applet not found") {
                return false;
            }
            if !m.contains(GROUPS_HEAD) && o.banner == Banner::Groups {
                return false;
            }
        }
        if o.note == Note::NoReexec && !m.contains("can't re-exec") {
            return false;
        }
        i += 1;
    }
    if degrade_n != 9 {
        return false;
    }
    let dangling = match case_at(8) {
        Some(v) => v,
        None => return false,
    };
    let err = errline_of(&dangling.shape, &dangling.actual());
    // 前缀是基名，不是 `argv[0]` 全文——路径留在现场，不进报错行。
    if err != "nope: 'nope': applet not found\n" {
        return false;
    }
    if find_from(dangling.shape.prog(), "/bin/nope", 0).is_some() {
        return false;
    }
    let ret = match case_at(5) {
        Some(v) => v,
        None => return false,
    };
    if errline_of(&ret.shape, &ret.actual()) != "@zzz: 'zzz': applet not found\n" {
        return false;
    }
    if ret.shape.prog() != "@zzz" {
        return false;
    }
    true
}

/// 渲染路五：逐槽释义行。
fn detail_ok() -> bool {
    let cs = cases();
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let d = detail(&c);
        let o = c.actual();
        // 错行原文自带行尾换行，所以带 `err=` 的那一档是"一行现场 + 原文"
        // 两个换行；不带错行的现场才是一个换行。两档都必须以换行收尾。
        let want_nl = if errline_of(&c.shape, &o).is_empty() {
            1
        } else {
            2
        };
        if !d.ends_with('\n') || nl_count(d.as_str()) != want_nl {
            return false;
        }
        if !d.starts_with(c.shape.prog()) {
            return false;
        }
        let keys = [
            " stage=",
            " code=",
            " exit=",
            " ready=",
            " note=",
            " hit=",
            " link=",
            " banner=",
            " flows=",
            " hint=",
        ];
        let mut j = 0usize;
        while j < keys.len() {
            if let Some(k) = keys.get(j) {
                if find_from(d.as_str(), k, 0).is_none() {
                    return false;
                }
            }
            j += 1;
        }
        if find_from(d.as_str(), o.stage.name(), 0).is_none() {
            return false;
        }
        if find_from(d.as_str(), " link=", 0).is_none() {
            return false;
        }
        if find_from(d.as_str(), o.hit.name(), 0).is_none() {
            return false;
        }
        if find_from(d.as_str(), o.banner.name(), 0).is_none() {
            return false;
        }
        if find_from(d.as_str(), o.flows.name(), 0).is_none() {
            return false;
        }
        if find_from(d.as_str(), o.hint.name(), 0).is_none() {
            return false;
        }
        if find_from(d.as_str(), o.note.name(), 0).is_none() {
            return false;
        }
        if find_from(d.as_str(), bool_text(o.default), 0).is_none() {
            return false;
        }
        if d.contains("claim=") || d.contains("\tat ") {
            return false;
        }
        if o.note.degrade_face() || o.note == Note::NoReexec {
            if find_from(d.as_str(), " err=\"", 0).is_none() {
                return false;
            }
        } else if find_from(d.as_str(), " err=", 0).is_some() {
            return false;
        }
        i += 1;
    }
    let run_case = match case_at(0) {
        Some(v) => v,
        None => return false,
    };
    if detail(&run_case) != "ls/ls stage=ready code=0 exit=0 ready=false note=ran hit=registered link=symlink-abs banner=suppressed flows=only-stdout hint=none\n"
    {
        return false;
    }
    let foo = match case_at(3) {
        Some(v) => v,
        None => return false,
    };
    if detail(&foo)
        != "foo/foo stage=ready code=2 exit=127 ready=true note=degraded hit=unregistered link=symlink-rel banner=full flows=two hint=try-list err=\"foo: 'foo': applet not found\n\"\n"
    {
        return false;
    }
    let grp = match case_at(6) {
        Some(v) => v,
        None => return false,
    };
    let gd = detail(&grp);
    // 释义行的字段次序是固定的（`hit` 后面还跟着 `link`），所以这三档只能
    // 逐档查，不能拼成一个连续子串——拼起来查就是查一个不存在版式。
    if find_from(gd.as_str(), " hit=group", 0).is_none()
        || find_from(gd.as_str(), " banner=groups", 0).is_none()
        || find_from(gd.as_str(), " hint=try-group", 0).is_none()
    {
        return false;
    }
    if find_from(gd.as_str(), "applet not found", 0).is_some() {
        return false;
    }
    let nx = match case_at(7) {
        Some(v) => v,
        None => return false,
    };
    let d = detail(&nx);
    if find_from(d.as_str(), "stage=exec code=38 exit=126", 0).is_none() {
        return false;
    }
    if find_from(d.as_str(), "can't re-exec", 0).is_none() {
        return false;
    }
    if detail(&Case::blank()).contains("err=\"") != true {
        return false;
    }
    let mut alt = match case_at(3) {
        Some(v) => v,
        None => return false,
    };
    alt.claim.note = Note::Sceneless;
    if detail(&alt) != detail(&foo) {
        return false;
    }
    true
}

/// 账本路一：阶段闸门单调。
fn gate_ok() -> bool {
    let mut g = Gate::new();
    if g.steps() != 0 || g.last() != -1 || g.closed() {
        return false;
    }
    let mut i = 0usize;
    while i < STAGES.len() {
        if let Some(s) = STAGES.get(i) {
            if g.step(*s) != Ok(()) {
                return false;
            }
        } else {
            return false;
        }
        if g.steps() as usize != i + 1 {
            return false;
        }
        if let Some(last) = STAGES.get(i) {
            if g.last() != last.order() as i32 {
                return false;
            }
        }
        i += 1;
    }
    if g.step(Stage::Ready) != Ok(()) {
        return false;
    }
    if g.steps() != 9 {
        return false;
    }
    if g.step(Stage::Argv0) != Err(AV_ESTAGE) {
        return false;
    }
    if g.step(Stage::Base) != Err(AV_ESTAGE) {
        return false;
    }
    if g.steps() != 9 || g.closed() {
        return false;
    }
    if g.close() != Ok(9) {
        return false;
    }
    if !g.closed() || g.step(Stage::Ready) != Err(AV_ESTATE) {
        return false;
    }
    if g.close() != Err(AV_ESTATE) {
        return false;
    }
    let mut back = Gate::new();
    if back.step(Stage::Stream) != Ok(()) {
        return false;
    }
    if back.step(Stage::Table) != Err(AV_ESTAGE) {
        return false;
    }
    if back.step(Stage::Stream) != Ok(()) {
        return false;
    }
    if back.last() != Stage::Stream.order() as i32 {
        return false;
    }
    if back.close() != Ok(2) {
        return false;
    }
    let mut fresh = Gate::new();
    if fresh.close() != Ok(0) {
        return false;
    }
    if fresh.close() != Err(AV_ESTATE) {
        return false;
    }
    let cs = cases();
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let mut w = Gate::new();
        let mut k = 0u8;
        let stop = c.actual().stage.order();
        while k <= stop {
            let s = match STAGES.get(k as usize) {
                Some(v) => *v,
                None => return false,
            };
            if s.order() != k {
                return false;
            }
            if w.step(s) != Ok(()) {
                return false;
            }
            k += 1;
        }
        if w.steps() as u8 != stop + 1 {
            return false;
        }
        if w.close().is_err() {
            return false;
        }
        i += 1;
    }
    true
}

/// 账本路二：逐格登记、对账、冻结与重放。
fn ledger_ok() -> bool {
    let mut l = build_ledger();
    if l.len() != SAMPLES || !l.is_empty() != true {
        return false;
    }
    if l.ok_count() != ACCEPT || l.err_count() != REJECT {
        return false;
    }
    if l.audit() != Ok(()) {
        return false;
    }
    if l.rejects() != 0 || l.is_frozen() || l.root() != 0 {
        return false;
    }
    if l.sample(SAMPLES).is_some() || l.sample(l.len()).is_some() {
        return false;
    }
    let mut i = 0usize;
    while i < SAMPLES {
        let s = match l.sample(i) {
            Some(v) => v,
            None => return false,
        };
        let w = match WANT_CELLS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if s.slot != w.slot || s.op != w.op || s.ok != w.ok || s.code != w.code {
            return false;
        }
        if s.value != w.value {
            return false;
        }
        if s.seal() == 0 {
            return false;
        }
        i += 1;
    }
    if l.sample(0).is_none() {
        return false;
    }
    if CAP as usize != REJECT || SAMPLES != CORPUS {
        return false;
    }
    if l.register(Sample { slot: 14, op: OP_REPLAY, ok: true, code: 0, value: 0 })
        != Err(AV_ESLOT)
    {
        return false;
    }
    if l.register(Sample::blank()) != Err(AV_ESLOT) {
        return false;
    }
    if l.rejects() != 2 || l.len() != SAMPLES {
        return false;
    }
    let sealed = l.seal();
    if sealed == 0 {
        return false;
    }
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if root != sealed || l.root() != root || !l.is_frozen() {
        return false;
    }
    if l.freeze().is_err() != true {
        return false;
    }
    if l.rejects() != 3 {
        return false;
    }
    if l.register(Sample { slot: 14, op: OP_REPLAY, ok: true, code: 0, value: 1 })
        != Err(AV_ESLOT)
    {
        return false;
    }
    if l.replay() != Ok(root) {
        return false;
    }
    let other = build_ledger();
    if other.replay() != Ok(root) {
        return false;
    }
    let mut bad = Ledger::new();
    if bad.audit() != Err(AV_EBAL) {
        return false;
    }
    if !bad.is_empty() || bad.len() != 0 || bad.sample(0).is_some() {
        return false;
    }
    if bad.register(Sample { slot: 3, op: OP_ARGV0, ok: true, code: 0, value: 0 })
        != Err(AV_ESLOT)
    {
        return false;
    }
    if bad.rejects() != 1 || !bad.is_empty() {
        return false;
    }
    if bad.register(count_cell(0, OP_CORPUS, Ok(14))) != Ok(()) {
        return false;
    }
    if bad.audit() != Err(AV_EBAL) {
        return false;
    }
    let mut wrong = Ledger::new();
    let mut i = 0usize;
    while i < SAMPLES {
        let mut cell = match WANT_CELLS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if i == 0 {
            cell.value = 13;
        }
        if wrong.register(cell) != Ok(()) {
            return false;
        }
        i += 1;
    }
    if wrong.audit() != Err(AV_EVALUE) {
        return false;
    }
    let mut jump = Ledger::new();
    if jump.register(Sample { slot: 0, op: OP_ARGV0, ok: false, code: 0, value: 0 }) != Ok(()) {
        return false;
    }
    if jump.audit() != Err(AV_EBAL) {
        return false;
    }
    let mut mix = Ledger::new();
    let mut i = 0usize;
    while i < SAMPLES {
        let mut cell = match WANT_CELLS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if i == 9 {
            cell.code = AV_ESTATE;
        }
        if mix.register(cell) != Ok(()) {
            return false;
        }
        i += 1;
    }
    if mix.audit() != Err(AV_EVALUE) {
        return false;
    }
    if mix.replay() == Ok(mix.seal()) {
        return false;
    }
    if mix.replay() != Err(AV_ESEAL) {
        return false;
    }
    true
}

// ---------------------------------------------------------------------------
// 十三判据（UNX-F11662 自检出口）
// ---------------------------------------------------------------------------

/// 本域的十三条判据。
///
/// 三条走表路（名称/枚举/链接形态）、两条走数据路（组表成员表、八闸纵线）、
/// 一条走输出面成形（横幅双流指路）、四条走判与统计（对判、矩阵、语料、
/// 槽位红档）、两条走文本与账（文案逐字、释义与账本）、一条收尾联签
/// （冻结重放四注入）。
pub fn run_unx11662_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11662_argv0_unregistered");

    s.add("AV01-名称与段基址", name_ok(), "static");

    s.add("AV02-枚举与纵线", enums_ok(), "static");

    s.add("AV03-链接形态六式", link_ok(), "static");

    s.add("AV04-组表与成员表两跳", table_ok(), "static");

    s.add("AV05-八闸降级纵线", define_ok(), "static");

    s.add("AV06-横幅双流与指路", banner_ok(), "static");

    s.add("AV07-申报对判六维", judge_ok(), "static");

    s.add("AV08-矩阵统计", matrix_ok(), "static");

    s.add("AV09-语料与渲染", corpus_ok(), "static");

    s.add("AV10-槽位与红档显式", slots_ok() && reds_ok(), "static");

    s.add("AV11-文案逐字", message_ok(), "static");

    s.add("AV12-释义与账本", detail_ok() && ledger_ok() && gate_ok(), "static");

    s.add(
        "AV13-冻结重放四注入",
        {
            let mut l = build_ledger();
            let a = l.audit() == Ok(());
            let b = match l.freeze() {
                Ok(r) => r != 0 && l.is_frozen() && l.replay() == Ok(r),
                Err(_) => false,
            };
            let inj = inject_four();
            a && b && inj.iter().all(|v| *v)
        },
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 超长名（真机不查长度，本卡也不许在闸三判死它）。
    const LONG_NAME: &str = concat!("abcdefghij", "abcdefghij", "abcdefghij", "abcdefghij", "kl");

    /// 造一个"只坏申报、不坏现场"的用例：现场固定取第 3 槽（完整降级）。
    fn degrade_case(k: Claim) -> Case {
        Case {
            shape: match SHAPES.get(3) {
                Some(v) => *v,
                None => Shape::blank(),
            },
            claim: k,
        }
    }

    /// 现场四要素（阶段/errno/退出码/注解）＋齐备位，便于逐槽核对。
    fn face(c: &Case) -> (Stage, i32, u16, Note, bool) {
        let a = c.actual();
        (a.stage, a.code, a.value, a.note, a.default)
    }

    #[test]
    fn seg_base_is_reserved_and_ladder_is_thirteen() {
        // 段基址是批次分配的 0x6d110，前后 0x6d100/0x6d120 都已被占。
        assert_eq!(SEG_BASE, 0x6d110);
        assert_eq!(AV_ESTAGE, SEG_BASE + 0x01);
        assert_eq!(AV_ECODE, SEG_BASE + 0x02);
        assert_eq!(AV_EVALUE, SEG_BASE + 0x03);
        assert_eq!(AV_ENOTE, SEG_BASE + 0x04);
        assert_eq!(AV_EDEFAULT, SEG_BASE + 0x05);
        assert_eq!(AV_ELINK, SEG_BASE + 0x06);
        assert_eq!(AV_ESTATE, SEG_BASE + 0x07);
        assert_eq!(AV_ECOUNT, SEG_BASE + 0x08);
        assert_eq!(AV_ESLOT, SEG_BASE + 0x09);
        assert_eq!(AV_ESEAL, SEG_BASE + 0x0a);
        assert_eq!(AV_ENOBS, SEG_BASE + 0x0b);
        assert_eq!(AV_EMATRIX, SEG_BASE + 0x0c);
        assert_eq!(AV_EBAL, SEG_BASE + 0x0d);
        // 阶梯不得越段（+0x0d 仍在 0x10 的段宽内）。
        assert!(AV_EBAL < SEG_BASE + 0x10);
    }

    #[test]
    fn scale_constants_are_the_contract_numbers() {
        assert_eq!(CORPUS, 14);
        assert_eq!(SAMPLES, 14);
        assert_eq!(ACCEPT, 8);
        assert_eq!(REJECT, 6);
        assert_eq!(TIER_COUNT, 4);
        assert_eq!(ACCEPT + REJECT, CORPUS);
        assert_eq!(GROUP_COUNT, 11);
        assert_eq!(TABLE_COUNT, 27);
        assert_eq!(SELF_NAME_COUNT, 4);
        assert_eq!(CAP, REJECT);
    }

    #[test]
    fn exit_code_ladder_keeps_127_126_and_1_apart() {
        // 本卡最容易被合并的三档：降级 127、重启判死 126、applet 自败 1。
        assert_eq!(EXIT_DEGRADE, 127);
        assert_eq!(EXIT_EXEC, 126);
        assert_eq!(EXIT_USAGE, 1);
        assert_eq!(EXIT_APPLET_FAIL, 1);
        assert_eq!(EXIT_GROUP_USAGE, 1);
        assert_eq!(EXIT_RUN_OK, 0);
        assert_ne!(EXIT_DEGRADE, EXIT_EXEC);
        assert_ne!(EXIT_DEGRADE, EXIT_APPLET_FAIL);
    }

    #[test]
    fn errno_ladder_matches_linux_values() {
        assert_eq!(ERR_ENOENT, 2);
        assert_eq!(ERR_ENOSYS, 38);
        assert_eq!(ERR_EINVAL, 22);
        assert_eq!(ERR_EBADARG, -1);
        assert_eq!(ERR_NONE, 0);
        assert_eq!(i32_text(ERR_EBADARG), "-1");
        assert_eq!(i32_text(ERR_NONE), "0");
        assert_eq!(i32_text(ERR_ENOENT), "2");
    }

    #[test]
    fn eight_gates_are_ordered_and_split_at_surface() {
        assert_eq!(STAGES.len(), 8);
        let mut i = 0usize;
        while i < STAGES.len() {
            let s = match STAGES.get(i) {
                Some(v) => *v,
                None => panic!("stage table short"),
            };
            assert_eq!(s.order() as usize, i);
            // 闸六起才谈得上"三件套"。
            assert_eq!(s.on_surface(), i >= 5);
            assert_eq!(s.sceneless(), i <= 2);
            i += 1;
        }
        assert_eq!(Stage::Ready.name(), "ready");
        assert_eq!(Stage::Argv0.name(), "argv0");
        assert!(!Stage::Exec.on_surface());
        assert!(Stage::Banner.on_surface());
    }

    #[test]
    fn hit_five_arms_classify_the_table_answer() {
        assert_eq!(HITS.len(), 5);
        assert!(Hit::Registered.runnable());
        assert!(Hit::Alias.runnable());
        assert!(!Hit::Group.runnable());
        // 降级面只有这两档——被裁件与未注册名同形，内部维度不外溢。
        assert!(Hit::Trimmed.degrades());
        assert!(Hit::Unregistered.degrades());
        assert!(!Hit::Registered.degrades());
        assert!(!Hit::Alias.degrades());
        assert!(!Hit::Group.degrades());
        assert!(Hit::Group.group_face());
        assert!(!Hit::Trimmed.group_face());
        assert!(Hit::Trimmed.internal());
        assert!(!Hit::Unregistered.internal());
        let mut names: Vec<&str> = Vec::new();
        for h in HITS.iter() {
            assert!(!names.contains(&h.name()), "答复档文本重复");
            names.push(h.name());
        }
    }

    #[test]
    fn banner_flows_and_hint_arms_are_unique() {
        let banners = [Banner::Full, Banner::Groups, Banner::GroupOnly, Banner::Suppressed];
        let mut bn: Vec<&str> = Vec::new();
        for b in banners.iter() {
            assert!(!bn.contains(&b.name()));
            bn.push(b.name());
        }
        assert!(Banner::Full.complete());
        assert!(!Banner::Groups.complete());
        assert!(!Banner::Suppressed.complete());
        assert!(!Banner::Suppressed.shows_list());
        assert!(Banner::GroupOnly.shows_list());
        assert_eq!(Banner::Full.stream(), "stdout");
        assert_eq!(Banner::Suppressed.stream(), "none");

        assert_eq!(FLOWS.len(), 5);
        assert!(Flows::Two.two());
        assert!(!Flows::Merged.two());
        assert!(Flows::Two.err_visible() && Flows::Two.banner_visible());
        assert!(!Flows::OnlyStdout.err_visible());
        assert!(!Flows::OnlyStderr.banner_visible());
        assert!(Flows::Merged.err_visible() && Flows::Merged.banner_visible());
        assert!(Flows::Suppressed.lost());
        assert!(!Flows::Two.lost());

        assert_eq!(HINTS.len(), 4);
        assert!(!Hint::None.present());
        assert!(Hint::TryList.present());
        assert!(Hint::TryListFull.present());
        assert!(Hint::TryGroup.points_to_group());
        assert!(!Hint::TryList.points_to_group());
    }

    #[test]
    fn thirteen_notes_cover_every_face_exactly_once() {
        assert_eq!(NOTES.len(), 13);
        let mut nm: Vec<&str> = Vec::new();
        for n in NOTES.iter() {
            assert!(!nm.contains(&n.name()), "注解文本重复");
            nm.push(n.name());
        }
        for n in NOTES.iter() {
            // 跑通面 / 降级面 / 无现场面 三面互斥，一档只属一面。
            let faces = [n.run_face(), n.degrade_face(), n.no_scene()];
            assert!(faces.iter().filter(|v| **v).count() <= 1);
        }
        assert!(Note::Ran.run_face() && Note::AliasRan.run_face() && Note::ReExecRun.run_face());
        assert!(Note::Degraded.exit_127_face());
        assert!(Note::Hintless.exit_127_face());
        // 组用法与重启判死都不是 127 面。
        assert!(!Note::GroupUsage.exit_127_face());
        assert!(!Note::NoReexec.exit_127_face());
        assert!(!Note::GroupUsage.degrade_face());
        assert!(Note::Sceneless.no_scene() && Note::Baseless.no_scene() && Note::AtBare.no_scene());
    }

    #[test]
    fn tier_projection_only_looks_at_the_actual_result() {
        let mut o = Outcome::blank();
        // 判死在输出面之前——齐备位无从谈起。
        assert_eq!(Tier::of(&o), Tier::Undef);
        o.stage = Stage::Stream;
        assert_eq!(Tier::of(&o), Tier::Undef);
        o.stage = Stage::Ready;
        o.value = EXIT_RUN_OK;
        assert_eq!(Tier::of(&o), Tier::Pass);
        o.value = EXIT_DEGRADE;
        o.default = false;
        assert_eq!(Tier::of(&o), Tier::Partial);
        o.default = true;
        assert_eq!(Tier::of(&o), Tier::Degrade);
        // 组用法退 1，不是跑通也不是降级。
        let g = match case_at(6) {
            Some(v) => v.actual(),
            None => panic!("no slot 6"),
        };
        assert_eq!(Tier::of(&g), Tier::Partial);
        assert_eq!(Tier::Pass.index(), 0);
        assert_eq!(Tier::Undef.index(), 3);
        assert!(Tier::Pass.success());
        assert!(!Tier::Degrade.success());
        assert!(Tier::Degrade.degrade_face() && Tier::Partial.degrade_face());
        assert!(!Tier::Undef.degrade_face());
        assert!(Tier::Degrade.ready_allowed());
        assert!(!Tier::Partial.ready_allowed());
        assert!(!Tier::Pass.ready_allowed());
    }

    #[test]
    fn fault_codes_pair_one_to_one_with_the_ladder() {
        assert_eq!(FAULTS.len(), 14);
        assert_eq!(Fault::Ok.code(), 0);
        assert!(!Fault::Ok.alive());
        assert!(Fault::Estate.alive());
        let mut i = 1usize;
        while i < FAULTS.len() {
            let f = match FAULTS.get(i) {
                Some(v) => *v,
                None => panic!("fault table short"),
            };
            assert_eq!(f.code(), SEG_BASE + i as u32);
            assert!(!fault_short(f).is_empty());
            i += 1;
        }
        assert_eq!(fault_short(Fault::Default), "default");
        assert_eq!(fault_short(Fault::Link), "link");
        assert_eq!(Fault::Link.code(), AV_ELINK);
    }

    #[test]
    fn six_link_forms_have_their_own_reexec_rules() {
        assert_eq!(LINKS.len(), 6);
        let mut ln: Vec<&str> = Vec::new();
        for l in LINKS.iter() {
            assert!(!ln.contains(&l.name()));
            ln.push(l.name());
        }
        // shebang 的重启权内核握着，busybox 说了不算——永远判死。
        assert!(!Link::Shebang.reexec_allowed(false));
        assert!(!Link::Shebang.reexec_allowed(true));
        // 改名副本只在 MMU 下有 /proc/self/exe 可用。
        assert!(Link::RenamedCopy.reexec_allowed(false));
        assert!(!Link::RenamedCopy.reexec_allowed(true));
        assert!(Link::SymlinkRel.reexec_allowed(true));
        assert!(Link::Hardlink.reexec_allowed(true));
        assert!(Link::ExecArgv0.reexec_allowed(true));
        // 只有三式允许 @ 改形。
        assert!(Link::SymlinkAbs.retitle_capable());
        assert!(Link::ExecArgv0.retitle_capable());
        assert!(!Link::RenamedCopy.retitle_capable());
        assert!(!Link::Shebang.retitle_capable());
        // 带目录的三式才需要 basename。
        assert!(Link::SymlinkAbs.keeps_dirpath());
        assert!(Link::Shebang.keeps_dirpath());
        assert!(!Link::SymlinkRel.keeps_dirpath());
        assert!(!Link::ExecArgv0.keeps_dirpath());
        assert!(Link::ExecArgv0.may_have_no_name());
        assert!(!Link::Hardlink.may_have_no_name());
        assert!(Link::SymlinkRel.targets_busybox());
        assert!(!Link::Hardlink.targets_busybox());
    }

    #[test]
    fn member_table_counts_are_self_consistent() {
        let t = table();
        assert_eq!(t.len(), TABLE_COUNT);
        assert_eq!(primary_count(), 25);
        assert_eq!(alias_count(), 2);
        assert_eq!(trimmed_count(), 1);
        assert_eq!(group_only_count(), 2);
        assert_eq!(dispatchable_count(), 24);
        // 注册序必须逐行 +1，别名不许插队。
        let mut i = 0usize;
        while i < t.len() {
            let row = match t.get(i) {
                Some(v) => *v,
                None => panic!("table hole"),
            };
            assert_eq!(row.prio as usize, i);
            assert!(!row.name.is_empty());
            assert!(row.is_primary() != row.is_alias());
            i += 1;
        }
    }

    #[test]
    fn alias_rows_point_at_a_live_primary() {
        // 别名指向主名，指错就等于把 `ash` 跑成不存在的实现。
        let t = table();
        let mut n = 0usize;
        let mut i = 0usize;
        while i < t.len() {
            let row = match t.get(i) {
                Some(v) => *v,
                None => panic!("table hole"),
            };
            if row.is_alias() {
                n += 1;
                let tgt = match row_at(row.alias_of) {
                    Some(v) => v,
                    None => panic!("别名指向不在表里的名字"),
                };
                assert!(tgt.is_primary(), "别名只能指主名");
                assert!(tgt.registered, "别名不能指向被裁件");
                assert_eq!(tgt.group, row.group);
            }
            i += 1;
        }
        assert_eq!(n, 2);
        assert_eq!(row_at("hush").map(|x| x.alias_of), Some("sh"));
        assert!(row_at("sh").map(|x| x.is_primary()).unwrap_or(false));
    }

    #[test]
    fn group_table_members_agree_with_the_member_rows() {
        let g = groups();
        assert_eq!(g.len(), GROUP_COUNT);
        let mut i = 0usize;
        while i < g.len() {
            let x = match g.get(i) {
                Some(v) => *v,
                None => panic!("group hole"),
            };
            assert_eq!(group_primary_count(x.name), x.members as usize, "{} 行数不符", x.name);
            assert_eq!(
                group_individual_count(x.name),
                x.individuals as usize,
                "{} 可直调数不符",
                x.name
            );
            assert!(x.individuals <= x.members);
            i += 1;
        }
        assert_eq!(group_index("coreutils"), Some(0));
        assert_eq!(group_index("crypto"), Some(10));
        // 未命中必须 None——返 0 就是把 `coreutils` 的账记给陌生名字。
        assert_eq!(group_index("coreutils2"), None);
        assert_eq!(row_index("lss"), None);
        assert_eq!(row_index("ls"), Some(0));
    }

    #[test]
    fn group_member_list_hides_the_trimmed_applet() {
        // 清单里列出被裁件就是骗人：它压根没进 applet_name[]。
        let editors = group_member_list("editors", ", ");
        assert!(editors.contains("awk"));
        assert!(editors.contains("vi"));
        assert!(!list_has(editors.as_str(), ", ", "ed"), "被裁件不许进清单");
        assert_eq!(list_items(editors.as_str(), ", "), 3);
        let core = group_member_list("coreutils", ", ");
        assert_eq!(list_items(core.as_str(), ", "), 10);
        // 空组列出来就是空串，不能是空格串。
        assert_eq!(group_member_list("fsck", ", "), String::new());
        assert_eq!(list_items("", ", "), 0);
    }

    #[test]
    fn list_membership_is_word_bounded_not_substring() {
        // `ping` 含 `in`：无边界判定会把不存在的 applet 说成在表里。
        let list = "ping,talk,find";
        assert!(list_has(list, ",", "ping"));
        assert!(list_has(list, ",", "find"));
        assert!(!list_has(list, ",", "in"));
        assert!(!list_has(list, ",", "ing"));
        assert!(!list_has(list, ",", ""));
        assert_eq!(list_items(list, ","), 3);
        let joined = join_names(["ls", "cat", "dd"].as_slice(), ", ");
        assert_eq!(joined, "ls, cat, dd");
        assert!(list_has(joined.as_str(), ", ", "cat"));
        assert!(!list_has(joined.as_str(), ", ", "cats"));
    }

    #[test]
    fn name_hash_and_bucket_never_substitute_for_strcmp() {
        assert_eq!(name_hash(""), 0);
        assert_eq!(name_hash("ls"), name_hash("ls"));
        assert_ne!(name_hash("ls"), name_hash("sl"));
        assert_ne!(name_hash("ls"), name_hash("cat"));
        // 桶号相同不等于同名：散列只能缩小候选，判等还得 strcmp。
        assert_eq!(hash_bucket("ls", 0), 0);
        assert_eq!(hash_bucket("cat", 0), 0);
        assert_ne!(row_at("ls").map(|x| x.name), row_at("cat").map(|x| x.name));
        let mut i = 1u32;
        while i < 8 {
            assert!(hash_bucket("coreutils", i) < (1u32 << i));
            i += 1;
        }
    }

    #[test]
    fn basename_and_slash_shape_drive_the_second_gate() {
        assert_eq!(last_component("/bin/ls"), "ls");
        assert_eq!(last_component("ls"), "ls");
        assert_eq!(last_component("/usr/sbin/"), "");
        assert_eq!(dir_prefix("/bin/ls"), Some("/bin"));
        assert_eq!(dir_prefix("ls"), None);
        assert_eq!(slash_shape("/usr/sbin/"), (true, true));
        assert_eq!(slash_shape("/bin/ls"), (true, false));
        assert_eq!(slash_shape("ls"), (false, false));
        assert!(is_dots(".."));
        assert!(is_dots("."));
        assert!(!is_dots(""));
        assert!(!is_dots(".ls"));
        assert!(has_control("a\tb"));
        assert!(!has_control("busybox"));
        assert_eq!(commas_to_slashes("/bin,ls"), "/bin/ls");
    }

    #[test]
    fn at_retitle_moves_the_name_but_not_the_prefix() {
        // 分发名走 argv[1]，报错前缀留在 argv[0]——两问分开是本卡的分工。
        let n = applet_name("@zzz", "real", 2, false, Link::SymlinkAbs);
        assert_eq!(n.text(), "real");
        assert_eq!(prog_prefix("@zzz", "real", 2, false), "@zzz");
        let msg = msg_applet_not_found(prog_prefix("@zzz", "real", 2, false), n.text());
        assert_eq!(msg, "@zzz: 'real': applet not found\n");
        // argc==1 时 @ 改形没有名字可取，键就是带 @ 的原串。
        let one = applet_name("@busybox", "", 1, false, Link::SymlinkAbs);
        assert_eq!(one.text(), "@busybox");
        // 链不在场时什么都不给。
        assert_eq!(applet_name("@zzz", "real", 2, true, Link::SymlinkAbs), Resolved::Absent);
        assert_eq!(prog_prefix("@zzz", "real", 2, true), "");
        assert_eq!(prog_prefix("@zzz", "real", 0, false), "");
    }

    #[test]
    fn empty_argv0_prefix_is_not_borrowed_from_argv1() {
        // 内核给了空 argv[0] 时，报错行的程序名就是空——借 argv[1] 等于编名字。
        assert_eq!(prog_prefix("", "ls", 2, false), "");
        assert_eq!(prog_prefix("", "", 1, false), "");
        assert_eq!(msg_no_name(prog_prefix("", "ls", 2, false)), ": can't determine applet name\n");
        assert_eq!(
            msg_applet_not_found(prog_prefix("", "ls", 2, false), ""),
            ": '': applet not found\n"
        );
    }

    #[test]
    fn shebang_and_copy_never_claim_retitle() {
        let mut sh = Shape::blank();
        sh.argv0 = "@busybox";
        sh.argv1 = "ls";
        sh.argc = 2;
        sh.link = Link::Shebang;
        assert!(!sh.retitled());
        // 非可改形式下 `@` 就是名字的一部分，键带着 @ 进表。
        assert_eq!(sh.key(), "@busybox");
        let mut ok = Shape::blank();
        ok.argv0 = "@busybox";
        ok.argv1 = "ls";
        ok.argc = 2;
        ok.link = Link::Hardlink;
        assert!(ok.retitled());
        assert_eq!(ok.key(), "ls");
        assert_eq!(ok.prog(), "@busybox");
        assert_eq!(exec_path("/bin/busybox", false, Link::RenamedCopy), Some("/bin/busybox"));
        assert_eq!(exec_path("busybox", false, Link::SymlinkAbs), None);
        assert_eq!(exec_path("/bin/busybox", true, Link::SymlinkAbs), None);
    }

    #[test]
    fn empty_argv_scene_dies_at_the_first_gate() {
        let mut sh = Shape::blank();
        sh.absent = true;
        sh.argc = 0;
        let o = define(&sh);
        assert_eq!(o.stage, Stage::Argv0);
        assert_eq!(o.code, ERR_EBADARG);
        assert_eq!(o.note, Note::Sceneless);
        assert_eq!(o.flows, Flows::Suppressed);
        assert!(!o.default);
        assert!(!o.ok());
        assert_eq!(o.tier(), Tier::Undef);
        // 现场不成立时一块输出都没有。
        assert_eq!(message(&sh, &o), String::new());
        // 错行**文本**照样拼得出来（`bb_error_msg` 的那一句），但双流档是全丢，
        // 所以它一个字都不落地——判据看的是输出面，不是拼装函数。
        assert_eq!(errline_of(&sh, &o), ": can't determine applet name\n");

        // argc==0 与 absent==true 同判死：两个档，一条出口。
        let mut zero = Shape::blank();
        zero.argc = 0;
        zero.absent = false;
        assert_eq!(define(&zero), o);
    }

    #[test]
    fn trailing_slash_stops_at_base_not_at_table() {
        let mut sh = Shape::blank();
        sh.argv0 = "/usr/sbin/";
        sh.link = Link::SymlinkAbs;
        let o = define(&sh);
        assert_eq!(o.stage, Stage::Base);
        assert_eq!(o.code, ERR_EINVAL);
        assert_eq!(o.value, EXIT_USAGE);
        assert_eq!(o.note, Note::Baseless);
        assert!(o.name.is_empty());
        assert_eq!(o.flows, Flows::OnlyStderr);
        assert_eq!(o.tier(), Tier::Undef);
        // 基名不可推的现场只有一条错行，没有横幅。
        let m = message(&sh, &o);
        assert!(m.contains("can't determine applet name"));
        assert!(!m.contains("BusyBox"));
        assert_eq!(nl_count(m.as_str()), 1);
    }

    #[test]
    fn bare_at_marker_is_a_name_gate_failure() {
        let mut sh = Shape::blank();
        sh.argv0 = "@";
        sh.argv1 = "";
        sh.argc = 1;
        sh.link = Link::SymlinkAbs;
        let o = define(&sh);
        assert_eq!(o.stage, Stage::Name);
        assert_eq!(o.note, Note::AtBare);
        assert_eq!(o.code, ERR_EINVAL);
        assert_eq!(sh.key(), "@");
        assert!(!o.ok());
    }

    #[test]
    fn malformed_names_are_not_lex_failures_on_this_path() {
        // 带 / 的基名、纯点、超长名——链接路上都进表，判死在 table 不在 name。
        let mut sh = Shape::blank();
        sh.argv0 = "a/b";
        sh.link = Link::SymlinkRel;
        assert_eq!(define(&sh).stage, Stage::Ready);
        assert_eq!(define(&sh).hit, Hit::Unregistered);

        let mut dots = Shape::blank();
        dots.argv0 = "..";
        dots.link = Link::SymlinkRel;
        let od = define(&dots);
        assert_eq!(od.hit, Hit::Unregistered);
        assert_eq!(od.value, EXIT_DEGRADE);
        assert!(is_dots(od.name));

        let mut lng = Shape::blank();
        lng.argv0 = LONG_NAME;
        lng.link = Link::SymlinkRel;
        assert!(text_len(lng.argv0) > APPLET_NAME_MAX);
        let ol = define(&lng);
        // 真机不设名字长度上限：超长名照样降级，齐备位照样为真。
        assert_eq!(ol.stage, Stage::Ready);
        assert_eq!(ol.note, Note::Degraded);
        assert!(ol.default);
        assert_eq!(ol.tier(), Tier::Degrade);
    }

    #[test]
    fn lookup_is_case_sensitive_and_exact() {
        assert!(row_at("ls").is_some());
        assert!(row_at("LS").is_none());
        assert!(row_at("Vi").is_none());
        assert!(row_at("l s").is_none());
        let mut sh = Shape::blank();
        sh.argv0 = "LS";
        sh.link = Link::SymlinkRel;
        let o = define(&sh);
        assert_eq!(o.hit, Hit::Unregistered);
        assert_eq!(o.code, ERR_ENOENT);
        assert_eq!(o.value, EXIT_DEGRADE);
    }

    #[test]
    fn busybox_self_name_is_handed_over_not_judged() {
        // `/bin/busybox foo` 由 F11661 判：本卡只把它归位，不报降级块。
        for nm in ["busybox", "@busybox", "/bin/busybox", "init"].iter() {
            assert!(is_self_name(nm));
            let mut sh = Shape::blank();
            sh.argv0 = nm;
            sh.argv1 = "ls";
            sh.argc = 2;
            sh.link = Link::SymlinkAbs;
            let o = define(&sh);
            assert_eq!(o.note, Note::MainPath, "{} 未归位", nm);
            assert_eq!(o.stage, Stage::Name);
            assert!(!o.ok());
            assert_eq!(o.tier(), Tier::Undef);
            // 本尊自调用打的是横幅（归 F11661 的现场），但绝不许长出降级块。
            let m = message(&sh, &o);
            assert!(m.contains(USAGE_VERSION), "{} 该打横幅", nm);
            assert!(!m.contains("applet not found"), "{} 被判成了降级", nm);
            assert_eq!(m, banner_text(&o));
            assert_eq!(o.flows, Flows::OnlyStdout);
        }
        assert!(!is_self_name("busybox2"));
        assert!(!is_self_name(""));
    }

    #[test]
    fn individual_name_runs_without_any_error_face() {
        let c = match case_at(0) {
            Some(v) => v,
            None => panic!("no slot 0"),
        };
        let o = c.actual();
        assert_eq!(o.stage, Stage::Ready);
        assert_eq!(o.code, ERR_NONE);
        assert_eq!(o.value, EXIT_RUN_OK);
        assert_eq!(o.note, Note::Ran);
        assert_eq!(o.hit, Hit::Registered);
        assert_eq!(o.banner, Banner::Suppressed);
        assert_eq!(o.flows, Flows::OnlyStdout);
        assert_eq!(o.hint, Hint::None);
        assert!(o.ok());
        assert!(!o.degraded());
        assert_eq!(o.path, Some("/bin/ls"));
        assert_eq!(message(&c.shape, &o), String::new());
    }

    #[test]
    fn alias_link_runs_the_primary_implementation() {
        // `ash` 是 `sh` 的别名：跑通了，但注解必须如实标出跑的不是同名实现。
        let c = match case_at(1) {
            Some(v) => v,
            None => panic!("no slot 1"),
        };
        let o = c.actual();
        assert_eq!(o.hit, Hit::Alias);
        assert_eq!(o.note, Note::AliasRan);
        assert_eq!(o.value, EXIT_RUN_OK);
        assert_eq!(o.tier(), Tier::Pass);
        assert_eq!(row_at("ash").map(|x| x.alias_of), Some("sh"));
        assert_eq!(row_at("sh").map(|x| x.alias_of), Some(""));
    }

    #[test]
    fn group_only_applet_reexecs_and_reports_a_pass() {
        let c = match case_at(2) {
            Some(v) => v,
            None => panic!("no slot 2"),
        };
        let o = c.actual();
        assert_eq!(o.hit, Hit::Group);
        assert_eq!(o.note, Note::ReExecRun);
        assert_eq!(o.stage, Stage::Ready);
        assert_eq!(o.tier(), Tier::Pass);
        // 只能进组调用的名字，按个体名直调**不该**报 not found。
        assert!(!o.degraded());
        assert_eq!(row_at("vi").map(|x| x.individual), Some(false));
        assert!(row_at("vi").map(|x| x.registered).unwrap_or(false));
    }

    #[test]
    fn nommu_renamed_copy_dies_at_exec_with_126() {
        let c = match case_at(7) {
            Some(v) => v,
            None => panic!("no slot 7"),
        };
        let o = c.actual();
        assert_eq!(o.stage, Stage::Exec);
        assert_eq!(o.code, ERR_ENOSYS);
        // 126 与 127 的分工：重启不了不是"名字找不到"。
        assert_eq!(o.value, EXIT_EXEC);
        assert_ne!(o.value, EXIT_DEGRADE);
        assert_eq!(o.note, Note::NoReexec);
        assert_eq!(o.flows, Flows::OnlyStderr);
        assert_eq!(o.banner, Banner::Suppressed);
        assert_eq!(o.tier(), Tier::Undef);
        let m = message(&c.shape, &o);
        assert!(m.contains("can't re-exec '/tmp/vi'"));
        assert!(!m.contains("applet not found"));
    }

    #[test]
    fn shebang_group_applet_dies_even_with_mmu() {
        let mut sh = Shape::blank();
        sh.argv0 = "/bin/vi";
        sh.link = Link::Shebang;
        sh.nommu = false;
        let o = define(&sh);
        assert_eq!(o.stage, Stage::Exec);
        assert_eq!(o.note, Note::NoReexec);
        assert_eq!(o.value, EXIT_EXEC);
        assert_eq!(o.path, Some("/bin/vi"));
    }

    #[test]
    fn group_name_invocation_is_usage_not_degrade() {
        let c = match case_at(6) {
            Some(v) => v,
            None => panic!("no slot 6"),
        };
        let o = c.actual();
        assert_eq!(o.hit, Hit::Group);
        assert_eq!(o.note, Note::GroupUsage);
        assert_eq!(o.value, EXIT_GROUP_USAGE);
        assert_eq!(o.code, ERR_NONE);
        assert!(o.ok());
        assert!(!o.default);
        assert!(!o.degraded());
        assert_eq!(o.tier(), Tier::Partial);
        assert_eq!(o.banner, Banner::Groups);
        let m = message(&c.shape, &o);
        assert!(m.contains(GROUPS_HEAD));
        // 组用法块里绝不能出现 not found。
        assert!(!m.contains("applet not found"));
    }

    #[test]
    fn group_hint_points_at_a_member_placeholder_not_itself() {
        // `busybox coreutils coreutils` 是把用户引回死路的指路行。
        let c = match case_at(6) {
            Some(v) => v,
            None => panic!("no slot 6"),
        };
        let o = c.actual();
        assert_eq!(hint_member(&o), "<applet>");
        let m = message(&c.shape, &o);
        assert!(m.contains("Run 'busybox coreutils <applet>'"));
        assert!(!m.contains("coreutils coreutils"));
        // 被裁件那一档填的却是真名字。
        let t = match case_at(4) {
            Some(v) => v,
            None => panic!("no slot 4"),
        };
        let ot = t.actual();
        assert_eq!(hint_member(&ot), "ed");
        assert!(message(&t.shape, &ot).contains("Run 'busybox editors ed'"));
    }

    // ===================================================================
    // 第二部分：判序遮蔽、期望表对拍、渲染与注入、账本与闸门、检查出口
    // ===================================================================

    /// 取一槽。越界就炸在取槽这一步——测试宁可炸在开头。
    fn slot(i: usize) -> Case {
        match case_at(i) {
            Some(v) => v,
            None => panic!("no slot {}", i),
        }
    }

    /// 取一格账本样张（与 `WANT_CELLS` 同序）。
    fn cell(i: usize) -> Sample {
        match WANT_CELLS.get(i) {
            Some(v) => *v,
            None => panic!("no cell {}", i),
        }
    }

    #[test]
    fn estate_verdict_masks_the_four_finer_dims() {
        // 申报把降级面认成"跑通了"：`stage` 恰好对得上（`success()` 由
        // `stage`+`code` 派生），后四维全错却一律不报——粗维遮蔽细维。
        let c = slot(8);
        let a = c.actual();
        let k = c.claim;
        assert_eq!(a.stage, Stage::Ready);
        assert_eq!(k.stage, a.stage);
        assert_ne!(k.code, a.code);
        assert_ne!(k.value, a.value);
        assert_ne!(k.note, a.note);
        assert_ne!(k.default, a.default);
        assert_ne!(k.success(), a.ok());
        assert_eq!(judge(&c), Fault::Estate);
        assert_eq!(expected(8), Some(Fault::Estate));
        assert_eq!(REJECT_FAULTS.first(), Some(&Fault::Estate));
        assert_eq!(k.code, ERR_NONE);
        assert_eq!(k.value, EXIT_RUN_OK);
    }

    #[test]
    fn coarser_dim_wins_when_several_are_broken() {
        // 在 `stage` 已经判错的基础上把后四维全写错：判罚仍只报 `stage`。
        let mut c = slot(9);
        c.claim.code = ERR_ENOSYS;
        c.claim.value = EXIT_EXEC;
        c.claim.note = Note::NoReexec;
        c.claim.default = false;
        assert_eq!(judge(&c), Fault::Stage);
        // 只把 `stage` 改回与现场一致，缺陷才轮到 `code` 露头。
        let mut d = slot(9);
        let live = d.actual();
        d.claim.stage = live.stage;
        d.claim.code = ERR_ENOSYS;
        d.claim.value = EXIT_RUN_OK;
        d.claim.note = Note::Ran;
        d.claim.default = false;
        assert_eq!(judge(&d), Fault::Code);
        // `note` 与 `default` 同理：都排在 `value` 之后。
        let mut e = slot(11);
        let lv = e.actual();
        e.claim.note = Note::Sceneless;
        e.claim.default = false;
        e.claim.value = lv.value;
        assert_eq!(judge(&e), Fault::Value);
    }

    #[test]
    fn each_reject_slot_after_the_first_breaks_exactly_one_dim() {
        // 第 8 槽是遮蔽档；9..13 每槽只许坏一维，否则"六维六档"就不成立。
        let mut i = 1usize;
        while i < REJECT {
            let c = slot(ACCEPT + i);
            let a = c.actual();
            let k = c.claim;
            assert!(!a.ok(), "败槽的现场本该走不到 ok()");
            let offs = [
                a.stage != k.stage,
                a.code != k.code,
                a.value != k.value,
                a.note != k.note,
                a.default != k.default,
            ];
            assert_eq!(
                offs.iter().filter(|v| **v).count(),
                1,
                "槽 {} 不该只坏一维",
                ACCEPT + i
            );
            assert_eq!(judge(&c), a_single(&a, &k), "槽 {} 判错了维", ACCEPT + i);
            assert_eq!(expected(ACCEPT + i), REJECT_FAULTS.get(i).copied());
            i += 1;
        }
    }

    /// 现场与申报之间唯一错位的那一维。
    fn a_single(a: &Outcome, k: &Claim) -> Fault {
        if a.stage != k.stage {
            Fault::Stage
        } else if a.code != k.code {
            Fault::Code
        } else if a.value != k.value {
            Fault::Value
        } else if a.note != k.note {
            Fault::Note
        } else if a.default != k.default {
            Fault::Default
        } else {
            Fault::Ok
        }
    }

    #[test]
    fn thirteen_want_tables_agree_with_the_eight_gates() {
        // 申报表与期望表是同一语料的两种陈述：任一处笔误都要在这里露出来。
        let mut i = 0usize;
        while i < CORPUS {
            let c = slot(i);
            let o = c.actual();
            assert_eq!(o.stage, cell_stage(i), "槽 {} 阶段", i);
            assert_eq!(o.code, cell_code(i), "槽 {} errno", i);
            assert_eq!(o.value, cell_value(i), "槽 {} 退出码", i);
            assert_eq!(o.note, cell_note(i), "槽 {} 注解", i);
            assert_eq!(o.default, cell_default(i), "槽 {} 齐备位", i);
            assert_eq!(o.hit, cell_hit(i), "槽 {} 答复档", i);
            assert_eq!(o.banner, cell_banner(i), "槽 {} 横幅档", i);
            assert_eq!(o.flows, cell_flows(i), "槽 {} 双流档", i);
            assert_eq!(o.hint, cell_hint(i), "槽 {} 指路档", i);
            assert_eq!(o.tier(), cell_tier(i), "槽 {} 档位", i);
            assert_eq!(o.name, cell_name(i), "槽 {} 名字", i);
            assert_eq!(c.shape.tag, cell_tag(i), "槽 {} 标签", i);
            assert_eq!(c.shape.link, cell_link(i), "槽 {} 形态", i);
            i += 1;
        }
    }

    fn cell_stage(i: usize) -> Stage {
        *WANT_STAGE.get(i).unwrap()
    }
    fn cell_code(i: usize) -> i32 {
        *WANT_CODE.get(i).unwrap()
    }
    fn cell_value(i: usize) -> u16 {
        *WANT_VALUE.get(i).unwrap()
    }
    fn cell_note(i: usize) -> Note {
        *WANT_NOTE.get(i).unwrap()
    }
    fn cell_default(i: usize) -> bool {
        *WANT_DEFAULT.get(i).unwrap()
    }
    fn cell_hit(i: usize) -> Hit {
        *WANT_HIT.get(i).unwrap()
    }
    fn cell_banner(i: usize) -> Banner {
        *WANT_BANNER.get(i).unwrap()
    }
    fn cell_flows(i: usize) -> Flows {
        *WANT_FLOWS.get(i).unwrap()
    }
    fn cell_hint(i: usize) -> Hint {
        *WANT_HINT.get(i).unwrap()
    }
    fn cell_tier(i: usize) -> Tier {
        *WANT_TIER.get(i).unwrap()
    }
    fn cell_name(i: usize) -> &'static str {
        *WANT_NAME.get(i).unwrap()
    }
    fn cell_tag(i: usize) -> &'static str {
        *WANT_TAGS.get(i).unwrap()
    }
    fn cell_link(i: usize) -> Link {
        *WANT_LINK.get(i).unwrap()
    }

    #[test]
    fn the_green_run_renders_the_frozen_line() {
        let r = match run() {
            Ok(v) => v,
            Err(_) => panic!("语料里有一格不绿"),
        };
        assert!(r.green());
        assert_eq!(r.gate(), Ok(()));
        assert_eq!(r.rate(), 1000);
        assert_eq!(r.matched, r.total);
        assert_eq!(r.total, CORPUS as u32);
        assert_eq!(r.pass, 3);
        assert_eq!(r.degrade, 3);
        assert_eq!(r.partial, 1);
        assert_eq!(r.undef, 1);
    }

    #[test]
    fn one_injection_moves_the_rate_off_a_thousand() {
        let r = match run_with(Some((0, Fault::Code))) {
            Ok(v) => v,
            Err(_) => panic!("注入槽 0 不该被拒"),
        };
        assert_eq!(r.matched, CORPUS as u32 - 1);
        assert_eq!(r.total, CORPUS as u32);
        assert_eq!(r.rate(), 929);
        assert_eq!(r.rate(), *VALS.get(ACCEPT - 1).unwrap(), "红档率就是第 8 格");
        assert!(!r.green());
        assert_eq!(r.gate(), Err(AV_ECOUNT));
        assert_eq!(r.accept, ACCEPT as u32 - 1);
        assert_eq!(r.reject, REJECT as u32 + 1);
    }

    #[test]
    fn flipping_a_reject_slot_moves_the_partition_not_the_corpus() {
        // 把最后一个败槽的期望缺陷抹掉：语料仍是 14 槽，只是 8/6 变成 9/5。
        let r = match run_with(Some((CORPUS - 1, Fault::Ok))) {
            Ok(v) => v,
            Err(_) => panic!("注入尾槽不该被拒"),
        };
        assert_eq!(r.total, CORPUS as u32);
        assert_eq!(r.accept, ACCEPT as u32 + 1);
        assert_eq!(r.reject, REJECT as u32 - 1);
        assert_eq!(r.matched, CORPUS as u32 - 1);
        assert_eq!(r.rate(), 929);
        assert_eq!(r.degrade, 4, "翻正的那一格仍压在降级行上");
        assert_eq!(r.pass, 3);
    }

    #[test]
    fn out_of_range_injection_is_refused_not_ignored() {
        assert_eq!(run_with(Some((CORPUS, Fault::Stage))), Err(AV_ESLOT));
        assert_eq!(run_with(Some((CORPUS + 1, Fault::Ok))), Err(AV_ESLOT));
        assert_eq!(run_with(Some((255, Fault::Default))), Err(AV_ESLOT));
        assert_eq!(run_with(None), run(), "不注入必须与全跑同果");
        assert!(case_at(CORPUS).is_none());
        assert!(case_at(0).is_some());
        assert_eq!(cover(CORPUS), Ok(()));
        assert_eq!(cover(CORPUS - 1), Err(AV_ENOBS));
        assert_eq!(cover(0), Err(AV_ENOBS));
        assert_eq!(expected(CORPUS), None);
        assert_eq!(expected(0), Some(Fault::Ok));
        assert_eq!(ACCEPT_SLOTS.last(), Some(&(ACCEPT - 1)));
        assert_eq!(REJECT_SLOTS.first(), Some(&ACCEPT));
        assert_eq!(REJECT_SLOTS.last(), Some(&(CORPUS - 1)));
    }

    #[test]
    fn every_red_piles_on_the_degrade_row() {
        let m = Matrix::of();
        assert!(m.reds_on_degrade_row());
        assert_eq!(m.reds(), REJECT);
        assert_eq!(m.slots(), CORPUS);
        assert!(m.covered());
        let d = match m.row(Tier::Degrade) {
            Some(v) => v,
            None => panic!("no degrade row"),
        };
        assert_eq!(d.total, ACCEPT + REJECT - 5);
        assert_eq!(d.good, 3);
        assert_eq!(d.red(), REJECT);
        assert!(!d.perfect());
        assert!(!d.empty());
        let p = match m.row(Tier::Pass) {
            Some(v) => v,
            None => panic!("no pass row"),
        };
        assert_eq!(p.total, 3);
        assert!(p.perfect());
        let s = m.render();
        assert!(s.contains("degrade total=9 good=3 red=6\n"), "{}", s);
        assert!(s.contains("pass total=3 good=3 red=0\n"));
        assert_eq!(nl_count(s.as_str()), TIER_COUNT);
    }

    #[test]
    fn the_ledger_root_survives_probe_rejects() {
        let mut l = build_ledger();
        assert_eq!(l.audit(), Ok(()));
        assert_eq!(l.ok_count(), ACCEPT);
        assert_eq!(l.err_count(), REJECT);
        let root = match l.freeze() {
            Ok(v) => v,
            Err(_) => panic!("first freeze must succeed"),
        };
        assert!(root != 0);
        // 冻结后投格只累计拒绝数，不该污染指纹：重放仍复现同一个 root。
        assert_eq!(l.register(Sample::blank()), Err(AV_ESLOT));
        assert_eq!(l.register(cell(0)), Err(AV_ESLOT));
        assert_eq!(l.rejects(), 2);
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.root(), root);
        assert!(l.is_frozen());
        assert_eq!(build_ledger().replay(), Ok(root));
    }

    #[test]
    fn tampered_cells_are_caught_by_audit_and_replay() {
        // 红档率格被改成"全绿"：对账与重放必须同时变红。
        let mut value_bad = Ledger::new();
        let mut i = 0usize;
        while i < SAMPLES {
            let mut one = cell(i);
            if i == 7 {
                one.value = 1000;
            }
            assert_eq!(value_bad.register(one), Ok(()));
            i += 1;
        }
        assert_eq!(value_bad.audit(), Err(AV_EVALUE));
        assert_eq!(value_bad.replay(), Err(AV_ESEAL));
        // 缺陷码换成本卡的另一种码：格数、成败分布都对，只有码不对。
        let mut code_bad = Ledger::new();
        i = 0;
        while i < SAMPLES {
            let mut one = cell(i);
            if i == 10 {
                one.code = AV_ELINK;
            }
            assert_eq!(code_bad.register(one), Ok(()));
            i += 1;
        }
        assert_eq!(code_bad.ok_count(), ACCEPT);
        assert_eq!(code_bad.err_count(), REJECT);
        assert_eq!(code_bad.audit(), Err(AV_EVALUE));
        assert_eq!(code_bad.replay(), Err(AV_ESEAL));
        // 少登记一格就休想对账。
        let mut shorty = Ledger::new();
        i = 0;
        while i < SAMPLES - 1 {
            assert_eq!(shorty.register(cell(i)), Ok(()));
            i += 1;
        }
        assert_eq!(shorty.audit(), Err(AV_EBAL));
        assert_eq!(shorty.len(), SAMPLES - 1);
    }

    #[test]
    fn register_must_climb_the_slot_ladder() {
        let mut l = Ledger::new();
        assert!(l.is_empty());
        assert_eq!(l.len(), 0);
        assert_eq!(l.sample(0), None);
        // 跳格：先登记第 1 格必拒。
        assert_eq!(l.register(cell(1)), Err(AV_ESLOT));
        assert_eq!(l.rejects(), 1);
        assert_eq!(l.register(cell(0)), Ok(()));
        assert_eq!(l.len(), 1);
        assert_eq!(l.sample(0).map(|x| x.seal()), Some(cell(0).seal()));
        // 同格重登也是跳格。
        assert_eq!(l.register(cell(0)), Err(AV_ESLOT));
        assert_eq!(l.rejects(), 2);
        assert!(!l.is_empty());
        assert_eq!(l.sample(1), None);
        assert_eq!(l.sample(200), None);
        assert_eq!(l.audit(), Err(AV_EBAL));
        assert_eq!(l.freeze(), Ok(()).map_or(0, |_| 2u32), "冻结返回段数");
    }

    #[test]
    fn gate_only_moves_forward_and_closes_once() {
        let mut g = Gate::new();
        assert_eq!(g.steps(), 0);
        assert_eq!(g.last(), -1);
        assert!(!g.closed());
        assert_eq!(g.step(Stage::Ready), Ok(()));
        assert_eq!(g.step(Stage::Table), Err(AV_ESTAGE), "回退复核不许过");
        assert_eq!(g.steps(), 1, "判失败的步数不计");
        assert_eq!(g.last(), Stage::Ready.order() as i32);
        assert_eq!(g.step(Stage::Ready), Ok(()), "同段重复观测合法");
        assert_eq!(g.steps(), 2);
        assert_eq!(g.close(), Ok(2));
        assert!(g.closed());
        assert_eq!(g.close(), Err(AV_ESTATE));
        assert_eq!(g.step(Stage::Argv0), Err(AV_ESTATE));
        // 空闸门也能关：段数为 0 不是错。
        let mut fresh = Gate::new();
        assert_eq!(fresh.close(), Ok(0));
        assert_eq!(fresh.close(), Err(AV_ESTATE));
    }

    #[test]
    fn every_case_walks_the_gate_without_regression() {
        let mut i = 0usize;
        while i < CORPUS {
            let c = slot(i);
            let mut g = Gate::new();
            let stop = c.actual().stage.order();
            let mut k = 0u8;
            while k <= stop {
                let s = match STAGES.get(k as usize) {
                    Some(v) => *v,
                    None => panic!("no stage"),
                };
                assert_eq!(g.step(s), Ok(()), "槽 {} 段 {} 不该被拒", i, k);
                k += 1;
            }
            assert_eq!(g.steps() as u8, stop + 1);
            assert_eq!(g.last(), stop as i32);
            assert!(g.close().is_ok());
            i += 1;
        }
    }

    #[test]
    fn four_injections_are_all_green_and_ship_thirteen_checks() {
        let inj = inject_four();
        assert_eq!(inj.len(), 4);
        assert!(inj.iter().all(|v| *v), "四注入有一路是红的");
        let s = run_unx11662_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
        assert_eq!(s.tally(), (13, 0));
        assert!(s.red_items_text().is_empty());
        assert_eq!(s.domain, "unx11662_argv0_unregistered");
        assert!(!s.truncated());
        let mut i = 0usize;
        while i < 13 {
            let it = match s.get(i) {
                Some(v) => v,
                None => panic!("no item {}", i),
            };
            assert!(it.name.starts_with("AV"), "{}", it.name);
            assert!(it.passed);
            assert_eq!(it.detail, "static");
            i += 1;
        }
        assert_eq!(s.get(0).map(|x| x.name), Some("AV01-名称与段基址"));
        assert_eq!(s.get(12).map(|x| x.name), Some("AV13-冻结重放四注入"));
        assert_eq!(s.get(13), None);
    }

    #[test]
    fn the_usage_block_lives_on_stdout_and_the_error_line_on_stderr() {
        let c = slot(3);
        let o = c.actual();
        let banner = banner_text(&o);
        let err = errline_of(&c.shape, &o);
        // 两块各走一流：横幅里不许有 not found，错行里不许有版本号。
        assert!(!banner.contains("applet not found"));
        assert!(!err.contains("BusyBox"));
        assert_eq!(o.banner.stream(), "stdout");
        let m = message(&c.shape, &o);
        let bi = match find_from(m.as_str(), banner.as_str(), 0) {
            Some(v) => v,
            None => panic!("横幅没进输出块"),
        };
        let ei = match find_from(m.as_str(), err.as_str(), 0) {
            Some(v) => v,
            None => panic!("错行没进输出块"),
        };
        assert!(bi < ei, "并流时顺序不能倒");
        // 只留 stderr：横幅整块消失，不是截半。
        let mut off = c.shape;
        off.banner_off = true;
        let o2 = define(&off);
        let mut want = String::new();
        want.push_str(errline_of(&off, &o2).as_str());
        want.push_str(hint_text(o2.hint, o2.group, hint_member(&o2)).as_str());
        assert_eq!(message(&off, &o2), want);
        assert!(!message(&off, &o2).contains(USAGE_VERSION));
        // 两流全丢：一块都没有。
        let mut gone = c.shape;
        gone.dropped_out = true;
        let o3 = define(&gone);
        assert!(o3.flows.lost());
        assert_eq!(message(&gone, &o3), String::new());
        assert!(!errline_of(&gone, &o3).is_empty(), "文本还在，只是不落地");
    }

    #[test]
    fn exit_127_126_and_1_are_three_different_faces() {
        for idx in [3usize, 4, 5, 8, 9, 10, 11, 12, 13].iter() {
            let o = slot(*idx).actual();
            assert_eq!(o.value, EXIT_DEGRADE, "槽 {} 该退 127", idx);
            assert_eq!(o.code, ERR_ENOENT);
            assert!(o.note.exit_127_face());
        }
        let nx = slot(7).actual();
        assert_eq!(nx.value, EXIT_EXEC, "重启判死不是名字找不到");
        assert_eq!(nx.code, ERR_ENOSYS);
        assert!(!nx.note.exit_127_face());
        let gu = slot(6).actual();
        assert_eq!(gu.value, EXIT_GROUP_USAGE);
        assert_eq!(gu.code, ERR_NONE, "组用法不是错");
        assert!(gu.ok());
        assert_eq!(slot(0).actual().value, EXIT_RUN_OK);
        assert_ne!(EXIT_DEGRADE, EXIT_EXEC);
        assert_ne!(EXIT_EXEC, EXIT_USAGE);
    }

    #[test]
    fn trimmed_and_unregistered_share_the_face_not_the_hint() {
        let t = slot(4);
        let u = slot(3);
        let ot = t.actual();
        let ou = u.actual();
        // 用户面上两档同形：同码、同退出码、同注解、同档。
        assert_eq!(ot.code, ou.code);
        assert_eq!(ot.value, ou.value);
        assert_eq!(ot.note, ou.note);
        assert_eq!(ot.tier(), ou.tier());
        assert_eq!(ot.default, ou.default);
        // 内部维度与指路行才分开它们。
        assert_ne!(ot.hit, ou.hit);
        assert_ne!(ot.hint, ou.hint);
        assert!(ot.hit.internal());
        assert!(!ou.hit.internal());
        let mt = message(&t.shape, &ot);
        let mu = message(&u.shape, &ou);
        assert_eq!(nl_count(mt.as_str()), nl_count(mu.as_str()));
        assert!(mt.contains("Run 'busybox editors ed'"));
        assert!(mu.contains("Run 'busybox --list' to find all built-in applets."));
    }

    #[test]
    fn the_detail_line_carries_the_verbatim_error_and_no_claim() {
        let c = slot(5);
        let d = detail(&c);
        assert!(d.starts_with("@zzz/zzz "), "{}", d);
        assert!(d.contains("@zzz: 'zzz': applet not found"));
        assert!(!d.contains("claim"));
        assert_eq!(nl_count(d.as_str()), 2, "错行原文自带换行");
        assert!(d.ends_with('\n'));
        // 跑通档没有 err 字段。
        let r = detail(&slot(0));
        assert!(!r.contains("err="));
        assert_eq!(nl_count(r.as_str()), 1);
    }

    #[test]
    fn number_text_primitives_hold_their_edges() {
        assert_eq!(dec_text(0), "0");
        assert_eq!(dec_text(929), "929");
        assert_eq!(dec_text(u32::MAX), "4294967295");
        assert_eq!(u32_text(1000), "1000");
        assert_eq!(i32_text(0), "0");
        assert_eq!(i32_text(-1), "-1");
        assert_eq!(i32_text(ERR_ENOSYS), "38");
        assert_eq!(hex_text(SEG_BASE, 6), "06d110");
        assert_eq!(hex_text(SEG_BASE + 0x0d, 6), "06d11d");
        assert_eq!(hex_text(0, 4), "0000");
        assert!(bool_text(true) == "true");
        assert!(bool_text(false) == "false");
    }

    #[test]
    fn report_shares_and_cells_are_integers() {
        let r = match run() {
            Ok(v) => v,
            Err(_) => panic!("green run"),
        };
        assert_eq!(r.success_cells(), 3);
        assert_eq!(r.failure_cells(), 5);
        assert_eq!(r.success_cells() + r.failure_cells(), r.matched);
        assert_eq!(r.surface_share(), 50);
        assert_eq!(r.degrade_share(), 37);
        let mut zero = r;
        zero.total = 0;
        zero.accept = 0;
        zero.matched = 0;
        assert_eq!(zero.rate(), 0);
        assert_eq!(zero.surface_share(), 0);
        assert_eq!(zero.degrade_share(), 0);
        assert!(!zero.green());
        assert_eq!(zero.gate(), Err(AV_ECOUNT));
    }

    #[test]
    fn the_frozen_green_render_text_is_byte_exact() {
        let r = match run() {
            Ok(v) => v,
            Err(_) => panic!("green run"),
        };
        assert_eq!(r.render().as_str(), RENDER_OK);
        assert_eq!(nl_count(RENDER_OK), 1);
        assert!(RENDER_OK.contains("corpus=14"));
        assert!(RENDER_OK.contains("rate=1000ppm"));
    }
}
