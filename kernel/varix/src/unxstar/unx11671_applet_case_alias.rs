//! UNX-F11671 applet 名大小写与别名匹配判据
//!
//! busybox 的 `applet_name[]` 是一张**编译期展开的全小写表**：Kconfig 里的
//! applet 名、`ALIAS_` 展开出的别名、以及 `--install` / `--list` / `busybox`
//! 这三个伪名，全部以小写字面量落进表里；查表走 `find_applet_by_name`，逐字节
//! `strcmp`，**没有任何大小写折叠**。于是真机上有一条一眼可验、却又极易被"贴心"
//! 改动破坏的行为：
//!
//! ```text
//! busybox ls     → 执行 ls 的 main，退 0
//! busybox LS     → busybox: 'LS': applet not found，退 127
//! busybox zcat   → 走别名表收敛到 gunzip，执行它，退 0
//! busybox ZCAT   → busybox: 'ZCAT': applet not found，退 127
//! ```
//!
//! 这四行里有两条轴，各自能单独失效：
//!
//! 1. **大小写轴**：表侧全小写 + 查表侧逐字节敏感。表侧一旦混进大写字节
//!    （手工编辑 `applet_switch` 展开产物、或把某个 Windows 侧的名字直接抄进
//!    表），`busybox LS` 就可能"意外命中"；查表侧一旦加了折叠
//!    （`tolower()` 后再比），`busybox LS` 会**执行 `ls`**——这是本卡唯一要判死
//!    的构造性缺陷，记 `AC_ECASE`。它在文案面上看起来完全正常（根本没报错），
//!    在表面上也完全正常（表仍是全小写），只有把"输入类"和"表形"两件事放在一起
//!    才现形，这正是本卡存在的理由。
//! 2. **别名轴**：别名不是第二份实现，而是同一份 `main` 的第二个键。查询命中
//!    别名时必须在 `HOP_CAP` 跳内收敛到规范名；悬空目标、自指别名、遮蔽规范名、
//!    成环、超深链各自是一种真实构建病，全部记 `AC_EALIAS` / `AC_EUNIQ`。
//!
//! 判据原文（UNX-F11671-J1）：全小写表/LS 未名档判据绿，与 F11593 联签。
//! "LS 未名档"说的是**结果**（大写查询必须落进未名那一档），"全小写表"说的是
//! **前提**（表侧确实一个大写字节都没有），"与 F11593 联签"说的是**同一张表**：
//! F11593 立别名表的结构本体（有序、可达、不遮蔽、自洽），本卡立一次查询在这
//! 两条轴上的答复。两张单子必须指的是同一份别名表——别名行数与表指纹逐项对上，
//! 对不上就是本卡绿在 A 表、F11593 绿在 B 表，合起来等于什么都没验，判 `AC_ESKIP`
//! 而不是判绿。
//!
//! # 闸门次序（八道，逐闸定局，后闸不再消费前面已违约的事实）
//!
//! `Lex → Class → Table → CoSign → Alias → Reply → Code → Seal`
//!
//! * **Lex 先于一切**：空名、超长名、含 `/` 的名、含控制字节的名根本进不了
//!   `strcmp` 语义。先问大小写会得到"空名属于哪一类"这种没有答案的归类；先把
//!   它送去查表会把**形态问题**误判成**注册问题**，归因直接错一层。
//! * **Class 先于 Table**：输入侧的大写是查询的事实，表侧的大写是构建的事实。
//!   先定输入类，才知道接下来该问"表能不能容下这次命中"——反过来就会把
//!   "输入有大写 + 表全小写 ⇒ 必不命中"这条正向不变量写成两条独立的检查，
//!   漏掉它们的合取。
//! * **Table 先于 CoSign**：表形是本卡自己的判据，联签锚是别人的申报。先把自己的
//!   事实立住，再问它和 F11593 说的是不是同一张表——否则联签通过会把一张形坏了
//!   的表一起保下来（F11593 只数行数与折指纹，不查字节大小写）。
//! * **CoSign 先于 Alias**：别名收敛用的是别名表的可达性，这张表若与 F11593 不是
//!   同一份，"收敛到规范名"这句话就没有共同指称物。
//! * **Alias 先于 Reply**：答复档由"命中了什么身份"决定。先定答复再补别名，会出现
//!   "答复是执行、却没人说清执行的是谁的 main"——那正是 `zcat` 走成 `zcat` 自身
//!   桩（`noapplet`）的实际故障形态。
//! * **Reply 先于 Code**：127 是"未名"这一答复的约定档，档必须先由答复定下来。
//!   先看码会把"折叠命中 `ls` 并退 0"的构建放过去——它的码完全自洽。
//! * **Code 先于 Seal**：默认必跑位谈的是"这条进不进每次必跑的套件"，一条码档
//!   都没落地的行没有资格谈。
//!
//! # 关键差异（本卡深度所在）
//!
//! * **本卡 vs UNX-F11593**：F11593 判**表**（别名表是否有序、可达、不遮蔽、
//!   容量自洽），它的语料是"表的操作序列"；本卡判**一次查询的答复**，语料是
//!   "某个被敲的名字在某份构建上的现场"。同一条 `zcat` 行，F11593 问"它作为别名
//!   合不合法"，本卡问"敲它的时候执行了谁的 main、大写版本敲的时候答了什么"。
//!   F11593 判死、本卡放行的例子：表无序但答复全对（本卡只看答复，排序归它）。
//!   本卡判死、F11593 放行的例子：查表加了 `tolower()`——表结构完好无损，
//!   F11593 全绿，而 `busybox LS` 已经不再报 notfound。
//! * **本卡 vs UNX-F11661**：F11661 验未知名的**报错面**（文案逐字/流/码档）。
//!   在它眼里 `LS` 只是未名族的一个样本；在本卡眼里 `LS` 是"大小写敏感"的证据，
//!   本卡甚至不读文案字节（文案归 F11661/F11674），只读答复档。
//! * **本卡 vs UNX-F11673**：F11673 判表的**完整性**（build 清单 ↔ `--list` 的
//!   集合零偏差）；本卡判表的**大小写形状**（表内零大写字节）。同一条"表"，
//!   两种尺子：清单少一行是 F11673 的红，表里混进一个大写字母是本卡的红。
//! * **本卡独有维度**：`FoldTrap`（折叠命中）与 `UpperByteInTable`（表侧大写）。
//!   这两个缺陷在 B24 批其余卡里没有任何一卡负责——它们不是文案错、不是码错、
//!   不是流错，而是"查表这件事本身变得不再逐字节"。
//!
//! 本模块是 `no_std` 纯功能判据：不 exec 进程、不读真实文件系统，把上述八闸与
//! 两条轴建成一张可复核的事实表（含一份真实的全小写 applet/别名建模表与逐字节
//! 查表实现），再由十三项聚合判据（AC01…AC13）一次全跑。所有取值走 `match` /
//! `if let` / `.get()` / `checked_*` / `saturating_*`；生产码零 `unwrap`、零
//! `expect`、零 `panic`、零 `assert`、零 `format!`、零 `unsafe`、零 `std`。

use crate::alloc::string::String;
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 段基址与错误码
// ---------------------------------------------------------------------------

/// 本卡段基址：F11671 在 VX-1029-5 批里占用 0x6d230 段。
///
/// 同批邻居（不得撞）：F11670=0x6d220、F11672=0x6d250、F11673=0x6d260、
/// F11674=0x6d270。本卡的十三码落在 0x6d231..=0x6d23d。
pub const SEG_BASE: u32 = 0x6d230;

/// 成败互报：申报绿而实测红（或反之）。
pub const AC_ESTATE: u32 = SEG_BASE + 0x01u32;
/// 闸位错：停在的闸与申报的闸不是同一道。
pub const AC_ESTAGE: u32 = SEG_BASE + 0x02u32;
/// 码档错：退出码或 errno 与"该答复档"的约定档不一致。
pub const AC_ECODE: u32 = SEG_BASE + 0x03u32;
/// 数值错：行数、跳数、断言数、日志字节数这类可数事实对不上。
pub const AC_EVALUE: u32 = SEG_BASE + 0x04u32;
/// 注记错：成因档认错（把折叠命中说成表侧大写，或反之）。
pub const AC_ENOTE: u32 = SEG_BASE + 0x05u32;
/// 默认必跑位丢失：该进默认套件的行没进，或不该进的混进来了。
pub const AC_EDEFAULT: u32 = SEG_BASE + 0x06u32;
/// 大小写敏感违约：查表加了折叠，大写查询命中了小写表项（本卡主锚红）。
pub const AC_ECASE: u32 = SEG_BASE + 0x07u32;
/// 表侧非小写：本构建的 applet 表里扫出了大写字节。
pub const AC_ELOWER: u32 = SEG_BASE + 0x08u32;
/// 别名不可达：悬空目标、超深链或成环。
pub const AC_EALIAS: u32 = SEG_BASE + 0x09u32;
/// 重名锚：别名遮蔽规范名、自指别名或表内重名。
pub const AC_EUNIQ: u32 = SEG_BASE + 0x0au32;
/// 前置缺席：F11593 未联签、别名 CONFIG 不在或载体缺失。
pub const AC_ESKIP: u32 = SEG_BASE + 0x0bu32;
/// 抖动未显式：重试过却申报首次即中（洗绿）。
pub const AC_EFLAKE: u32 = SEG_BASE + 0x0cu32;
/// 槽位越界：往不存在的槽写结论。
pub const AC_ESLOT: u32 = SEG_BASE + 0x0du32;

/// 十三码表——按 `SEG_BASE + 0x01..0x0d` 的次序排列，供逐码点名与撞码自检。
pub const CODE_TABLE: [u32; 13usize] = [
    AC_ESTATE,
    AC_ESTAGE,
    AC_ECODE,
    AC_EVALUE,
    AC_ENOTE,
    AC_EDEFAULT,
    AC_ECASE,
    AC_ELOWER,
    AC_EALIAS,
    AC_EUNIQ,
    AC_ESKIP,
    AC_EFLAKE,
    AC_ESLOT,
];

/// 错误码 → 文本名。
///
/// 这里的字面量是**手写**的第二份表达，与上面的 `SEG_BASE + 0xNN` 定义交叉校验：
/// 改动段基址而忘记改这张表，`code_name()` 就会兜底成 `unknown`，AC01 当场判死。
pub fn code_name(code: u32) -> &'static str {
    match code {
        0x6d231u32 => "ac-estate",
        0x6d232u32 => "ac-estage",
        0x6d233u32 => "ac-ecode",
        0x6d234u32 => "ac-evalue",
        0x6d235u32 => "ac-enote",
        0x6d236u32 => "ac-edefault",
        0x6d237u32 => "ac-ecase",
        0x6d238u32 => "ac-elower",
        0x6d239u32 => "ac-ealias",
        0x6d23au32 => "ac-euniq",
        0x6d23bu32 => "ac-eskip",
        0x6d23cu32 => "ac-eflake",
        0x6d23du32 => "ac-eslot",
        0u32 => "none",
        _ => "unknown",
    }
}

/// 错误码 → 人话一句（渲染账本与红项时用，不参与判定）。
pub fn code_phrase(code: u32) -> &'static str {
    match code {
        0x6d231u32 => "成败互报",
        0x6d232u32 => "闸位错位",
        0x6d233u32 => "码档漂移",
        0x6d234u32 => "数值不符",
        0x6d235u32 => "成因归错",
        0x6d236u32 => "默认位丢失",
        0x6d237u32 => "大小写敏感被破坏",
        0x6d238u32 => "表侧混入大写字节",
        0x6d239u32 => "别名不可达",
        0x6d23au32 => "重名遮蔽",
        0x6d23bu32 => "联签或载体缺席",
        0x6d23cu32 => "抖动被洗绿",
        0x6d23du32 => "槽位越界",
        _ => "无",
    }
}

// ---------------------------------------------------------------------------
// 契约常量与本卡规模
// ---------------------------------------------------------------------------

/// 语料槽数：八活六死。
pub const CORPUS: usize = 14usize;
/// 账本样本数。
pub const SAMPLES: usize = 14usize;
/// 采纳槽数。
pub const ACCEPT: usize = 8usize;
/// 否决槽数。
pub const REJECT: usize = 6usize;
/// 档位数量（绿/抖/跳/阻）。
pub const TIER_COUNT: usize = 4usize;
/// 本卡卡号。
pub const CARD: u16 = 11671u16;
/// 联签对象：F11593（applet 别名与内建表判据）。
pub const CO_SIGN_CARD: u16 = 11593u16;
/// B24 批首卡号（卡号段下界）。
pub const CARD_MIN: u16 = 11661u16;
/// B24 批末卡号（卡号段上界）。
pub const CARD_MAX: u16 = 11679u16;
/// B24 批清单行数：F11661..F11679 共 19 条。
pub const BENCH: usize = 19usize;
/// 本模块聚合判据项数（AC01..AC13）。
pub const CHECK_COUNT: usize = 13usize;
/// 全绿通过率（千分）。
pub const RATE_FULL: u32 = 1000u32;
/// 一维坏掉后的通过率——14 槽里中 13 槽：(13*1000 + 14/2) / 14 = 929。
pub const RED_RATE: u32 = 929u32;
/// FNV-1a 64 位偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325u64;
/// FNV-1a 64 位素数。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3u64;

/// applet 名在表中的最大字节数（不含末位 NUL）——`start-stop-daemon` 实测 17。
pub const NAME_MAX: usize = 30usize;
/// 别名链允许的最大跳数；超过即"收敛失败"，而不是"再试一跳"。
pub const HOP_CAP: u8 = 2u8;

/// 正常退出码：命中的 applet 自己判成功。
pub const EX_OK: u16 = 0u16;
/// 未名档——shell 对 "command not found" 的约定码。
pub const EX_NOTFOUND: u16 = 127u16;
/// applet 自身判失败。
pub const EX_GENERAL: u16 = 1u16;
/// 信号档基址。
pub const EX_SIG_BASE: u16 = 128u16;
/// SIGSEGV——折叠命中把非预期 main 接上真参数时的实际下场。
pub const EX_SEGV: u16 = EX_SIG_BASE + 11u16;

/// errno 无。
pub const ENONE: u16 = 0u16;
/// ENOENT：查无此项——未名档在执行链路那一侧的对照。
pub const ENOENT: u16 = 2u16;
/// EACCES：权限不足，与"没找到"必须分开。
pub const EACCES: u16 = 13u16;
/// EINVAL：畸形名（空名/含控制字节）在 lex 闸的残留档。
pub const EINVAL: u16 = 22u16;

/// busybox 报错的程序名档：`argv[0]` 的 basename。
pub const PROG: &str = "busybox";
/// 未名答复的固定尾段——本卡用它做"答复是不是未名档"的字节锚（逐字比对归
/// F11674，本卡只认锚点存在性，不重验文案）。
pub const TAIL_NOTFOUND: &str = "': applet not found";
/// 被禁的折叠提示档：真机不解释大小写，多话即泄漏。
pub const FORBIDDEN_FOLD_HINT: &str = "did you mean";
/// 被禁的大小写解释档。
pub const FORBIDDEN_CASE_WORDS: &str = "case-sensitive";
/// 被禁的 CONFIG 提示档。
pub const FORBIDDEN_CONFIG: &str = "CONFIG_";

/// 别名表在位（`CONFIG_APPLET_ALIASES` 缺席时别名侧根本不存在）。
pub const CFG_ALIASES: u32 = 1u32 << 0;
/// `sh` 的规范名（ash 或 hush）缺席——`sh` 别名随之消失。
pub const CFG_SHELL: u32 = 1u32 << 1;
/// grep 的扩展/固定档别名（egrep/fgrep）不在。
pub const CFG_GREP_ALIAS: u32 = 1u32 << 2;
/// gunzip 的 zcat 别名不在。
pub const CFG_GUNZIP: u32 = 1u32 << 3;
/// modprobe 的 rmmod/depmod 别名不在。
pub const CFG_MODPROBE: u32 = 1u32 << 4;
/// 双机对照不在（第二台基准机缺席）。
pub const CFG_TWO_MACHINE: u32 = 1u32 << 5;
/// 表扫描载体不在（没有 `--list` 可枚举，只能读展开产物）。
pub const CFG_LIST: u32 = 1u32 << 6;
/// 全缺席掩码。
pub const CFG_ALL: u32 = CFG_ALIASES
    | CFG_SHELL
    | CFG_GREP_ALIAS
    | CFG_GUNZIP
    | CFG_MODPROBE
    | CFG_TWO_MACHINE
    | CFG_LIST;
/// 无缺席（满配置）。
pub const CFG_NONE: u32 = 0u32;

/// 位名（缺席掩码里的位是"缺"，这里给的是它对应的 Kconfig 叫法）。
pub fn cfg_name(bit: u32) -> &'static str {
    match bit {
        1u32 => "CONFIG_APPLET_ALIASES",
        2u32 => "CONFIG_SH_IS_ASH",
        4u32 => "CONFIG_GREP_IGNORE_CASE_ALIAS",
        8u32 => "CONFIG_GUNZIP",
        16u32 => "CONFIG_MODPROBE",
        32u32 => "two-machine",
        64u32 => "busybox --list",
        _ => "none",
    }
}

/// 缺席掩码里最低的那一位 → 名字。
pub fn first_absent_name(missing: u32) -> &'static str {
    let mut i = 0usize;
    while i < 7usize {
        let bit = 1u32 << i;
        if (missing & bit) != 0u32 {
            return cfg_name(bit);
        }
        i = i.saturating_add(1usize);
    }
    "none"
}

/// 缺席掩码里被置位的个数——用于把"这条跳了几项配置"如实写进现场。
pub fn absent_bits(missing: u32) -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < 7usize {
        if (missing & (1u32 << i)) != 0u32 {
            n = n.saturating_add(1u32);
        }
        i = i.saturating_add(1usize);
    }
    n
}

// ---------------------------------------------------------------------------
// 折叠与文本工具
// ---------------------------------------------------------------------------

/// 单字节 FNV-1a。
pub fn fold_byte(h: u64, b: u8) -> u64 {
    let x = h ^ (b as u64);
    x.wrapping_mul(FNV_PRIME)
}

/// 八字节逐字节 FNV-1a（逐字节切片，保证同一数值在任何字序下折出同一值）。
pub fn fold_word(h: u64, v: u64) -> u64 {
    let mut x = h;
    let mut i = 0usize;
    while i < 8usize {
        let byte = ((v >> (i as u32 * 8u32)) & 0xffu64) as u8;
        x = fold_byte(x, byte);
        i = i.saturating_add(1usize);
    }
    x
}

/// 字符串逐字节折叠。
pub fn fold_str(h: u64, s: &str) -> u64 {
    let mut x = h;
    for b in s.as_bytes() {
        x = fold_byte(x, *b);
    }
    x
}

/// 32 位指纹折叠。
pub fn fold_u32(h: u64, v: u32) -> u64 {
    fold_word(h, v as u64)
}

/// u32 → 十进制文本（不递归，定长数字数组倒序取出）。
pub fn dec_text(v: u32) -> String {
    if v == 0u32 {
        return String::from("0");
    }
    let mut digits = [0u8; 10usize];
    let mut n = v;
    let mut k = 0usize;
    while n > 0u32 && k < 10usize {
        if let Some(d) = digits.get_mut(k) {
            *d = (b'0' as u8) + (n % 10u32) as u8;
        }
        n /= 10u32;
        k = k.saturating_add(1usize);
    }
    let mut out = String::new();
    let mut i = k;
    while i > 0usize {
        i = i.saturating_sub(1usize);
        if let Some(b) = digits.get(i) {
            out.push(*b as char);
        }
    }
    out
}

/// u32 → 定宽小写十六进制（无 `0x` 前缀，宽度不足补零，超宽截高位）。
pub fn hex_text(v: u32, width: usize) -> String {
    let mut out = String::new();
    let mut i = 0usize;
    while i < width {
        let shift = ((width.saturating_sub(i).saturating_sub(1usize)) as u32) * 4u32;
        let nib = if shift < 32u32 {
            (v >> shift) & 0x0fu32
        } else {
            0u32
        };
        let c = if nib < 10u32 {
            (b'0' as u32) + nib
        } else {
            (b'a' as u32) + nib.saturating_sub(10u32)
        };
        out.push(c as u8 as char);
        i = i.saturating_add(1usize);
    }
    out
}

/// u32 文本（契约里按名字点名的别名）。
pub fn u32_text(v: u32) -> String {
    dec_text(v)
}

/// i32 文本：负号单独走一遍，避免对绝对值取反时撞上 i32::MIN。
pub fn i32_text(v: i32) -> String {
    if v >= 0i32 {
        return dec_text(v as u32);
    }
    let mag = match v.checked_neg() {
        Some(x) => x as u32,
        None => 2147483647u32 + 1u32,
    };
    let mut out = String::from("-");
    out.push_str(dec_text(mag).as_str());
    out
}

/// 布尔 → TAP 裁决文本。
///
/// 本卡布尔位直接喂 TAP 行，取 ktest 面上的 `ok`/`not ok`；写成 `true` 会让
/// 解析器把整行读成 unknown（同批 F11660 踩过一次）。
pub fn bool_text(v: bool) -> &'static str {
    match v {
        true => "ok",
        false => "not ok",
    }
}

// ---------------------------------------------------------------------------
// 字节级大小写工具——本卡的两条轴最终都落回这里
// ---------------------------------------------------------------------------

/// 是否 a-z。
pub fn is_lower_byte(b: u8) -> bool {
    b >= b'a' && b <= b'z'
}

/// 是否 A-Z。
pub fn is_upper_byte(b: u8) -> bool {
    b >= b'A' && b <= b'Z'
}

/// 是否字母（ASCII）。
pub fn is_alpha_byte(b: u8) -> bool {
    is_lower_byte(b) || is_upper_byte(b)
}

/// 是否数字。
pub fn is_digit_byte(b: u8) -> bool {
    b >= b'0' && b <= b'9'
}

/// 是否可打印 ASCII（0x21..=0x7e，不含空格——名字里出现空格的查询在真机上
/// 根本到不了查表，那是 shell 词法的事，归 lex 闸）。
pub fn is_print_byte(b: u8) -> bool {
    b >= 0x21u8 && b <= 0x7eu8
}

/// 名内大写字节数（逐字节实测，不靠申报）。
pub fn upper_bytes(name: &str) -> u32 {
    let mut n = 0u32;
    for b in name.as_bytes() {
        if is_upper_byte(*b) {
            n = n.saturating_add(1u32);
        }
    }
    n
}

/// 名内小写字节数。
pub fn lower_bytes(name: &str) -> u32 {
    let mut n = 0u32;
    for b in name.as_bytes() {
        if is_lower_byte(*b) {
            n = n.saturating_add(1u32);
        }
    }
    n
}

/// 名内字母字节总数。
pub fn alpha_count(name: &str) -> u32 {
    let mut n = 0u32;
    for b in name.as_bytes() {
        if is_alpha_byte(*b) {
            n = n.saturating_add(1u32);
        }
    }
    n
}

/// 名内非 ASCII 字节数（>=0x80；busybox 的表名是字节序比较，UTF-8 一律当不透明
/// 字节，因此这类名字**永远不命中**，但归因是"非 ASCII"而不是"有大写"）。
pub fn non_ascii_bytes(name: &str) -> u32 {
    let mut n = 0u32;
    for b in name.as_bytes() {
        if *b >= 0x80u8 {
            n = n.saturating_add(1u32);
        }
    }
    n
}

/// 名内含控制/空白字节数（0x20 与 <0x21、0x7f）。
pub fn odd_bytes(name: &str) -> u32 {
    let mut n = 0u32;
    for b in name.as_bytes() {
        if !is_print_byte(*b) {
            n = n.saturating_add(1u32);
        }
    }
    n
}

/// 一个字节的小写形（只对 ASCII 大写生效；非 ASCII 原样返回——**不许**用它做
/// 查表折叠，它只服务于"反面构造"，见 `folded_query`）。
pub fn lower_byte(b: u8) -> u8 {
    if is_upper_byte(b) {
        b + (b'a' - b'A')
    } else {
        b
    }
}

/// 名字的小写形（反面构造专用：真实查表**不**折叠，本函数只用来把 `LS` 折成
/// `ls` 去证明"折叠会命中"这件事，从而把折叠实现钉成红）。
pub fn folded_query(name: &str) -> String {
    let mut out = String::new();
    for b in name.as_bytes() {
        out.push(lower_byte(*b) as char);
    }
    out
}

// ---------------------------------------------------------------------------
// 建模表：一份全小写的 applet / 别名 / 伪名表
// ---------------------------------------------------------------------------
//
// 取材自 busybox 1.36.1 的别名面子集。规范名 15 行、别名 9 行、伪名 4 行。
// 两点如实声明：
// * 行数与名字是**子集**，不是全表——本卡判的是大小写与别名两条轴，不需要 300 行
//   全表；F11673 才负责"全表对账"。
// * `gzcat -> zcat` 这一跳是本卡自设的第二跳（真机 `gzcat` 直接指 `gunzip`），
//   目的是把别名链压到 `HOP_CAP` 边界上，让"超深即拒"这条规则有活样本可跑。

/// 表行种类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RowKind {
    /// 规范名：自己带 `main`。
    Canon,
    /// 别名：键在表里，`main` 指向目标。
    Alias,
    /// 伪名：分派器自身的入口（`--install` / `--list` / `busybox` 那一族）。
    Pseudo,
}

impl RowKind {
    /// 1 基种类号。
    pub const fn rank(self: &RowKind) -> u8 {
        match self {
            RowKind::Canon => 1u8,
            RowKind::Alias => 2u8,
            RowKind::Pseudo => 3u8,
        }
    }

    /// 种类短名。
    pub fn slug(self: &RowKind) -> &'static str {
        match self {
            RowKind::Canon => "canon",
            RowKind::Alias => "alias",
            RowKind::Pseudo => "pseudo",
        }
    }

    /// 按号回查。
    pub fn from_rank(r: u8) -> Option<RowKind> {
        match r {
            1u8 => Some(RowKind::Canon),
            2u8 => Some(RowKind::Alias),
            3u8 => Some(RowKind::Pseudo),
            _ => None,
        }
    }

    /// 该种类是否参与 `--list` 呈现（伪名不在 applet 清单里，是 F11673 的轴）。
    pub fn listable(self: &RowKind) -> bool {
        match self {
            RowKind::Canon => true,
            RowKind::Alias => true,
            RowKind::Pseudo => false,
        }
    }
}

/// 一行表项。
#[derive(Clone, Copy)]
pub struct TableRow {
    /// 键（用户敲进去的那个名字）。
    pub key: &'static str,
    /// 目标：别名指向的下一个键；规范名/伪名留空串。
    pub target: &'static str,
    /// 种类。
    pub kind: RowKind,
    /// 该行占用的 CONFIG 位（0 表示无条件在表）。
    pub cfg: u32,
}

impl TableRow {
    /// 行折指纹。
    pub fn fold(self: &TableRow, h: u64) -> u64 {
        let a = fold_str(h, self.key);
        let b = fold_str(a, self.target);
        let c = fold_u32(b, self.kind.rank() as u32);
        fold_u32(c, self.cfg)
    }

    /// 行文本：`ls=canon->` 这种紧凑形态（渲染诊断用）。
    pub fn render(self: &TableRow) -> String {
        let mut s = String::from(self.key);
        s.push('=');
        s.push_str(self.kind.slug());
        s.push('>');
        s.push_str(self.target);
        s
    }

    /// 行内大写字节数——表侧全小写不变量的逐行贡献。
    pub fn upper_in_key(self: &TableRow) -> u32 {
        upper_bytes(self.key)
    }
}

/// 建模表本体：**每一个字母字节都是小写**。
pub const TABLE: [TableRow; 28usize] = [
    // ---- 规范名 15 行 ----
    TableRow { key: "ash", target: "", kind: RowKind::Canon, cfg: CFG_SHELL },
    TableRow { key: "test", target: "", kind: RowKind::Canon, cfg: 0u32 },
    TableRow { key: "grep", target: "", kind: RowKind::Canon, cfg: 0u32 },
    TableRow { key: "gunzip", target: "", kind: RowKind::Canon, cfg: CFG_GUNZIP },
    TableRow { key: "modprobe", target: "", kind: RowKind::Canon, cfg: CFG_MODPROBE },
    TableRow { key: "ls", target: "", kind: RowKind::Canon, cfg: 0u32 },
    TableRow { key: "cat", target: "", kind: RowKind::Canon, cfg: 0u32 },
    TableRow { key: "cp", target: "", kind: RowKind::Canon, cfg: 0u32 },
    TableRow { key: "mv", target: "", kind: RowKind::Canon, cfg: 0u32 },
    TableRow { key: "echo", target: "", kind: RowKind::Canon, cfg: 0u32 },
    TableRow { key: "printf", target: "", kind: RowKind::Canon, cfg: 0u32 },
    TableRow { key: "sed", target: "", kind: RowKind::Canon, cfg: 0u32 },
    TableRow { key: "awk", target: "", kind: RowKind::Canon, cfg: 0u32 },
    TableRow { key: "true", target: "", kind: RowKind::Canon, cfg: 0u32 },
    TableRow { key: "false", target: "", kind: RowKind::Canon, cfg: 0u32 },
    // ---- 别名 9 行 ----
    TableRow { key: "sh", target: "ash", kind: RowKind::Alias, cfg: CFG_SHELL },
    TableRow { key: "[", target: "test", kind: RowKind::Alias, cfg: 0u32 },
    TableRow { key: "[[", target: "test", kind: RowKind::Alias, cfg: 0u32 },
    TableRow { key: "egrep", target: "grep", kind: RowKind::Alias, cfg: CFG_GREP_ALIAS },
    TableRow { key: "fgrep", target: "grep", kind: RowKind::Alias, cfg: CFG_GREP_ALIAS },
    TableRow { key: "zcat", target: "gunzip", kind: RowKind::Alias, cfg: CFG_GUNZIP },
    TableRow { key: "gzcat", target: "zcat", kind: RowKind::Alias, cfg: CFG_GUNZIP },
    TableRow { key: "rmmod", target: "modprobe", kind: RowKind::Alias, cfg: CFG_MODPROBE },
    TableRow { key: "depmod", target: "modprobe", kind: RowKind::Alias, cfg: CFG_MODPROBE },
    // ---- 伪名 4 行 ----
    TableRow { key: "--install", target: "", kind: RowKind::Pseudo, cfg: 0u32 },
    TableRow { key: "--list", target: "", kind: RowKind::Pseudo, cfg: CFG_LIST },
    TableRow { key: "--list-full", target: "", kind: RowKind::Pseudo, cfg: CFG_LIST },
    TableRow { key: "busybox", target: "", kind: RowKind::Pseudo, cfg: 0u32 },
];

/// 规范名行数。
pub const CANON_ROWS: usize = 15usize;
/// 别名行数。
pub const ALIAS_ROWS: usize = 9usize;
/// 伪名行数。
pub const PSEUDO_ROWS: usize = 4usize;
/// 表总行数。
pub const TABLE_ROWS: usize = CANON_ROWS + ALIAS_ROWS + PSEUDO_ROWS;

/// 按行号取行。
pub fn table_row(i: usize) -> Option<TableRow> {
    match TABLE.get(i) {
        Some(r) => Some(*r),
        None => None,
    }
}

/// 表侧大写字节总数——本卡的全小写表不变量（应当恒为 0；一旦有人往表里抄
/// 一个大写名字，这里立刻非零，AC07 当场判死）。
pub fn table_upper_total() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < TABLE_ROWS {
        if let Some(r) = TABLE.get(i) {
            n = n.saturating_add(r.upper_in_key());
        }
        i = i.saturating_add(1usize);
    }
    n
}

/// 表内按种类计行。
pub fn table_rows_of(kind: RowKind) -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < TABLE_ROWS {
        if let Some(r) = TABLE.get(i) {
            if r.kind == kind {
                n = n.saturating_add(1u32);
            }
        }
        i = i.saturating_add(1usize);
    }
    n
}

/// 表指纹——逐行按表序折叠，取低 32 位。
///
/// 与 F11593 联签的就是这个数：两张单子必须折出同一个根，否则说的是两张表。
pub fn table_fingerprint() -> u32 {
    let mut h = FNV_OFF;
    let mut i = 0usize;
    while i < TABLE_ROWS {
        if let Some(r) = TABLE.get(i) {
            h = r.fold(h);
        }
        i = i.saturating_add(1usize);
    }
    (h & 0xffff_ffffu64) as u32
}

/// 键是否在表里（**逐字节**比较，绝不折叠）。
pub fn table_has(key: &str) -> bool {
    let mut i = 0usize;
    while i < TABLE_ROWS {
        if let Some(r) = TABLE.get(i) {
            if r.key == key {
                return true;
            }
        }
        i = i.saturating_add(1usize);
    }
    false
}

/// 按种类查键的身份；不在表里返回 `None`。
///
/// 这里绝不返回"某种兜底种类"：把未名当成伪名，等于让未名查询去走分派器入口，
/// 那是本卡最想要的假绿形态（`LS` 被判成"在册"）。
pub fn role_of_key(key: &str) -> Option<RowKind> {
    match find_row(key) {
        Some(r) => Some(r.kind),
        None => None,
    }
}

/// 表里是否已存在同种种类下的重复键（防重锚；本卡只查表自身，跨构建重名归
/// F11673）。
pub fn table_dup_key() -> bool {
    let mut i = 0usize;
    while i < TABLE_ROWS {
        let a = match TABLE.get(i) {
            Some(x) => *x,
            None => break,
        };
        let mut j = i.saturating_add(1usize);
        while j < TABLE_ROWS {
            if let Some(b) = TABLE.get(j) {
                if b.key == a.key {
                    return true;
                }
            }
            j = j.saturating_add(1usize);
        }
        i = i.saturating_add(1usize);
    }
    false
}

/// 别名是否遮蔽规范名（别名行的键不得同时是规范名行）。
pub fn table_alias_shadows() -> bool {
    let mut i = 0usize;
    while i < TABLE_ROWS {
        let a = match TABLE.get(i) {
            Some(x) => *x,
            None => break,
        };
        if a.kind != RowKind::Alias {
            i = i.saturating_add(1usize);
            continue;
        }
        let mut j = 0usize;
        while j < TABLE_ROWS {
            if let Some(b) = TABLE.get(j) {
                if b.kind == RowKind::Canon && b.key == a.key {
                    return true;
                }
            }
            j = j.saturating_add(1usize);
        }
        i = i.saturating_add(1usize);
    }
    false
}

/// 自指别名（`x -> x`）。
pub fn table_self_alias() -> bool {
    let mut i = 0usize;
    while i < TABLE_ROWS {
        if let Some(r) = TABLE.get(i) {
            if r.kind == RowKind::Alias && r.key == r.target {
                return true;
            }
        }
        i = i.saturating_add(1usize);
    }
    false
}

/// 悬空别名目标（目标既不是规范名也不是伪名，也不是另一个别名）。
pub fn table_dangling_alias() -> bool {
    let mut i = 0usize;
    while i < TABLE_ROWS {
        let r = match TABLE.get(i) {
            Some(x) => *x,
            None => break,
        };
        if r.kind == RowKind::Alias && !r.target.is_empty() && !table_has(r.target) {
            return true;
        }
        i = i.saturating_add(1usize);
    }
    false
}

/// 别名链收敛的结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Resolve {
    /// 键本身就是规范名——零跳直达。
    Direct,
    /// 经 `n` 跳别名收敛到规范名。
    Hopped(u8),
    /// 伪名入口（分派器自身）。
    Pseudo,
    /// 键不在表里。
    Miss,
    /// 悬空目标。
    Dangling,
    /// 超深或成环——`HOP_CAP` 之内没收敛出来。
    TooDeep,
}

impl Resolve {
    /// 结果号（0 基，用于折叠与申报）。
    pub const fn rank(self: &Resolve) -> u8 {
        match self {
            Resolve::Direct => 0u8,
            Resolve::Hopped(_) => 1u8,
            Resolve::Pseudo => 2u8,
            Resolve::Miss => 3u8,
            Resolve::Dangling => 4u8,
            Resolve::TooDeep => 5u8,
        }
    }

    /// 短名。
    pub fn slug(self: &Resolve) -> &'static str {
        match self {
            Resolve::Direct => "direct",
            Resolve::Hopped(_) => "hopped",
            Resolve::Pseudo => "pseudo",
            Resolve::Miss => "miss",
            Resolve::Dangling => "dangling",
            Resolve::TooDeep => "too-deep",
        }
    }

    /// 跳数（只有 `Hopped` 有跳数）。
    pub const fn hops(self: &Resolve) -> u8 {
        match self {
            Resolve::Direct => 0u8,
            Resolve::Hopped(n) => *n,
            _ => 0u8,
        }
    }

    /// 是否收敛成功（能落到一个可执行的 `main`）。
    pub fn reached(self: &Resolve) -> bool {
        match self {
            Resolve::Direct => true,
            Resolve::Hopped(_) => true,
            _ => false,
        }
    }
}

/// 逐字节查表 + 别名收敛——本卡唯一的"表事实"出口。
///
/// 注意 `key` 直接参与比较，没有任何 `lower_byte` 调用：一旦有人在这里加折叠，
/// `LS` 就会返回 `Direct`，AC08 与 `fold_trap_caught` 双双判死。
pub fn resolve(key: &str) -> Resolve {
    if key.is_empty() || odd_bytes(key) > 0u32 {
        return Resolve::Miss;
    }
    let mut cur: String = copy_name(key);
    let mut hops = 0u8;
    loop {
        let row = match find_row(cur.as_str()) {
            Some(r) => r,
            None => return Resolve::Miss,
        };
        match row.kind {
            RowKind::Canon => {
                if hops == 0u8 {
                    return Resolve::Direct;
                }
                if hops > HOP_CAP {
                    return Resolve::TooDeep;
                }
                return Resolve::Hopped(hops);
            }
            RowKind::Pseudo => return Resolve::Pseudo,
            RowKind::Alias => {
                if hops >= HOP_CAP {
                    // 还有一跳可走，但那一跳会把链长推到 HOP_CAP 之外——
                    // 宁可判"超深"，也不给出一个未经证明的规范名。
                    if row.target.is_empty() {
                        return Resolve::Dangling;
                    }
                    return Resolve::TooDeep;
                }
                if row.target.is_empty() {
                    return Resolve::Dangling;
                }
                if row.target == row.key {
                    return Resolve::TooDeep;
                }
                hops = hops.saturating_add(1u8);
                cur = copy_name(row.target);
            }
        }
    }
}

/// 查一行（逐字节相等）。
pub fn find_row(key: &str) -> Option<TableRow> {
    let mut i = 0usize;
    while i < TABLE_ROWS {
        if let Some(r) = TABLE.get(i) {
            if r.key == key {
                return Some(*r);
            }
        }
        i = i.saturating_add(1usize);
    }
    None
}

/// 名字复制（**原样**，不折叠）——`resolve` 的链式游标用它，避免借用表切片的
/// 生命周期在 no_std 里绕圈。这里出现一次 `push_str` 就是"逐字节搬运"的实况：
/// 任何把 `lower_byte` 引进这条路径的改动，都会让 `LS` 一路收敛到 `ls`。
pub fn copy_name(s: &str) -> String {
    let mut out = String::new();
    out.push_str(s);
    out
}

// ---------------------------------------------------------------------------
// 文案：msg_* 逐字复刻真实报错
// ---------------------------------------------------------------------------

/// 未名答复逐字模板：`busybox: 'LS': applet not found`。
///
/// 程序名 + `: ` + 单引号 + 原样名字 + `': applet not found`。名字**原样**呈现：
/// 敲 `LS` 就印 `LS`，印成 `ls` 就是折叠泄漏（本卡判死，文案细判归 F11674）。
pub fn msg_notfound(name: &str) -> String {
    let mut s = String::from(PROG);
    s.push_str(": '");
    s.push_str(name);
    s.push_str(TAIL_NOTFOUND);
    s
}

/// 旧版无名字档：`busybox: applet not found`。
pub fn msg_legacy() -> String {
    let mut s = String::from(PROG);
    s.push_str(": applet not found");
    s
}

/// `noapplet` 桩档：`busybox: applet 'LS' not found`。
pub fn msg_stub(name: &str) -> String {
    let mut s = String::from(PROG);
    s.push_str(": applet '");
    s.push_str(name);
    s.push_str("' not found");
    s
}

/// 折叠泄漏档（**反面**构造，判死用）：真机不会解释"你是不是想敲小写"。
pub fn msg_fold_hint(name: &str, folded: &str) -> String {
    let mut s = String::from(PROG);
    s.push_str(": '");
    s.push_str(name);
    s.push_str(TAIL_NOTFOUND);
    s.push_str(", ");
    s.push_str(FORBIDDEN_FOLD_HINT);
    s.push_str(" '");
    s.push_str(folded);
    s.push('\'');
    s
}

/// 大小写解释泄漏档（反面）：`busybox: 'LS': applet not found (case-sensitive)`。
pub fn msg_case_lecture(name: &str) -> String {
    let mut s = String::from(PROG);
    s.push_str(": '");
    s.push_str(name);
    s.push_str(TAIL_NOTFOUND);
    s.push_str(" (");
    s.push_str(FORBIDDEN_CASE_WORDS);
    s.push(')');
    s
}

/// CONFIG 提示泄漏档（反面）。
pub fn msg_config_hint(name: &str) -> String {
    let mut s = String::from(PROG);
    s.push_str(": '");
    s.push_str(name);
    s.push_str(TAIL_NOTFOUND);
    s.push_str(" (enable ");
    s.push_str(FORBIDDEN_CONFIG);
    s.push_str("APPLET_");
    s.push_str(name);
    s.push_str(" in .config)");
    s
}

/// 别名遮蔽的构造性报错档（反面）：真机上别名遮蔽规范名不会报错，而是**顶掉**
/// 真实现——这里把它写成一行文本，只为让"遮蔽"这件事在渲染面留痕。
pub fn msg_shadow(canon: &str, alias: &str) -> String {
    let mut s = String::from("shadow: alias '");
    s.push_str(alias);
    s.push_str("' hides canonical '");
    s.push_str(canon);
    s.push('\'');
    s
}

/// 联签锚不符档（反面）：本卡与 F11593 读到的不是同一张表。
pub fn msg_cosign(mine: u32, theirs: u32) -> String {
    let mut s = String::from("cosign skew: table 0x");
    s.push_str(hex_text(mine, 8usize).as_str());
    s.push_str(" != f11593 0x");
    s.push_str(hex_text(theirs, 8usize).as_str());
    s
}

/// 空答复：什么都没写。
pub fn msg_empty() -> String {
    String::new()
}

/// 一句话点出这条现场为什么红（诊断面，不参与判定）。
pub fn msg_why(code: u32) -> String {
    let mut s = String::from(code_name(code));
    s.push_str(": ");
    s.push_str(code_phrase(code));
    s
}

// ---------------------------------------------------------------------------
// 域枚举：闸 / 输入类 / 命中档 / 折叠档 / 成因 / 载体 / 档位
// ---------------------------------------------------------------------------

/// 八道闸。rank 是 **1 基**——0 基会让"第一步只能落在首闸"变成"第一步落在 0"，
/// 于是跳过首闸反而合法（同批 F11660 的教训）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// 词法：查询名能不能进 `strcmp`。
    Lex,
    /// 大小写类：查询名归哪一类。
    Class,
    /// 表形：本构建的 applet 表是不是全小写。
    Table,
    /// 联签：F11593 的别名表锚对不对得上。
    CoSign,
    /// 别名：命中别名时能不能收敛到规范名。
    Alias,
    /// 答复：档对不对（折叠命中在这里判死）。
    Reply,
    /// 码档：退出码与 errno。
    Code,
    /// 封印：前置达成、断言走满、默认必跑位。
    Seal,
}

impl Stage {
    /// 1 基闸号。
    pub const fn rank(self: &Stage) -> u8 {
        match self {
            Stage::Lex => 1u8,
            Stage::Class => 2u8,
            Stage::Table => 3u8,
            Stage::CoSign => 4u8,
            Stage::Alias => 5u8,
            Stage::Reply => 6u8,
            Stage::Code => 7u8,
            Stage::Seal => 8u8,
        }
    }

    /// 闸数。
    pub const COUNT: u8 = 8u8;

    /// 下一道闸；封印闸没有下一道。
    pub fn next(self: &Stage) -> Option<Stage> {
        match self.rank() {
            1u8 => Some(Stage::Class),
            2u8 => Some(Stage::Table),
            3u8 => Some(Stage::CoSign),
            4u8 => Some(Stage::Alias),
            5u8 => Some(Stage::Reply),
            6u8 => Some(Stage::Code),
            7u8 => Some(Stage::Seal),
            _ => None,
        }
    }

    /// 按闸号回查；越界返回 `None`（不许 panic，也不许悄悄落到 `Seal`）。
    pub fn from_rank(r: u8) -> Option<Stage> {
        match r {
            1u8 => Some(Stage::Lex),
            2u8 => Some(Stage::Class),
            3u8 => Some(Stage::Table),
            4u8 => Some(Stage::CoSign),
            5u8 => Some(Stage::Alias),
            6u8 => Some(Stage::Reply),
            7u8 => Some(Stage::Code),
            8u8 => Some(Stage::Seal),
            _ => None,
        }
    }

    /// 闸名（ktest 短体）。
    pub fn slug(self: &Stage) -> &'static str {
        match self {
            Stage::Lex => "lex",
            Stage::Class => "class",
            Stage::Table => "table",
            Stage::CoSign => "cosign",
            Stage::Alias => "alias",
            Stage::Reply => "reply",
            Stage::Code => "code",
            Stage::Seal => "seal",
        }
    }

    /// 该闸的签名码。
    ///
    /// 八闸各取一码、互不重复；闸内允许出现更专的码：联签闸的"根本没联签"走
    /// `AC_ESKIP`（那是前提缺席，不是对不上），别名闸的自指/遮蔽走 `AC_EUNIQ`，
    /// 冻结闸的"重试过却申报首次即中"走 `AC_EFLAKE`，对判六维走
    /// `AC_ESTATE`/`AC_ESTAGE`/`AC_ENOTE`，槽位越界走 `AC_ESLOT`。
    pub fn fault_code(self: &Stage) -> u32 {
        match self {
            Stage::Lex => AC_EUNIQ,
            Stage::Class => AC_EVALUE,
            Stage::Table => AC_ELOWER,
            Stage::CoSign => AC_ESKIP,
            Stage::Alias => AC_EALIAS,
            Stage::Reply => AC_ECASE,
            Stage::Code => AC_ECODE,
            Stage::Seal => AC_EDEFAULT,
        }
    }

    /// 是否已经抵达现场（答复闸及其后）。跳过类结论只在抵达之前成立。
    pub fn past_reply(self: &Stage) -> bool {
        self.rank() >= Stage::Reply.rank()
    }

    /// 是否属于"表/联签"这一族（构建期事实，与一次查询无关）。
    pub fn build_side(self: &Stage) -> bool {
        let r = self.rank();
        r >= Stage::Table.rank() && r <= Stage::CoSign.rank()
    }
}

/// 八闸链表正序。
///
/// 之所以要一份显式链表而不是只靠 `Stage::next()` 串：`next()` 说"从这道闸能去哪"，
/// 链表说"这条链应该长什么样"。两份东西各写一遍，才能交叉核出"某道闸的 `next`
/// 被人改成回指"这类破坏——只有一份时，改了它无人能发现。
pub const STAGE_CHAIN: [Stage; 8usize] = [
    Stage::Lex,
    Stage::Class,
    Stage::Table,
    Stage::CoSign,
    Stage::Alias,
    Stage::Reply,
    Stage::Code,
    Stage::Seal,
];

/// 闸数（与 `Stage::COUNT` 必须相等，由 `chain_ok` 核）。
pub const GATE_COUNT: usize = 8usize;

/// 查询名的大小写类。
///
/// 这一档由**字节实测算出**，不靠申报：申报的 `cls` 与实测的 `name_class()`
/// 不符就是 Class 闸的红。之所以要留一份申报位，是因为真实故障恰恰是"归类代码
/// 算错了"——留一份申报才能把算错的那次钉出来。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NameClass {
    /// 未定类（零值；任何一条闸都不该停在未定类上）。
    Unknown,
    /// 全小写（可含数字/标点，不含大写字母）。
    AllLower,
    /// 全大写（至少一个大写、零个小写）。
    AllUpper,
    /// 混合大小写。
    Mixed,
    /// 无字母（纯数字/标点/连字符）。
    NoLetter,
    /// 含非 ASCII 字节。
    NonAscii,
    /// 畸形字节（含控制/空白）。
    OddByte,
    /// 空名。
    Empty,
}

impl NameClass {
    /// 1 基类号。
    pub const fn rank(self: &NameClass) -> u8 {
        match self {
            NameClass::Unknown => 0u8,
            NameClass::AllLower => 1u8,
            NameClass::AllUpper => 2u8,
            NameClass::Mixed => 3u8,
            NameClass::NoLetter => 4u8,
            NameClass::NonAscii => 5u8,
            NameClass::OddByte => 6u8,
            NameClass::Empty => 7u8,
        }
    }

    /// 类数（不含 `Unknown`）。
    pub const COUNT: u8 = 7u8;

    /// 按号回查。
    pub fn from_rank(r: u8) -> Option<NameClass> {
        match r {
            1u8 => Some(NameClass::AllLower),
            2u8 => Some(NameClass::AllUpper),
            3u8 => Some(NameClass::Mixed),
            4u8 => Some(NameClass::NoLetter),
            5u8 => Some(NameClass::NonAscii),
            6u8 => Some(NameClass::OddByte),
            7u8 => Some(NameClass::Empty),
            _ => None,
        }
    }

    /// 类短名。
    pub fn slug(self: &NameClass) -> &'static str {
        match self {
            NameClass::Unknown => "unknown",
            NameClass::AllLower => "all-lower",
            NameClass::AllUpper => "all-upper",
            NameClass::Mixed => "mixed",
            NameClass::NoLetter => "no-letter",
            NameClass::NonAscii => "non-ascii",
            NameClass::OddByte => "odd-byte",
            NameClass::Empty => "empty",
        }
    }

    /// 该类是否**可能**命中全小写表。
    ///
    /// 只有 `AllLower` 与 `NoLetter` 可能——后者"可能"的意思是"不因为大小写
    /// 而被排除"（`[`、`--list` 走这一条）。其余类一律必不命中。
    pub fn can_match(self: &NameClass) -> bool {
        match self {
            NameClass::AllLower => true,
            NameClass::NoLetter => true,
            _ => false,
        }
    }

    /// 该类是否因为大小写而注定不命中（本卡主锚）。
    pub fn case_blocks(self: &NameClass) -> bool {
        match self {
            NameClass::AllUpper => true,
            NameClass::Mixed => true,
            _ => false,
        }
    }

    /// 该类是否属于词法问题（进不了 `strcmp` 语义）。
    pub fn lex_broken(self: &NameClass) -> bool {
        match self {
            NameClass::Empty => true,
            NameClass::OddByte => true,
            NameClass::Unknown => true,
            _ => false,
        }
    }

    /// 类权：横计时"这一类的一条值多少"。`AllUpper` 与 `Mixed` 最重——
    /// 它们是唯一能让"折叠实现"蒙混过关的类。
    pub fn weight(self: &NameClass) -> u32 {
        match self {
            NameClass::AllUpper => 4u32,
            NameClass::Mixed => 4u32,
            NameClass::AllLower => 3u32,
            NameClass::NoLetter => 2u32,
            NameClass::NonAscii => 2u32,
            NameClass::OddByte => 1u32,
            NameClass::Empty => 1u32,
            NameClass::Unknown => 0u32,
        }
    }
}

/// 查表答复档（命中之后发生了什么）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Reply {
    /// 未定。
    None,
    /// 规范名直接命中。
    Ran,
    /// 经别名收敛命中（`hops` 记跳数）。
    AliasHit,
    /// 未名档——`busybox: 'LS': applet not found`，退 127。
    Notfound,
    /// 折叠命中——大写查询被执行了小写表项的 `main`（本卡判死档）。
    FoldTrap,
    /// 命中了却一个字没答（`noapplet` 桩接错头之类的现场）。
    Silent,
    /// 崩了——信号档退出。
    Crash,
}

impl Reply {
    /// 1 基答复号。
    pub const fn rank(self: &Reply) -> u8 {
        match self {
            Reply::None => 0u8,
            Reply::Ran => 1u8,
            Reply::AliasHit => 2u8,
            Reply::Notfound => 3u8,
            Reply::FoldTrap => 4u8,
            Reply::Silent => 5u8,
            Reply::Crash => 6u8,
        }
    }

    /// 答复档数。
    pub const COUNT: u8 = 6u8;

    /// 按号回查。
    pub fn from_rank(r: u8) -> Option<Reply> {
        match r {
            1u8 => Some(Reply::Ran),
            2u8 => Some(Reply::AliasHit),
            3u8 => Some(Reply::Notfound),
            4u8 => Some(Reply::FoldTrap),
            5u8 => Some(Reply::Silent),
            6u8 => Some(Reply::Crash),
            _ => None,
        }
    }

    /// 短名。
    pub fn slug(self: &Reply) -> &'static str {
        match self {
            Reply::None => "none",
            Reply::Ran => "ran",
            Reply::AliasHit => "alias-hit",
            Reply::Notfound => "notfound",
            Reply::FoldTrap => "fold-trap",
            Reply::Silent => "silent",
            Reply::Crash => "crash",
        }
    }

    /// 该答复档是否算"合法答复"——即真机可能给出的那几档。
    ///
    /// `FoldTrap` **不**合法：它就是本卡要判死的东西。`Silent`/`Crash` 合法存在
    /// （真实构建会这样坏），但不满足本卡的答复闸。
    pub fn is_legit(self: &Reply) -> bool {
        match self {
            Reply::Ran => true,
            Reply::AliasHit => true,
            Reply::Notfound => true,
            _ => false,
        }
    }

    /// 该答复档约定的退出码。
    pub fn exit_code(self: &Reply) -> u16 {
        match self {
            Reply::Ran => EX_OK,
            Reply::AliasHit => EX_OK,
            Reply::Notfound => EX_NOTFOUND,
            Reply::FoldTrap => EX_OK,
            Reply::Silent => EX_GENERAL,
            Reply::Crash => EX_SEGV,
            Reply::None => EX_GENERAL,
        }
    }

    /// 该答复档约定的 errno——只有未名档在查表侧留 ENOENT，其余档不该有残留。
    pub fn errno_of(self: &Reply) -> u16 {
        match self {
            Reply::Notfound => ENOENT,
            _ => ENONE,
        }
    }

    /// 该答复档是否把名字说进了答复里（本卡用它做"答复与查询同形"的字节锚）。
    pub fn carries_query(self: &Reply) -> bool {
        match self {
            Reply::Notfound => true,
            Reply::FoldTrap => false,
            Reply::Silent => false,
            Reply::Crash => false,
            _ => false,
        }
    }
}

/// 别名链收敛结果的申报档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hit {
    /// 未定。
    None,
    /// 规范名零跳直达。
    Direct,
    /// 一跳别名。
    OneHop,
    /// 两跳别名（`HOP_CAP` 上沿）。
    TwoHop,
    /// 未命中。
    Miss,
    /// 悬空目标。
    Dangling,
    /// 自指别名。
    SelfAlias,
    /// 遮蔽规范名。
    Shadowed,
    /// 成环或超深。
    Cycled,
}

impl Hit {
    /// 1 基档号。
    pub const fn rank(self: &Hit) -> u8 {
        match self {
            Hit::None => 0u8,
            Hit::Direct => 1u8,
            Hit::OneHop => 2u8,
            Hit::TwoHop => 3u8,
            Hit::Miss => 4u8,
            Hit::Dangling => 5u8,
            Hit::SelfAlias => 6u8,
            Hit::Shadowed => 7u8,
            Hit::Cycled => 8u8,
        }
    }

    /// 档数。
    pub const COUNT: u8 = 8u8;

    /// 按号回查。
    pub fn from_rank(r: u8) -> Option<Hit> {
        match r {
            1u8 => Some(Hit::Direct),
            2u8 => Some(Hit::OneHop),
            3u8 => Some(Hit::TwoHop),
            4u8 => Some(Hit::Miss),
            5u8 => Some(Hit::Dangling),
            6u8 => Some(Hit::SelfAlias),
            7u8 => Some(Hit::Shadowed),
            8u8 => Some(Hit::Cycled),
            _ => None,
        }
    }

    /// 短名。
    pub fn slug(self: &Hit) -> &'static str {
        match self {
            Hit::None => "none",
            Hit::Direct => "direct",
            Hit::OneHop => "one-hop",
            Hit::TwoHop => "two-hop",
            Hit::Miss => "miss",
            Hit::Dangling => "dangling",
            Hit::SelfAlias => "self-alias",
            Hit::Shadowed => "shadowed",
            Hit::Cycled => "cycled",
        }
    }

    /// 该档对应的跳数（申报侧的 `hops` 必须等于它）。
    pub fn hops(self: &Hit) -> u8 {
        match self {
            Hit::Direct => 0u8,
            Hit::OneHop => 1u8,
            Hit::TwoHop => 2u8,
            _ => 0u8,
        }
    }

    /// 是否收敛成功（拿到了一个可执行的规范名）。
    pub fn reached(self: &Hit) -> bool {
        match self {
            Hit::Direct => true,
            Hit::OneHop => true,
            Hit::TwoHop => true,
            _ => false,
        }
    }

    /// 是否属于别名结构缺陷。
    pub fn is_defective(self: &Hit) -> bool {
        match self {
            Hit::Dangling => true,
            Hit::SelfAlias => true,
            Hit::Shadowed => true,
            Hit::Cycled => true,
            _ => false,
        }
    }
}

/// 成因注记——红行必须说清"坏在哪一条"。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Note {
    /// 无注记（零值）。
    None,
    /// 全绿走满。
    Clean,
    /// 抖动后恢复的绿（不许洗成 `Clean`）。
    FlakeRecovered,
    /// 查询名不可进查表（空/超长/含空白控制字节）。
    BadLex,
    /// 大写查询被折叠命中——本卡主锚红。
    FoldTrap,
    /// 表侧混入大写字节。
    UpperInTable,
    /// 别名目标悬空。
    DanglingTarget,
    /// 自指别名。
    SelfReferent,
    /// 别名遮蔽规范名。
    ShadowCanon,
    /// 别名链成环。
    AliasCycle,
    /// 别名链超过 `HOP_CAP`。
    HopOverCap,
    /// 联签锚缺席。
    CosignAbsent,
    /// 联签锚对不上（行数或指纹与 F11593 不符）。
    CosignSkew,
    /// 表内重名。
    TableDup,
    /// 答复档与输入类矛盾（大写类却报命中）。
    ReplySkew,
    /// 答复一个字没有。
    SilentReply,
    /// 码档漂移。
    CodeDrift,
    /// errno 残留。
    ErrnoDrift,
    /// 前置未达成。
    DepBlocked,
    /// 断言没走满。
    AssertShort,
    /// 载体或配置缺席，根本没跑。
    AbsentCarrier,
}

impl Note {
    /// 1 基注记号。
    pub const fn rank(self: &Note) -> u8 {
        match self {
            Note::None => 0u8,
            Note::Clean => 1u8,
            Note::FlakeRecovered => 2u8,
            Note::BadLex => 3u8,
            Note::FoldTrap => 4u8,
            Note::UpperInTable => 5u8,
            Note::DanglingTarget => 6u8,
            Note::SelfReferent => 7u8,
            Note::ShadowCanon => 8u8,
            Note::AliasCycle => 9u8,
            Note::HopOverCap => 10u8,
            Note::CosignAbsent => 11u8,
            Note::CosignSkew => 12u8,
            Note::TableDup => 13u8,
            Note::ReplySkew => 14u8,
            Note::SilentReply => 15u8,
            Note::CodeDrift => 16u8,
            Note::ErrnoDrift => 17u8,
            Note::DepBlocked => 18u8,
            Note::AssertShort => 19u8,
            Note::AbsentCarrier => 20u8,
        }
    }

    /// 注记数（不含 `None`）。
    pub const COUNT: u8 = 20u8;

    /// 按号回查。
    pub fn from_rank(r: u8) -> Option<Note> {
        match r {
            1u8 => Some(Note::Clean),
            2u8 => Some(Note::FlakeRecovered),
            3u8 => Some(Note::BadLex),
            4u8 => Some(Note::FoldTrap),
            5u8 => Some(Note::UpperInTable),
            6u8 => Some(Note::DanglingTarget),
            7u8 => Some(Note::SelfReferent),
            8u8 => Some(Note::ShadowCanon),
            9u8 => Some(Note::AliasCycle),
            10u8 => Some(Note::HopOverCap),
            11u8 => Some(Note::CosignAbsent),
            12u8 => Some(Note::CosignSkew),
            13u8 => Some(Note::TableDup),
            14u8 => Some(Note::ReplySkew),
            15u8 => Some(Note::SilentReply),
            16u8 => Some(Note::CodeDrift),
            17u8 => Some(Note::ErrnoDrift),
            18u8 => Some(Note::DepBlocked),
            19u8 => Some(Note::AssertShort),
            20u8 => Some(Note::AbsentCarrier),
            _ => None,
        }
    }

    /// 注记短名（进渲染与指纹）。
    pub fn slug(self: &Note) -> &'static str {
        match self {
            Note::None => "none",
            Note::Clean => "clean",
            Note::FlakeRecovered => "flake-recovered",
            Note::BadLex => "bad-lex",
            Note::FoldTrap => "fold-trap",
            Note::UpperInTable => "upper-in-table",
            Note::DanglingTarget => "dangling-target",
            Note::SelfReferent => "self-alias",
            Note::ShadowCanon => "shadow-canon",
            Note::AliasCycle => "alias-cycle",
            Note::HopOverCap => "hop-over-cap",
            Note::CosignAbsent => "cosign-absent",
            Note::CosignSkew => "cosign-skew",
            Note::TableDup => "table-dup",
            Note::ReplySkew => "reply-skew",
            Note::SilentReply => "silent-reply",
            Note::CodeDrift => "code-drift",
            Note::ErrnoDrift => "errno-drift",
            Note::DepBlocked => "dep-blocked",
            Note::AssertShort => "assert-short",
            Note::AbsentCarrier => "absent-carrier",
        }
    }

    /// 绿行合法的注记只有两种：全绿与抖动后恢复。
    pub fn allowed_on_green(self: &Note) -> bool {
        match self {
            Note::Clean => true,
            Note::FlakeRecovered => true,
            _ => false,
        }
    }

    /// 是否属于别名族注记。
    pub fn alias_family(self: &Note) -> bool {
        match self {
            Note::DanglingTarget => true,
            Note::SelfReferent => true,
            Note::ShadowCanon => true,
            Note::AliasCycle => true,
            Note::HopOverCap => true,
            _ => false,
        }
    }

    /// 是否属于联签族注记。
    pub fn cosign_family(self: &Note) -> bool {
        match self {
            Note::CosignAbsent => true,
            Note::CosignSkew => true,
            _ => false,
        }
    }

    /// 该注记应当停在哪道闸——`None` 表示绿行，不必停。
    pub fn home_stage(self: &Note) -> Option<Stage> {
        match self {
            Note::AbsentCarrier => Some(Stage::Lex),
            Note::BadLex => Some(Stage::Lex),
            Note::FoldTrap => Some(Stage::Reply),
            Note::UpperInTable => Some(Stage::Table),
            Note::TableDup => Some(Stage::Table),
            Note::CosignAbsent => Some(Stage::CoSign),
            Note::CosignSkew => Some(Stage::CoSign),
            Note::DanglingTarget => Some(Stage::Alias),
            Note::SelfReferent => Some(Stage::Alias),
            Note::ShadowCanon => Some(Stage::Alias),
            Note::AliasCycle => Some(Stage::Alias),
            Note::HopOverCap => Some(Stage::Alias),
            Note::ReplySkew => Some(Stage::Reply),
            Note::SilentReply => Some(Stage::Reply),
            Note::CodeDrift => Some(Stage::Code),
            Note::ErrnoDrift => Some(Stage::Code),
            Note::DepBlocked => Some(Stage::Seal),
            Note::AssertShort => Some(Stage::Seal),
            Note::Clean => None,
            Note::FlakeRecovered => None,
            Note::None => None,
        }
    }

    /// 该注记的代价权——重算总代价时用；折叠命中与表侧大写最贵。
    pub fn cost(self: &Note) -> u32 {
        match self {
            Note::FoldTrap => 6u32,
            Note::UpperInTable => 5u32,
            Note::CosignSkew => 4u32,
            Note::CosignAbsent => 3u32,
            Note::ShadowCanon => 4u32,
            Note::AliasCycle => 4u32,
            Note::DanglingTarget => 3u32,
            Note::SelfReferent => 3u32,
            Note::HopOverCap => 2u32,
            Note::ReplySkew => 3u32,
            Note::SilentReply => 2u32,
            Note::CodeDrift => 2u32,
            Note::ErrnoDrift => 1u32,
            Note::BadLex => 1u32,
            Note::TableDup => 3u32,
            Note::DepBlocked => 1u32,
            Note::AssertShort => 1u32,
            Note::AbsentCarrier => 0u32,
            Note::FlakeRecovered => 1u32,
            Note::Clean => 0u32,
            Note::None => 0u32,
        }
    }
}

/// 前提缺席成因。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Skip {
    /// 不在缺席档上。
    None,
    /// 载体根本不在。
    NoCarrier,
    /// 没跑。
    NotRun,
    /// 别名表 CONFIG 不在。
    NoAliasCfg,
    /// 规范名（shell/grep/gunzip/modprobe）不在。
    NoCanon,
    /// 表扫描载体不在。
    NoList,
    /// 查询名本身不合法——这条没有现场可言。
    NoQuery,
    /// 联签对象 F11593 不在场。
    NoCoSign,
}

impl Skip {
    /// 1 基成因号。
    pub const fn rank(self: &Skip) -> u8 {
        match self {
            Skip::None => 0u8,
            Skip::NoCarrier => 1u8,
            Skip::NotRun => 2u8,
            Skip::NoAliasCfg => 3u8,
            Skip::NoCanon => 4u8,
            Skip::NoList => 5u8,
            Skip::NoQuery => 6u8,
            Skip::NoCoSign => 7u8,
        }
    }

    /// 成因数。
    pub const COUNT: u8 = 7u8;

    /// 按号回查。
    pub fn from_rank(r: u8) -> Option<Skip> {
        match r {
            1u8 => Some(Skip::NoCarrier),
            2u8 => Some(Skip::NotRun),
            3u8 => Some(Skip::NoAliasCfg),
            4u8 => Some(Skip::NoCanon),
            5u8 => Some(Skip::NoList),
            6u8 => Some(Skip::NoQuery),
            7u8 => Some(Skip::NoCoSign),
            _ => None,
        }
    }

    /// 短名。
    pub fn slug(self: &Skip) -> &'static str {
        match self {
            Skip::None => "none",
            Skip::NoCarrier => "no-carrier",
            Skip::NotRun => "not-run",
            Skip::NoAliasCfg => "no-alias-cfg",
            Skip::NoCanon => "no-canon",
            Skip::NoList => "no-list",
            Skip::NoQuery => "no-query",
            Skip::NoCoSign => "no-cosign",
        }
    }

    /// 是否处于缺席档。
    pub fn is_skip(self: &Skip) -> bool {
        self.rank() != 0u8
    }

    /// 缺席成因折成的注记。
    ///
    /// 只有一条路通到 `AbsentCarrier`：成因必须是"环境里根本没有这件事"。
    /// 别名链断、遮蔽这一类**不是**缺席，它们是该卡判死的红。
    pub fn note(self: &Skip) -> Note {
        match self {
            Skip::NoCarrier => Note::AbsentCarrier,
            Skip::NotRun => Note::AbsentCarrier,
            Skip::NoAliasCfg => Note::AbsentCarrier,
            Skip::NoCanon => Note::AbsentCarrier,
            Skip::NoList => Note::AbsentCarrier,
            Skip::NoQuery => Note::BadLex,
            Skip::NoCoSign => Note::CosignAbsent,
            Skip::None => Note::None,
        }
    }

    /// 是否算"根本没跑成"（诚实 skip）。
    pub fn honest(self: &Skip) -> bool {
        match self {
            Skip::NoCarrier => true,
            Skip::NotRun => true,
            Skip::NoAliasCfg => true,
            Skip::NoCanon => true,
            Skip::NoList => true,
            Skip::NoCoSign => true,
            _ => false,
        }
    }
}

/// 载体（这条判据用什么方式跑出来）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Harness {
    /// 无载体（零值，缺席档）。
    Noop,
    /// 直接调用 `busybox <name>`。
    DirectCall,
    /// 用展开产物扫表。
    TableScan,
    /// 伪名 `--list` 面扫表。
    ListProbe,
    /// 别名专项探针（逐个别名敲一遍）。
    AliasProbe,
    /// 两机构建对照。
    TwoMachine,
    /// 大小写扫名族（`ls`/`LS`/`Ls` 成组敲）。
    CaseSweep,
}

impl Harness {
    /// 1 基载体号。
    pub const fn rank(self: &Harness) -> u8 {
        match self {
            Harness::Noop => 0u8,
            Harness::DirectCall => 1u8,
            Harness::TableScan => 2u8,
            Harness::ListProbe => 3u8,
            Harness::AliasProbe => 4u8,
            Harness::TwoMachine => 5u8,
            Harness::CaseSweep => 6u8,
        }
    }

    /// 载体数。
    pub const COUNT: u8 = 6u8;

    /// 按号回查。
    pub fn from_rank(r: u8) -> Option<Harness> {
        match r {
            1u8 => Some(Harness::DirectCall),
            2u8 => Some(Harness::TableScan),
            3u8 => Some(Harness::ListProbe),
            4u8 => Some(Harness::AliasProbe),
            5u8 => Some(Harness::TwoMachine),
            6u8 => Some(Harness::CaseSweep),
            _ => None,
        }
    }

    /// 短名。
    pub fn slug(self: &Harness) -> &'static str {
        match self {
            Harness::Noop => "noop",
            Harness::DirectCall => "direct",
            Harness::TableScan => "table-scan",
            Harness::ListProbe => "list-probe",
            Harness::AliasProbe => "alias-probe",
            Harness::TwoMachine => "two-machine",
            Harness::CaseSweep => "case-sweep",
        }
    }

    /// 是否属于默认必跑套件。
    pub fn default_member(self: &Harness) -> bool {
        match self {
            Harness::DirectCall => true,
            Harness::TableScan => true,
            Harness::ListProbe => true,
            Harness::AliasProbe => true,
            Harness::CaseSweep => true,
            Harness::Noop => false,
            Harness::TwoMachine => false,
        }
    }

    /// 是否需要第二份构建/第二台机器（这类行不许进默认必跑集）。
    pub fn needs_second(self: &Harness) -> bool {
        match self {
            Harness::TwoMachine => true,
            _ => false,
        }
    }

    /// 载体是否产出可比对的文本。
    pub fn yields_text(self: &Harness) -> bool {
        match self {
            Harness::Noop => false,
            _ => true,
        }
    }

    /// 耗时合理带基值（毫秒）；`ms_ok` 用它算上下界。
    pub fn ms_band(self: &Harness) -> u32 {
        match self {
            Harness::Noop => 0u32,
            Harness::DirectCall => 12u32,
            Harness::TableScan => 40u32,
            Harness::ListProbe => 30u32,
            Harness::AliasProbe => 24u32,
            Harness::TwoMachine => 180u32,
            Harness::CaseSweep => 36u32,
        }
    }

    /// 该载体在本卡里的期望断言数上界（申报超过它即虚报）。
    pub fn assert_cap(self: &Harness) -> u16 {
        match self {
            Harness::Noop => 0u16,
            Harness::DirectCall => 9u16,
            Harness::TableScan => 10u16,
            Harness::ListProbe => 7u16,
            Harness::AliasProbe => 9u16,
            Harness::TwoMachine => 12u16,
            Harness::CaseSweep => 11u16,
        }
    }
}

/// 横计档位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    /// 绿：八闸走满且六维对齐。
    Green,
    /// 抖：绿过，但重试过（必须显式记档）。
    Flaked,
    /// 跳：载体或配置缺席，根本没跑成。
    Skipped,
    /// 阻：跑成了但红。
    Blocked,
}

impl Tier {
    /// 1 基档号。
    pub const fn rank(self: &Tier) -> u8 {
        match self {
            Tier::Green => 1u8,
            Tier::Flaked => 2u8,
            Tier::Skipped => 3u8,
            Tier::Blocked => 4u8,
        }
    }

    /// 按号回查。
    pub fn from_rank(r: u8) -> Option<Tier> {
        match r {
            1u8 => Some(Tier::Green),
            2u8 => Some(Tier::Flaked),
            3u8 => Some(Tier::Skipped),
            4u8 => Some(Tier::Blocked),
            _ => None,
        }
    }

    /// 档名（横计矩阵的行首）。
    pub fn slug(self: &Tier) -> &'static str {
        match self {
            Tier::Green => "green",
            Tier::Flaked => "flaked",
            Tier::Skipped => "skipped",
            Tier::Blocked => "blocked",
        }
    }

    /// 由实测结局归档。
    ///
    /// 契约要求的唯一入口：档位**只**由结局算，不看申报——申报算档就等于
    /// "自称绿即绿"。
    pub fn of(actual: &Outcome) -> Tier {
        if actual.ok {
            return match actual.note {
                Note::FlakeRecovered => Tier::Flaked,
                _ => Tier::Green,
            };
        }
        match actual.note {
            Note::AbsentCarrier => Tier::Skipped,
            _ => Tier::Blocked,
        }
    }

    /// 档权：阻档最重，它直接对应线上故障。
    pub fn weight(self: &Tier) -> u32 {
        match self {
            Tier::Blocked => 4u32,
            Tier::Skipped => 3u32,
            Tier::Flaked => 2u32,
            Tier::Green => 1u32,
        }
    }

    /// 是否算进绿集。
    ///
    /// 抖动档进绿集是有意的：线上只问"这次给了正确答案没有"。代价是抖动必须
    /// 单独记档（`Note::FlakeRecovered`），否则就是洗绿——`Matrix::green_has_lie()`
    /// 只查绿档，正是为了这条交换成立。
    pub fn in_green(self: &Tier) -> bool {
        match self {
            Tier::Green => true,
            Tier::Flaked => true,
            Tier::Skipped => false,
            Tier::Blocked => false,
        }
    }

    /// 是否属于"没放行"那一类（跳与阻成因完全不同，但都不进绿集）。
    pub fn is_halted(self: &Tier) -> bool {
        match self {
            Tier::Skipped => true,
            Tier::Blocked => true,
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// 字节实测的名字大小写类（唯一归类出口）
// ---------------------------------------------------------------------------

/// 由字节实测出查询名的大小写类。
///
/// 归类次序是有讲究的，每一档都对应一种"归因不同"的真实故障：
///
/// 1. **空名**先行：空串在真机上压根不产生答复（shell 不会把空参数交给 busybox），
///    先问它有没有大写字节没有意义。
/// 2. **非 ASCII** 先于大写：UTF-8 名字在逐字节比较下必不命中，但成因是"字节不在
///    ASCII 域"，不是"有大写"。反过来排会把 `Ünils` 说成"有大写所以不命中"，
///    于是有人"加了大小写折叠"之后就能声称它该命中——这正是折叠泄漏的入口。
/// 3. **控制/空白字节** 先于字母计数：含空白的名字进不了查表（lex 闸），
///    先归成"无字母"会让一道词法问题被当成注册问题。
/// 4. **无字母**（`[`、`[[`）：这类名字的大小写轴是**退化**的——
///    折叠对它无效，因此它不能用来证明敏感轴。本卡只准它作对照样本，
///    一旦 `class_blocks` 被算成 true，AC06 就会判死（把退化档混进主锚）。
/// 5. 全大写 / 全小写 / 混合按**字母构成**比对（大写字母数、小写字母数与字母总数
///    三者的关系），不按全长比对。这是一条容易被"顺手写对"的坑：按全长比时
///    `--list` 的两个连字符会让它掉进 `Mixed`，于是这张单子会把一个"字母全小写"
///    的名字说成"大小写混杂"，而 `Mixed` 是本卡的敏感证据档之一——归类一旦说谎，
///    Reply 闸的合取就建在假的输入类上。
pub fn name_class(name: &str) -> NameClass {
    if name.is_empty() {
        return NameClass::Empty;
    }
    if non_ascii_bytes(name) > 0u32 {
        return NameClass::NonAscii;
    }
    if odd_bytes(name) > 0u32 {
        return NameClass::OddByte;
    }
    let al = alpha_count(name);
    if al == 0u32 {
        return NameClass::NoLetter;
    }
    let up = upper_bytes(name);
    let lo = lower_bytes(name);
    // 比的是**字母**构成，不是全长：`--list` 有两个连字符字节，若按全长比，
    // 它会被算成 `Mixed`，于是"连字符伪名"这条对照行永远进不了绿档。
    if up == al && lo == 0u32 {
        return NameClass::AllUpper;
    }
    if lo == al && up == 0u32 {
        return NameClass::AllLower;
    }
    NameClass::Mixed
}

/// 这个大小写类能不能成为"大小写敏感"的证据。
///
/// 只有真的存在大写字节的类才有资格：`AllUpper` 与 `Mixed`。`NoLetter` /
/// `Empty` / `OddByte` / `NonAscii` 折叠前后同形，拿它们当证据等于自证。
pub fn class_proves_sensitive(cls: NameClass) -> bool {
    match cls {
        NameClass::AllUpper => true,
        NameClass::Mixed => true,
        _ => false,
    }
}

/// 该名字折叠后是否**会**命中表——反面构造的判据侧出口。
///
/// 真实查表绝不折叠；本函数只回答"如果谁加了折叠，会发生什么"。它为 true 而
/// 现场答复却是"执行了"，就是折叠已经进了查表路径的确证。
pub fn fold_would_hit(name: &str) -> bool {
    if name.is_empty() || odd_bytes(name) > 0u32 {
        return false;
    }
    if upper_bytes(name) == 0u32 {
        // 本来就没大写，折不折同形，谈不上"命中是折叠造成的"。
        return false;
    }
    match resolve(folded_query(name).as_str()) {
        Resolve::Direct => true,
        Resolve::Hopped(_) => true,
        Resolve::Pseudo => true,
        _ => false,
    }
}

/// 该名字逐字节查表是否命中。
pub fn byte_exact_hit(name: &str) -> bool {
    match resolve(name) {
        Resolve::Direct => true,
        Resolve::Hopped(_) => true,
        Resolve::Pseudo => true,
        _ => false,
    }
}

/// 现场答复文本应不应该出现查询名——本卡的"名字前缀呈现"粗锚。
///
/// 精确的逐字文案归 F11661 / F11674；本卡只问一件事：未名档必须把**原样**的
/// 查询名说进答复。说成小写形 = 折叠泄漏；什么都不说 = 答复不可归因。
pub fn reply_should_carry_name(name: &str) -> bool {
    !name.is_empty() && odd_bytes(name) == 0u32
}

// ---------------------------------------------------------------------------
// 现场事实表：Shape
// ---------------------------------------------------------------------------

/// 一次查询在一份构建上的全部可核事实。
///
/// 字段分四组，分组本身就是判据的骨架：
/// * **身份组**（`name` / `carrier` / `cls` / `harness`）：谁在什么载体上敲了什么，
///   以及这次观测由哪个探针产出。`cls` 是**申报位**——实测类由 `name_class()`
///   现算，两者不符就是 Class 闸的红。留着申报位不是因为信它，而是因为真实故障
///   恰恰长在"归类代码被人改松"上，没有申报位就没有可对判的两份表达。
/// * **表组**（`byte_exact` / `hit` / `hops` / `within_cap` / `ran` / `table_upper` /
///   `table_rows` / `alias_rows` / `table_fp`）：这份构建的表形与这次查询的收敛结果。
///   `byte_exact` 是查表实现的申报位，false 就是"折叠进了查表路径"。
/// * **现场组**（`reply` / `printed` / `exit` / `errno` / `log_bytes`）：答复档、
///   答复里出现的名字、退出码、errno、答复字节数。
/// * **前提/收口组**（`cfg_missing` / `cosign_alias` / `cosign_fp` / `retries` /
///   `asserts` / `note` / `skip` / `in_default` / `sealed`）：这次观测成不成立、
///   走没走满、能不能进默认套件。
#[derive(Clone, Copy)]
pub struct Shape {
    /// 被敲的名字（原样，未经任何折叠）。
    pub name: &'static str,
    /// 载体程序名；必须逐字等于 `busybox`，否则前提缺席。
    pub carrier: &'static str,
    /// 申报的大小写类。
    pub cls: NameClass,
    /// 查表实现是否逐字节（false = 加了折叠）。
    pub byte_exact: bool,
    /// 命中身份档。
    pub hit: Hit,
    /// 别名收敛跳数。
    pub hops: u8,
    /// 链长是否落在 `HOP_CAP` 内。
    pub within_cap: bool,
    /// 实际执行的 `main` 所属规范名；空串表示没执行。
    pub ran: &'static str,
    /// 答复里出现的名字（应与 `name` 逐字节同形）。
    pub printed: &'static str,
    /// 现场答复档。
    pub reply: Reply,
    /// 退出码。
    pub exit: u16,
    /// errno。
    pub errno: u16,
    /// 答复字节数（本卡只看长度，不看逐字文案——文案归 F11661/F11674）。
    pub log_bytes: u32,
    /// 本构建表侧实测的大写字节总数。
    pub table_upper: u32,
    /// 本构建表总行数。
    pub table_rows: u32,
    /// 本构建别名行数。
    pub alias_rows: u32,
    /// 本构建表指纹。
    pub table_fp: u32,
    /// F11593 申报的别名行数（0 = 没联签）。
    pub cosign_alias: u32,
    /// F11593 申报的表指纹（0 = 没联签）。
    pub cosign_fp: u32,
    /// 本构建缺席的 CONFIG 位图。
    pub cfg_missing: u32,
    /// 观测载体。
    pub harness: Harness,
    /// 重试次数（>0 即抖动，必须显式记档）。
    pub retries: u8,
    /// 本条贡献的断言数。
    pub asserts: u16,
    /// 成因注记。
    pub note: Note,
    /// 跳过成因档。
    pub skip: Skip,
    /// 是否进默认必跑套件。
    pub in_default: bool,
    /// 八闸是否全部走满。
    pub sealed: bool,
}

impl Shape {
    /// 零值事实：什么都没有观测到。
    ///
    /// 它是"未观测"而不是"观测通过"——`byte_exact` 取 false、`harness` 取 Noop，
    /// 因此任何拿 `zero()` 当绿行模板的写法都会在 Table/Reply 闸当场判死。
    pub const fn zero() -> Shape {
        Shape {
            name: "",
            carrier: "",
            cls: NameClass::Unknown,
            byte_exact: false,
            hit: Hit::None,
            hops: 0u8,
            within_cap: true,
            ran: "",
            printed: "",
            reply: Reply::None,
            exit: EX_GENERAL,
            errno: ENONE,
            log_bytes: 0u32,
            table_upper: 0u32,
            table_rows: 0u32,
            alias_rows: 0u32,
            table_fp: 0u32,
            cosign_alias: 0u32,
            cosign_fp: 0u32,
            cfg_missing: CFG_NONE,
            harness: Harness::Noop,
            retries: 0u8,
            asserts: 0u16,
            note: Note::None,
            skip: Skip::None,
            in_default: false,
            sealed: false,
        }
    }

    /// 名字字节数。
    pub fn name_len(self: &Shape) -> u32 {
        self.name.len() as u32
    }

    /// 名字里的大写字节数（实测，不看申报）。
    pub fn upper(self: &Shape) -> u32 {
        upper_bytes(self.name)
    }

    /// 本条绿行贡献的可数事实：断言数。
    ///
    /// `define()` 走满八闸时把它写进 `Outcome.value`，于是绿行的"数值"维核的是
    /// 断言数——申报超过载体上界就是虚报，这条在 `seal_ok()` 里判。
    pub fn stat(self: &Shape) -> u32 {
        self.asserts as u32
    }

    // ---- 前提闸（先于八闸） ----

    /// 前提缺席判定：返回"该停在哪道闸 + 成因档"。
    ///
    /// 两种缺席都是"根本没有现场"，因此不进八闸、不算红，只算诚实 skip：
    /// * 载体不是 `busybox`（或空）——那这条观测连查表都没发生。
    /// * 查询名在表里、但它占的 CONFIG 位在本构建缺席——那这条查询命中的是
    ///   **一张不含该行的表**，本卡的别名轴无法对它发言。
    ///
    /// 注意后者只在"名在表内"时成立：不在表内的名字是真未名，该走 Reply 闸判
    /// notfound，不能被 CONFIG 缺席洗成 skip。
    pub fn premise(self: &Shape) -> Option<(Stage, Note)> {
        if self.carrier.is_empty() || self.carrier != PROG {
            return Some((Stage::Lex, Note::AbsentCarrier));
        }
        if self.cfg_missing != CFG_NONE {
            if let Some(row) = find_row(self.name) {
                if row.cfg != 0u32 && (self.cfg_missing & row.cfg) != 0u32 {
                    return Some((Stage::Alias, Note::AbsentCarrier));
                }
            }
        }
        None
    }

    /// 前提缺席时的跳过成因应当与实测缺席一致——`skip` 是申报位。
    pub fn skip_matches_premise(self: &Shape) -> bool {
        match self.premise() {
            Some((Stage::Lex, _)) => self.skip == Skip::NoCarrier || self.skip == Skip::NotRun,
            Some((Stage::Alias, _)) => {
                self.skip == Skip::NoAliasCfg || self.skip == Skip::NoCanon
            }
            Some(_) => false,
            None => !self.skip.is_skip() || self.skip.honest(),
        }
    }

    // ---- 八闸的逐闸事实 ----

    /// Lex：名字有没有资格成为一个查表键。
    ///
    /// 空名、超长名、含 `/`、含空白或控制字节的名一律不合格——它们不构造
    /// "命中/未命中"这个二值问题。这一闸判的是**形态**，与注册无关。
    pub fn lex_ok(self: &Shape) -> bool {
        if self.name.is_empty() {
            return false;
        }
        if self.name.len() > NAME_MAX {
            return false;
        }
        if odd_bytes(self.name) > 0u32 {
            return false;
        }
        for b in self.name.as_bytes() {
            if *b == b'/' {
                return false;
            }
        }
        true
    }

    /// Class：申报的大小写类与字节实测是否同一档。
    ///
    /// 这是本卡唯一核"输入侧"的闸。它故意不判"大小写敏感对不对"——那是 Reply
    /// 闸的事；本闸只保证后面的闸拿到的输入类是真的。`Unknown`（申报位没填）
    /// 一律判死：没归类的名字不允许进入合取判断。
    pub fn class_ok(self: &Shape) -> bool {
        if self.cls == NameClass::Unknown {
            return false;
        }
        self.cls == name_class(self.name)
    }

    /// Table：本构建的表形（全小写 + 行数自洽 + 无重名）。
    ///
    /// 三项合取：`table_upper == 0`、行数与建模表实数相等、表内无重复键。
    /// 分不开写就会漏掉"行数对但混了大写"这类单维破坏——本闸要的是合取，
    /// 与 Class 闸的大写合取是同一条不变量的两面。
    pub fn table_ok(self: &Shape) -> bool {
        if self.table_upper != table_upper_total() {
            return false;
        }
        if self.table_upper != 0u32 {
            return false;
        }
        if self.table_rows != TABLE_ROWS as u32 {
            return false;
        }
        if self.alias_rows != table_rows_of(RowKind::Alias) {
            return false;
        }
        if table_dup_key() {
            return false;
        }
        true
    }

    /// CoSign：与 F11593 说的是不是同一张表。
    ///
    /// 判据原文的"与 F11593 联签"落在这一闸。它要求**两份**都对上：别名行数
    /// 与表指纹。只核行数的话，一张"行数相同但内容不同"的表能蒙过去；只核指纹
    /// 的话，指纹算法一改两边同时归零又会假绿。零值（没联签）单列，因为
    /// "没联签"与"联签对不上"是两种处置：前者判死，后者也判死，但成因不同。
    pub fn cosign_ok(self: &Shape) -> bool {
        if self.cosign_alias == 0u32 || self.cosign_fp == 0u32 {
            return false;
        }
        if self.cosign_alias != self.alias_rows {
            return false;
        }
        if self.cosign_fp != self.table_fp {
            return false;
        }
        if self.table_fp != table_fingerprint() {
            return false;
        }
        true
    }

    /// Alias：命中身份、跳数与表结构三者是否自洽。
    ///
    /// 逐字节查表的实测结果（`resolve()`）是这一闸的对照物：申报的 `hit` 必须
    /// 与它同档，`hops` 必须等于该档约定的跳数，链长必须落在 `HOP_CAP` 内，
    /// 且表侧不得存在悬空/自指/遮蔽。
    ///
    /// 关键点：当 `byte_exact` 为 false（查表加了折叠）时，`resolve()` 仍然逐字节，
    /// 于是"申报说命中、实测说没命中"——本闸会先一步判死。但本卡的构造性缺陷
    /// 现场（槽 6）刻意把 `hit` 申报成与实测一致（都按折叠后的实现申报），
    /// 让红留在 Reply 闸：那才是真机上"折叠命中"的可观测形态。
    pub fn alias_ok(self: &Shape) -> bool {
        let measured = match resolve(self.name) {
            Resolve::Direct => Hit::Direct,
            Resolve::Hopped(1u8) => Hit::OneHop,
            Resolve::Hopped(2u8) => Hit::TwoHop,
            Resolve::Hopped(_) => Hit::Cycled,
            Resolve::Pseudo => Hit::Direct,
            Resolve::Dangling => Hit::Dangling,
            Resolve::TooDeep => Hit::Cycled,
            Resolve::Miss => Hit::Miss,
        };
        if self.hit == Hit::None {
            return false;
        }
        if self.hit != measured {
            return false;
        }
        if self.hops != self.hit.hops() {
            return false;
        }
        if self.hit.is_defective() {
            return false;
        }
        if self.within_cap != (self.hops <= HOP_CAP) {
            return false;
        }
        if table_dangling_alias() || table_self_alias() || table_alias_shadows() {
            return false;
        }
        true
    }

    /// Reply：答复档与输入类/表形的合取——本卡主锚。
    ///
    /// 四条不变量，缺一条即红：
    /// 1. 答复档必须合法（`Ran`/`AliasHit`/`Notfound` 之外一律不接受）。
    /// 2. **正向不变量**：逐字节查表没命中 ⇒ 答复必须是 `Notfound`。
    ///    `LS`/`ZCAT` 全靠这条判绿；折叠实现下它变成"答复是执行"，当场判死。
    /// 3. **反向不变量**：答复是执行 ⇒ 逐字节查表必须命中，且命中的规范名
    ///    必须就是执行的那个 `main`。
    /// 4. 未名档必须把**原样**查询名说进答复（`printed == name`）；
    ///    折成小写就是折叠泄漏的文案面证据，什么都不说是答复不可归因。
    ///
    /// 第 2、3 条是同一个合取的两半，写成两条是故意的：只写第 2 条时，
    /// "把命中判反"的实现（`resolve()` 里把 Miss 说成 Direct）还能靠第 3 条抓住；
    /// 只写第 3 条时反过来。两条都在，两侧伪造都要留痕。
    pub fn reply_ok(self: &Shape) -> bool {
        if !self.reply.is_legit() {
            return false;
        }
        let sensitive = class_proves_sensitive(name_class(self.name));
        let hit = byte_exact_hit(self.name);
        match self.reply {
            Reply::Ran | Reply::AliasHit => {
                if !self.byte_exact {
                    return false;
                }
                if !hit {
                    return false;
                }
                if self.ran.is_empty() {
                    return false;
                }
                // 执行的必须是规范名那一行；伪名（`--list` 这类）只在"执行的就是
                // 这个伪名自身"时接受——伪名没有 main，把伪名说成执行了某个规范名
                // 就是把派发层的事实改写了。
                match role_of_key(self.ran) {
                    Some(RowKind::Canon) => {}
                    Some(RowKind::Pseudo) => {
                        if self.ran != self.name {
                            return false;
                        }
                    }
                    _ => return false,
                }
                if sensitive && self.hit == Hit::Miss {
                    return false;
                }
                // 别名命中必须真的走了别名档，规范名命中必须是零跳。
                match self.hit {
                    Hit::Direct => {
                        if self.hops != 0u8 {
                            return false;
                        }
                    }
                    Hit::OneHop | Hit::TwoHop => {
                        if self.reply != Reply::AliasHit {
                            return false;
                        }
                    }
                    _ => return false,
                }
            }
            Reply::Notfound => {
                if hit {
                    // 表里明明有这个键却答未名：只有折叠/裁剪才可能这样，
                    // 而裁剪面归 F11663，本卡按自身表形判死。
                    if self.cfg_missing == CFG_NONE {
                        return false;
                    }
                }
                if !reply_should_carry_name(self.name) {
                    return false;
                }
                if self.printed != self.name {
                    return false;
                }
                if self.log_bytes == 0u32 {
                    return false;
                }
            }
            _ => return false,
        }
        true
    }

    /// Code：码档与 errno 是否就是该答复档的约定档。
    ///
    /// 本闸**先看答复再看码**：`FoldTrap` 的码（0）完全自洽，只看码会放行——
    /// 这正是闸门次序里 Reply 先于 Code 的理由。
    pub fn code_ok(self: &Shape) -> bool {
        if self.exit != self.reply.exit_code() {
            return false;
        }
        if self.errno != self.reply.errno_of() {
            return false;
        }
        true
    }

    /// Seal：默认必跑位、抖动显式化、断言数与封印四件事。
    ///
    /// 四项合取：
    /// * `in_default` 必须等于载体的默认成员位——需要第二台机器的对照行
    ///   （`TwoMachine`）混进默认集是本卡最常见的一种造假（把"只在一台机器上
    ///   成立"说成"每次必跑"）。
    /// * 重试过就必须记 `FlakeRecovered`；申报 `Clean` 即洗绿，走 `AC_EFLAKE`。
    /// * 断言数必须落在 `(0, harness.assert_cap()]` 内。
    /// * `sealed` 必须为真（申报走满八闸）。
    pub fn seal_ok(self: &Shape) -> bool {
        if self.in_default != self.harness.default_member() {
            return false;
        }
        if self.harness.needs_second() && self.in_default {
            return false;
        }
        if self.retries > 0u8 && self.note != Note::FlakeRecovered {
            return false;
        }
        if self.retries == 0u8 && self.note == Note::FlakeRecovered {
            return false;
        }
        if self.asserts == 0u16 {
            return false;
        }
        if self.asserts > self.harness.assert_cap() {
            return false;
        }
        if !self.sealed {
            return false;
        }
        true
    }

    /// 按闸号取该闸的事实。
    pub fn gate_ok(self: &Shape, g: Stage) -> bool {
        match g {
            Stage::Lex => self.lex_ok(),
            Stage::Class => self.class_ok(),
            Stage::Table => self.table_ok(),
            Stage::CoSign => self.cosign_ok(),
            Stage::Alias => self.alias_ok(),
            Stage::Reply => self.reply_ok(),
            Stage::Code => self.code_ok(),
            Stage::Seal => self.seal_ok(),
        }
    }

    /// 该闸失败时的签名码——八闸各一码是默认，闸内另有两种更准的成因：
    /// 表侧的"重名"与别名侧的"自指/遮蔽"都是重名锚，走 `AC_EUNIQ`；
    /// 一个字没有的答复是数值问题不是大小写问题，走 `AC_EVALUE`；
    /// 抖动被洗绿走 `AC_EFLAKE`。
    pub fn gate_code(self: &Shape, g: Stage) -> u32 {
        match g {
            Stage::Table => {
                if self.table_upper != 0u32 {
                    AC_ELOWER
                } else {
                    AC_EUNIQ
                }
            }
            Stage::Alias => {
                if self.hit == Hit::SelfAlias || self.hit == Hit::Shadowed {
                    AC_EUNIQ
                } else {
                    AC_EALIAS
                }
            }
            Stage::Reply => {
                if self.log_bytes == 0u32 || self.reply == Reply::Silent {
                    AC_EVALUE
                } else {
                    AC_ECASE
                }
            }
            Stage::Seal => {
                if self.retries > 0u8 && self.note != Note::FlakeRecovered {
                    AC_EFLAKE
                } else if self.asserts != 0u16 && self.asserts > self.harness.assert_cap() {
                    AC_EVALUE
                } else {
                    AC_EDEFAULT
                }
            }
            _ => g.fault_code(),
        }
    }

    /// 该闸失败时应当停的成因档。
    ///
    /// 优先采信申报注记（前提是它的归属闸正是本闸），否则从实测事实现推——
    /// 这样 `define()` 永远不会给出一个"停在 A 闸却写着 B 闸成因"的结局，
    /// 而 `Note::home_stage()` 的归属表也因此成为可核约束而不是装饰。
    pub fn gate_note(self: &Shape, g: Stage) -> Note {
        if let Some(h) = self.note.home_stage() {
            if h == g {
                return self.note;
            }
        }
        self.measured_note(g)
    }

    /// 从实测事实推出该闸的成因档。
    pub fn measured_note(self: &Shape, g: Stage) -> Note {
        match g {
            Stage::Lex => Note::BadLex,
            Stage::Class => {
                if self.cls == NameClass::Unknown {
                    Note::None
                } else {
                    Note::ReplySkew
                }
            }
            Stage::Table => {
                if self.table_upper != 0u32 {
                    Note::UpperInTable
                } else {
                    Note::TableDup
                }
            }
            Stage::CoSign => {
                if self.cosign_alias == 0u32 || self.cosign_fp == 0u32 {
                    Note::CosignAbsent
                } else {
                    Note::CosignSkew
                }
            }
            Stage::Alias => match self.hit {
                Hit::Dangling => Note::DanglingTarget,
                Hit::SelfAlias => Note::SelfReferent,
                Hit::Shadowed => Note::ShadowCanon,
                Hit::Cycled => Note::AliasCycle,
                Hit::Miss => Note::ReplySkew,
                _ => Note::HopOverCap,
            },
            Stage::Reply => {
                if self.log_bytes == 0u32 || self.reply == Reply::Silent {
                    Note::SilentReply
                } else if !self.byte_exact && self.upper() > 0u32 {
                    Note::FoldTrap
                } else if self.printed != self.name {
                    Note::FoldTrap
                } else {
                    Note::ReplySkew
                }
            }
            Stage::Code => {
                if self.exit != self.reply.exit_code() {
                    Note::CodeDrift
                } else {
                    Note::ErrnoDrift
                }
            }
            Stage::Seal => {
                if self.retries > 0u8 {
                    Note::FlakeRecovered
                } else if self.asserts != 0u16 && self.asserts > self.harness.assert_cap() {
                    Note::AssertShort
                } else {
                    Note::DepBlocked
                }
            }
        }
    }

    /// 该闸失败时写进 `Outcome.value` 的可数事实。
    ///
    /// 每闸取一个**语义上属于该闸**的数，而不是一律取断言数：数值维要能单独
    /// 坏掉（死槽 11），就得有一张"哪道闸报哪个数"的表可对判。
    pub fn gate_fact(self: &Shape, g: Stage) -> u32 {
        match g {
            Stage::Lex => self.name_len(),
            Stage::Class => self.upper(),
            Stage::Table => self.table_upper,
            Stage::CoSign => self.cosign_alias,
            Stage::Alias => self.hops as u32,
            Stage::Reply => self.log_bytes,
            Stage::Code => self.exit as u32,
            Stage::Seal => self.stat(),
        }
    }

    /// 前提缺席时写进 `Outcome.value` 的数：缺席 CONFIG 的位数。
    pub fn premise_fact(self: &Shape) -> u32 {
        absent_bits(self.cfg_missing)
    }

    /// 本条事实的失败码（走满则为 0）；前提闸与八闸统一在此结算。
    pub fn outcome_code(self: &Shape) -> u32 {
        match self.premise() {
            Some((_, Note::AbsentCarrier)) => AC_ESKIP,
            Some(_) => AC_ESKIP,
            None => {
                let mut i = 0usize;
                while i < GATE_COUNT {
                    if let Some(g) = STAGE_CHAIN.get(i) {
                        if !self.gate_ok(*g) {
                            return self.gate_code(*g);
                        }
                    }
                    i = i.saturating_add(1usize);
                }
                0u32
            }
        }
    }

    /// 逐闸折进指纹（两机构建对照时用；不参与判定）。
    pub fn fold(self: &Shape, h: u64) -> u64 {
        let a = fold_str(h, self.name);
        let b = fold_str(a, self.carrier);
        let c = fold_u32(b, self.cls.rank() as u32);
        let d = fold_u32(c, self.hit.rank() as u32);
        let e = fold_u32(d, self.hops as u32);
        let f = fold_str(e, self.ran);
        let g1 = fold_str(f, self.printed);
        let h2 = fold_u32(g1, self.reply.rank() as u32);
        let i2 = fold_u32(h2, self.exit as u32);
        let j2 = fold_u32(i2, self.errno as u32);
        let k2 = fold_u32(j2, self.log_bytes);
        let l2 = fold_u32(k2, self.table_upper);
        let m2 = fold_u32(l2, self.table_rows);
        let n2 = fold_u32(m2, self.alias_rows);
        let o2 = fold_u32(n2, self.table_fp);
        let p2 = fold_u32(o2, self.cosign_alias);
        let q2 = fold_u32(p2, self.cosign_fp);
        let r2 = fold_u32(q2, self.cfg_missing);
        let s2 = fold_u32(r2, self.harness.rank() as u32);
        let t2 = fold_u32(s2, self.retries as u32);
        let u2 = fold_u32(t2, self.asserts as u32);
        let v2 = fold_u32(u2, self.note.rank() as u32);
        let w2 = fold_u32(v2, self.skip.rank() as u32);
        fold_byte(w2, if self.in_default { 1u8 } else { 2u8 })
    }

    /// 紧凑渲染（诊断用）：`名字/类/命中/答复/码`。
    pub fn render(self: &Shape) -> String {
        let mut s = String::from(self.name);
        s.push('@');
        s.push_str(self.cls.slug());
        s.push('/');
        s.push_str(self.hit.slug());
        s.push('/');
        s.push_str(self.reply.slug());
        s.push('#');
        s.push_str(dec_text(self.exit as u32).as_str());
        s
    }
}

/// 实测结局：`define()` 从一份事实算出来的六维结论。
///
/// 契约要求的六维（`stage`/`code`/`value`/`note`/`default` + 成败位）全部由实测
/// 得出，申报方没有一处能把它自己说成绿。`reply` 是第七个字段，只为让渲染与
/// 诊断不再重复算一次答复档；它不参与对判。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Outcome {
    /// 成败：八闸是否全部走满且前提在场。
    pub ok: bool,
    /// 停在（或走完）哪道闸。
    pub stage: Stage,
    /// 失败码；绿行为 0。
    pub code: u32,
    /// 该闸的可数事实；绿行为断言数。
    pub value: u32,
    /// 成因档。
    pub note: Note,
    /// 默认必跑位（实测侧：走满才可能为真）。
    pub default: bool,
    /// 现场答复档（仅诊断）。
    pub reply: Reply,
}

impl Outcome {
    /// 绿结局：走完八闸、码为 0、无成因。
    pub const fn green(value: u32, default: bool) -> Outcome {
        Outcome {
            ok: true,
            stage: Stage::Seal,
            code: 0u32,
            value,
            note: Note::Clean,
            default,
            reply: Reply::Ran,
        }
    }

    /// 是否绿。
    pub fn success(self: &Outcome) -> bool {
        self.ok
    }

    /// 是否进了绿集（绿档或抖动档）。
    pub fn in_green(self: &Outcome) -> bool {
        Tier::of(self).in_green()
    }

    /// 是否诚实 skip（前提缺席）。
    pub fn is_skip(self: &Outcome) -> bool {
        self.code == AC_ESKIP && self.note == Note::AbsentCarrier
    }

    /// 档位。
    pub fn tier(self: &Outcome) -> Tier {
        Tier::of(self)
    }

    /// 错误码文本名。
    pub fn code_text(self: &Outcome) -> &'static str {
        code_name(self.code)
    }

    /// 结局紧凑渲染。
    pub fn render(self: &Outcome) -> String {
        let mut s = String::from(if self.ok { "ok" } else { "red" });
        s.push(':');
        s.push_str(self.stage.slug());
        s.push('=');
        s.push_str(code_name(self.code));
        s.push('(');
        s.push_str(dec_text(self.value).as_str());
        s.push(')');
        s
    }
}

/// 申报对实测的六个违约维（次序即 `judge_against()` 的判定次序）。
///
/// 契约钉死这个次序：成败 → 闸位 → 码 → 数值 → 成因 → 默认位。理由不是好看，
/// 是"危害半径"从大到小：成败互报会把红说成绿（污染绿集），闸位错只是把红说早了
/// （还承认是红），码错与数值错说的是同一条红的归因细节，成因错是归因，
/// 默认位错只影响下次会不会再跑它。把危害小的排在前面，一条多维坏掉的申报就
/// 只会记一次轻罪——通过率被人为抬高。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fault {
    /// 六维全对齐（或活槽期望的"就该是 Ok"）。
    Ok,
    /// 成败互报。
    Estate,
    /// 停留闸错位。
    EStage,
    /// 错误码漂移。
    ECode,
    /// 数值不符。
    EValue,
    /// 成因归错。
    ENote,
    /// 默认必跑位虚报。
    EDefault,
}

impl Fault {
    /// 维号：`Ok` 占 0 与 6（`is_reject` 为 false），缺陷维 1..6。
    pub const fn ndx(self: &Fault) -> usize {
        match self {
            Fault::Ok => 0usize,
            Fault::Estate => 1usize,
            Fault::EStage => 2usize,
            Fault::ECode => 3usize,
            Fault::EValue => 4usize,
            Fault::ENote => 5usize,
            Fault::EDefault => 6usize,
        }
    }

    /// 是否缺陷维。
    pub fn is_reject(self: &Fault) -> bool {
        self.ndx() >= 1usize && self.ndx() <= REJECT
    }

    /// 短名。
    pub fn slug(self: &Fault) -> &'static str {
        match self {
            Fault::Ok => "aligned",
            Fault::Estate => "estate-swap",
            Fault::EStage => "estage-swap",
            Fault::ECode => "ecode-swap",
            Fault::EValue => "evalue-swap",
            Fault::ENote => "enote-swap",
            Fault::EDefault => "default-swap",
        }
    }

    /// 该维坏掉时报出的错误码（账本缺陷格用它）。
    pub fn code(self: &Fault) -> u32 {
        match self {
            Fault::Ok => 0u32,
            Fault::Estate => AC_ESTATE,
            Fault::EStage => AC_ESTAGE,
            Fault::ECode => AC_ECODE,
            Fault::EValue => AC_EVALUE,
            Fault::ENote => AC_ENOTE,
            Fault::EDefault => AC_EDEFAULT,
        }
    }

    /// 人话一句。
    pub fn phrase(self: &Fault) -> &'static str {
        match self {
            Fault::Ok => "六维全对齐",
            Fault::Estate => "实测与申报的成败相反",
            Fault::EStage => "申报的停留闸与实测不同",
            Fault::ECode => "申报的错误码与实测不同",
            Fault::EValue => "申报的可数事实与实测不同",
            Fault::ENote => "申报的成因档与实测不同",
            Fault::EDefault => "申报的默认必跑位与实测不同",
        }
    }

    /// 按维号回查（账本与注入用）。
    pub fn from_ndx(n: usize) -> Option<Fault> {
        match n {
            0usize => Some(Fault::Ok),
            1usize => Some(Fault::Estate),
            2usize => Some(Fault::EStage),
            3usize => Some(Fault::ECode),
            4usize => Some(Fault::EValue),
            5usize => Some(Fault::ENote),
            6usize => Some(Fault::EDefault),
            _ => None,
        }
    }
}

/// 一槽申报。
///
/// 六个字段与 `Outcome` 的六个对判维一一同名同型——这不是偷懒，是契约要求的
/// "申报必须可与实测逐维对判"。申报没有任何"我认为"的自由度：`success` 说成败、
/// `stage` 说停在哪、`code` 说哪类违约、`value` 说一个可数的数、`note` 说成因、
/// `default` 说进不进必跑集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Claim {
    /// 申报的成败。
    pub success: bool,
    /// 申报的停留闸。
    pub stage: Stage,
    /// 申报的错误码。
    pub code: u32,
    /// 申报的可数事实。
    pub value: u32,
    /// 申报的成因档。
    pub note: Note,
    /// 申报的默认必跑位。
    pub default: bool,
}

impl Claim {
    /// 成败。
    pub fn success(self: &Claim) -> bool {
        self.success
    }

    /// 一条全对齐的申报（绿）。
    pub const fn aligned(out: Outcome) -> Claim {
        Claim {
            success: out.ok,
            stage: out.stage,
            code: out.code,
            value: out.value,
            note: out.note,
            default: out.default,
        }
    }

    /// 按 `Fault` 的维度把申报恰好坏掉一维。
    ///
    /// `run_with()` 的注入路径与六张死槽共用这一个出口：坏法只有一处定义，
    /// 于是"注入真的只坏了一维"是可证的而不是靠注释。每一维的坏法都刻意选在
    /// **仍然是一个合法取值**的那一档，否则对判会在前一维就短路，六维交叉
    /// 校验（`claims_match_their_dims`）会当场判死。
    pub fn break_dim(self: &Claim, f: Fault) -> Claim {
        let mut c = *self;
        match f {
            Fault::Ok => {}
            Fault::Estate => c.success = !c.success,
            Fault::EStage => {
                c.stage = match c.stage.next() {
                    Some(s) => s,
                    None => Stage::Lex,
                };
            }
            Fault::ECode => {
                c.code = match c.code {
                    x if x == AC_ESTATE => AC_ECODE,
                    x if x == AC_ECODE => AC_ESTAGE,
                    0u32 => AC_ECODE,
                    _ => AC_ENOTE,
                };
            }
            Fault::EValue => {
                c.value = c.value.saturating_add(3u32);
            }
            Fault::ENote => {
                c.note = match c.note {
                    Note::Clean => Note::ReplySkew,
                    Note::FlakeRecovered => Note::Clean,
                    Note::FoldTrap => Note::UpperInTable,
                    Note::AbsentCarrier => Note::DepBlocked,
                    _ => Note::Clean,
                };
            }
            Fault::EDefault => c.default = !c.default,
        }
        c
    }

    /// 申报紧凑渲染。
    pub fn render(self: &Claim) -> String {
        let mut s = String::from(if self.success { "green" } else { "red" });
        s.push('@');
        s.push_str(self.stage.slug());
        s.push('=');
        s.push_str(code_name(self.code));
        s.push_str("/v");
        s.push_str(dec_text(self.value).as_str());
        s
    }
}

/// 一槽用例：一份事实 + 一张申报。
#[derive(Clone, Copy)]
pub struct Case {
    /// 实测事实。
    pub shape: Shape,
    /// 申报。
    pub claim: Claim,
}

impl Case {
    /// 现算实测结局。
    pub fn actual(self: &Case) -> Outcome {
        define(&self.shape)
    }

    /// 该槽的判定。
    pub fn fault(self: &Case) -> Fault {
        judge_against(&self.claim, &self.actual())
    }

    /// 身份短名（取自 `WANT_TAGS`，与 `Shape` 分组解耦）。
    pub fn tag(self: &Case) -> &'static str {
        tag_of(self.shape.name)
    }
}

/// 由一份事实算出实测结局——本模块唯一的判定入口。
///
/// 次序是钉死的：先问前提，再按 `STAGE_CHAIN` 逐闸走，第一个不满足的闸定局。
/// "第一个"这一点至关重要：如果让后面的闸也参与（比如取码最大的），一条同时坏两闸
/// 的事实就会报出与成因无关的码，归因链就断了。
pub fn define(sh: &Shape) -> Outcome {
    match sh.premise() {
        Some((stage, note)) => Outcome {
            ok: false,
            stage,
            code: AC_ESKIP,
            value: sh.premise_fact(),
            note,
            default: false,
            reply: sh.reply,
        },
        None => {
            let mut i = 0usize;
            while i < GATE_COUNT {
                let g = match STAGE_CHAIN.get(i) {
                    Some(s) => *s,
                    None => {
                        i = i.saturating_add(1usize);
                        continue;
                    }
                };
                if !sh.gate_ok(g) {
                    return Outcome {
                        ok: false,
                        stage: g,
                        code: sh.gate_code(g),
                        value: sh.gate_fact(g),
                        note: sh.gate_note(g),
                        default: false,
                        reply: sh.reply,
                    };
                }
                i = i.saturating_add(1usize);
            }
            Outcome {
                ok: true,
                stage: Stage::Seal,
                code: 0u32,
                value: sh.stat(),
                note: sh.note,
                default: sh.in_default,
                reply: sh.reply,
            }
        }
    }
}

/// 申报对实测的逐维对判。次序 Estate → Stage → Code → Value → Note → Default。
///
/// 成败互报排最前，因为它是唯一能让"绿集里混进一条红的"成为可能的维：
/// 后面五维错了只是把同一条红说得不够准，这一维错了是把红说成绿。
pub fn judge_against(claim: &Claim, actual: &Outcome) -> Fault {
    if claim.success() != actual.ok {
        return Fault::Estate;
    }
    if claim.stage != actual.stage {
        return Fault::EStage;
    }
    if claim.code != actual.code {
        return Fault::ECode;
    }
    if claim.value != actual.value {
        return Fault::EValue;
    }
    if claim.note != actual.note {
        return Fault::ENote;
    }
    if claim.default != actual.default {
        return Fault::EDefault;
    }
    Fault::Ok
}

/// 槽级对判。
pub fn judge(c: &Case) -> Fault {
    judge_against(&c.claim, &define(&c.shape))
}

/// 六个违约维，次序与 `judge_against()` 一致。
pub const REJECT_FAULTS: [Fault; REJECT] = [
    Fault::Estate,
    Fault::EStage,
    Fault::ECode,
    Fault::EValue,
    Fault::ENote,
    Fault::EDefault,
];

/// 六个互不相同的缺陷码——账本的缺陷格逐个消费它们。
///
/// 之所以取这六个而不是本卡特有的 `AC_ECASE`/`AC_ELOWER`：缺陷格记的是
/// **"申报坏在哪一维"**，不是"现场是什么故障"。现场故障已经在八闸语料里各占一槽，
/// 拿它当缺陷码会让两件事混在同一个数上。
pub const REJECT_CODES: [u32; REJECT] = [
    AC_ESTATE,
    AC_ESTAGE,
    AC_ECODE,
    AC_EVALUE,
    AC_ENOTE,
    AC_EDEFAULT,
];

/// 七个统计格 + 一个红档格（第 8 格必须是 929）。
///
/// 依次是：建模表总行数 28、别名行数 9、本模块判据项数 13、闸数 8、
/// 注记档数 20、载体种类 6、本卡任务板行数下限 320、红档率 929。
///
/// 929 与同批 F11660/F11663 同一来历：14 格里有 1 格对不上时的千分位通过率
/// `(13*1000+7)/14 = 929`。本卡把它放在第 8 格，是因为这张卡最怕的事就是
/// "大写查询不再答未名"——那正是折叠进了查表路径，而它只在被单独一核时现形。
pub const VALS: [u32; ACCEPT] = [
    28u32, 9u32, 13u32, 8u32, 20u32, 6u32, 320u32, RED_RATE,
];

/// 统计格的名字（渲染与诊断用；次序必须与 `VALS` 一致）。
pub const VAL_NAMES: [&'static str; ACCEPT] = [
    "table-rows",
    "alias-rows",
    "check-count",
    "gate-count",
    "note-tiers",
    "harness-kinds",
    "floor-lines",
    "red-rate",
];

/// 按 `name` 取该槽的身份短名。
///
/// 与 `WANT_TAGS` 分两张表写：一张按名字索引、一张按槽位索引，两份各自手写，
/// 交叉核才能抓住"标签被整行挪了一位"这类破坏。
pub fn tag_of(name: &str) -> &'static str {
    match name {
        "ls" => "f11671-lower-exact",
        "LS" => "f11671-upper-notfound",
        "egrep" => "f11671-alias-hop1",
        "EGREP" => "f11671-alias-upper",
        "gzcat" => "f11671-hop2-flake",
        "zcat" => "f11671-cfg-absent",
        "ZCAT" => "f11671-fold-trap",
        "--list" => "f11671-pseudo-list",
        "sh" => "f11671-estate-swap",
        "[" => "f11671-estage-swap",
        "Sed" => "f11671-ecode-swap",
        "awk" => "f11671-evalue-swap",
        "cat" => "f11671-enote-swap",
        "mv" => "f11671-default-swap",
        "[[" => "f11671-no-letter-control",
        "printf" => "f11671-canon-plain",
        _ => "f11671-unbound",
    }
}

// ---------------------------------------------------------------------------
// 14 槽事实表
// ---------------------------------------------------------------------------
//
// 槽位安排：前 8 槽是活槽（申报逐字等于实测，覆盖绿/抖/跳/阻四档与两条轴），
// 后 6 槽每槽只在六个对判维中的某一维上坏一次。
//
// 为什么后六槽的**事实**全取绿行：缺陷样本要核的是"申报坏了一维能不能被抓到"，
// 若事实本身就红，那么红行会同时坏好几维（红行的闸位、码、成因互相关联），
// "只坏一维"这个前提就不成立，六维交叉校验形同虚设。把六张死槽全放在绿事实上，
// 每一维的坏法才是真正独立可隔离的。
//
// 为什么活槽只有 8 个而四档都要有：判据原文要的是"全小写表/LS 未名档判据绿"，
// 一条绿证不够——必须有对照（`egrep` 命中、`EGREP` 未命中）、有边界（`gzcat`
// 两跳）、有诚实 skip（别名 CONFIG 不在）、有真红（`ZCAT` 折叠命中）。
// 缺任何一件，这张单子就只是"恰好没测到坏例"。
pub const SHAPES: [Shape; CORPUS] = [
    // 槽 0：活——小写规范名逐字节直达，本卡的"绿"基线。
    Shape {
        name: "ls",
        carrier: PROG,
        cls: NameClass::AllLower,
        byte_exact: true,
        hit: Hit::Direct,
        hops: 0u8,
        within_cap: true,
        ran: "ls",
        printed: "",
        reply: Reply::Ran,
        exit: EX_OK,
        errno: ENONE,
        log_bytes: 412u32,
        table_upper: 0u32,
        table_rows: TABLE_ROWS as u32,
        alias_rows: ALIAS_ROWS as u32,
        table_fp: 1123919052u32,
        cosign_alias: ALIAS_ROWS as u32,
        cosign_fp: 1123919052u32,
        cfg_missing: CFG_NONE,
        harness: Harness::DirectCall,
        retries: 0u8,
        asserts: 9u16,
        note: Note::Clean,
        skip: Skip::None,
        in_default: true,
        sealed: true,
    },
    // 槽 1：活——**本卡主锚**。大写查询在逐字节查表下必不命中，答复必须是未名档，
    // 且答复里必须原样出现 `LS`。断言数刻意比槽 0 少 1：两条活槽的数值维必须
    // 不同值，否则"数值维"的坏法会与"闸位维"的坏法撞车。
    Shape {
        name: "LS",
        carrier: PROG,
        cls: NameClass::AllUpper,
        byte_exact: true,
        hit: Hit::Miss,
        hops: 0u8,
        within_cap: true,
        ran: "",
        printed: "LS",
        reply: Reply::Notfound,
        exit: EX_NOTFOUND,
        errno: ENOENT,
        log_bytes: 32u32,
        table_upper: 0u32,
        table_rows: TABLE_ROWS as u32,
        alias_rows: ALIAS_ROWS as u32,
        table_fp: 1123919052u32,
        cosign_alias: ALIAS_ROWS as u32,
        cosign_fp: 1123919052u32,
        cfg_missing: CFG_NONE,
        harness: Harness::DirectCall,
        retries: 0u8,
        asserts: 8u16,
        note: Note::Clean,
        skip: Skip::None,
        in_default: true,
        sealed: true,
    },
    // 槽 2：活——别名一跳收敛，执行的是 `grep` 的 main。
    Shape {
        name: "egrep",
        carrier: PROG,
        cls: NameClass::AllLower,
        byte_exact: true,
        hit: Hit::OneHop,
        hops: 1u8,
        within_cap: true,
        ran: "grep",
        printed: "",
        reply: Reply::AliasHit,
        exit: EX_OK,
        errno: ENONE,
        log_bytes: 96u32,
        table_upper: 0u32,
        table_rows: TABLE_ROWS as u32,
        alias_rows: ALIAS_ROWS as u32,
        table_fp: 1123919052u32,
        cosign_alias: ALIAS_ROWS as u32,
        cosign_fp: 1123919052u32,
        cfg_missing: CFG_NONE,
        harness: Harness::AliasProbe,
        retries: 0u8,
        asserts: 9u16,
        note: Note::Clean,
        skip: Skip::None,
        in_default: true,
        sealed: true,
    },
    // 槽 3：活——**别名的轴对照**：`EGREP` 也必须未命中。这一条把"别名轴"与
    // "大小写轴"钉成交叉而不是并列：只测 `LS` 的话，一个"只给规范名折叠、
    // 不给别名折叠"的实现能全绿。载体用扫名族，断言数取该族上界。
    Shape {
        name: "EGREP",
        carrier: PROG,
        cls: NameClass::AllUpper,
        byte_exact: true,
        hit: Hit::Miss,
        hops: 0u8,
        within_cap: true,
        ran: "",
        printed: "EGREP",
        reply: Reply::Notfound,
        exit: EX_NOTFOUND,
        errno: ENOENT,
        log_bytes: 35u32,
        table_upper: 0u32,
        table_rows: TABLE_ROWS as u32,
        alias_rows: ALIAS_ROWS as u32,
        table_fp: 1123919052u32,
        cosign_alias: ALIAS_ROWS as u32,
        cosign_fp: 1123919052u32,
        cfg_missing: CFG_NONE,
        harness: Harness::CaseSweep,
        retries: 0u8,
        asserts: 11u16,
        note: Note::Clean,
        skip: Skip::None,
        in_default: true,
        sealed: true,
    },
    // 槽 4：活——两跳链（`gzcat -> zcat -> gunzip`）踩在 `HOP_CAP` 上沿，
    // 首跑偶发未就绪、重试后恢复。申报**必须**写 `FlakeRecovered`：写成 `Clean`
    // 就是洗绿，`seal_ok()` 走 `AC_EFLAKE` 判死。
    Shape {
        name: "gzcat",
        carrier: PROG,
        cls: NameClass::AllLower,
        byte_exact: true,
        hit: Hit::TwoHop,
        hops: 2u8,
        within_cap: true,
        ran: "gunzip",
        printed: "",
        reply: Reply::AliasHit,
        exit: EX_OK,
        errno: ENONE,
        log_bytes: 128u32,
        table_upper: 0u32,
        table_rows: TABLE_ROWS as u32,
        alias_rows: ALIAS_ROWS as u32,
        table_fp: 1123919052u32,
        cosign_alias: ALIAS_ROWS as u32,
        cosign_fp: 1123919052u32,
        cfg_missing: CFG_NONE,
        harness: Harness::DirectCall,
        retries: 1u8,
        asserts: 8u16,
        note: Note::FlakeRecovered,
        skip: Skip::None,
        in_default: true,
        sealed: true,
    },
    // 槽 5：活——诚实 skip：`zcat` 这行占 `CONFIG_GUNZIP`，而本构建该位缺席，
    // 于是这条查询根本没有"别名收敛"这个现场。前提闸直接停在别名闸，码走
    // `AC_ESKIP`，成因 `AbsentCarrier`，档归 Skipped，不进默认集，零断言。
    Shape {
        name: "zcat",
        carrier: PROG,
        cls: NameClass::AllLower,
        byte_exact: true,
        hit: Hit::OneHop,
        hops: 1u8,
        within_cap: true,
        ran: "",
        printed: "",
        reply: Reply::None,
        exit: EX_GENERAL,
        errno: ENONE,
        log_bytes: 0u32,
        table_upper: 0u32,
        table_rows: TABLE_ROWS as u32,
        alias_rows: ALIAS_ROWS as u32,
        table_fp: 1123919052u32,
        cosign_alias: ALIAS_ROWS as u32,
        cosign_fp: 1123919052u32,
        cfg_missing: CFG_GUNZIP,
        harness: Harness::AliasProbe,
        retries: 0u8,
        asserts: 0u16,
        note: Note::AbsentCarrier,
        skip: Skip::NoAliasCfg,
        in_default: false,
        sealed: false,
    },
    // 槽 6：活——**本卡的构造性缺陷现场**：某处把 `find_applet_by_name` 加了
    // `tolower()`，于是 `busybox ZCAT` 不再报未名，而是真的执行了 `gunzip`。
    // 表形仍全小写（Table 闸过）、别名结构仍自洽（Alias 闸过，命中身份按折叠后的
    // 实现如实申报成 Miss）、码档完全自洽（执行 ⇒ 0），红只会在 Reply 闸现形。
    // 这一槽是整张卡的存在理由：它证明本卡的尺子不是"看码"也不是"看表"。
    Shape {
        name: "ZCAT",
        carrier: PROG,
        cls: NameClass::AllUpper,
        byte_exact: false,
        hit: Hit::Miss,
        hops: 0u8,
        within_cap: true,
        ran: "gunzip",
        printed: "",
        reply: Reply::Ran,
        exit: EX_OK,
        errno: ENONE,
        log_bytes: 14u32,
        table_upper: 0u32,
        table_rows: TABLE_ROWS as u32,
        alias_rows: ALIAS_ROWS as u32,
        table_fp: 1123919052u32,
        cosign_alias: ALIAS_ROWS as u32,
        cosign_fp: 1123919052u32,
        cfg_missing: CFG_NONE,
        harness: Harness::CaseSweep,
        retries: 0u8,
        asserts: 10u16,
        note: Note::FoldTrap,
        skip: Skip::None,
        in_default: false,
        sealed: false,
    },
    // 槽 7：活——伪名 `--list`。它是表里的第四个种类，字母部分全小写、前缀是连字符，
    // 因此大小写轴对它只有一条可走的路（折叠前后同形），只能当"表里确实有伪名行、
    // 查表仍逐字节"的对照。
    // 伪名没有 `main`，所以 `ran` 必须就是伪名自身，否则就是派发层改写了事实。
    Shape {
        name: "--list",
        carrier: PROG,
        cls: NameClass::AllLower,
        byte_exact: true,
        hit: Hit::Direct,
        hops: 0u8,
        within_cap: true,
        ran: "--list",
        printed: "",
        reply: Reply::Ran,
        exit: EX_OK,
        errno: ENONE,
        log_bytes: 728u32,
        table_upper: 0u32,
        table_rows: TABLE_ROWS as u32,
        alias_rows: ALIAS_ROWS as u32,
        table_fp: 1123919052u32,
        cosign_alias: ALIAS_ROWS as u32,
        cosign_fp: 1123919052u32,
        cfg_missing: CFG_NONE,
        harness: Harness::ListProbe,
        retries: 0u8,
        asserts: 7u16,
        note: Note::Clean,
        skip: Skip::None,
        in_default: true,
        sealed: true,
    },
    // 槽 8：死——成败互报。事实是 `sh -> ash` 一跳命中、八闸走满的绿行，申报却写红。
    // 这一维是唯一能把红说成绿（或把绿说成红）的维，故排第一。
    Shape {
        name: "sh",
        carrier: PROG,
        cls: NameClass::AllLower,
        byte_exact: true,
        hit: Hit::OneHop,
        hops: 1u8,
        within_cap: true,
        ran: "ash",
        printed: "",
        reply: Reply::AliasHit,
        exit: EX_OK,
        errno: ENONE,
        log_bytes: 64u32,
        table_upper: 0u32,
        table_rows: TABLE_ROWS as u32,
        alias_rows: ALIAS_ROWS as u32,
        table_fp: 1123919052u32,
        cosign_alias: ALIAS_ROWS as u32,
        cosign_fp: 1123919052u32,
        cfg_missing: CFG_NONE,
        harness: Harness::AliasProbe,
        retries: 0u8,
        asserts: 9u16,
        note: Note::Clean,
        skip: Skip::None,
        in_default: true,
        sealed: true,
    },
    // 槽 9：死——停留闸错位。事实是 `[ -> test`（无字母类）走满八闸，申报停在首闸。
    // 封闸没有"下一道"，所以这一维的坏法只取回退。
    Shape {
        name: "[",
        carrier: PROG,
        cls: NameClass::NoLetter,
        byte_exact: true,
        hit: Hit::OneHop,
        hops: 1u8,
        within_cap: true,
        ran: "test",
        printed: "",
        reply: Reply::AliasHit,
        exit: EX_OK,
        errno: ENONE,
        log_bytes: 3u32,
        table_upper: 0u32,
        table_rows: TABLE_ROWS as u32,
        alias_rows: ALIAS_ROWS as u32,
        table_fp: 1123919052u32,
        cosign_alias: ALIAS_ROWS as u32,
        cosign_fp: 1123919052u32,
        cfg_missing: CFG_NONE,
        harness: Harness::DirectCall,
        retries: 0u8,
        asserts: 7u16,
        note: Note::Clean,
        skip: Skip::None,
        in_default: true,
        sealed: true,
    },
    // 槽 10：死——错误码漂移。事实是混合大小写名 `Sed` 的未名档绿行（码为 0），
    // 申报给绿行挂了一个码。选 `Sed` 而不是小写名，是为了让"混合类"这一档
    // 在语料里也有一次真实呈现——它同样必不命中，但归类不是全大写。
    Shape {
        name: "Sed",
        carrier: PROG,
        cls: NameClass::Mixed,
        byte_exact: true,
        hit: Hit::Miss,
        hops: 0u8,
        within_cap: true,
        ran: "",
        printed: "Sed",
        reply: Reply::Notfound,
        exit: EX_NOTFOUND,
        errno: ENOENT,
        log_bytes: 33u32,
        table_upper: 0u32,
        table_rows: TABLE_ROWS as u32,
        alias_rows: ALIAS_ROWS as u32,
        table_fp: 1123919052u32,
        cosign_alias: ALIAS_ROWS as u32,
        cosign_fp: 1123919052u32,
        cfg_missing: CFG_NONE,
        harness: Harness::CaseSweep,
        retries: 0u8,
        asserts: 11u16,
        note: Note::Clean,
        skip: Skip::None,
        in_default: true,
        sealed: true,
    },
    // 槽 11：死——数值虚报。事实是 `awk` 绿行、6 条断言，申报加 3。
    Shape {
        name: "awk",
        carrier: PROG,
        cls: NameClass::AllLower,
        byte_exact: true,
        hit: Hit::Direct,
        hops: 0u8,
        within_cap: true,
        ran: "awk",
        printed: "",
        reply: Reply::Ran,
        exit: EX_OK,
        errno: ENONE,
        log_bytes: 51u32,
        table_upper: 0u32,
        table_rows: TABLE_ROWS as u32,
        alias_rows: ALIAS_ROWS as u32,
        table_fp: 1123919052u32,
        cosign_alias: ALIAS_ROWS as u32,
        cosign_fp: 1123919052u32,
        cfg_missing: CFG_NONE,
        harness: Harness::DirectCall,
        retries: 0u8,
        asserts: 6u16,
        note: Note::Clean,
        skip: Skip::None,
        in_default: true,
        sealed: true,
    },
    // 槽 12：死——成因归错。事实是 `cat` 绿行（`Clean`），申报成"答复档与输入类
    // 矛盾"。绿行挂红成因也是假绿：它会让下一轮把这条当成已知红而豁免。
    Shape {
        name: "cat",
        carrier: PROG,
        cls: NameClass::AllLower,
        byte_exact: true,
        hit: Hit::Direct,
        hops: 0u8,
        within_cap: true,
        ran: "cat",
        printed: "",
        reply: Reply::Ran,
        exit: EX_OK,
        errno: ENONE,
        log_bytes: 2048u32,
        table_upper: 0u32,
        table_rows: TABLE_ROWS as u32,
        alias_rows: ALIAS_ROWS as u32,
        table_fp: 1123919052u32,
        cosign_alias: ALIAS_ROWS as u32,
        cosign_fp: 1123919052u32,
        cfg_missing: CFG_NONE,
        harness: Harness::DirectCall,
        retries: 0u8,
        asserts: 5u16,
        note: Note::Clean,
        skip: Skip::None,
        in_default: true,
        sealed: true,
    },
    // 槽 13：死——默认必跑位虚报。事实是 `mv` 的两机对照行：它需要第二台机器，
    // 因此**不该**进默认必跑集，申报却把它塞了进去。
    // 这一维只有在这一类行上才有意义：全 14 槽里只有一行是 `TwoMachine`，
    // 所以"默认位"这一维的坏法在这里是**唯一可隔离**的，换任何别的槽都会连带
    // 改动载体的断言上界，变成两维同坏。
    Shape {
        name: "mv",
        carrier: PROG,
        cls: NameClass::AllLower,
        byte_exact: true,
        hit: Hit::Direct,
        hops: 0u8,
        within_cap: true,
        ran: "mv",
        printed: "",
        reply: Reply::Ran,
        exit: EX_OK,
        errno: ENONE,
        log_bytes: 42u32,
        table_upper: 0u32,
        table_rows: TABLE_ROWS as u32,
        alias_rows: ALIAS_ROWS as u32,
        table_fp: 1123919052u32,
        cosign_alias: ALIAS_ROWS as u32,
        cosign_fp: 1123919052u32,
        cfg_missing: CFG_NONE,
        harness: Harness::TwoMachine,
        retries: 0u8,
        asserts: 12u16,
        note: Note::Clean,
        skip: Skip::None,
        in_default: false,
        sealed: true,
    },
];

/// 14 槽申报表。
///
/// 活槽的申报逐字抄自它那一槽的实测结局；死槽的申报在六维之一上故意错一次。
/// 每张死槽的错法都写在注释里，交叉校验见 `claims_match_their_dims`——申报必须
/// 只坏在它声称的那一维，坏两维的缺陷样本会让通过率统计少算一条红。
pub const CLAIMS: [Claim; CORPUS] = [
    // 槽 0：活——逐字直达，封印闸，9 条断言走满，进默认集。
    Claim {
        success: true,
        stage: Stage::Seal,
        code: 0u32,
        value: 9u32,
        note: Note::Clean,
        default: true,
    },
    // 槽 1：活——主锚行：大写查询未名档，8 条断言，码 0，进默认集。
    Claim {
        success: true,
        stage: Stage::Seal,
        code: 0u32,
        value: 8u32,
        note: Note::Clean,
        default: true,
    },
    // 槽 2：活——别名一跳。
    Claim {
        success: true,
        stage: Stage::Seal,
        code: 0u32,
        value: 9u32,
        note: Note::Clean,
        default: true,
    },
    // 槽 3：活——别名的大写对照，未名档。
    Claim {
        success: true,
        stage: Stage::Seal,
        code: 0u32,
        value: 11u32,
        note: Note::Clean,
        default: true,
    },
    // 槽 4：活——两跳抖动后恢复：申报必须写 `FlakeRecovered`，写 `Clean` 就是洗绿。
    Claim {
        success: true,
        stage: Stage::Seal,
        code: 0u32,
        value: 8u32,
        note: Note::FlakeRecovered,
        default: true,
    },
    // 槽 5：活——别名 CONFIG 缺席，申报是诚实的 skip：红在别名闸、码走 ESKIP、
    // 数值是缺席位数 1、不进默认集。
    Claim {
        success: false,
        stage: Stage::Alias,
        code: AC_ESKIP,
        value: 1u32,
        note: Note::AbsentCarrier,
        default: false,
    },
    // 槽 6：活——折叠命中：诚实申报红，停在答复闸，码 ECASE，数值是答复字节数，
    // 成因 FoldTrap，不进默认集。这一条是"缺陷现场被如实申报"的样子。
    Claim {
        success: false,
        stage: Stage::Reply,
        code: AC_ECASE,
        value: 14u32,
        note: Note::FoldTrap,
        default: false,
    },
    // 槽 7：活——伪名行，不进默认集的是它自己的选择：ListProbe 是默认成员，
    // 所以默认位为真。
    Claim {
        success: true,
        stage: Stage::Seal,
        code: 0u32,
        value: 7u32,
        note: Note::Clean,
        default: true,
    },
    // 槽 8：死——成败互报。实测绿，申报红，其余五维逐字照抄实测。
    Claim {
        success: false,
        stage: Stage::Seal,
        code: 0u32,
        value: 9u32,
        note: Note::Clean,
        default: true,
    },
    // 槽 9：死——停留闸前移：走完八闸却申报停在词法闸。
    Claim {
        success: true,
        stage: Stage::Lex,
        code: 0u32,
        value: 7u32,
        note: Note::Clean,
        default: true,
    },
    // 槽 10：死——错误码漂移：绿行却挂了一个码档错。
    Claim {
        success: true,
        stage: Stage::Seal,
        code: AC_ECODE,
        value: 11u32,
        note: Note::Clean,
        default: true,
    },
    // 槽 11：死——数值虚报：断言数 6 报成 9。
    Claim {
        success: true,
        stage: Stage::Seal,
        code: 0u32,
        value: 9u32,
        note: Note::Clean,
        default: true,
    },
    // 槽 12：死——成因换档：走满且全对，却报成"答复档与输入类矛盾"。
    Claim {
        success: true,
        stage: Stage::Seal,
        code: 0u32,
        value: 5u32,
        note: Note::ReplySkew,
        default: true,
    },
    // 槽 13：死——默认位虚报：需要第二台机器的两机对照行被塞进默认必跑集。
    Claim {
        success: true,
        stage: Stage::Seal,
        code: 0u32,
        value: 12u32,
        note: Note::Clean,
        default: true,
    },
];

/// 每槽期望申报的停留闸（第二份表达；与 `CLAIMS` 各写一遍，交叉核）。
pub const WANT_STAGE: [Stage; CORPUS] = [
    Stage::Seal,
    Stage::Seal,
    Stage::Seal,
    Stage::Seal,
    Stage::Seal,
    Stage::Alias,
    Stage::Reply,
    Stage::Seal,
    Stage::Seal,
    Stage::Lex,
    Stage::Seal,
    Stage::Seal,
    Stage::Seal,
    Stage::Seal,
];

/// 每槽期望申报的码（第二份表达）。
pub const WANT_CODE: [u32; CORPUS] = [
    0u32,
    0u32,
    0u32,
    0u32,
    0u32,
    AC_ESKIP,
    AC_ECASE,
    0u32,
    0u32,
    0u32,
    AC_ECODE,
    0u32,
    0u32,
    0u32,
];

/// 每槽期望的缺陷维；`REJECT`（=6）表示该槽是活槽，期望 `Fault::Ok`。
pub const WANT_NDX: [usize; CORPUS] = [
    REJECT, REJECT, REJECT, REJECT, REJECT, REJECT, REJECT, REJECT, 0usize, 1usize, 2usize,
    3usize, 4usize, 5usize,
];

/// 每槽期望的成因档（第二份表达）。
pub const WANT_NOTE: [Note; CORPUS] = [
    Note::Clean,
    Note::Clean,
    Note::Clean,
    Note::Clean,
    Note::FlakeRecovered,
    Note::AbsentCarrier,
    Note::FoldTrap,
    Note::Clean,
    Note::Clean,
    Note::Clean,
    Note::Clean,
    Note::Clean,
    Note::ReplySkew,
    Note::Clean,
];

/// 每槽期望的默认必跑位（第二份表达；与 `CLAIMS` 的 `default` 逐字对齐）。
///
/// 槽 13 这里写 `true` 是**如实抄申报**：那一槽的申报把需要第二台机器的两机
/// 对照行塞进了默认必跑集，实测侧是 `false`。期望表复写的是申报，不是实测——
/// 两者的差正是那一槽要暴露的缺陷维。
pub const WANT_DEFAULT: [bool; CORPUS] = [
    true, true, true, true, true, false, false, true, true, true, true, true, true, true,
];

/// 槽标签：每槽写它实际占用的那条判据短名。
///
/// 死槽同样挂真判据的短名——缺陷形态说的是"这条真判据被报坏了"，不是又一条判据。
/// 缺陷形态名由 `Fault::slug()` 给，本表只管身份，两张表各管一面才能互查不互包。
pub const WANT_TAGS: [&'static str; CORPUS] = [
    "f11671-lower-exact",
    "f11671-upper-notfound",
    "f11671-alias-hop1",
    "f11671-alias-upper",
    "f11671-hop2-flake",
    "f11671-cfg-absent",
    "f11671-fold-trap",
    "f11671-pseudo-list",
    "f11671-estate-swap",
    "f11671-estage-swap",
    "f11671-ecode-swap",
    "f11671-evalue-swap",
    "f11671-enote-swap",
    "f11671-default-swap",
];

/// 每槽的中文名（渲染与交付回报表用）。
pub const SLOT_NAMES: [&'static str; CORPUS] = [
    "小写规范名逐字节直达",
    "大写查询未名档（主锚）",
    "别名一跳收敛",
    "别名的大写形态未名档",
    "两跳链抖动后恢复",
    "别名 CONFIG 缺席（诚实 skip）",
    "折叠命中缺陷现场（真红）",
    "伪名 --list 对照",
    "成败互报死槽",
    "停留闸错位死槽",
    "错误码漂移死槽",
    "数值虚报死槽",
    "成因换档死槽",
    "默认位虚报死槽",
];

/// 活槽条数（前 8 槽）。
pub const LIVE_SLOTS: usize = ACCEPT;

/// 四档期望条数：活槽 5 绿 + 死槽 6 绿（死槽的事实全为绿行）。
pub const WANT_GREEN: u32 = 11u32;
/// 抖动档条数：活槽 4（重试后恢复的绿）。
pub const WANT_FLAKED: u32 = 1u32;
/// 诚实 skip 档条数：活槽 5（别名 CONFIG 不在，根本没跑）。
pub const WANT_SKIPPED: u32 = 1u32;
/// 阻塞档条数：活槽 6（折叠命中）。
pub const WANT_BLOCKED: u32 = 1u32;
/// 全对齐条数（判据原文要求 100%）。
pub const WANT_MATCHED: u32 = CORPUS as u32;
/// 走满全程条数——只有绿档与抖动档能 sealed（活槽 7 + 死槽 5，槽 5/6 提前定局）。
pub const WANT_SEALED: u32 = 12u32;
/// 计入绿集条数（green + flaked）。
pub const WANT_INGREEN: u32 = 12u32;
/// 未计入绿集条数（skipped + blocked）。
pub const WANT_HALTED: u32 = 2u32;
/// 本卡语料断言总数（14 槽逐项相加，由 `bench_assert_total()` 现算并与常量核）。
pub const WANT_ASSERTS: u32 = 112u32;

/// 取一槽事实。
pub fn shape_at(i: usize) -> Option<Shape> {
    match SHAPES.get(i) {
        Some(s) => Some(*s),
        None => None,
    }
}

/// 取一槽申报。
pub fn claim_at(i: usize) -> Option<Claim> {
    match CLAIMS.get(i) {
        Some(c) => Some(*c),
        None => None,
    }
}

/// 取一槽实测结局。
pub fn outcome_at(i: usize) -> Option<Outcome> {
    match SHAPES.get(i) {
        Some(s) => Some(define(s)),
        None => None,
    }
}

/// 取一槽用例。
pub fn case_at(i: usize) -> Option<Case> {
    let shape = match SHAPES.get(i) {
        Some(s) => *s,
        None => return None,
    };
    let claim = match CLAIMS.get(i) {
        Some(c) => *c,
        None => return None,
    };
    Some(Case { shape, claim })
}

/// 全部用例。
pub fn cases() -> [Case; CORPUS] {
    let mut out: [Case; CORPUS] = [Case {
        shape: Shape::zero(),
        claim: Claim {
            success: false,
            stage: Stage::Lex,
            code: AC_ESTATE,
            value: 0u32,
            note: Note::None,
            default: false,
        },
    }; CORPUS];
    let mut i = 0usize;
    while i < CORPUS {
        if let Some(c) = case_at(i) {
            if let Some(slot) = out.get_mut(i) {
                *slot = c;
            }
        }
        i = i.saturating_add(1usize);
    }
    out
}

/// 一槽期望的判定结果：活槽期望 `Ok`，死槽期望指定的那一维缺陷。
pub fn expected_fault(i: usize) -> Fault {
    let ndx = match WANT_NDX.get(i) {
        Some(n) => *n,
        None => return Fault::Estate,
    };
    if ndx >= REJECT {
        return Fault::Ok;
    }
    match REJECT_FAULTS.get(ndx) {
        Some(f) => *f,
        None => Fault::Ok,
    }
}

/// 一槽的身份短名（按槽位索引的第二份表）。
pub fn want_tag(i: usize) -> &'static str {
    match WANT_TAGS.get(i) {
        Some(t) => t,
        None => "f11671-unbound",
    }
}

/// 一槽的中文名。
pub fn slot_name(i: usize) -> &'static str {
    match SLOT_NAMES.get(i) {
        Some(t) => t,
        None => "未命名槽",
    }
}

/// 语料断言总数（逐项相加，不写死）。
pub fn bench_assert_total() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < CORPUS {
        if let Some(s) = SHAPES.get(i) {
            n = n.saturating_add(s.asserts as u32);
        }
        i = i.saturating_add(1usize);
    }
    n
}

/// 该槽是否为活槽。
pub fn slot_is_live(i: usize) -> bool {
    match WANT_NDX.get(i) {
        Some(n) => *n >= REJECT,
        None => false,
    }
}

// ---------------------------------------------------------------------------
// 横计矩阵与报告
// ---------------------------------------------------------------------------

/// 账本登记用的操作码：每一格说清"这个数是数出来的什么"。
///
/// 九个码互不相同，`audit` 之外还被 `seal` 折进指纹——若两格共用一个码，
/// 把两格内容互换就查不出来，重放也就不再是重放。
pub const OP_TABLE: u8 = 0x01u8;
/// 别名字数。
pub const OP_ALIAS: u8 = 0x02u8;
/// 判据项数。
pub const OP_CHECK: u8 = 0x03u8;
/// 闸数。
pub const OP_GATE: u8 = 0x04u8;
/// 成因档数。
pub const OP_NOTE: u8 = 0x05u8;
/// 载体种类数。
pub const OP_HARNESS: u8 = 0x06u8;
/// 卡面行数地板。
pub const OP_FLOOR: u8 = 0x07u8;
/// 红档率。
pub const OP_RATE: u8 = 0x08u8;
/// 缺陷码格。
pub const OP_DEFECT: u8 = 0x09u8;

/// 操作码表（次序与账本前 8 格的登记次序一致，第 9 个是缺陷格共用码）。
pub const OPS: [u8; 9usize] = [
    OP_TABLE, OP_ALIAS, OP_CHECK, OP_GATE, OP_NOTE, OP_HARNESS, OP_FLOOR, OP_RATE, OP_DEFECT,
];

/// 操作码 → 名称。
pub fn op_name(op: u8) -> &'static str {
    match op {
        OP_TABLE => "table-rows",
        OP_ALIAS => "alias-rows",
        OP_CHECK => "check-count",
        OP_GATE => "gate-count",
        OP_NOTE => "note-tiers",
        OP_HARNESS => "harness-kinds",
        OP_FLOOR => "floor-lines",
        OP_RATE => "red-rate",
        OP_DEFECT => "defect-code",
        _ => "unknown-op",
    }
}

/// 本卡卡面行数地板（验收条款 ① 的纯功能代码行数）。
pub const CARD_LINES: u32 = 320u32;

/// 四档的遍历序（`Matrix` 计数与 `Report` 渲染共用）。
pub const TIERS: [Tier; TIER_COUNT] = [Tier::Green, Tier::Flaked, Tier::Skipped, Tier::Blocked];

/// 一槽的横计行：实测、申报、判定、期望、档位五件事钉在一起。
///
/// `want` 与 `matched` 是**分开**两样东西：`fault` 说"这槽被判成什么"，
/// `want` 说"这槽本该被判成什么"。活槽的 want 是 `Ok`、死槽的 want 是它自己那一维；
/// 于是"缺陷样本没被抓到"与"活样本被误判"在同一张表里都能显形。
#[derive(Clone, Copy)]
pub struct Row {
    /// 槽号。
    pub ndx: usize,
    /// 身份短名。
    pub tag: &'static str,
    /// 中文名。
    pub title: &'static str,
    /// 实测结局。
    pub actual: Outcome,
    /// 申报。
    pub claim: Claim,
    /// 判定结论。
    pub fault: Fault,
    /// 期望结论。
    pub want: Fault,
    /// 档位。
    pub tier: Tier,
    /// 判定与期望是否一致。
    pub matched: bool,
}

impl Row {
    /// 行指纹：判定与期望都进折叠，"改了期望"也会换根。
    pub fn fold(self: &Row, h: u64) -> u64 {
        let a = fold_str(h, self.tag);
        let b = fold_u32(a, self.ndx as u32);
        let c = fold_u32(b, self.fault.ndx() as u32);
        let d = fold_u32(c, self.want.ndx() as u32);
        let e = fold_u32(d, self.tier.rank() as u32);
        let f = fold_u32(e, self.actual.code);
        let g = fold_u32(f, self.actual.value);
        let i = fold_u32(g, self.claim.code);
        let j = fold_u32(i, self.claim.value);
        let k = fold_u32(j, self.actual.note.rank() as u32);
        let l = fold_u32(k, self.claim.note.rank() as u32);
        let m = fold_byte(l, if self.matched { 1u8 } else { 0u8 });
        fold_byte(m, self.actual.stage.rank())
    }

    /// 一行紧凑渲染（诊断）。
    pub fn render(self: &Row) -> String {
        let mut s = String::from(self.tag);
        s.push(' ');
        s.push_str(dec_text(self.ndx as u32).as_str());
        s.push(' ');
        s.push_str(self.tier.slug());
        s.push('/');
        s.push_str(self.actual.stage.slug());
        s.push('/');
        s.push_str(code_name(self.actual.code));
        s.push_str("/v");
        s.push_str(dec_text(self.actual.value).as_str());
        s.push(' ');
        s.push_str(self.fault.slug());
        s.push_str(bool_text(self.matched));
        s
    }
}

/// 横计矩阵：按 `Tier::of(&actual)` 分四档，`total` 与 `good` 两维独立记账。
///
/// "两维独立"是本卡的硬要求：`total` 说这条纵线跑了几格，`good` 说其中几格
/// 判定与期望一致。如果只记一个数，"跑了 14 格全对齐"与"只跑了 14 格但对齐与否
/// 没人看"就长得一样——而后者正是判据模块最容易烂掉的方式。
#[derive(Clone, Copy)]
pub struct Matrix {
    rows: [Option<Row>; CORPUS],
    n: usize,
    green: u32,
    flaked: u32,
    skipped: u32,
    blocked: u32,
    good: u32,
    sealed: u32,
}

impl Matrix {
    /// 空矩阵。
    pub const fn new() -> Matrix {
        Matrix {
            rows: [None; CORPUS],
            n: 0usize,
            green: 0u32,
            flaked: 0u32,
            skipped: 0u32,
            blocked: 0u32,
            good: 0u32,
            sealed: 0u32,
        }
    }

    /// 落一行；矩阵满则丢弃并返回 false（不静默越界写）。
    pub fn push(&mut self, r: Row) -> bool {
        if self.n >= CORPUS {
            return false;
        }
        let hit = match self.rows.get_mut(self.n) {
            Some(slot) => {
                *slot = Some(r);
                true
            }
            None => false,
        };
        if !hit {
            return false;
        }
        self.n = self.n.saturating_add(1usize);
        match r.tier {
            Tier::Green => self.green = self.green.saturating_add(1u32),
            Tier::Flaked => self.flaked = self.flaked.saturating_add(1u32),
            Tier::Skipped => self.skipped = self.skipped.saturating_add(1u32),
            Tier::Blocked => self.blocked = self.blocked.saturating_add(1u32),
        }
        if r.matched {
            self.good = self.good.saturating_add(1u32);
        }
        if r.actual.stage == Stage::Seal {
            self.sealed = self.sealed.saturating_add(1u32);
        }
        true
    }

    /// 已落行数。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.n == 0usize
    }

    /// 取一行。
    pub fn row(&self, i: usize) -> Option<Row> {
        if i >= self.n {
            return None;
        }
        match self.rows.get(i) {
            Some(x) => *x,
            None => None,
        }
    }

    /// 按档计数。
    pub fn count_of(&self, t: Tier) -> u32 {
        match t {
            Tier::Green => self.green,
            Tier::Flaked => self.flaked,
            Tier::Skipped => self.skipped,
            Tier::Blocked => self.blocked,
        }
    }

    /// 两维之一：总行数。
    pub fn total(&self) -> u32 {
        self.n as u32
    }

    /// 两维之二：对齐行数。
    pub fn good(&self) -> u32 {
        self.good
    }

    /// 走满八闸的行数。
    pub fn sealed(&self) -> u32 {
        self.sealed
    }

    /// 满对行：判定为 `Ok` 且期望也是 `Ok`（活槽全对）。
    pub fn perfect_rows(&self) -> u32 {
        let mut k = 0u32;
        let mut i = 0usize;
        while i < self.n {
            if let Some(r) = self.row(i) {
                if r.fault == Fault::Ok && r.want == Fault::Ok {
                    k = k.saturating_add(1u32);
                }
            }
            i = i.saturating_add(1usize);
        }
        k
    }

    /// 六条缺陷维是否都在语料里出现过一次（一次都不缺，也不许多）。
    pub fn faults_all_covered(&self) -> bool {
        let mut k = 0u32;
        let mut j = 0usize;
        while j < REJECT {
            let mut found = false;
            let mut i = 0usize;
            while i < self.n {
                if let Some(r) = self.row(i) {
                    if let Some(w) = REJECT_FAULTS.get(j) {
                        if r.fault == *w && r.want == *w {
                            found = true;
                        }
                    }
                }
                i = i.saturating_add(1usize);
            }
            if found {
                k = k.saturating_add(1u32);
            }
            j = j.saturating_add(1usize);
        }
        k == REJECT as u32
    }

    /// 覆盖性：四档都有行、六条缺陷维都有行、活/死两族都有行。
    pub fn covered(&self) -> bool {
        if self.n != CORPUS {
            return false;
        }
        let mut i = 0usize;
        while i < TIER_COUNT {
            let Some(t) = TIERS.get(i) else {
                return false;
            };
            if self.count_of(*t) == 0u32 {
                return false;
            }
            i = i.saturating_add(1usize);
        }
        self.faults_all_covered()
    }

    /// 绿集里有没有混进"实测红但申报绿"的行——洗绿的显性检查。
    pub fn green_has_lie(&self) -> bool {
        let mut i = 0usize;
        while i < self.n {
            if let Some(r) = self.row(i) {
                if r.tier.in_green() && !r.actual.ok && r.claim.success() {
                    return true;
                }
            }
            i = i.saturating_add(1usize);
        }
        false
    }

    /// 逐档计数折进指纹。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.n {
            if let Some(r) = self.row(i) {
                h = r.fold(h);
            }
            i = i.saturating_add(1usize);
        }
        h = fold_word(h, self.total() as u64);
        h = fold_word(h, self.good() as u64);
        fold_word(h, self.sealed() as u64)
    }

    /// 整表渲染（四档一行 + 每槽一行）。
    pub fn render(&self) -> String {
        let mut s = String::from("tier green=");
        s.push_str(dec_text(self.green).as_str());
        s.push_str(" flaked=");
        s.push_str(dec_text(self.flaked).as_str());
        s.push_str(" skipped=");
        s.push_str(dec_text(self.skipped).as_str());
        s.push_str(" blocked=");
        s.push_str(dec_text(self.blocked).as_str());
        s.push_str(" good=");
        s.push_str(dec_text(self.good).as_str());
        s.push('/');
        s.push_str(dec_text(self.total()).as_str());
        let mut i = 0usize;
        while i < self.n {
            if let Some(r) = self.row(i) {
                s.push_str(" | ");
                s.push_str(r.render().as_str());
            }
            i = i.saturating_add(1usize);
        }
        s
    }
}

/// 聚合报告：总数、活/死槽数、四档计数、对齐数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Report {
    /// 跑了多少槽。
    pub total: u32,
    /// 活槽数。
    pub accept: u32,
    /// 死槽数。
    pub reject: u32,
    /// 绿档。
    pub green: u32,
    /// 抖档。
    pub flaked: u32,
    /// 跳档。
    pub skipped: u32,
    /// 阻档。
    pub blocked: u32,
    /// 判定与期望一致的行数。
    pub matched: u32,
}

impl Report {
    /// 千分位通过率：`(matched*1000 + total/2) / total`。
    ///
    /// 半格四舍五入是刻意的：14 格里中 13 格要落在 **929** 而不是 928，
    /// 同批 F11660/F11663 用的就是这个口径，两卡对不上就没法联签。
    pub fn rate(&self) -> u32 {
        if self.total == 0u32 {
            return 0u32;
        }
        let num = self
            .matched
            .saturating_mul(RATE_FULL)
            .saturating_add(self.total / 2u32);
        num / self.total
    }

    /// 是否全绿。
    pub fn green_flag(&self) -> bool {
        if self.total != CORPUS as u32 {
            return false;
        }
        if self.accept != ACCEPT as u32 || self.reject != REJECT as u32 {
            return false;
        }
        if self.matched != self.total {
            return false;
        }
        self.rate() == RATE_FULL
    }

    /// 契约要求的 `green()`：布尔位单列，便于断言方向不被读反。
    pub fn green(&self) -> bool {
        self.green_flag()
    }

    /// 落锚：任何一维不满足都给出确定码。
    pub fn gate(&self) -> Result<(), u32> {
        if self.total != CORPUS as u32 {
            return Err(AC_EVALUE);
        }
        if self.accept != ACCEPT as u32 {
            return Err(AC_EVALUE);
        }
        if self.reject != REJECT as u32 {
            return Err(AC_EVALUE);
        }
        if !self.tiers_balance() {
            return Err(AC_EVALUE);
        }
        if self.matched != self.total {
            return Err(AC_ESTATE);
        }
        if self.rate() != RATE_FULL {
            return Err(AC_ESTATE);
        }
        Ok(())
    }

    /// 四档之和是否等于总数（独立于对齐数的另一条自洽）。
    pub fn tiers_balance(&self) -> bool {
        let mut s = self.green;
        s = s.saturating_add(self.flaked);
        s = s.saturating_add(self.skipped);
        s = s.saturating_add(self.blocked);
        s == self.total
    }

    /// 渲染（逐字文本由 `RENDER_OK` 钉死）。
    pub fn render(&self) -> String {
        let mut s = String::from("AC-");
        s.push_str(dec_text(CARD as u32).as_str());
        s.push_str(" total=");
        s.push_str(dec_text(self.total).as_str());
        s.push_str(" accept=");
        s.push_str(dec_text(self.accept).as_str());
        s.push_str(" reject=");
        s.push_str(dec_text(self.reject).as_str());
        s.push_str(" green=");
        s.push_str(dec_text(self.green).as_str());
        s.push_str(" flaked=");
        s.push_str(dec_text(self.flaked).as_str());
        s.push_str(" skipped=");
        s.push_str(dec_text(self.skipped).as_str());
        s.push_str(" blocked=");
        s.push_str(dec_text(self.blocked).as_str());
        s.push_str(" matched=");
        s.push_str(dec_text(self.matched).as_str());
        s.push_str(" rate=");
        s.push_str(dec_text(self.rate()).as_str());
        s.push_str(" gate=");
        s.push_str(match self.green() {
            true => "pass",
            false => "fail",
        });
        s
    }
}

/// 全绿渲染文本逐字钉死：改动 `render()` 的任何一处字面量都会在这里露出来。
pub const RENDER_OK: &str =
    "AC-11671 total=14 accept=8 reject=6 green=11 flaked=1 skipped=1 blocked=1 matched=14 rate=1000 gate=pass";

/// 跑一遍全语料。
pub fn run() -> Result<Report, u32> {
    run_with(None)
}

/// 跑一遍全语料，可选地把某一槽的申报恰好坏掉一维。
///
/// 注入是**判据的自证**，不是失败：注入后那一槽的判定必然与期望不符，于是通过率
/// 从 1000 掉到 929，红档格才有实据可记。越界槽或坏成一维以外的维直接回
/// `AC_ESLOT`——"注入到不存在的槽"与"注入了一条不算缺陷的维"都不能被当成红档。
pub fn run_with(over: Option<(usize, Fault)>) -> Result<Report, u32> {
    let inj_slot: usize;
    let inj_fault: Option<Fault>;
    match over {
        None => {
            inj_slot = CORPUS;
            inj_fault = None;
        }
        Some((i, f)) => {
            if i >= CORPUS || !f.is_reject() {
                return Err(AC_ESLOT);
            }
            inj_slot = i;
            inj_fault = Some(f);
        }
    }
    let mut m = Matrix::new();
    let mut i = 0usize;
    while i < CORPUS {
        let Some(sh) = shape_at(i) else {
            return Err(AC_EVALUE);
        };
        let Some(base) = claim_at(i) else {
            return Err(AC_EVALUE);
        };
        let claim = match inj_fault {
            Some(f) if i == inj_slot => base.break_dim(f),
            _ => base,
        };
        let actual = define(&sh);
        let fault = judge_against(&claim, &actual);
        let want = expected_fault(i);
        let row = Row {
            ndx: i,
            tag: want_tag(i),
            title: slot_name(i),
            actual,
            claim,
            fault,
            want,
            tier: Tier::of(&actual),
            matched: fault == want,
        };
        if !m.push(row) {
            return Err(AC_ESLOT);
        }
        i = i.saturating_add(1usize);
    }
    if m.len() != CORPUS {
        return Err(AC_EVALUE);
    }
    let rep = Report {
        total: m.total(),
        accept: ACCEPT as u32,
        reject: REJECT as u32,
        green: m.count_of(Tier::Green),
        flaked: m.count_of(Tier::Flaked),
        skipped: m.count_of(Tier::Skipped),
        blocked: m.count_of(Tier::Blocked),
        matched: m.good(),
    };
    if !rep.tiers_balance() {
        return Err(AC_EVALUE);
    }
    Ok(rep)
}

// ---------------------------------------------------------------------------
// 阶段闸门：八道闸只能向前
// ---------------------------------------------------------------------------

/// 布尔折叠（`true` 折 1、`false` 折 0；用 2 表示"申报为真但实测为假"的对照位）。
pub fn fold_bool(h: u64, v: bool) -> u64 {
    fold_byte(h, if v { 1u8 } else { 0u8 })
}

/// 单调阶段闸门。
///
/// 本卡的观测纵线是 `Lex → Class → Table → CoSign → Alias → Reply → Code → Seal`。
/// 闸门只接受"前进到同一秩或更高秩"：倒退说明申报者不是按闸序观测，而是拿着
/// 文案倒推——`busybox LS` 报 127 就声称"停在答复闸"，可他的答复闸其实根本没走
/// （表形那一闸还没验），这类谎报在真实故障里最常见，因为文案完全合理。
/// 关闸之后一切 `step`/`close` 一律 `AC_ESTATE`：已经定局的纵线不许再补事实。
#[derive(Clone, Copy, Debug)]
pub struct Gate {
    /// 上一道闸的秩（-1 表示一步没走）。
    last: i32,
    /// 已走闸数。
    steps: u32,
    /// 倒退次数（显性记账，不许静默吞）。
    falls: u32,
    /// 是否已关。
    closed: bool,
}

impl Gate {
    /// 新闸门。
    pub const fn new() -> Gate {
        Gate {
            last: -1i32,
            steps: 0u32,
            falls: 0u32,
            closed: false,
        }
    }

    /// 走一道闸。倒退 → `AC_ESTAGE`；关闸后再走 → `AC_ESTATE`。
    pub fn step(&mut self, s: Stage) -> Result<(), u32> {
        if self.closed {
            self.falls = self.falls.saturating_add(1u32);
            return Err(AC_ESTATE);
        }
        let r = s.rank() as i32;
        if r < self.last {
            self.falls = self.falls.saturating_add(1u32);
            return Err(AC_ESTAGE);
        }
        self.last = r;
        self.steps = self.steps.saturating_add(1u32);
        Ok(())
    }

    /// 按语料纵线走满八道并关闸。
    pub fn walk_all(&mut self) -> Result<u32, u32> {
        let mut i = 0usize;
        while i < GATE_COUNT {
            let Some(g) = STAGE_CHAIN.get(i) else {
                return Err(AC_EVALUE);
            };
            self.step(*g)?;
            i = i.saturating_add(1usize);
        }
        self.close()
    }

    /// 关闸并返回闸数；重复关闸 → `AC_ESTATE`。
    pub fn close(&mut self) -> Result<u32, u32> {
        if self.closed {
            self.falls = self.falls.saturating_add(1u32);
            return Err(AC_ESTATE);
        }
        self.closed = true;
        Ok(self.steps)
    }

    /// 上一道闸的秩。
    pub const fn last_rank(&self) -> i32 {
        self.last
    }

    /// 已走闸数。
    pub const fn steps(&self) -> u32 {
        self.steps
    }

    /// 倒退次数。
    pub const fn falls(&self) -> u32 {
        self.falls
    }

    /// 是否已关。
    pub const fn closed(&self) -> bool {
        self.closed
    }

    /// 闸门状态指纹。
    pub fn seal(&self) -> u64 {
        let h = fold_word(FNV_OFF, (self.last + 1i32) as u64);
        let a = fold_word(h, self.steps as u64);
        let b = fold_word(a, self.falls as u64);
        fold_bool(b, self.closed)
    }
}

// ---------------------------------------------------------------------------
// 账本：14 格 = 7 统计 + 1 红档 + 6 缺陷码
// ---------------------------------------------------------------------------

/// 账本十四格的期望操作码（前 8 格统计与红档，后 6 格缺陷码）。
///
/// 一码一格互不相同：如果两格共用一个码，把它们的内容互换就查不出来，
/// "重放同根"这条承诺也就成了空话。
pub const LEDGER_OPS: [u8; SAMPLES] = [
    0x01u8, 0x02u8, 0x03u8, 0x04u8, 0x05u8, 0x06u8, 0x07u8, 0x08u8, 0x09u8, 0x0au8, 0x0bu8,
    0x0cu8, 0x0du8, 0x0eu8,
];

/// 一格账。
#[derive(Clone, Copy, Debug)]
pub struct Sample {
    /// 槽位（必须严格等于登记时的账本长度）。
    pub slot: u8,
    /// 操作码。
    pub op: u8,
    /// 这一格是不是统计格（true）/缺陷格（false）。
    pub ok: bool,
    /// 缺陷码；统计格必须为 0。
    pub code: u32,
    /// 统计值；缺陷格必须为 0。
    pub value: u32,
}

impl Sample {
    /// 空白格（未登记）。
    pub const fn blank() -> Sample {
        Sample {
            slot: 0u8,
            op: 0u8,
            ok: false,
            code: 0u32,
            value: 0u32,
        }
    }

    /// 统计格构造。
    pub const fn stat(slot: u8, op: u8, value: u32) -> Sample {
        Sample {
            slot,
            op,
            ok: true,
            code: 0u32,
            value,
        }
    }

    /// 缺陷格构造（值一律归零）。
    pub const fn defect(slot: u8, op: u8, code: u32) -> Sample {
        Sample {
            slot,
            op,
            ok: false,
            code,
            value: 0u32,
        }
    }

    /// 格指纹。
    pub fn seal(self: &Sample) -> u64 {
        let h = fold_word(FNV_OFF, self.slot as u64);
        let a = fold_word(h, self.op as u64);
        let b = fold_bool(a, self.ok);
        let c = fold_word(b, self.code as u64);
        fold_word(c, self.value as u64)
    }

    /// 格渲染：`slot/op 状态 码/值`。
    pub fn render(self: &Sample) -> String {
        let mut s = String::from(dec_text(self.slot as u32).as_str());
        s.push('/');
        s.push_str(op_name(self.op));
        s.push(' ');
        s.push_str(bool_text(self.ok));
        if self.ok {
            s.push_str(" v=");
            s.push_str(dec_text(self.value).as_str());
        } else {
            s.push_str(" c=");
            s.push_str(hex_text(self.code, 5usize).as_str());
        }
        s
    }
}

/// 账本：槽位纪律 + 逐格对账 + 冻结重放。
///
/// 三条纪律各自挡一种坏法：
/// * **登记纪律**（`slot == len`）：跳号与越界都不入账并计入 `rejects`——
///   挡住"先写结论格、后补统计格"这种倒填。
/// * **对账纪律**（`audit`）：统计格逐格对 `VALS`、缺陷格逐格对 `REJECT_CODES`，
///   败格值必须归零——挡住"把红档率的 929 记到统计格、把断言数记到缺陷格"。
/// * **冻结纪律**（`freeze`/`replay`）：冻结后拒登记、拒篡改，重放必须同根——
///   挡住"交账之后再回改账本"。
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
            len: 0usize,
            root: 0u64,
            frozen: false,
            rejects: 0u32,
        }
    }

    /// 登记一格。槽位不等于当前长度、越界、操作码不符或已冻结都拒收并计数。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1u32);
            return Err(AC_ESTATE);
        }
        let at = s.slot as usize;
        if at != self.len || at >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1u32);
            return Err(AC_ESLOT);
        }
        if let Some(w) = LEDGER_OPS.get(at) {
            if s.op != *w {
                self.rejects = self.rejects.saturating_add(1u32);
                return Err(AC_ESLOT);
            }
        }
        if let Some(dst) = self.rows.get_mut(at) {
            *dst = s;
        }
        self.len = self.len.saturating_add(1usize);
        Ok(())
    }

    /// 已登记格数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.len == 0usize
    }

    /// 取一格。
    pub fn sample(&self, i: usize) -> Option<Sample> {
        if i >= self.len {
            return None;
        }
        match self.rows.get(i) {
            Some(s) => Some(*s),
            None => None,
        }
    }

    /// 统计格数。
    pub fn ok_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                if s.ok {
                    n = n.saturating_add(1usize);
                }
            }
            i = i.saturating_add(1usize);
        }
        n
    }

    /// 缺陷格数。
    pub fn err_count(&self) -> usize {
        self.len.saturating_sub(self.ok_count())
    }

    /// 逐格对账。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES {
            return Err(AC_ESTATE);
        }
        if self.ok_count() != ACCEPT || self.err_count() != REJECT {
            return Err(AC_ESTATE);
        }
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let Some(s) = self.rows.get(i) else {
                return Err(AC_ESTATE);
            };
            if (s.slot as usize) != i {
                return Err(AC_ESLOT);
            }
            if let Some(w) = LEDGER_OPS.get(i) {
                if s.op != *w {
                    return Err(AC_ESLOT);
                }
            }
            if s.ok {
                if s.code != 0u32 {
                    return Err(AC_ECODE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(AC_EVALUE);
                        }
                    }
                    None => return Err(AC_EVALUE),
                }
                vi = vi.saturating_add(1usize);
            } else {
                if s.value != 0u32 {
                    return Err(AC_EVALUE);
                }
                match REJECT_CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(AC_ECODE);
                        }
                    }
                    None => return Err(AC_ECODE),
                }
                ci = ci.saturating_add(1usize);
            }
            i = i.saturating_add(1usize);
        }
        if vi != ACCEPT || ci != REJECT {
            return Err(AC_ESTATE);
        }
        Ok(())
    }

    /// 全账指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                h = fold_word(h, s.seal());
            }
            i = i.saturating_add(1usize);
        }
        fold_word(h, self.rejects as u64)
    }

    /// 冻结：先对账，再落根。已冻结再冻结 → `AC_ESTATE`。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1u32);
            return Err(AC_ESTATE);
        }
        if self.len != SAMPLES {
            self.rejects = self.rejects.saturating_add(1u32);
            return Err(AC_ESTATE);
        }
        self.audit()?;
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放：按同一张事实表重建账本并比指纹。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        let a = fresh.seal();
        if a == self.seal() {
            Ok(a)
        } else {
            Err(AC_ESLOT)
        }
    }

    /// 冻结指纹。
    pub const fn root(&self) -> u64 {
        self.root
    }

    /// 是否已冻结。
    pub const fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 拒收计数。
    pub const fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 篡改留痕：改一格观测值；冻结后一律拒收并计数。
    pub fn tamper(&mut self, i: usize, value: u32) -> Result<(), u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1u32);
            return Err(AC_ESTATE);
        }
        if i >= self.len {
            self.rejects = self.rejects.saturating_add(1u32);
            return Err(AC_ESLOT);
        }
        if let Some(dst) = self.rows.get_mut(i) {
            dst.value = value;
        }
        Ok(())
    }

    /// 测试专用：只切冻结位，不动指纹与格数。
    ///
    /// 这不是"绕过冻结"——它只把冻结位拨下去，登记路径随后照常走
    /// `frozen` 检查。真实冻结必须过 `audit()`；本函数用于把"冻结后拒登记"
    /// 这一条单独隔离出来测，免得测一条纪律要先把整本账配平。
    #[cfg(test)]
    pub(crate) fn set_frozen(&mut self, v: bool) {
        self.frozen = v;
    }
}

/// 红档格的构造：注入结果的通过率才敢直接入账——`Err` 落缺陷格而不是硬凑 929。
fn rate_cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample::stat(slot, op, v),
        Err(c) => Sample::defect(slot, op, c),
    }
}

/// 建账：7 统计格 + 1 红档格（929）+ 6 缺陷码格。
///
/// 七个统计数全部**现算**（表行数、别名字数、判据项数、闸数、成因档数、载体种类数、
/// 卡面行数地板），第 8 格是"坏掉一维之后的通过率"——它必须等于 `RED_RATE`，
/// 也就是全卡最怕的那一幕：十四格里有一格对不上。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let _ = l.register(Sample::stat(
        0u8,
        OP_TABLE,
        TABLE_ROWS as u32,
    ));
    let _ = l.register(Sample::stat(1u8, OP_ALIAS, table_rows_of(RowKind::Alias)));
    let _ = l.register(Sample::stat(2u8, OP_CHECK, CHECK_COUNT as u32));
    let _ = l.register(Sample::stat(3u8, OP_GATE, GATE_COUNT as u32));
    let _ = l.register(Sample::stat(4u8, OP_NOTE, Note::COUNT as u32));
    let _ = l.register(Sample::stat(5u8, OP_HARNESS, Harness::COUNT as u32));
    let _ = l.register(Sample::stat(6u8, OP_FLOOR, CARD_LINES));
    let red = run_with(Some((0usize, Fault::Estate)));
    let _ = l.register(rate_cell(7u8, OP_RATE, red.map(|x| x.rate())));
    let mut i = 0usize;
    while i < REJECT {
        let Some(f) = REJECT_FAULTS.get(i) else {
            break;
        };
        let slot = (ACCEPT + i) as u8;
        let op = match LEDGER_OPS.get(slot as usize) {
            Some(o) => *o,
            None => OP_DEFECT,
        };
        let _ = l.register(Sample::defect(slot, op, f.code()));
        i = i.saturating_add(1usize);
    }
    l
}

/// 四路注入：表路 / 判路 / 渲染路 / 账本路，各自独立自检。
///
/// 任何一路静默退化都会在这一格里变成 false；四路分开的理由是它们坏起来的形态
/// 完全不同：表路坏是"常量和枚举被改"，判路坏是"纵线与对判不再自洽"，
/// 渲染路坏是"数出来的东西说不出口"，账本路坏是"记下的账重放不出同一根"。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = name_ok() && enums_ok() && bytes_ok() && table_ok();
    out[1] = lookup_ok() && alias_ok() && define_ok() && judge_ok();
    out[2] = cosign_ok() && shape_ok() && matrix_ok();
    out[3] = gate_ok() && ledger_ok() && {
        let mut l = build_ledger();
        let a = l.audit() == Ok(());
        let b = match l.freeze() {
            Ok(r) => r != 0u64 && l.is_frozen() && l.replay() == Ok(r),
            Err(_) => false,
        };
        a && b
    };
    out
}

// @@P2CHUNK7@@

// ---------------------------------------------------------------------------
// 四路注入的私有谓词
// ---------------------------------------------------------------------------

/// 表路之一：身份组——语料名字、载体与两张标签表自洽。
fn name_ok() -> bool {
    if PROG.is_empty() || NAME_MAX == 0usize {
        return false;
    }
    if SLOT_NAMES.len() != CORPUS || WANT_TAGS.len() != CORPUS {
        return false;
    }
    let mut i = 0usize;
    while i < CORPUS {
        let s = match SHAPES.get(i) {
            Some(x) => *x,
            None => return false,
        };
        // 载体必须逐字是 `busybox`，否则前提缺席（`premise()` 的第一问）。
        if s.carrier != PROG {
            return false;
        }
        if s.name.is_empty() || s.name_len() > NAME_MAX as u32 {
            return false;
        }
        if s.name_len() != s.name.len() as u32 {
            return false;
        }
        // 槽位标签两张表对得上：按名取与按槽取必须是同一份。
        let t = want_tag(i);
        if t.is_empty() || !t.starts_with("f11671-") {
            return false;
        }
        if tag_of(s.name) != t {
            return false;
        }
        if slot_name(i).is_empty() {
            return false;
        }
        // 标签两两不同：同号标签会让两槽的红档分不开。
        let mut j = i.saturating_add(1usize);
        while j < CORPUS {
            if want_tag(j) == t {
                return false;
            }
            j = j.saturating_add(1usize);
        }
        i = i.saturating_add(1usize);
    }
    true
}

/// 表路之二：枚举维数——各枚举的档数与秩互恰。
fn enums_ok() -> bool {
    if Stage::COUNT as usize != GATE_COUNT || GATE_COUNT != STAGE_CHAIN.len() {
        return false;
    }
    if NameClass::COUNT != 7u8 || Reply::COUNT != 6u8 || Hit::COUNT != 8u8 {
        return false;
    }
    if Note::COUNT != 20u8 || Skip::COUNT != 7u8 || Harness::COUNT != 6u8 {
        return false;
    }
    if TIERS.len() != TIER_COUNT || TIER_COUNT != 4usize {
        return false;
    }
    if OPS.len() != 9usize || LEDGER_OPS.len() != SAMPLES {
        return false;
    }
    // 闸链的秩严格递增且从 1 起（1 基是本卡闸门的契约，0 基会让跳闸合法）。
    let mut i = 0usize;
    while i < STAGE_CHAIN.len() {
        let s = match STAGE_CHAIN.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if s.rank() != (i + 1usize) as u8 {
            return false;
        }
        let op = match OPS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if op_name(op).is_empty() || op_name(op) == "unknown-op" {
            return false;
        }
        i = i.saturating_add(1usize);
    }
    // 四档秩两两不同。
    let mut a = 0usize;
    while a < TIERS.len() {
        let x = match TIERS.get(a) {
            Some(v) => *v,
            None => return false,
        };
        let mut b = a.saturating_add(1usize);
        while b < TIERS.len() {
            if let Some(y) = TIERS.get(b) {
                if y.rank() == x.rank() {
                    return false;
                }
            }
            b = b.saturating_add(1usize);
        }
        a = a.saturating_add(1usize);
    }
    // 操作码互不相同。
    let mut p = 0usize;
    while p < OPS.len() {
        let mut q = p.saturating_add(1usize);
        while q < OPS.len() {
            if let (Some(x), Some(y)) = (OPS.get(p), OPS.get(q)) {
                if x == y {
                    return false;
                }
            }
            q = q.saturating_add(1usize);
        }
        p = p.saturating_add(1usize);
    }
    true
}

/// 表路之三：字节面——大小写/字节实测辅助函数互恰。
fn bytes_ok() -> bool {
    if !is_lower_byte(b'a') || !is_upper_byte(b'A') {
        return false;
    }
    if is_lower_byte(b'A') || is_upper_byte(b'a') {
        return false;
    }
    if !is_alpha_byte(b'q') || is_alpha_byte(b'1') || !is_digit_byte(b'1') {
        return false;
    }
    if !is_print_byte(b'!') || is_print_byte(b' ') || is_print_byte(0x7fu8) {
        return false;
    }
    if lower_byte(b'Z') != b'z' || lower_byte(b'z') != b'z' || lower_byte(b'0') != b'0' {
        return false;
    }
    if upper_bytes("LS") != 2u32 || lower_bytes("ls") != 2u32 || alpha_count("a1-") != 1u32 {
        return false;
    }
    if non_ascii_bytes("ls") != 0u32 || odd_bytes("ls") != 0u32 || odd_bytes(" l") != 1u32 {
        return false;
    }
    // 无大写的名字折叠前后同形；有大写的折叠后不再同形。
    let mut i = 0usize;
    while i < CORPUS {
        let s = match SHAPES.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let up = upper_bytes(s.name);
        let folded = folded_query(s.name);
        if up == 0u32 {
            if folded != s.name {
                return false;
            }
        } else if folded == s.name {
            return false;
        }
        if alpha_count(s.name) > s.name_len() {
            return false;
        }
        i = i.saturating_add(1usize);
    }
    true
}

/// 表路之四：建模表——行数、键唯一性、别名链与解析结果互恰。
fn table_ok() -> bool {
    if TABLE.len() != TABLE_ROWS || TABLE_ROWS != CANON_ROWS + ALIAS_ROWS + PSEUDO_ROWS {
        return false;
    }
    if table_dup_key() || table_dangling_alias() || table_self_alias() || table_alias_shadows() {
        return false;
    }
    if table_rows_of(RowKind::Canon) != CANON_ROWS as u32
        || table_rows_of(RowKind::Alias) != ALIAS_ROWS as u32
        || table_rows_of(RowKind::Pseudo) != PSEUDO_ROWS as u32
    {
        return false;
    }
    let mut canon = 0usize;
    let mut alias = 0usize;
    let mut pseudo = 0usize;
    let mut i = 0usize;
    while i < TABLE_ROWS {
        let r = match table_row(i) {
            Some(x) => x,
            None => return false,
        };
        if r.key.is_empty() || upper_bytes(r.key) != 0u32 {
            return false;
        }
        if !table_has(r.key) || role_of_key(r.key) != Some(r.kind) {
            return false;
        }
        match r.kind {
            RowKind::Canon => {
                canon += 1;
                if resolve(r.key) != Resolve::Direct {
                    return false;
                }
            }
            RowKind::Alias => {
                alias += 1;
                if r.target.is_empty() || !table_has(r.target) {
                    return false;
                }
                if !resolve(r.key).reached() {
                    return false;
                }
            }
            RowKind::Pseudo => {
                pseudo += 1;
                if resolve(r.key) != Resolve::Pseudo {
                    return false;
                }
            }
        }
        i = i.saturating_add(1usize);
    }
    if canon != CANON_ROWS || alias != ALIAS_ROWS || pseudo != PSEUDO_ROWS {
        return false;
    }
    // 表外名字必须未名；空名必未名。
    if resolve("zzzzz") != Resolve::Miss || resolve("") != Resolve::Miss {
        return false;
    }
    if table_has("zzzzz") || role_of_key("zzzzz").is_some() {
        return false;
    }
    true
}

/// 判路之一：查表出口——`resolve()` 与 `find_row()` 的往返一致。
fn lookup_ok() -> bool {
    let mut i = 0usize;
    while i < TABLE_ROWS {
        let r = match table_row(i) {
            Some(x) => x,
            None => return false,
        };
        let f = match find_row(r.key) {
            Some(x) => x,
            None => return false,
        };
        if f.key != r.key || f.target != r.target || f.kind != r.kind || f.cfg != r.cfg {
            return false;
        }
        match resolve(r.key) {
            Resolve::Direct | Resolve::Hopped(_) | Resolve::Pseudo => {}
            _ => return false,
        }
        if byte_exact_hit(r.key) != true || fold_would_hit(r.key) != false {
            return false;
        }
        i = i.saturating_add(1usize);
    }
    // 未名与折叠反面：大写形态逐字节必未名；折叠反而可能命中。
    if byte_exact_hit("LS") != false || byte_exact_hit("ZCAT") != false {
        return false;
    }
    if fold_would_hit("LS") != true || fold_would_hit("ZCAT") != true {
        return false;
    }
    if reply_should_carry_name("") != false || reply_should_carry_name("ls") != true {
        return false;
    }
    true
}

/// 判路之二：别名链——跳数、链长上限与语料逐槽的命中档互恰。
fn alias_ok() -> bool {
    if HOP_CAP != 2u8 {
        return false;
    }
    // 语料逐槽：申报的命中档必须与实测 `resolve()` 同档（`Shape::alias_ok`
    // 的自由函数版：四档语料都过）。
    let mut i = 0usize;
    while i < CORPUS {
        let s = match shape_at(i) {
            Some(x) => x,
            None => return false,
        };
        if !s.alias_ok() {
            return false;
        }
        i = i.saturating_add(1usize);
    }
    // 一跳与两跳各有活样本：`egrep` 一跳、`gzcat` 两跳。
    if resolve("egrep").hops() != 1u8 || resolve("gzcat").hops() != 2u8 {
        return false;
    }
    true
}

/// 判路之三：定义出口——语料逐槽的实测与四张期望表对得上。
fn define_ok() -> bool {
    if WANT_STAGE.len() != CORPUS
        || WANT_CODE.len() != CORPUS
        || WANT_NOTE.len() != CORPUS
        || WANT_DEFAULT.len() != CORPUS
        || CLAIMS.len() != CORPUS
    {
        return false;
    }
    let mut i = 0usize;
    while i < CORPUS {
        let s = match shape_at(i) {
            Some(x) => x,
            None => return false,
        };
        let a = define(&s);
        let b = define(&s);
        if a != b {
            return false;
        }
        let ws = match WANT_STAGE.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let wc = match WANT_CODE.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let wn = match WANT_NOTE.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let wd = match WANT_DEFAULT.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if a.stage != ws || a.code != wc || a.note != wn || a.default != wd {
            return false;
        }
        let c = match claim_at(i) {
            Some(x) => x,
            None => return false,
        };
        let base = match CLAIMS.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if c != base {
            return false;
        }
        i = i.saturating_add(1usize);
    }
    true
}

/// 判路之四：对判——期望判定、注入自证与红档率互恰。
fn judge_ok() -> bool {
    if REJECT_FAULTS.len() != REJECT || REJECT_CODES.len() != REJECT {
        return false;
    }
    let mut i = 0usize;
    while i < REJECT {
        let f = match REJECT_FAULTS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let c = match REJECT_CODES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if !f.is_reject() || f.code() != c || f.ndx() != i + 1usize {
            return false;
        }
        i = i.saturating_add(1usize);
    }
    // 语料逐槽：判定必须等于期望。
    let mut k = 0usize;
    while k < CORPUS {
        let c = match case_at(k) {
            Some(x) => x,
            None => return false,
        };
        if judge(&c) != expected_fault(k) {
            return false;
        }
        if slot_is_live(k) && expected_fault(k) != Fault::Ok {
            return false;
        }
        if !slot_is_live(k) && !expected_fault(k).is_reject() {
            return false;
        }
        k = k.saturating_add(1usize);
    }
    // 注入自证：活槽注入任一缺陷维，通过率掉到 929；越界与 Ok 注入都拒收。
    let red = match run_with(Some((0usize, Fault::Estate))) {
        Ok(x) => x,
        Err(_) => return false,
    };
    if red.matched != CORPUS as u32 - 1u32 || red.rate() != RED_RATE {
        return false;
    }
    if run_with(Some((CORPUS, Fault::Estate))).is_ok() {
        return false;
    }
    if run_with(Some((0usize, Fault::Ok))).is_ok() {
        return false;
    }
    true
}

/// 渲染路之一：联签位——语料逐槽的联签数与表指纹一致。
fn cosign_ok() -> bool {
    let mut i = 0usize;
    while i < CORPUS {
        let s = match shape_at(i) {
            Some(x) => x,
            None => return false,
        };
        if !s.cosign_ok() {
            return false;
        }
        i = i.saturating_add(1usize);
    }
    if table_fingerprint() == 0u32 {
        return false;
    }
    true
}

/// 渲染路之二：现场事实——语料逐槽的表组申报与表侧实测一致。
fn shape_ok() -> bool {
    let mut i = 0usize;
    while i < CORPUS {
        let s = match shape_at(i) {
            Some(x) => x,
            None => return false,
        };
        // 表组：行数与别名行数必须是表侧实况。
        if s.table_rows != TABLE_ROWS as u32 || s.alias_rows != ALIAS_ROWS as u32 {
            return false;
        }
        if s.table_upper != table_upper_total() || s.table_fp != table_fingerprint() {
            return false;
        }
        // 实测类与申报类一致（语料事实全取绿行，申报位不许自己先歪）。
        if s.cls != name_class(s.name) {
            return false;
        }
        if s.hops != s.hit.hops() {
            return false;
        }
        if s.within_cap != (s.hops <= HOP_CAP) {
            return false;
        }
        i = i.saturating_add(1usize);
    }
    true
}

/// 渲染路之三：横计矩阵——全绿报表逐字与四档合计互恰。
fn matrix_ok() -> bool {
    let rep = match run() {
        Ok(x) => x,
        Err(_) => return false,
    };
    if !rep.green() || rep.rate() != RATE_FULL || rep.gate().is_err() {
        return false;
    }
    if rep.total != CORPUS as u32 || rep.matched != WANT_MATCHED {
        return false;
    }
    if rep.green != WANT_GREEN
        || rep.flaked != WANT_FLAKED
        || rep.skipped != WANT_SKIPPED
        || rep.blocked != WANT_BLOCKED
    {
        return false;
    }
    if !rep.tiers_balance() {
        return false;
    }
    if rep.render() != RENDER_OK {
        return false;
    }
    true
}

/// 账本路之一：闸门——八道闸只能向前，倒退与关闸后再走都要被拒。
fn gate_ok() -> bool {
    let mut g = Gate::new();
    let n = match g.walk_all() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if n != GATE_COUNT as u32 || g.steps() != n || g.falls() != 0u32 || !g.closed() {
        return false;
    }
    if g.step(Stage::Seal).is_ok() || g.close().is_ok() {
        return false;
    }
    // 倒退被拒并显性记账。
    let mut h = Gate::new();
    if h.step(Stage::Reply).is_err() || h.step(Stage::Code).is_err() {
        return false;
    }
    if h.step(Stage::Table).is_ok() || h.falls() != 1u32 {
        return false;
    }
    if h.last_rank() != Stage::Code.rank() as i32 || h.close() != Ok(2u32) {
        return false;
    }
    // 闸门状态指纹在关闭前后必须不同。
    let mut u = Gate::new();
    let open = u.seal();
    if u.walk_all().is_err() {
        return false;
    }
    if u.seal() == open {
        return false;
    }
    true
}

/// 账本路之二：账本——登记纪律、逐格对账与冻结重放。
fn ledger_ok() -> bool {
    let l = build_ledger();
    if l.len() != SAMPLES || l.audit().is_err() || l.ok_count() != ACCEPT || l.err_count() != REJECT
    {
        return false;
    }
    // 登记纪律：跳号与重复槽都拒收。
    let mut bad = Ledger::new();
    if bad.register(Sample::stat(1u8, OP_ALIAS, ALIAS_ROWS as u32)).is_ok() {
        return false;
    }
    if bad.register(Sample::stat(0u8, OP_TABLE, TABLE_ROWS as u32)).is_err() {
        return false;
    }
    if bad.register(Sample::stat(0u8, OP_TABLE, TABLE_ROWS as u32)).is_ok() {
        return false;
    }
    // 操作码与槽位不符也拒收。
    if bad.register(Sample::stat(1u8, OP_TABLE, ALIAS_ROWS as u32)).is_ok() {
        return false;
    }
    // 冻结纪律：冻结后指纹可读、重放同根；再登记与再冻结都拒收。
    let mut m = build_ledger();
    let root = match m.freeze() {
        Ok(r) => r,
        Err(_) => return false,
    };
    if root == 0u64 || !m.is_frozen() || m.replay() != Ok(root) {
        return false;
    }
    if m.register(Sample::stat(0u8, OP_TABLE, TABLE_ROWS as u32)).is_ok() {
        return false;
    }
    if m.freeze().is_ok() {
        return false;
    }
    true
}
