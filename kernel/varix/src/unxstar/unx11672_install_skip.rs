//! UNX-F11672 busybox 对未知名 `--install` 降级判据
//!
//! 本卡只判一件事：**`busybox --install -s <dir>` 里混进一个不在表里的名字时，
//! 整趟安装怎么收场**。它既不是"未知选项"（F11669：选项层就拒了，根本没进安装），
//! 也不是"未名 applet 文案逐字"（F11674：那条文案的字节归它），更不是"install
//! 软链全覆盖 + argv[0] 回派"（F11584：那是一趟全成功的装盘计划），也不是
//! "applet 表完整性"（F11673：清单 vs `--list` 的零偏差）。本卡的现场永远是
//! **一趟 `--install` 已经跑完，表里少了一个名字**。
//!
//! 真机上这条路径的答复是一句话能说清、但极容易被"顺手修好"破坏的：
//!
//! ```text
//! $ busybox --install -s /bin        # 表里有 ls cp mv ...，没有 foo
//! foo: applet not found              # 一行，**stderr**，跳过
//! $ echo $?
//! 0                                  # 整趟仍退 0
//! $ ls -l /bin/foo
//! ls: /bin/foo: No such file or directory
//! ```
//!
//! 三行合起来才是一条判据：**说了（一行日志）、没做（没建链）、没把整趟拖红
//! （退 0）**。任何一行单独成立都不算数——只"说了"却建了链是伪链；只"没做"却
//! 一字不说，用户无从知道少了什么；"没做也说清了"却把整趟退成 127，等于让一个
//! 名字的缺席否掉其余十三条的成果。
//!
//! # 闸门次序（七道闸，秩从 1 起，只许前进）
//!
//! `Invoke → Class → Decide → Target → Emit → Code → Report`
//!
//! 1. `Invoke` —— 这一趟是不是安装调用：`--install -s DIR` / `--install DIR` 进卡；
//!    `busybox ls` 这种普通运行不进（兜底档）；`--install -s` 不带目录是用法错
//!    （一字不落盘）。先定调用形态，才知道后面几闸谈的是"安装语境"里的动作，
//!    而不是一次普通执行的退出码。
//! 2. `Class` —— 这个名字是什么身份：实名（表里，装）、别名（表里，收敛到规范名
//!    后装）、伪名（`busybox` / `--list`，在表里但自己就是分发器，不装）、未名
//!    （不在表里，跳过）。名字本身的形态（空名、带 `/`、超长）在这一闸就被挡下，
//!    因为形态坏掉的名字连"在不在表里"都没有答案。
//! 3. `Decide` —— **本卡的核心闸**：这一条要不要落盘。实名/别名必须建链；伪名/
//!    未名**必须不建**。未名却建了链就是伪链，重名的第二趟什么都没做也不留痕
//!    就是静默吞掉——两者都死在这一闸，因为它们的病根是"决定错了"，不是"目标
//!    坏了"、更不是"日志写错了"。
//! 4. `Target` —— 落盘面上的真相：实名/别名的链必须指向真 busybox 且目标存在
//!    （悬空、缺链、指旧目标全在这一闸落 `target-dangling`）；伪名/未名**必须
//!    一条链都没有**，包括上一趟残留的陈链——陈链不归 Decide 管（本趟根本没做
//!    决定），它是"不该存在的东西还在"，所以单列一闸、单记一档 `stale-left`。
//! 5. `Emit` —— 说没说、说在哪：未名条目必须**恰好一行**落在 **stderr**；落到
//!    stdout 就是通道误读。这一闸排在 Target 之后，是因为"说"必须晚于"做"：
//!    先确认落盘面没错，再问这句话说得对不对；顺序倒了就会出现"日志正确、
//!    盘上多了一条链"这种把伪链放过去的判法。
//! 6. `Code` —— 整趟退出码：跳过不改档（仍 0），缺目录才 1，`127` 是**未名执行**
//!    那一档的码，安装趟里出现它就是把"跳过"误报成"命令没找到"。
//! 7. `Report` —— 收账：本条目新建链接数（0/1）、跳过计数与清单是否对得上，最后
//!    落注解档并封账。
//!
//! 这条纵线为什么不能倒：先辨调用才知道有没有安装语境，先辨身份才知道要不要
//! 落盘，先定落盘才知道盘上该有什么、才轮得到"说"，先有"说"才谈得上码档与收账。
//! 允许倒退就会长出"先看码是 0 再回头判它没建链"的写法——一个把未名名字建了
//! 伪链、日志还写得漂漂亮亮的构建会被判绿，本卡当场失效。
//!
//! # 关键差异（本卡深度所在：同一串字节，两个码档）
//!
//! * **同一行文案，两条路径两个码**。`<名>: applet not found` 这串字节在直接执行
//!   路径上是"未名"的判决书（F11664 / F11674：**退 127**）；在 `--install` 里它
//!   只是"这一条我跳过"的告知（**整趟退 0**）。文案归属 F11674，码档归属本卡与
//!   F11664，两边共用一行字却是两个结论——这正是本卡单独立卡的理由：谁把 127
//!   搬进安装趟，谁就是把"跳过"升级成"失败"。
//! * **不建伪链 ≠ 不报错**，也 **≠ 建了条悬空链**。三种"看起来都失败了"的形态
//!   必须分开记：伪链（`fake-link`，Decide 闸，本趟建了）、悬空（`target-dangling`，
//!   Target 闸，链在但目标不是真 applet）、陈链（`stale-left`，Target 闸，上一趟
//!   留下的、本趟连碰都没碰）。合成一档就再也分不清"是谁建的"。
//! * **重名 ≠ 未名**。清单里同名出现两次时，第二趟必须判"已就位"（幂等，F11584
//!   的幂等面）并计入复用；静默吞掉（既不建链也不计复用、还不留痕）是
//!   `dup-collapsed`，与"跳过计数不对"（`count-drift`）是两件事：前者是某一行的
//!   去向丢了，后者是汇总数错了。
//! * **与 F11671 / F11673 的边界**：名字大小写与别名收敛归 F11671（本卡只消费
//!   它的结论：别名算"已知"）；清单与 `--list` 的零偏差归 F11673（本卡只消费
//!   清单本身，不重建清单）。
//!
//! 判据原文（UNX-F11672-J1）：install 对未名跳过档判据绿（不建伪链），与基准机一致。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码；账本 14 格
//! （7 统计 + 1 红档率 + 6 缺陷码）在 `audit` 逐格对账，冻结后重放必须同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 段基址与锚点
// ---------------------------------------------------------------------------

/// 本卡段基址（分配表钉死，不许改）。
pub const SEG_BASE: u32 = 0x6d250;

/// 前一张卡（F11671 applet 名大小写与别名）的段基址。
pub const ANCHOR_F11671_SEG: u32 = 0x6d230;

/// 本卡与前卡的间距（0x20：F11671 十三码占满 0x0d 后仍有 0x13 余量）。
pub const ANCHOR_F11671_GAP: u32 = 0x20;

/// 后一张卡（F11673 applet 表完整性）的段基址。
pub const ANCHOR_F11673_SEG: u32 = 0x6d260;

/// 本卡与后卡的间距。
pub const ANCHOR_F11673_GAP: u32 = 0x10;

/// 锚点：F11674（未名 applet 文案逐字）——跳过行的那串字节归它。
pub const ANCHOR_F11674_SEG: u32 = 0x6d270;

/// 锚点：F11664（notfound 退出码 127）——同一串字节的另一个码档归它。
pub const ANCHOR_F11664_SEG: u32 = 0x6d1a0;

/// 锚点：F11584（--install 软链全覆盖）——幂等面与全成功装盘归它。
pub const ANCHOR_F11584_SEG: u32 = 0xf990;

// ---------------------------------------------------------------------------
// 规模常量
// ---------------------------------------------------------------------------

/// 语料槽数（八活六死）。
pub const CORPUS: usize = 14;

/// 台账格数（七统计 + 一红档 + 六缺陷码）。
pub const SAMPLES: usize = 14;

/// 正例槽数。
pub const ACCEPT: usize = 8;

/// 反例槽数。
pub const REJECT: usize = 6;

/// 档位数（有链 / 没动 / 有病 / 兜底）。
pub const TIER_COUNT: usize = 4;

/// 闸数。
pub const STAGE_COUNT: u32 = 7;

/// 末闸秩。
pub const STAGE_LAST_RANK: u32 = 7;

/// 统计格数（不含红档格）。
pub const STAT_CELLS: usize = 7;

/// 红档率标定值：14 格里 13 格判对的千分率。
pub const RED_RATE: u32 = 929;

/// 满一致率千分比。
pub const RATE_FULL: u32 = 1000;

/// 安装清单行数（本卡全部域数据都挂在这 14 行上）。
pub const LIST_LEN: usize = 14;

/// 名字身份档数（实名 / 别名 / 伪名 / 未名）。
pub const CLASS_COUNT: usize = 4;

/// 调用形态档数（软链 / 硬链 / 缺目录 / 非安装）。
pub const INVOKE_COUNT: usize = 4;

/// 进本卡的调用形态数（非安装只作兜底锚）。
pub const INVOKE_JUDGED: usize = 3;

/// 链接状态档数（新建 / 已就位 / 无 / 悬空 / 陈链）。
pub const LINK_COUNT: usize = 5;

/// 缺陷种类数（八种真实构建病）。
pub const DEFECT_FAULTS: usize = 8;

/// 缺陷枚举格数（八种病 + 无病）。
pub const DEFECT_COUNT: usize = 9;

/// 注解档数。
pub const NOTE_COUNT: usize = 16;

/// 文案表格数：跳过行 ×2 通道 + 用法错 + 建链失败 + 未名执行 + 伪名静默 = 6 格。
pub const MESSAGE_CELLS: usize = 6;

/// 清单里出现"跳过"的行数（未名条目三条）。
pub const SKIP_ROWS: u32 = 3;

/// 清单里新建链接的行数（实名八条；重名那条只算"已就位"）。
pub const LINK_ROWS: u32 = 8;

/// 清单里"链已存在但本行没建"的行数（重名那条）。
pub const REUSE_ROWS: u32 = 1;

/// 清单里伪名的行数（`busybox` 与 `--list`）。
pub const PSEUDO_ROWS: u32 = 2;

/// 名字长度上限（超过就不是本卡现场）。
pub const NAME_CAP: u32 = 255;

/// 安装目录（软链落点，报错行里的前缀）。
pub const INSTALL_DIR: &str = "/bin";

/// 成功码（安装趟里跳过一个未名仍退这一档）。
pub const EXIT_OK: u32 = 0;

/// 用法错（`--install -s` 缺目录）。
pub const EXIT_USAGE: u32 = 1;

/// 未名执行的码（F11664 的锚；安装趟里出现它即误报）。
pub const EXIT_NOTFOUND: u32 = 127;

/// 跳过行的尾巴（逐字归 F11674，本卡只借它的字节数与通道）。
pub const NOT_FOUND_TAIL: &str = ": applet not found";

/// 用法错那一行的主语与内容（`--install` 不带目录）。
pub const USAGE_LINE: &str = "busybox: --install: no directory specified";

/// 建链失败那一行的前缀（perror 风格家族，本卡只建模不复判）。
pub const CREATE_HEAD: &str = "busybox: can't create ";

/// 建链失败的典型原因（同名普通文件占位，归 F11584 的冲突面）。
pub const CREATE_REASON: &str = ": File exists";

/// 多调用二进制名（伪名之一）。
pub const MULTICALL_NAME: &str = "busybox";

/// 第二个伪名（`--list` 也是表里的特殊键，永不装链）。
pub const LIST_PSEUDO: &str = "--list";

/// 清单里那个不存在的实名（跳过档的正锚）。
pub const NOSUCH_ONE: &str = "foo";

/// 清单里第二个不存在的实名。
pub const NOSUCH_TWO: &str = "bar";

/// 清单里带连字符的不存在名（字节数最长的一格）。
pub const NOSUCH_DASH: &str = "ls-zzz";

/// 空名（形态闸的现场）。
pub const NAME_EMPTY: &str = "";

/// 带目录的名字（形态闸的现场：名字里不许有 `/`）。
pub const NAME_SLASH: &str = "ls/zzz";

/// 超长名（形态闸的现场：> NAME_CAP）。
pub const NAME_LONG: &str = "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzabcdefghijkl\
mnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzabcdefghijkl\
mnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzabcdefghijkl\
mnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzabcdefghijkl\
mnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzabcdefghijkl\
mnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz";

// ---------------------------------------------------------------------------
// 十三码
// ---------------------------------------------------------------------------

/// 成败互报（先于一切：绿红说反了后面五维都无从谈起）。
pub const INS_ESTATE: u32 = SEG_BASE + 0x01;

/// 退出码档错（跳过把整趟顶成非零 / 报了 127）。
pub const INS_ECODE: u32 = SEG_BASE + 0x02;

/// 落点闸错（病根在别的闸上）。
pub const INS_ESTAGE: u32 = SEG_BASE + 0x03;

/// 落盘效果数（新建链接数）错。
pub const INS_EVALUE: u32 = SEG_BASE + 0x04;

/// 注解档错（伪链 / 悬空 / 陈链 / 计数漂移档记错）。
pub const INS_ENOTE: u32 = SEG_BASE + 0x05;

/// 兜底锚错（非安装趟被当成结论）。
pub const INS_EDEFAULT: u32 = SEG_BASE + 0x06;

/// "未名必无链"不变量被破坏（伪链 / 陈链）。
pub const INS_ELINK: u32 = SEG_BASE + 0x07;

/// 通道错（跳过行跑到 stdout）。
pub const INS_ECHAN: u32 = SEG_BASE + 0x08;

/// 槽位越界。
pub const INS_ESLOT: u32 = SEG_BASE + 0x09;

/// 指纹为零。
pub const INS_ESEAL: u32 = SEG_BASE + 0x0a;

/// 无观测。
pub const INS_ENOBS: u32 = SEG_BASE + 0x0b;

/// 矩阵不齐。
pub const INS_EMATRIX: u32 = SEG_BASE + 0x0c;

/// 账目不平。
pub const INS_EBAL: u32 = SEG_BASE + 0x0d;

/// 十三码全集（顺序即偏移）。
pub const ALL_CODES: [u32; 13] = [
    INS_ESTATE,
    INS_ECODE,
    INS_ESTAGE,
    INS_EVALUE,
    INS_ENOTE,
    INS_EDEFAULT,
    INS_ELINK,
    INS_ECHAN,
    INS_ESLOT,
    INS_ESEAL,
    INS_ENOBS,
    INS_EMATRIX,
    INS_EBAL,
];

// ---------------------------------------------------------------------------
// 算子号（台账格用：一格一个算子，闸序即算子序）
// ---------------------------------------------------------------------------

/// 算子：闸 1 invoke。
pub const OP_INVOKE: u8 = 0x01;

/// 算子：闸 2 class。
pub const OP_CLASS: u8 = 0x02;

/// 算子：闸 3 decide。
pub const OP_DECIDE: u8 = 0x03;

/// 算子：闸 4 target。
pub const OP_TARGET: u8 = 0x04;

/// 算子：闸 5 emit。
pub const OP_EMIT: u8 = 0x05;

/// 算子：闸 6 code。
pub const OP_CODE: u8 = 0x06;

/// 算子：闸 7 report。
pub const OP_REPORT: u8 = 0x07;

/// 七个统计格各自对应的算子（顺序即闸序）。
pub const STAT_OPS: [u8; 7] = [
    OP_INVOKE,
    OP_CLASS,
    OP_DECIDE,
    OP_TARGET,
    OP_EMIT,
    OP_CODE,
    OP_REPORT,
];

/// 红档统计格的算子号。
pub const OPID_STAT_RED: u8 = 0x08;

/// 缺陷码格算子的基号（六个缺陷格依次 +0..+5）。
pub const DEFECT_OP_BASE: u8 = 0x21;

// ---------------------------------------------------------------------------
// FNV-1a 折叠
// ---------------------------------------------------------------------------

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;

const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折一个字节。
fn fold_byte(h: u64, b: u8) -> u64 {
    let x = h ^ (b as u64);
    x.wrapping_mul(FNV_PRIME)
}

/// 折一个字（八字节逐位，与小端一致）。
fn fold_word(h: u64, w: u64) -> u64 {
    let mut out = h;
    let mut i = 0u32;
    while i < 8 {
        let shift = i * 8;
        let b = ((w >> shift) & 0xff) as u8;
        out = fold_byte(out, b);
        i += 1;
    }
    out
}

/// 折一个 i32（先取补码再折字，负号不会塌成同一条）。
fn fold_i32(h: u64, v: i32) -> u64 {
    fold_word(h, v as u32 as u64)
}

/// 折一个布尔。
fn fold_bool(h: u64, b: bool) -> u64 {
    fold_byte(h, if b { 1u8 } else { 0u8 })
}

/// 折一串文本（逐字节，不用字符串下标）。
fn fold_str(h: u64, s: &str) -> u64 {
    let b = s.as_bytes();
    let mut out = h;
    let mut i = 0usize;
    while i < b.len() {
        if let Some(c) = b.get(i) {
            out = fold_byte(out, *c);
        }
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// 文本助手（全部无分配式拼接，失败一律给空串而不是 panic）
// ---------------------------------------------------------------------------

/// u32 → 十进制文本（不递归）。
fn dec_text(mut v: u32) -> String {
    let mut buf = [0u8; 10];
    let mut n = 10usize;
    loop {
        let d = (v % 10) as u8;
        n = n.saturating_sub(1);
        if let Some(slot) = buf.get_mut(n) {
            *slot = b'0' + d;
        }
        v /= 10;
        if v == 0 {
            break;
        }
    }
    let mut s = String::new();
    let mut i = n;
    while i < 10 {
        if let Some(b) = buf.get(i) {
            s.push(*b as char);
        }
        i += 1;
    }
    s
}

/// u32 → 固定宽度小写十六进制。
fn hex_text(v: u32, width: usize) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::new();
    let mut i = width;
    while i > 0 {
        i = i.saturating_sub(1);
        let shift = (i * 4) as u32;
        if shift >= 32 {
            s.push('0');
            continue;
        }
        let nib = (v >> shift) & 0x0f;
        if let Some(c) = DIGITS.get(nib as usize) {
            s.push(*c as char);
        }
    }
    s
}

/// 段址文本（六位十六进制）。
pub fn seg_text(v: u32) -> String {
    let mut s = String::from("0x");
    s.push_str(hex_text(v, 6).as_str());
    s
}

/// 无符号数文本对外口。
pub fn u32_text(v: u32) -> String {
    dec_text(v)
}

/// 有符号数文本（带负号）。
fn i32_text(v: i32) -> String {
    if v < 0 {
        let mut s = String::from("-");
        s.push_str(dec_text(v.unsigned_abs()).as_str());
        s
    } else {
        dec_text(v as u32)
    }
}

/// 布尔文本。
fn bool_text(v: bool) -> &'static str {
    if v {
        "true"
    } else {
        "false"
    }
}

/// 千分率文本。
pub fn permille_text(v: u32) -> String {
    let mut s = dec_text(v);
    s.push_str("permille");
    s
}

/// 文本行数（按 `\n` 计）。
fn lines_of(s: &str) -> u32 {
    let b = s.as_bytes();
    let mut n = 0u32;
    let mut i = 0usize;
    while i < b.len() {
        if let Some(c) = b.get(i) {
            if *c == b'\n' {
                n = n.saturating_add(1);
            }
        }
        i += 1;
    }
    n
}

/// 文本字节数。
fn bytes_of(s: &str) -> u32 {
    s.as_bytes().len() as u32
}

/// 落点路径文本（`/bin/<name>`，手工拼接，不碰字符串下标）。
pub fn link_path(name: &str) -> String {
    let mut s = String::from(INSTALL_DIR);
    s.push('/');
    s.push_str(name);
    s
}

/// 名字字节数。
fn name_len(name: &str) -> u32 {
    bytes_of(name)
}

/// 名字形态是否可判（空名 / 带 `/` / 含控制字节 / 超长一律不可判）。
fn name_shape_ok(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    if name_len(name) > NAME_CAP {
        return false;
    }
    let b = name.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        let c = match b.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if c == b'/' || c == 0 || c < 0x20 || c == 0x7f {
            return false;
        }
        i += 1;
    }
    true
}

/// 跳过行文本：`<名>: applet not found\n`（逐字归 F11674，本卡只借字节与通道）。
pub fn msg_skip(name: &str) -> String {
    let mut s = String::from(name);
    s.push_str(NOT_FOUND_TAIL);
    s.push('\n');
    s
}

/// 用法错那一行：缺目录，退 1。
pub fn msg_usage() -> String {
    let mut s = String::from(USAGE_LINE);
    s.push('\n');
    s
}

/// 建链失败那一行：`busybox: can't create /bin/<名>: File exists`。
pub fn msg_create(name: &str) -> String {
    let mut s = String::from(CREATE_HEAD);
    s.push_str(link_path(name).as_str());
    s.push_str(CREATE_REASON);
    s.push('\n');
    s
}

/// 伪名静默：一个字都不出。
pub fn msg_quiet() -> String {
    String::new()
}

// ---------------------------------------------------------------------------
// 域模型 ①：调用形态
// ---------------------------------------------------------------------------

/// 一次安装调用的形态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Invoke {
    /// `busybox --install -s DIR`：建软链（本卡的正锚）。
    Soft,
    /// `busybox --install DIR`：建硬链。
    Hard,
    /// `busybox --install -s`：缺目录，用法错。
    NoDir,
    /// `busybox <applet>`：普通运行，不是安装趟（兜底）。
    Plain,
}

impl Invoke {
    /// 形态名。
    pub fn name(self) -> &'static str {
        match self {
            Invoke::Soft => "soft",
            Invoke::Hard => "hard",
            Invoke::NoDir => "no-dir",
            Invoke::Plain => "plain",
        }
    }

    /// 序号。
    pub const fn index(self) -> usize {
        match self {
            Invoke::Soft => 0usize,
            Invoke::Hard => 1usize,
            Invoke::NoDir => 2usize,
            Invoke::Plain => 3usize,
        }
    }

    /// 按序号取；越界给 `None`。
    pub fn at(i: usize) -> Option<Invoke> {
        match i {
            0usize => Some(Invoke::Soft),
            1usize => Some(Invoke::Hard),
            2usize => Some(Invoke::NoDir),
            3usize => Some(Invoke::Plain),
            _ => None,
        }
    }

    /// 全集（顺序=序号）。
    pub fn all() -> [Invoke; INVOKE_COUNT] {
        [Invoke::Soft, Invoke::Hard, Invoke::NoDir, Invoke::Plain]
    }

    /// 是不是安装趟（真正会落盘的两档）。
    pub fn is_install(self) -> bool {
        matches!(self, Invoke::Soft | Invoke::Hard)
    }

    /// 是不是软链安装（本卡的正锚）。
    pub fn is_soft(self) -> bool {
        matches!(self, Invoke::Soft)
    }

    /// 这一形态进不进本卡的闸线（非安装只作兜底锚）。
    pub fn judged(self) -> bool {
        !matches!(self, Invoke::Plain)
    }

    /// 这一形态有没有目录可落（缺目录档没有）。
    pub fn has_dir(self) -> bool {
        !matches!(self, Invoke::NoDir)
    }
}

// ---------------------------------------------------------------------------
// 域模型 ②：名字身份
// ---------------------------------------------------------------------------

/// 一个名字在 applet 表里的身份。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Class {
    /// 实名：表里有，装链。
    Real,
    /// 别名：表里有（指向规范名），装链（收敛归 F11671）。
    Alias,
    /// 伪名：`busybox` / `--list`，在表里但自己就是分发器，永不装链。
    Pseudo,
    /// 未名：不在表里，跳过。
    NoSuch,
}

impl Class {
    /// 身份名。
    pub fn name(self) -> &'static str {
        match self {
            Class::Real => "real",
            Class::Alias => "alias",
            Class::Pseudo => "pseudo",
            Class::NoSuch => "nosuch",
        }
    }

    /// 序号。
    pub const fn index(self) -> usize {
        match self {
            Class::Real => 0usize,
            Class::Alias => 1usize,
            Class::Pseudo => 2usize,
            Class::NoSuch => 3usize,
        }
    }

    /// 按序号取；越界给 `None`。
    pub fn at(i: usize) -> Option<Class> {
        match i {
            0usize => Some(Class::Real),
            1usize => Some(Class::Alias),
            2usize => Some(Class::Pseudo),
            3usize => Some(Class::NoSuch),
            _ => None,
        }
    }

    /// 全集。
    pub fn all() -> [Class; CLASS_COUNT] {
        [Class::Real, Class::Alias, Class::Pseudo, Class::NoSuch]
    }

    /// 在表里（含伪名）。
    pub fn known(self) -> bool {
        !matches!(self, Class::NoSuch)
    }

    /// 该不该建链（伪名不该：它自己就是分发器）。
    pub fn installs(self) -> bool {
        matches!(self, Class::Real | Class::Alias)
    }

    /// 该不该留一行跳过日志。
    pub fn speaks(self) -> bool {
        matches!(self, Class::NoSuch)
    }

    /// 该不该安静地跳过。
    pub fn quiet(self) -> bool {
        matches!(self, Class::Pseudo)
    }
}

// ---------------------------------------------------------------------------
// 域模型 ③：链接状态
// ---------------------------------------------------------------------------

/// 一条清单行走完之后，落点上的链接状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LinkState {
    /// 本行新建了链，目标是真的 busybox。
    Created,
    /// 链本已就位（重跑 / 重名第二趟），本行没建。
    Reused,
    /// 落点上没有链。
    Absent,
    /// 有链，但目标不是真 applet（悬空）。
    Dangling,
    /// 有链，是上一趟留下的旧链（陈链）。
    Stale,
}

impl LinkState {
    /// 状态名。
    pub fn name(self) -> &'static str {
        match self {
            LinkState::Created => "created",
            LinkState::Reused => "reused",
            LinkState::Absent => "absent",
            LinkState::Dangling => "dangling",
            LinkState::Stale => "stale",
        }
    }

    /// 序号。
    pub const fn index(self) -> usize {
        match self {
            LinkState::Created => 0usize,
            LinkState::Reused => 1usize,
            LinkState::Absent => 2usize,
            LinkState::Dangling => 3usize,
            LinkState::Stale => 4usize,
        }
    }

    /// 按序号取；越界给 `None`。
    pub fn at(i: usize) -> Option<LinkState> {
        match i {
            0usize => Some(LinkState::Created),
            1usize => Some(LinkState::Reused),
            2usize => Some(LinkState::Absent),
            3usize => Some(LinkState::Dangling),
            4usize => Some(LinkState::Stale),
            _ => None,
        }
    }

    /// 全集。
    pub fn all() -> [LinkState; LINK_COUNT] {
        [
            LinkState::Created,
            LinkState::Reused,
            LinkState::Absent,
            LinkState::Dangling,
            LinkState::Stale,
        ]
    }

    /// 落点上有没有链（不管目标对不对）。
    pub fn exists(self) -> bool {
        !matches!(self, LinkState::Absent)
    }

    /// 本行新建的链接数（0 或 1）——这是 `Outcome.value` 的唯一来源。
    pub fn effect(self) -> u32 {
        match self {
            LinkState::Created => 1u32,
            _ => 0u32,
        }
    }

    /// 链目标是不是真 applet（只有 Created / Reused 是）。
    pub fn target_real(self) -> bool {
        matches!(self, LinkState::Created | LinkState::Reused)
    }

    /// 是不是"本趟建出来的"（未名条目上出现即为伪链）。
    pub fn built_now(self) -> bool {
        matches!(self, LinkState::Created | LinkState::Dangling)
    }
}

// ---------------------------------------------------------------------------
// 域模型 ④：日志通道
// ---------------------------------------------------------------------------

/// 跳过行走哪条流。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LineRoute {
    /// 标准错误（唯一合法通道）。
    Stderr,
    /// 标准输出（跳过行跑这里就是通道误读）。
    Stdout,
}

impl LineRoute {
    /// 通道名。
    pub fn name(self) -> &'static str {
        match self {
            LineRoute::Stderr => "stderr",
            LineRoute::Stdout => "stdout",
        }
    }

    /// 序号。
    pub const fn index(self) -> usize {
        match self {
            LineRoute::Stderr => 0usize,
            LineRoute::Stdout => 1usize,
        }
    }

    /// 全集。
    pub fn all() -> [LineRoute; 2] {
        [LineRoute::Stderr, LineRoute::Stdout]
    }

    /// 是不是诊断该走的流。
    pub fn is_diag(self) -> bool {
        matches!(self, LineRoute::Stderr)
    }
}

// ---------------------------------------------------------------------------
// 域模型 ⑤：缺陷（八种真实构建病）
// ---------------------------------------------------------------------------

/// 本卡建模的八种缺陷，外加"无病"。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Defect {
    /// 无病。
    None,
    /// 未名条目建了伪链（Decide 闸）。
    FakeLink,
    /// 跳过一次把整趟退出码顶成非零（Code 闸）。
    SkipExit,
    /// 跳过行跑到 stdout（Emit 闸）。
    Misroute,
    /// 整趟报了 127（Code 闸）。
    Code127,
    /// 已知条目的链指向不存在的 applet（Target 闸）。
    Dangling,
    /// 重名第二趟被静默吞掉（Decide 闸）。
    Duplicate,
    /// 跳过计数与清单对不上（Report 闸）。
    SkipCount,
    /// 陈链残留，破坏"未名必无链"（Target 闸）。
    StaleLink,
}

impl Defect {
    /// 病名。
    pub fn name(self) -> &'static str {
        match self {
            Defect::None => "none",
            Defect::FakeLink => "fake-link",
            Defect::SkipExit => "skip-exit",
            Defect::Misroute => "misroute",
            Defect::Code127 => "code-127",
            Defect::Dangling => "dangling",
            Defect::Duplicate => "duplicate",
            Defect::SkipCount => "skip-count",
            Defect::StaleLink => "stale-link",
        }
    }

    /// 序号。
    pub const fn index(self) -> usize {
        match self {
            Defect::None => 0usize,
            Defect::FakeLink => 1usize,
            Defect::SkipExit => 2usize,
            Defect::Misroute => 3usize,
            Defect::Code127 => 4usize,
            Defect::Dangling => 5usize,
            Defect::Duplicate => 6usize,
            Defect::SkipCount => 7usize,
            Defect::StaleLink => 8usize,
        }
    }

    /// 按序号取；越界给 `None`。
    pub fn at(i: usize) -> Option<Defect> {
        match i {
            0usize => Some(Defect::None),
            1usize => Some(Defect::FakeLink),
            2usize => Some(Defect::SkipExit),
            3usize => Some(Defect::Misroute),
            4usize => Some(Defect::Code127),
            5usize => Some(Defect::Dangling),
            6usize => Some(Defect::Duplicate),
            7usize => Some(Defect::SkipCount),
            8usize => Some(Defect::StaleLink),
            _ => None,
        }
    }

    /// 全集。
    pub fn all() -> [Defect; DEFECT_COUNT] {
        [
            Defect::None,
            Defect::FakeLink,
            Defect::SkipExit,
            Defect::Misroute,
            Defect::Code127,
            Defect::Dangling,
            Defect::Duplicate,
            Defect::SkipCount,
            Defect::StaleLink,
        ]
    }

    /// 是不是病（`None` 不是）。
    pub fn is_fault(self) -> bool {
        !matches!(self, Defect::None)
    }

    /// 破坏的是不是"未名必无链"这条不变量。
    pub fn breaks_link(self) -> bool {
        matches!(
            self,
            Defect::FakeLink | Defect::Dangling | Defect::StaleLink
        )
    }

    /// 病根落在码档上。
    pub fn is_code(self) -> bool {
        matches!(self, Defect::SkipExit | Defect::Code127)
    }

    /// 病根落在"说没说 / 说在哪"上。
    pub fn is_emit(self) -> bool {
        matches!(self, Defect::Misroute)
    }
}

// ---------------------------------------------------------------------------
// 域模型 ⑥：注解档
// ---------------------------------------------------------------------------

/// 一条清单行的注解档（本卡的"这一格长成什么样"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Note {
    /// 实名装上了（新建一条链）。
    LinkMade,
    /// 别名收敛后装上了。
    AliasLink,
    /// 链已就位（重跑 / 重名第二趟），本行没建。
    AlreadyLinked,
    /// 未名：跳过并留一行 stderr。
    SkipLogged,
    /// 伪名：不装也不报。
    PseudoQuiet,
    /// 未名却建了链（伪链）。
    FakeLink,
    /// 跳过一次把整趟退出码顶成非零。
    ExitBumped,
    /// 跳过行跑到 stdout。
    LineOnStdout,
    /// 链目标不是真 applet（悬空 / 缺链 / 指旧目标）。
    TargetDangling,
    /// 重名第二趟被静默吞掉。
    DupCollapsed,
    /// 跳过计数与清单对不上。
    CountDrift,
    /// 陈链残留。
    StaleLeft,
    /// 整趟报了 127。
    PassCode127,
    /// `--install` 缺目录（用法错）。
    UsageNoDir,
    /// 名字形态不可判。
    BadName,
    /// 不是安装趟（兜底）。
    NotInstall,
}

impl Note {
    /// 档名。
    pub fn name(self) -> &'static str {
        match self {
            Note::LinkMade => "link-made",
            Note::AliasLink => "alias-link",
            Note::AlreadyLinked => "already-linked",
            Note::SkipLogged => "skip-logged",
            Note::PseudoQuiet => "pseudo-quiet",
            Note::FakeLink => "fake-link",
            Note::ExitBumped => "exit-bumped",
            Note::LineOnStdout => "line-on-stdout",
            Note::TargetDangling => "target-dangling",
            Note::DupCollapsed => "dup-collapsed",
            Note::CountDrift => "count-drift",
            Note::StaleLeft => "stale-left",
            Note::PassCode127 => "pass-code-127",
            Note::UsageNoDir => "usage-no-dir",
            Note::BadName => "bad-name",
            Note::NotInstall => "not-install",
        }
    }

    /// 序号。
    pub const fn index(self) -> usize {
        match self {
            Note::LinkMade => 0usize,
            Note::AliasLink => 1usize,
            Note::AlreadyLinked => 2usize,
            Note::SkipLogged => 3usize,
            Note::PseudoQuiet => 4usize,
            Note::FakeLink => 5usize,
            Note::ExitBumped => 6usize,
            Note::LineOnStdout => 7usize,
            Note::TargetDangling => 8usize,
            Note::DupCollapsed => 9usize,
            Note::CountDrift => 10usize,
            Note::StaleLeft => 11usize,
            Note::PassCode127 => 12usize,
            Note::UsageNoDir => 13usize,
            Note::BadName => 14usize,
            Note::NotInstall => 15usize,
        }
    }

    /// 全集。
    pub fn all() -> [Note; NOTE_COUNT] {
        [
            Note::LinkMade,
            Note::AliasLink,
            Note::AlreadyLinked,
            Note::SkipLogged,
            Note::PseudoQuiet,
            Note::FakeLink,
            Note::ExitBumped,
            Note::LineOnStdout,
            Note::TargetDangling,
            Note::DupCollapsed,
            Note::CountDrift,
            Note::StaleLeft,
            Note::PassCode127,
            Note::UsageNoDir,
            Note::BadName,
            Note::NotInstall,
        ]
    }

    /// 档数。
    pub const COUNT: usize = NOTE_COUNT;

    /// 这一档是不是"本趟做对了"。
    pub fn live(self) -> bool {
        matches!(
            self,
            Note::LinkMade
                | Note::AliasLink
                | Note::AlreadyLinked
                | Note::SkipLogged
                | Note::PseudoQuiet
        )
    }

    /// 这一档是不是八种病之一。
    pub fn defect(self) -> bool {
        matches!(
            self,
            Note::FakeLink
                | Note::ExitBumped
                | Note::LineOnStdout
                | Note::TargetDangling
                | Note::DupCollapsed
                | Note::CountDrift
                | Note::StaleLeft
                | Note::PassCode127
        )
    }

    /// 这一档是不是兜底（没有结论）。
    pub fn defaulted(self) -> bool {
        matches!(self, Note::NotInstall)
    }

    /// 这一档有没有一行可对账的正文。
    pub fn has_line(self) -> bool {
        matches!(
            self,
            Note::SkipLogged | Note::LineOnStdout | Note::PassCode127 | Note::UsageNoDir
        )
    }

    /// 这一档该不该带链（做对的实名/别名才带）。
    pub fn links(self) -> bool {
        matches!(self, Note::LinkMade | Note::AliasLink)
    }

    /// 这一档落在"未名必无链"的未名侧。
    pub fn on_unknown_side(self) -> bool {
        matches!(
            self,
            Note::SkipLogged
                | Note::PseudoQuiet
                | Note::FakeLink
                | Note::LineOnStdout
                | Note::StaleLeft
                | Note::PassCode127
                | Note::CountDrift
                | Note::ExitBumped
        )
    }

    /// 这一档是不是"跳过的整趟仍绿"（本卡的正锚）。
    pub fn skip_green(self) -> bool {
        matches!(self, Note::SkipLogged | Note::PseudoQuiet)
    }
}

// ---------------------------------------------------------------------------
// 域模型 ⑦：四档
// ---------------------------------------------------------------------------

/// 四档：按**实际结论**归档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    /// 一档：本行建链成功（实名/别名装上）。
    Linked,
    /// 二档：本行没动盘面（跳过 / 伪名 / 已就位）。
    Untouched,
    /// 三档：本行有病（八种病之一）。
    Faulted,
    /// 四档：兜底（不是安装趟，没有结论）。
    Fallback,
}

impl Tier {
    /// 档名。
    pub fn name(self) -> &'static str {
        match self {
            Tier::Linked => "linked",
            Tier::Untouched => "untouched",
            Tier::Faulted => "faulted",
            Tier::Fallback => "fallback",
        }
    }

    /// 序号。
    pub const fn index(self) -> usize {
        match self {
            Tier::Linked => 0usize,
            Tier::Untouched => 1usize,
            Tier::Faulted => 2usize,
            Tier::Fallback => 3usize,
        }
    }

    /// 全表。
    pub fn all() -> [Tier; TIER_COUNT] {
        [Tier::Linked, Tier::Untouched, Tier::Faulted, Tier::Fallback]
    }

    /// 该档是否算"落盘面有正当结果"。
    pub fn has_effect(self) -> bool {
        matches!(self, Tier::Linked)
    }

    /// 该档是否"没病"。
    pub fn clean(self) -> bool {
        matches!(self, Tier::Linked | Tier::Untouched)
    }

    /// 由一次结果归档；四档互斥且全覆盖。
    pub fn of(o: &Outcome) -> Tier {
        if !o.consistent {
            return Tier::Faulted;
        }
        if o.default {
            return Tier::Fallback;
        }
        if !o.note.live() {
            return Tier::Faulted;
        }
        if o.note.links() {
            return Tier::Linked;
        }
        Tier::Untouched
    }
}

// ---------------------------------------------------------------------------
// 域模型 ⑧：阶段
// ---------------------------------------------------------------------------

/// 本卡七道闸，秩从 1 起。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// 闸 1：调用形态（是不是安装趟）。
    Invoke,
    /// 闸 2：名字身份与形态。
    Class,
    /// 闸 3：要不要落盘（本卡的核心闸）。
    Decide,
    /// 闸 4：落点上的真相（有链 / 悬空 / 陈链）。
    Target,
    /// 闸 5：说没说、说在哪。
    Emit,
    /// 闸 6：整趟退出码。
    Code,
    /// 闸 7：收账与封账。
    Report,
}

impl Stage {
    /// 秩（1..7）。
    pub const fn rank(self) -> u32 {
        match self {
            Stage::Invoke => 1u32,
            Stage::Class => 2u32,
            Stage::Decide => 3u32,
            Stage::Target => 4u32,
            Stage::Emit => 5u32,
            Stage::Code => 6u32,
            Stage::Report => 7u32,
        }
    }

    /// 小写阶段名。
    pub fn name(self) -> &'static str {
        match self {
            Stage::Invoke => "invoke",
            Stage::Class => "class",
            Stage::Decide => "decide",
            Stage::Target => "target",
            Stage::Emit => "emit",
            Stage::Code => "code",
            Stage::Report => "report",
        }
    }

    /// 按秩取阶段；越界给 `None`。
    pub fn from_rank(r: u32) -> Option<Stage> {
        match r {
            1u32 => Some(Stage::Invoke),
            2u32 => Some(Stage::Class),
            3u32 => Some(Stage::Decide),
            4u32 => Some(Stage::Target),
            5u32 => Some(Stage::Emit),
            6u32 => Some(Stage::Code),
            7u32 => Some(Stage::Report),
            _ => None,
        }
    }

    /// 全集（按闸序）。
    pub fn all() -> [Stage; 7] {
        [
            Stage::Invoke,
            Stage::Class,
            Stage::Decide,
            Stage::Target,
            Stage::Emit,
            Stage::Code,
            Stage::Report,
        ]
    }

    /// 这一闸是否还没谈落盘面。
    pub fn before_disk(self) -> bool {
        matches!(self, Stage::Invoke | Stage::Class | Stage::Decide)
    }

    /// 这一闸是否还要谈"说"。
    pub fn after_disk(self) -> bool {
        matches!(self, Stage::Emit | Stage::Code | Stage::Report)
    }
}

// ---------------------------------------------------------------------------
// 域模型 ⑨：缺陷维（判序 Estate → Stage → Code → Value → Note → Default → Ok）
// ---------------------------------------------------------------------------

/// 一格的缺陷维。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fault {
    /// 全对。
    Ok,
    /// 成败互报。
    Estate,
    /// 落点闸错。
    Stage,
    /// 退出码档错。
    Code,
    /// 落盘效果数错。
    Value,
    /// 注解档错。
    Note,
    /// 兜底锚错。
    Default,
}

impl Fault {
    /// 本维错误码（`Ok` 给 0）。
    pub const fn code(self) -> u32 {
        match self {
            Fault::Ok => 0u32,
            Fault::Estate => INS_ESTATE,
            Fault::Stage => INS_ESTAGE,
            Fault::Code => INS_ECODE,
            Fault::Value => INS_EVALUE,
            Fault::Note => INS_ENOTE,
            Fault::Default => INS_EDEFAULT,
        }
    }

    /// 维名。
    pub fn name(self) -> &'static str {
        match self {
            Fault::Ok => "ok",
            Fault::Estate => "estate",
            Fault::Stage => "stage",
            Fault::Code => "code",
            Fault::Value => "value",
            Fault::Note => "note",
            Fault::Default => "default",
        }
    }

    /// 判序位次（`Ok` 排最后）。
    pub const fn order(self) -> usize {
        match self {
            Fault::Estate => 0usize,
            Fault::Stage => 1usize,
            Fault::Code => 2usize,
            Fault::Value => 3usize,
            Fault::Note => 4usize,
            Fault::Default => 5usize,
            Fault::Ok => 6usize,
        }
    }
}

/// 短名（排障行用）。
pub fn fault_short(f: Fault) -> &'static str {
    match f {
        Fault::Ok => "OK",
        Fault::Estate => "ESTATE",
        Fault::Stage => "ESTAGE",
        Fault::Code => "ECODE",
        Fault::Value => "EVALUE",
        Fault::Note => "ENOTE",
        Fault::Default => "EDEFAUL",
    }
}

/// 判序全集（六维顺序固定）。
pub const REJECT_FAULTS: [Fault; REJECT] = [
    Fault::Estate,
    Fault::Stage,
    Fault::Code,
    Fault::Value,
    Fault::Note,
    Fault::Default,
];

/// 六个缺陷码（与判序一一对应，两两互异）。
pub const REJECT_CODES: [u32; REJECT] = [
    INS_ESTATE,
    INS_ESTAGE,
    INS_ECODE,
    INS_EVALUE,
    INS_ENOTE,
    INS_EDEFAULT,
];

// ---------------------------------------------------------------------------
// 文案表：六格逐字快照（`{N}` 是名字的位置）
// ---------------------------------------------------------------------------

/// 安装趟里可能出现的六种日志行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MsgKind {
    /// 跳过行（未名，stderr，整趟仍 0）——本卡的正锚。
    SkipOnStderr,
    /// 同一串字节跑到 stdout（通道误读）。
    SkipOnStdout,
    /// 同一串字节出现在直接执行路径上（退 127，归 F11664/F11674）。
    NotFoundExec,
    /// 建链失败行（同名占位，归 F11584 的冲突面）。
    CreateFail,
    /// `--install` 缺目录的用法行。
    Usage,
    /// 伪名：一个字都不出。
    QuietPseudo,
}

impl MsgKind {
    /// 格名。
    pub fn name(self) -> &'static str {
        match self {
            MsgKind::SkipOnStderr => "skip-stderr",
            MsgKind::SkipOnStdout => "skip-stdout",
            MsgKind::NotFoundExec => "notfound-exec",
            MsgKind::CreateFail => "create-fail",
            MsgKind::Usage => "usage",
            MsgKind::QuietPseudo => "quiet-pseudo",
        }
    }

    /// 序号。
    pub const fn index(self) -> usize {
        match self {
            MsgKind::SkipOnStderr => 0usize,
            MsgKind::SkipOnStdout => 1usize,
            MsgKind::NotFoundExec => 2usize,
            MsgKind::CreateFail => 3usize,
            MsgKind::Usage => 4usize,
            MsgKind::QuietPseudo => 5usize,
        }
    }

    /// 按序号取；越界给 `None`。
    pub fn at(i: usize) -> Option<MsgKind> {
        match i {
            0usize => Some(MsgKind::SkipOnStderr),
            1usize => Some(MsgKind::SkipOnStdout),
            2usize => Some(MsgKind::NotFoundExec),
            3usize => Some(MsgKind::CreateFail),
            4usize => Some(MsgKind::Usage),
            5usize => Some(MsgKind::QuietPseudo),
            _ => None,
        }
    }

    /// 全集。
    pub fn all() -> [MsgKind; MESSAGE_CELLS] {
        [
            MsgKind::SkipOnStderr,
            MsgKind::SkipOnStdout,
            MsgKind::NotFoundExec,
            MsgKind::CreateFail,
            MsgKind::Usage,
            MsgKind::QuietPseudo,
        ]
    }
}

/// 一格文案快照：种类 + 通道 + 模板 + 该格的退出码。
#[derive(Clone, Copy)]
pub struct MsgCell {
    /// 哪一格。
    pub kind: MsgKind,
    /// 落在哪条流。
    pub route: LineRoute,
    /// 模板（`{N}` 待填）。
    pub tpl: &'static str,
    /// 这一格的退出码。
    pub code: u32,
}

/// 兜底格（取不到时用来判错的空快照，绝不冒充真快照）。
pub const CELL_NONE: MsgCell = MsgCell {
    kind: MsgKind::QuietPseudo,
    route: LineRoute::Stderr,
    tpl: "",
    code: EXIT_OK,
};

/// 六格文案表。
pub const MSG_TABLE: [MsgCell; MESSAGE_CELLS] = [
    // 跳过行：本卡的正锚。字节与 F11674 的直接执行行一模一样，只有码档不同。
    MsgCell {
        kind: MsgKind::SkipOnStderr,
        route: LineRoute::Stderr,
        tpl: "{N}: applet not found\n",
        code: EXIT_OK,
    },
    // 同一行字跑到 stdout：字节不变，通道错了。
    MsgCell {
        kind: MsgKind::SkipOnStdout,
        route: LineRoute::Stdout,
        tpl: "{N}: applet not found\n",
        code: EXIT_OK,
    },
    // 同一行字在直接执行路径上：退 127。
    MsgCell {
        kind: MsgKind::NotFoundExec,
        route: LineRoute::Stderr,
        tpl: "{N}: applet not found\n",
        code: EXIT_NOTFOUND,
    },
    // 建链失败行：同名占位（本卡不判它的对错，只判它不许出现在跳过档上）。
    MsgCell {
        kind: MsgKind::CreateFail,
        route: LineRoute::Stderr,
        tpl: "busybox: can't create /bin/{N}: File exists\n",
        code: EXIT_USAGE,
    },
    // 用法行：缺目录。
    MsgCell {
        kind: MsgKind::Usage,
        route: LineRoute::Stderr,
        tpl: "busybox: --install: no directory specified\n",
        code: EXIT_USAGE,
    },
    // 伪名：不出字。
    MsgCell {
        kind: MsgKind::QuietPseudo,
        route: LineRoute::Stderr,
        tpl: "",
        code: EXIT_OK,
    },
];

/// 取一格文案快照（逐格扫表，不用下标）。
pub fn msg_cell(k: MsgKind) -> Option<&'static MsgCell> {
    let mut i = 0usize;
    while i < MESSAGE_CELLS {
        if let Some(c) = MSG_TABLE.get(i) {
            if c.kind == k {
                return Some(c);
            }
        }
        i += 1;
    }
    None
}

/// 把模板里的 `{N}` 换成真名（逐段拼接，不碰字符串下标）。
pub fn subst(tpl: &str, name: &str) -> String {
    let mut out = String::new();
    let mut first = true;
    for part in tpl.split("{N}") {
        if !first {
            out.push_str(name);
        }
        out.push_str(part);
        first = false;
    }
    out
}

/// 这一格的实际文本（取不到格给空串）。
pub fn msg_text(k: MsgKind, name: &str) -> String {
    match msg_cell(k) {
        Some(c) => subst(c.tpl, name),
        None => String::new(),
    }
}

/// 这一格的退出码（取不到格给 0）。
pub fn msg_code(k: MsgKind) -> u32 {
    match msg_cell(k) {
        Some(c) => c.code,
        None => EXIT_OK,
    }
}

/// 这一格的通道（取不到格给 stderr）。
pub fn msg_route(k: MsgKind) -> LineRoute {
    match msg_cell(k) {
        Some(c) => c.route,
        None => LineRoute::Stderr,
    }
}

/// 同一串字节、两个码档的现场（本卡存在的理由）。
pub fn same_bytes_two_codes(name: &str) -> bool {
    let skip = msg_text(MsgKind::SkipOnStderr, name);
    let exec = msg_text(MsgKind::NotFoundExec, name);
    bytes_of(skip.as_str()) == bytes_of(exec.as_str())
        && skip.as_str() == exec.as_str()
        && msg_code(MsgKind::SkipOnStderr) == EXIT_OK
        && msg_code(MsgKind::NotFoundExec) == EXIT_NOTFOUND
}

// ---------------------------------------------------------------------------
// 安装清单：14 行 = 实名 8 + 别名 1 + 伪名 2 + 未名 3（含一行重名）
// ---------------------------------------------------------------------------

/// 清单里的一行：装完之后这一格该长成什么样。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Entry {
    /// 名字。
    pub name: &'static str,
    /// 身份。
    pub class: Class,
    /// 在不在表里（与身份互校用）。
    pub known: bool,
    /// 本行新建了链没有。
    pub link_created: bool,
    /// 本行走完之后落点上有链没有（重名行"有但没有新建"）。
    pub link_present: bool,
    /// 整趟退出码（跳过一个未名仍是 0）。
    pub exit: u32,
    /// 本行落了多少字节到 stderr。
    pub stderr_bytes: u32,
    /// 会不会留下一条悬空链（正确行为恒 false）。
    pub dangling: bool,
}

impl Entry {
    /// 空行（格式占位，绝不冒充真行）。
    pub const fn blank() -> Entry {
        Entry {
            name: NAME_EMPTY,
            class: Class::NoSuch,
            known: false,
            link_created: false,
            link_present: false,
            exit: EXIT_OK,
            stderr_bytes: 0,
            dangling: false,
        }
    }

    /// 这一行是不是"跳过"（没建链、留了一行）。
    pub fn skipped(&self) -> bool {
        self.class.speaks() && !self.link_created && self.stderr_bytes > 0
    }

    /// 这一行是不是"伪名静默跳过"。
    pub fn quiet(&self) -> bool {
        self.class.quiet() && !self.link_created && self.stderr_bytes == 0
    }

    /// 这一行是不是"装上了"。
    pub fn installed(&self) -> bool {
        self.class.installs() && self.link_created && self.link_present
    }

    /// 这一行是不是"已就位"（本行没建但链在）。
    pub fn reused(&self) -> bool {
        self.class.installs() && !self.link_created && self.link_present
    }

    /// 这一行的指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_str(h, self.name);
        h = fold_word(h, self.class.index() as u64);
        h = fold_bool(h, self.known);
        h = fold_bool(h, self.link_created);
        h = fold_bool(h, self.link_present);
        h = fold_word(h, self.exit as u64);
        h = fold_word(h, self.stderr_bytes as u64);
        fold_bool(h, self.dangling)
    }

    /// 呈现一行。
    pub fn render(&self) -> String {
        let mut s = String::from("entry ");
        s.push_str(self.name);
        s.push_str(" class=");
        s.push_str(self.class.name());
        s.push_str(" link=");
        s.push_str(bool_text(self.link_created));
        s.push_str(" exit=");
        s.push_str(dec_text(self.exit).as_str());
        s.push_str(" bytes=");
        s.push_str(dec_text(self.stderr_bytes).as_str());
        s.push_str(" dangling=");
        s.push_str(bool_text(self.dangling));
        s
    }
}

/// 十四行安装清单（`busybox --install -s /bin` 的正确落盘账）。
pub const LIST: [Entry; LIST_LEN] = [
    Entry {
        name: "ls",
        class: Class::Real,
        known: true,
        link_created: true,
        link_present: true,
        exit: EXIT_OK,
        stderr_bytes: 0,
        dangling: false,
    },
    Entry {
        name: "cp",
        class: Class::Real,
        known: true,
        link_created: true,
        link_present: true,
        exit: EXIT_OK,
        stderr_bytes: 0,
        dangling: false,
    },
    Entry {
        name: "mv",
        class: Class::Real,
        known: true,
        link_created: true,
        link_present: true,
        exit: EXIT_OK,
        stderr_bytes: 0,
        dangling: false,
    },
    Entry {
        name: "rm",
        class: Class::Real,
        known: true,
        link_created: true,
        link_present: true,
        exit: EXIT_OK,
        stderr_bytes: 0,
        dangling: false,
    },
    Entry {
        name: "cat",
        class: Class::Real,
        known: true,
        link_created: true,
        link_present: true,
        exit: EXIT_OK,
        stderr_bytes: 0,
        dangling: false,
    },
    // 别名：表里在，链装上去之后 argv[0] 回派靠 F11671 的收敛。
    Entry {
        name: "zcat",
        class: Class::Alias,
        known: true,
        link_created: true,
        link_present: true,
        exit: EXIT_OK,
        stderr_bytes: 0,
        dangling: false,
    },
    // 伪名之一：分发器自己，永不装。
    Entry {
        name: MULTICALL_NAME,
        class: Class::Pseudo,
        known: true,
        link_created: false,
        link_present: false,
        exit: EXIT_OK,
        stderr_bytes: 0,
        dangling: false,
    },
    Entry {
        name: "grep",
        class: Class::Real,
        known: true,
        link_created: true,
        link_present: true,
        exit: EXIT_OK,
        stderr_bytes: 0,
        dangling: false,
    },
    // 未名一号：跳过，留一行 22 字节，不建链，整趟仍 0。
    Entry {
        name: NOSUCH_ONE,
        class: Class::NoSuch,
        known: false,
        link_created: false,
        link_present: false,
        exit: EXIT_OK,
        stderr_bytes: 22,
        dangling: false,
    },
    // 未名二号。
    Entry {
        name: NOSUCH_TWO,
        class: Class::NoSuch,
        known: false,
        link_created: false,
        link_present: false,
        exit: EXIT_OK,
        stderr_bytes: 22,
        dangling: false,
    },
    // 未名三号（带连字符，字节数最长）。
    Entry {
        name: NOSUCH_DASH,
        class: Class::NoSuch,
        known: false,
        link_created: false,
        link_present: false,
        exit: EXIT_OK,
        stderr_bytes: 25,
        dangling: false,
    },
    // 重名的第二趟：链已在，本行只判"已就位"，不重造也不静默吞。
    Entry {
        name: "cp",
        class: Class::Real,
        known: true,
        link_created: false,
        link_present: true,
        exit: EXIT_OK,
        stderr_bytes: 0,
        dangling: false,
    },
    // 伪名之二：`--list`。
    Entry {
        name: LIST_PSEUDO,
        class: Class::Pseudo,
        known: true,
        link_created: false,
        link_present: false,
        exit: EXIT_OK,
        stderr_bytes: 0,
        dangling: false,
    },
    Entry {
        name: "sed",
        class: Class::Real,
        known: true,
        link_created: true,
        link_present: true,
        exit: EXIT_OK,
        stderr_bytes: 0,
        dangling: false,
    },
];

/// 取清单一行；越界给 `None`。
pub fn entry_at(i: usize) -> Option<Entry> {
    match LIST.get(i) {
        Some(e) => Some(*e),
        None => None,
    }
}

/// 清单里新建链的行数。
pub fn created_rows() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < LIST_LEN {
        if let Some(e) = LIST.get(i) {
            if e.link_created {
                n = n.saturating_add(1);
            }
        }
        i += 1;
    }
    n
}

/// 清单里落点有链的行数（含"已就位"那一行）。
pub fn present_rows() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < LIST_LEN {
        if let Some(e) = LIST.get(i) {
            if e.link_present {
                n = n.saturating_add(1);
            }
        }
        i += 1;
    }
    n
}

/// 清单里跳过的行数。
pub fn skip_rows() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < LIST_LEN {
        if let Some(e) = LIST.get(i) {
            if e.class.speaks() {
                n = n.saturating_add(1);
            }
        }
        i += 1;
    }
    n
}

/// 清单里伪名的行数。
pub fn pseudo_rows() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < LIST_LEN {
        if let Some(e) = LIST.get(i) {
            if e.class.quiet() {
                n = n.saturating_add(1);
            }
        }
        i += 1;
    }
    n
}

/// 清单里"已就位"的行数。
pub fn reused_rows() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < LIST_LEN {
        if let Some(e) = LIST.get(i) {
            if e.reused() {
                n = n.saturating_add(1);
            }
        }
        i += 1;
    }
    n
}

/// 清单里 stderr 的总字节数（三条跳过行之和）。
pub fn skip_bytes_total() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < LIST_LEN {
        if let Some(e) = LIST.get(i) {
            n = n.saturating_add(e.stderr_bytes);
        }
        i += 1;
    }
    n
}

// ---------------------------------------------------------------------------
// Shape：一次安装趟里"这一条"能读到的全部维度
// ---------------------------------------------------------------------------

/// 一格输入形状。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Shape {
    /// 调用形态。
    pub invoke: Invoke,
    /// 名字身份。
    pub class: Class,
    /// 名字（原样，不预先清洗）。
    pub name: &'static str,
    /// 落点上的链接状态。
    pub link: LinkState,
    /// 跳过行落在哪条流。
    pub line: LineRoute,
    /// 采集记下的整趟退出码。
    pub exit_declared: u32,
    /// 汇总里申报的跳过条数。
    pub skip_declared: u32,
    /// 这一行是不是清单里的第二次出现。
    pub dup: bool,
    /// 本趟清单的行数（必须等于 LIST_LEN）。
    pub list_len: u32,
}

impl Shape {
    /// 基型：`--install -s /bin`，实名 `ls`，新建链，退 0。
    pub const fn blank() -> Shape {
        Shape {
            invoke: Invoke::Soft,
            class: Class::Real,
            name: "ls",
            link: LinkState::Created,
            line: LineRoute::Stderr,
            exit_declared: EXIT_OK,
            skip_declared: 0,
            dup: false,
            list_len: LIST_LEN as u32,
        }
    }

    /// 形状整体是否自洽。不自洽只把 `consistent` 压假，不改变落点。
    pub fn valid(&self) -> bool {
        if self.exit_declared > EXIT_NOTFOUND {
            return false;
        }
        if self.list_len != LIST_LEN as u32 {
            return false;
        }
        if self.skip_declared > LIST_LEN as u32 {
            return false;
        }
        // 只有未名条目会开口；别的身份申报"跳过 N 条"就是谎报。
        if !self.class.speaks() && self.skip_declared != 0 {
            return false;
        }
        // 只有未名条目有"跳过行走哪条流"这回事。
        if !self.class.speaks() && self.line != LineRoute::Stderr {
            return false;
        }
        // 重名只可能是实名/别名的第二次出现。
        if self.dup && !self.class.installs() {
            return false;
        }
        true
    }

    /// 只换调用形态。
    pub fn with_invoke(&self, v: Invoke) -> Shape {
        let mut s = *self;
        s.invoke = v;
        s
    }

    /// 只换身份。
    pub fn with_class(&self, c: Class) -> Shape {
        let mut s = *self;
        s.class = c;
        s
    }

    /// 只换名字。
    pub fn with_name(&self, n: &'static str) -> Shape {
        let mut s = *self;
        s.name = n;
        s
    }

    /// 只换链接状态。
    pub fn with_link(&self, l: LinkState) -> Shape {
        let mut s = *self;
        s.link = l;
        s
    }

    /// 只换通道。
    pub fn with_line(&self, r: LineRoute) -> Shape {
        let mut s = *self;
        s.line = r;
        s
    }

    /// 只换退出码。
    pub fn with_exit(&self, v: u32) -> Shape {
        let mut s = *self;
        s.exit_declared = v;
        s
    }

    /// 只换跳过计数。
    pub fn with_skip(&self, v: u32) -> Shape {
        let mut s = *self;
        s.skip_declared = v;
        s
    }

    /// 这一行的跳过计数该是几（未名 1，其余 0）。
    pub fn skip_want(&self) -> u32 {
        if self.class.speaks() {
            1u32
        } else {
            0u32
        }
    }

    /// 这一行做对时的注解档（不含病档）。
    pub fn note_live(&self) -> Note {
        match (self.class, self.link) {
            (Class::Alias, LinkState::Created) => Note::AliasLink,
            (Class::Real, LinkState::Created) => Note::LinkMade,
            (Class::Alias, _) | (Class::Real, _) => Note::AlreadyLinked,
            (Class::Pseudo, _) => Note::PseudoQuiet,
            (Class::NoSuch, _) => Note::SkipLogged,
        }
    }

    /// 形状指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.invoke.index() as u64);
        h = fold_word(h, self.class.index() as u64);
        h = fold_str(h, self.name);
        h = fold_word(h, self.link.index() as u64);
        h = fold_word(h, self.line.index() as u64);
        h = fold_word(h, self.exit_declared as u64);
        h = fold_word(h, self.skip_declared as u64);
        h = fold_bool(h, self.dup);
        fold_word(h, self.list_len as u64)
    }
}

/// 一次安装趟在"这一条"上的实际结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Outcome {
    /// 落点闸。
    pub stage: Stage,
    /// 整趟退出码。
    pub code: i32,
    /// 本行新建的链接数（0 或 1）。
    pub value: u32,
    /// 注解档。
    pub note: Note,
    /// 兜底位：这一格没有可对账的结论。
    pub default: bool,
    /// 结构自洽：谎报的形状在这里被压假，八种病也在这里落假。
    pub consistent: bool,
}

impl Outcome {
    /// 全零初值（取不到任何现场时的形态）。
    pub const fn blank() -> Outcome {
        Outcome {
            stage: Stage::Report,
            code: 0i32,
            value: 0u32,
            note: Note::NotInstall,
            default: true,
            consistent: true,
        }
    }

    /// 构造一格。
    pub const fn make(
        stage: Stage,
        code: u32,
        value: u32,
        note: Note,
        default: bool,
        consistent: bool,
    ) -> Outcome {
        Outcome {
            stage,
            code: code as i32,
            value,
            note,
            default,
            consistent,
        }
    }

    /// 是否绿（比 `code == 0` 更严：不自洽一律不绿）。
    pub fn ok(&self) -> bool {
        self.code == 0 && self.consistent
    }

    /// 是否红。
    pub fn failed(&self) -> bool {
        self.code != 0
    }

    /// 是否落进兜底档。
    pub fn anchored(&self) -> bool {
        self.default
    }

    /// 是否拒收（不自洽）。
    pub fn refused(&self) -> bool {
        !self.consistent
    }

    /// 应然退出码。
    pub fn exit(&self) -> u32 {
        if self.ok() {
            return EXIT_OK;
        }
        if self.code < 0 {
            return EXIT_USAGE;
        }
        self.code as u32
    }

    /// "未名必无链"这条不变量在这一格成立吗。
    pub fn link_invariant(&self) -> bool {
        if self.note.on_unknown_side() {
            return self.value == 0;
        }
        true
    }

    /// 结论指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.stage.rank() as u64);
        h = fold_i32(h, self.code);
        h = fold_word(h, self.value as u64);
        h = fold_str(h, self.note.name());
        h = fold_bool(h, self.default);
        fold_bool(h, self.consistent)
    }
}

/// 申报（读到的那一段账；与 `Outcome` 同构但没有自洽权）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Claim {
    /// 申报读成绿还是红。
    pub success: bool,
    /// 申报落在哪道闸。
    pub stage: Stage,
    /// 申报的退出码。
    pub code: i32,
    /// 申报的新建链接数。
    pub value: u32,
    /// 申报的注解档。
    pub note: Note,
    /// 申报的兜底位。
    pub default: bool,
}

impl Claim {
    /// 全零初值。
    pub const fn blank() -> Claim {
        Claim {
            success: false,
            stage: Stage::Invoke,
            code: 0i32,
            value: 0u32,
            note: Note::NotInstall,
            default: false,
        }
    }

    /// 构造一格。
    pub const fn make(
        success: bool,
        stage: Stage,
        code: i32,
        value: u32,
        note: Note,
        default: bool,
    ) -> Claim {
        Claim {
            success,
            stage,
            code,
            value,
            note,
            default,
        }
    }

    /// 申报的成败。
    pub fn success(&self) -> bool {
        self.success
    }

    /// 申报指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_bool(h, self.success);
        h = fold_word(h, self.stage.rank() as u64);
        h = fold_i32(h, self.code);
        h = fold_word(h, self.value as u64);
        h = fold_str(h, self.note.name());
        fold_bool(h, self.default)
    }
}

/// 一槽 = 形状 + 申报 + 短名。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Case {
    /// 输入形状。
    pub shape: Shape,
    /// 申报。
    pub claim: Claim,
    /// 短名。
    pub tag: &'static str,
}

impl Case {
    /// 空格。
    pub const fn blank() -> Case {
        Case {
            shape: Shape::blank(),
            claim: Claim::blank(),
            tag: "f11672-blank",
        }
    }

    /// 现场真值。
    pub fn actual(&self) -> Outcome {
        define(&self.shape)
    }

    /// 短名。
    pub fn tag(&self) -> &'static str {
        self.tag
    }

    /// 本槽的判读结果。
    pub fn fault(&self) -> Fault {
        judge(self)
    }

    /// 是否判对。
    pub fn ok(&self) -> bool {
        self.fault() == Fault::Ok
    }

    /// 本槽指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.shape.seal());
        h = fold_word(h, self.claim.seal());
        fold_str(h, self.tag)
    }
}

// ---------------------------------------------------------------------------
// 判据本体：七道闸，谁先谁后钉死
// ---------------------------------------------------------------------------

/// 由形状推出实际结论（本卡的唯一真值来源）。
///
/// 闸序 `Invoke → Class → Decide → Target → Emit → Code → Report`；
/// 每道闸只判自己那一件事，判死即返回，绝不回头改前一闸的结论。
pub fn define(s: &Shape) -> Outcome {
    let consistent = s.valid();

    // --- 闸 1：invoke ------------------------------------------------------
    // 缺目录档先判：它带着 `--install` 却落不了盘，是用法错而不是"不是安装趟"。
    if s.invoke == Invoke::NoDir {
        // `--install -s` 不带目录：用法错，一字不落盘。
        return Outcome::make(Stage::Invoke, EXIT_USAGE, 0, Note::UsageNoDir, false, false);
    }
    if !s.invoke.is_install() {
        // 普通运行：不是安装趟，本卡没有现场可判——兜底位为真。
        return Outcome::make(Stage::Invoke, EXIT_OK, 0, Note::NotInstall, true, consistent);
    }

    // --- 闸 2：class -------------------------------------------------------
    if !name_shape_ok(s.name) {
        // 空名 / 带 `/` / 超长：连"在不在表里"都没有答案。
        return Outcome::make(Stage::Class, EXIT_USAGE, 0, Note::BadName, false, false);
    }

    let effect = s.link.effect();

    // --- 闸 3：decide ------------------------------------------------------
    if s.class.installs() {
        if s.dup && s.link == LinkState::Absent {
            // 重名第二趟：链没建、复用没记、痕迹也没有——静默吞掉。
            return Outcome::make(Stage::Decide, EXIT_OK, 0, Note::DupCollapsed, false, false);
        }
    } else {
        if s.link.built_now() {
            // 未名 / 伪名却建了链（可能同时悬空）：伪链。
            return Outcome::make(Stage::Decide, EXIT_OK, effect, Note::FakeLink, false, false);
        }
    }

    // --- 闸 4：target ------------------------------------------------------
    if s.class.installs() {
        if matches!(
            s.link,
            LinkState::Absent | LinkState::Dangling | LinkState::Stale
        ) {
            // 实名/别名的链必须指向真 busybox 且目标存在。
            return Outcome::make(Stage::Target, EXIT_OK, 0, Note::TargetDangling, false, false);
        }
    } else {
        if matches!(s.link, LinkState::Stale | LinkState::Reused) {
            // 未名条目上还挂着一条链（上一趟留的，本趟连碰都没碰）：陈链。
            return Outcome::make(Stage::Target, EXIT_OK, 0, Note::StaleLeft, false, false);
        }
    }

    // --- 闸 5：emit --------------------------------------------------------
    if s.class.speaks() && !s.line.is_diag() {
        // 跳过行跑到 stdout：字节对、通道错。
        return Outcome::make(Stage::Emit, EXIT_OK, effect, Note::LineOnStdout, false, false);
    }

    // --- 闸 6：code --------------------------------------------------------
    if s.exit_declared == EXIT_NOTFOUND {
        // 安装趟报了 127：把"跳过"误报成"命令没找到"。
        return Outcome::make(Stage::Code, EXIT_NOTFOUND, effect, Note::PassCode127, false, false);
    }
    if s.exit_declared != EXIT_OK {
        // 一个未名的缺席不许把整趟顶红。
        return Outcome::make(
            Stage::Code,
            s.exit_declared,
            effect,
            Note::ExitBumped,
            false,
            false,
        );
    }

    // --- 闸 7：report ------------------------------------------------------
    if s.skip_declared != s.skip_want() {
        return Outcome::make(Stage::Report, EXIT_OK, effect, Note::CountDrift, false, false);
    }
    Outcome::make(Stage::Report, EXIT_OK, effect, s.note_live(), false, consistent)
}

// ---------------------------------------------------------------------------
// 语料：八活六死，逐槽手写
// ---------------------------------------------------------------------------

/// 语料现场表。
pub const SHAPES: [Shape; CORPUS] = [
    // 0 母版锚：`--install -s`，实名 `ls` 新建软链，退 0。
    Shape {
        ..Shape::blank()
    },
    // 1 别名 `zcat`：链装上去，回派靠 F11671 收敛（本卡只消费"已知"）。
    Shape {
        class: Class::Alias,
        name: "zcat",
        ..Shape::blank()
    },
    // 2 硬链安装（`--install DIR`）：`cat` 同样装上，码档不变。
    Shape {
        invoke: Invoke::Hard,
        name: "cat",
        ..Shape::blank()
    },
    // 3 未名 `foo`：不建链、留一行 stderr、整趟仍 0（本卡的正锚）。
    Shape {
        class: Class::NoSuch,
        name: NOSUCH_ONE,
        link: LinkState::Absent,
        skip_declared: 1,
        ..Shape::blank()
    },
    // 4 未名 `ls-zzz`：带连字符，跳过行字节数最长的一格。
    Shape {
        class: Class::NoSuch,
        name: NOSUCH_DASH,
        link: LinkState::Absent,
        skip_declared: 1,
        ..Shape::blank()
    },
    // 5 伪名 `busybox`：在表里但不装，也不出声。
    Shape {
        class: Class::Pseudo,
        name: MULTICALL_NAME,
        link: LinkState::Absent,
        ..Shape::blank()
    },
    // 6 重名 `cp` 第二趟：链已在，判"已就位"，不重造也不吞。
    Shape {
        name: "cp",
        link: LinkState::Reused,
        dup: true,
        ..Shape::blank()
    },
    // 7 未名 `bar`（硬链趟）：跳过的账一样要留。
    Shape {
        invoke: Invoke::Hard,
        class: Class::NoSuch,
        name: NOSUCH_TWO,
        link: LinkState::Absent,
        skip_declared: 1,
        ..Shape::blank()
    },
    // 8 Estate：未名 `foo` 建了伪链，日志却申报"整趟绿"。
    Shape {
        class: Class::NoSuch,
        name: NOSUCH_ONE,
        link: LinkState::Created,
        skip_declared: 1,
        ..Shape::blank()
    },
    // 9 Stage：未名 `bar` 跳过正常，申报的落点闸说成 emit。
    Shape {
        class: Class::NoSuch,
        name: NOSUCH_TWO,
        link: LinkState::Absent,
        skip_declared: 1,
        ..Shape::blank()
    },
    // 10 Code：跳过一次把整趟顶成 2，申报的码却说 0。
    Shape {
        class: Class::NoSuch,
        name: NOSUCH_DASH,
        link: LinkState::Absent,
        exit_declared: 2,
        skip_declared: 1,
        ..Shape::blank()
    },
    // 11 Value：实名 `grep` 建了一条链，申报的新建数说成 2。
    Shape {
        name: "grep",
        ..Shape::blank()
    },
    // 12 Note：伪名 `--list` 静默，申报的注解档说成"跳过并留了一行"。
    Shape {
        class: Class::Pseudo,
        name: LIST_PSEUDO,
        link: LinkState::Absent,
        ..Shape::blank()
    },
    // 13 Default：普通运行 `busybox ls`（不是安装趟），申报却说"没落兜底"。
    Shape {
        invoke: Invoke::Plain,
        link: LinkState::Reused,
        ..Shape::blank()
    },
];

/// 母版申报：实名建链、退 0、判绿。
pub const GOOD_CLAIM: Claim = Claim {
    success: true,
    stage: Stage::Report,
    code: EXIT_OK as i32,
    value: 1,
    note: Note::LinkMade,
    default: false,
};

/// 申报表。
pub const CLAIMS: [Claim; CORPUS] = [
    // 0-2 三条装上档。
    Claim { ..GOOD_CLAIM },
    Claim {
        value: 1,
        note: Note::AliasLink,
        ..GOOD_CLAIM
    },
    Claim { ..GOOD_CLAIM },
    // 3-5 跳过与伪名档：新建数一律 0。
    Claim {
        value: 0,
        note: Note::SkipLogged,
        ..GOOD_CLAIM
    },
    Claim {
        value: 0,
        note: Note::SkipLogged,
        ..GOOD_CLAIM
    },
    Claim {
        value: 0,
        note: Note::PseudoQuiet,
        ..GOOD_CLAIM
    },
    // 6 重名第二趟：判"已就位"，不建链。
    Claim {
        value: 0,
        note: Note::AlreadyLinked,
        ..GOOD_CLAIM
    },
    // 7 硬链趟的跳过。
    Claim {
        value: 0,
        note: Note::SkipLogged,
        ..GOOD_CLAIM
    },
    // 8 成败互报：伪链现场被申报成"整趟绿"。
    Claim {
        success: true,
        stage: Stage::Decide,
        code: EXIT_OK as i32,
        value: 1,
        note: Note::FakeLink,
        default: false,
    },
    // 9 阶段不符：落点是 report，申报说成 emit；其余五维照实现场。
    Claim {
        success: true,
        stage: Stage::Emit,
        code: EXIT_OK as i32,
        value: 0,
        note: Note::SkipLogged,
        default: false,
    },
    // 10 码档不符：被顶成 2 的整趟，申报说成 0；其余五维照实现场。
    Claim {
        success: false,
        stage: Stage::Code,
        code: EXIT_OK as i32,
        value: 0,
        note: Note::ExitBumped,
        default: false,
    },
    // 11 新建数不符：一条链被申报成两条；其余五维照实现场。
    Claim {
        success: true,
        stage: Stage::Report,
        code: EXIT_OK as i32,
        value: 2,
        note: Note::LinkMade,
        default: false,
    },
    // 12 注解不符：伪名静默被申报成"跳过留行"；其余五维照实现场。
    Claim {
        success: true,
        stage: Stage::Report,
        code: EXIT_OK as i32,
        value: 0,
        note: Note::SkipLogged,
        default: false,
    },
    // 13 兜底不符：普通运行被申报成"没落兜底"；其余五维照实现场。
    Claim {
        success: true,
        stage: Stage::Invoke,
        code: EXIT_OK as i32,
        value: 0,
        note: Note::NotInstall,
        default: false,
    },
];

// ---------------------------------------------------------------------------
// 期望表（申报表的第二种形式，任一处笔误都要露出来）
// ---------------------------------------------------------------------------

/// 期望阶段（现场**真实**落到的闸）。
pub const WANT_STAGE: [Stage; CORPUS] = [
    Stage::Report,
    Stage::Report,
    Stage::Report,
    Stage::Report,
    Stage::Report,
    Stage::Report,
    Stage::Report,
    Stage::Report,
    Stage::Decide,
    Stage::Report,
    Stage::Code,
    Stage::Report,
    Stage::Report,
    Stage::Invoke,
];

/// 期望退出码。
pub const WANT_CODE: [u32; CORPUS] = [
    EXIT_OK, EXIT_OK, EXIT_OK, EXIT_OK, EXIT_OK, EXIT_OK, EXIT_OK, EXIT_OK, EXIT_OK, EXIT_OK, 2,
    EXIT_OK, EXIT_OK, EXIT_OK,
];

/// 期望新建链接数。
pub const WANT_VALUE: [u32; CORPUS] = [1, 1, 1, 0, 0, 0, 0, 0, 1, 0, 0, 1, 0, 0];

/// 期望注解。
pub const WANT_NOTE: [Note; CORPUS] = [
    Note::LinkMade,
    Note::AliasLink,
    Note::LinkMade,
    Note::SkipLogged,
    Note::SkipLogged,
    Note::PseudoQuiet,
    Note::AlreadyLinked,
    Note::SkipLogged,
    Note::FakeLink,
    Note::SkipLogged,
    Note::ExitBumped,
    Note::LinkMade,
    Note::PseudoQuiet,
    Note::NotInstall,
];

/// 期望兜底位。
pub const WANT_DEFAULT: [bool; CORPUS] = [
    false, false, false, false, false, false, false, false, false, false, false, false, false, true,
];

/// 期望成败（`ok() = code == 0 && consistent`）。
pub const WANT_SUCCESS: [bool; CORPUS] = [
    true, true, true, true, true, true, true, true, false, true, false, true, true, true,
];

/// 期望缺陷维序位（0..5 六维，6 = 无缺陷）。
pub const WANT_NDX: [usize; CORPUS] = [6, 6, 6, 6, 6, 6, 6, 6, 0, 1, 2, 3, 4, 5];

/// 期望短名（标签必须挂本卡号）。
pub const WANT_TAGS: [&str; CORPUS] = [
    "f11672-ls-install-soft",
    "f11672-zcat-alias-install",
    "f11672-cat-install-hard",
    "f11672-foo-skip-logged",
    "f11672-lszzz-skip-logged",
    "f11672-busybox-pseudo-quiet",
    "f11672-cp-dup-reused",
    "f11672-bar-skip-hard",
    "f11672-estate-fake-link",
    "f11672-stage-emit-shift",
    "f11672-code-skip-bump",
    "f11672-value-link-count",
    "f11672-note-pseudo-flip",
    "f11672-default-plain-run",
];

// ---------------------------------------------------------------------------
// 八病表：每一行是一种真实构建病，带现场形状与应落闸档
// ---------------------------------------------------------------------------

/// 一格病：病名 + 现场形状 + 应落闸 + 应落注解 + 应落码 + 应落新建数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FaultRow {
    /// 病名。
    pub defect: Defect,
    /// 现场形状。
    pub shape: Shape,
    /// 应落闸。
    pub stage: Stage,
    /// 应落注解。
    pub note: Note,
    /// 应落退出码。
    pub code: u32,
    /// 应落新建链接数。
    pub value: u32,
}

/// 八病表（顺序即 `Defect` 的 1..8）。
pub const FAULT_TABLE: [FaultRow; DEFECT_FAULTS] = [
    // ① 未名却建了链：伪链，死在 Decide。
    FaultRow {
        defect: Defect::FakeLink,
        shape: Shape {
            class: Class::NoSuch,
            name: NOSUCH_ONE,
            link: LinkState::Created,
            skip_declared: 1,
            ..Shape::blank()
        },
        stage: Stage::Decide,
        note: Note::FakeLink,
        code: EXIT_OK,
        value: 1,
    },
    // ② 跳过一次把整趟顶成非零：死在 Code。
    FaultRow {
        defect: Defect::SkipExit,
        shape: Shape {
            class: Class::NoSuch,
            name: NOSUCH_TWO,
            link: LinkState::Absent,
            exit_declared: 2,
            skip_declared: 1,
            ..Shape::blank()
        },
        stage: Stage::Code,
        note: Note::ExitBumped,
        code: 2,
        value: 0,
    },
    // ③ 跳过行跑到 stdout：死在 Emit。
    FaultRow {
        defect: Defect::Misroute,
        shape: Shape {
            class: Class::NoSuch,
            name: NOSUCH_ONE,
            link: LinkState::Absent,
            line: LineRoute::Stdout,
            skip_declared: 1,
            ..Shape::blank()
        },
        stage: Stage::Emit,
        note: Note::LineOnStdout,
        code: EXIT_OK,
        value: 0,
    },
    // ④ 安装趟报了 127：死在 Code（同一串字节的另一个码档被搬错了地方）。
    FaultRow {
        defect: Defect::Code127,
        shape: Shape {
            class: Class::NoSuch,
            name: NOSUCH_TWO,
            link: LinkState::Absent,
            exit_declared: EXIT_NOTFOUND,
            skip_declared: 1,
            ..Shape::blank()
        },
        stage: Stage::Code,
        note: Note::PassCode127,
        code: EXIT_NOTFOUND,
        value: 0,
    },
    // ⑤ 链指向不存在的 applet：死在 Target（链在，目标不真）。
    FaultRow {
        defect: Defect::Dangling,
        shape: Shape {
            name: "ls",
            link: LinkState::Dangling,
            ..Shape::blank()
        },
        stage: Stage::Target,
        note: Note::TargetDangling,
        code: EXIT_OK,
        value: 0,
    },
    // ⑥ 重名第二趟被静默吞掉：死在 Decide。
    FaultRow {
        defect: Defect::Duplicate,
        shape: Shape {
            name: "cp",
            link: LinkState::Absent,
            dup: true,
            ..Shape::blank()
        },
        stage: Stage::Decide,
        note: Note::DupCollapsed,
        code: EXIT_OK,
        value: 0,
    },
    // ⑦ 跳过计数与清单对不上：死在 Report。
    FaultRow {
        defect: Defect::SkipCount,
        shape: Shape {
            class: Class::NoSuch,
            name: NOSUCH_DASH,
            link: LinkState::Absent,
            skip_declared: 0,
            ..Shape::blank()
        },
        stage: Stage::Report,
        note: Note::CountDrift,
        code: EXIT_OK,
        value: 0,
    },
    // ⑧ 陈链残留，破坏"未名必无链"：死在 Target。
    FaultRow {
        defect: Defect::StaleLink,
        shape: Shape {
            class: Class::NoSuch,
            name: NOSUCH_DASH,
            link: LinkState::Stale,
            skip_declared: 1,
            ..Shape::blank()
        },
        stage: Stage::Target,
        note: Note::StaleLeft,
        code: EXIT_OK,
        value: 0,
    },
];

/// 取一格病；越界给 `None`。
pub fn fault_row(i: usize) -> Option<FaultRow> {
    match FAULT_TABLE.get(i) {
        Some(r) => Some(*r),
        None => None,
    }
}

/// 由形状判病名（八病 + None，注解即病名的唯一真源）。
pub fn defect_of(s: &Shape) -> Defect {
    match define(s).note {
        Note::FakeLink => Defect::FakeLink,
        Note::ExitBumped => Defect::SkipExit,
        Note::LineOnStdout => Defect::Misroute,
        Note::PassCode127 => Defect::Code127,
        Note::TargetDangling => Defect::Dangling,
        Note::DupCollapsed => Defect::Duplicate,
        Note::CountDrift => Defect::SkipCount,
        Note::StaleLeft => Defect::StaleLink,
        _ => Defect::None,
    }
}

// ---------------------------------------------------------------------------
// 判定
// ---------------------------------------------------------------------------

/// 序位 → 缺陷维。
pub fn fault_ndx(f: Fault) -> usize {
    f.order()
}

/// 序位 → 缺陷维（越界给 `None`）。
pub fn ndx_fault(n: usize) -> Option<Fault> {
    match n {
        0usize => Some(Fault::Estate),
        1usize => Some(Fault::Stage),
        2usize => Some(Fault::Code),
        3usize => Some(Fault::Value),
        4usize => Some(Fault::Note),
        5usize => Some(Fault::Default),
        6usize => Some(Fault::Ok),
        _ => None,
    }
}

/// 申报对判——固定次序 Estate → Stage → Code → Value → Note → Default → Ok。
pub fn judge(c: &Case) -> Fault {
    let a = c.actual();
    let k = &c.claim;
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
    // 兜底双向判死：现场真落兜底，无论怎么申报都不许当结论；现场没落兜底却
    // 申报"落了"，同样是把结论塞进兜底。
    if a.default || a.default != k.default {
        return Fault::Default;
    }
    Fault::Ok
}

/// 语料取槽。
pub fn case_at(i: usize) -> Option<Case> {
    match (SHAPES.get(i), CLAIMS.get(i), WANT_TAGS.get(i)) {
        (Some(s), Some(c), Some(t)) => Some(Case {
            shape: *s,
            claim: *c,
            tag: *t,
        }),
        _ => None,
    }
}

/// 全语料。
pub fn cases() -> [Case; CORPUS] {
    let mut out = [Case::blank(); CORPUS];
    let mut i = 0usize;
    while i < CORPUS {
        if let Some(c) = case_at(i) {
            if let Some(slot) = out.get_mut(i) {
                *slot = c;
            }
        }
        i += 1;
    }
    out
}

/// 覆盖校验：槽数必须落在语料范围内。
pub fn cover(n: usize) -> Result<(), u32> {
    if n > CORPUS {
        return Err(INS_ESLOT);
    }
    Ok(())
}

/// 第 `i` 槽期望的缺陷维。
pub fn expected(i: usize) -> Option<Fault> {
    match WANT_NDX.get(i) {
        Some(n) => ndx_fault(*n),
        None => None,
    }
}

/// 反例区第 `i` 槽期望的缺陷维（取不到按 `Ok`，绝不冒充某一维）。
fn expected_reject_ndx(i: usize) -> Fault {
    match expected(i) {
        Some(f) => f,
        None => Fault::Ok,
    }
}

// ---------------------------------------------------------------------------
// 四档矩阵
// ---------------------------------------------------------------------------

/// 一档的统计行：`total` 与 `good` 两维独立。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Row {
    /// 档位。
    pub tier: Tier,
    /// 该行扫过的槽数。
    pub total: usize,
    /// 该行判对的槽数。
    pub good: usize,
}

impl Row {
    /// 该行是否全对。
    pub fn perfect(&self) -> bool {
        self.total == self.good
    }

    /// 该行红格数。
    pub fn red(&self) -> usize {
        self.total.saturating_sub(self.good)
    }

    /// 呈现一行。
    pub fn render(&self) -> String {
        let mut s = String::from(self.tier.name());
        s.push('=');
        s.push_str(dec_text(self.good as u32).as_str());
        s.push('/');
        s.push_str(dec_text(self.total as u32).as_str());
        s
    }
}

/// 四档矩阵：横看语料槽，纵看档位。
///
/// 每一档都要对**全语料**负责（`total` 恒为 `CORPUS`）：一个槽落在哪一档是
/// 事实，而"这一档有没有把该进的槽认出来、把不该进的槽挡住"才是判据。所以
/// `good` 只数"落进本档且判对"的槽，`red` 是"本档没认出来的其余槽"——
/// 两维独立，不许拿 `total` 去凑 `good`。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Matrix {
    rows: [Row; TIER_COUNT],
}

impl Matrix {
    /// 由语料构造。
    pub fn build() -> Matrix {
        let table = Tier::all();
        let mut rows = [Row {
            tier: Tier::Linked,
            total: 0,
            good: 0,
        }; TIER_COUNT];
        let mut t = 0usize;
        while t < TIER_COUNT {
            let tier = match table.get(t) {
                Some(x) => *x,
                None => return Matrix { rows },
            };
            let mut total = 0usize;
            let mut good = 0usize;
            let mut i = 0usize;
            while i < CORPUS {
                let c = match case_at(i) {
                    Some(x) => x,
                    None => return Matrix { rows },
                };
                total += 1;
                let landed = Tier::of(&c.actual());
                let hit = if i < ACCEPT {
                    landed == tier && c.ok()
                } else {
                    landed == tier && c.fault() == expected_reject_ndx(i)
                };
                if hit {
                    good += 1;
                }
                i += 1;
            }
            if let Some(slot) = rows.get_mut(t) {
                *slot = Row { tier, total, good };
            }
            t += 1;
        }
        Matrix { rows }
    }

    /// 取一档。
    pub fn row(&self, t: Tier) -> Option<Row> {
        let table = Tier::all();
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(x) = table.get(i) {
                if *x == t {
                    if let Some(r) = self.rows.get(i) {
                        return Some(*r);
                    }
                }
            }
            i += 1;
        }
        None
    }

    /// 扫过的格总数。
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

    /// 判对的格总数。
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

    /// 全对的行数。
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

    /// 红格总数。
    pub fn red_total(&self) -> usize {
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

    /// 四档是否都有落点。
    pub fn covered(&self) -> bool {
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(r) = self.rows.get(i) {
                if r.total == 0 || r.good == 0 {
                    return false;
                }
            }
            i += 1;
        }
        true
    }

    /// 呈现矩阵。
    pub fn render(&self) -> String {
        let mut s = String::from("matrix ");
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(r) = self.rows.get(i) {
                if i > 0 {
                    s.push(' ');
                }
                s.push_str(r.render().as_str());
            }
            i += 1;
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 结算报告
// ---------------------------------------------------------------------------

/// 全绿渲染文本（逐字钉住，改一个字都要红）。
pub const RENDER_OK: &str =
    "install-skip corpus=14 accept=8 reject=6 linked=4 untouched=7 faulted=2 fallback=1 match=14 rate=1000permille\n";

/// 安装降级批结算报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Report {
    /// 语料分母。
    pub total: usize,
    /// 正例槽数。
    pub accept: usize,
    /// 反例槽数。
    pub reject: usize,
    /// 建链档计数。
    pub linked: usize,
    /// 没动盘面档计数。
    pub untouched: usize,
    /// 有病档计数。
    pub faulted: usize,
    /// 兜底档计数。
    pub fallback: usize,
    /// 判对槽数。
    pub matched: usize,
}

impl Report {
    /// 一致率千分比。
    pub fn rate(&self) -> u32 {
        if self.total == 0 {
            return 0;
        }
        (self.matched as u32 * RATE_FULL + (self.total as u32) / 2) / (self.total as u32)
    }

    /// 是否全绿。
    pub fn green(&self) -> bool {
        self.accept == ACCEPT
            && self.reject == REJECT
            && self.total == CORPUS
            && self.tier_cells() == CORPUS as u32
            && self.matched == self.total
            && self.rate() == RATE_FULL
    }

    /// 红项计数。
    pub fn reds(&self) -> u32 {
        (self.total.saturating_sub(self.matched)) as u32
    }

    /// 闸门：全绿才放行。
    pub fn gate(&self) -> Result<(), u32> {
        if self.total == 0 {
            return Err(INS_ENOBS);
        }
        if self.tier_cells() != self.total as u32 {
            return Err(INS_EMATRIX);
        }
        if self.matched > self.total {
            return Err(INS_EBAL);
        }
        if self.rate() < RATE_FULL {
            return Err(INS_EBAL);
        }
        Ok(())
    }

    /// 四档格数。
    pub fn tier_cells(&self) -> u32 {
        (self.linked as u32) + (self.untouched as u32) + (self.faulted as u32) + (self.fallback as u32)
    }

    /// 逐字渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("install-skip corpus=");
        s.push_str(dec_text(self.accept as u32 + self.reject as u32).as_str());
        s.push_str(" accept=");
        s.push_str(dec_text(self.accept as u32).as_str());
        s.push_str(" reject=");
        s.push_str(dec_text(self.reject as u32).as_str());
        s.push_str(" linked=");
        s.push_str(dec_text(self.linked as u32).as_str());
        s.push_str(" untouched=");
        s.push_str(dec_text(self.untouched as u32).as_str());
        s.push_str(" faulted=");
        s.push_str(dec_text(self.faulted as u32).as_str());
        s.push_str(" fallback=");
        s.push_str(dec_text(self.fallback as u32).as_str());
        s.push_str(" match=");
        s.push_str(dec_text(self.matched as u32).as_str());
        s.push_str(" rate=");
        s.push_str(dec_text(self.rate()).as_str());
        s.push_str("permille\n");
        s
    }
}

/// 空报告（未跑任何语料时的形态，绝不与全绿混淆）。
pub fn empty_report() -> Report {
    Report {
        total: 0,
        accept: 0,
        reject: 0,
        linked: 0,
        untouched: 0,
        faulted: 0,
        fallback: 0,
        matched: 0,
    }
}

/// 由语料结算。
fn settle(over: Option<(usize, Fault)>) -> Report {
    let mut rep = Report {
        total: CORPUS,
        accept: ACCEPT,
        reject: REJECT,
        linked: 0,
        untouched: 0,
        faulted: 0,
        fallback: 0,
        matched: 0,
    };
    let mut i = 0usize;
    while i < CORPUS {
        let c = match case_at(i) {
            Some(x) => x,
            None => return empty_report(),
        };
        match Tier::of(&c.actual()) {
            Tier::Linked => rep.linked += 1,
            Tier::Untouched => rep.untouched += 1,
            Tier::Faulted => rep.faulted += 1,
            Tier::Fallback => rep.fallback += 1,
        }
        let landed = match over {
            Some((slot, fault)) => {
                if slot == i {
                    fault
                } else {
                    c.fault()
                }
            }
            None => c.fault(),
        };
        let want = if i < ACCEPT {
            Fault::Ok
        } else {
            expected_reject_ndx(i)
        };
        if landed == want {
            rep.matched += 1;
        }
        i += 1;
    }
    rep
}

/// 主结算：全绿才给报告，否则给闸门码。
pub fn run() -> Result<Report, u32> {
    let rep = settle(None);
    match rep.gate() {
        Ok(()) => Ok(rep),
        Err(code) => Err(code),
    }
}

/// 带注入的结算：越界槽给槽位码。
pub fn run_with(over: Option<(usize, Fault)>) -> Result<Report, u32> {
    if let Some((slot, _)) = over {
        if slot >= CORPUS {
            return Err(INS_ESLOT);
        }
    }
    Ok(settle(over))
}

/// 报告与矩阵是否互相印证。
pub fn report_agrees_with_matrix() -> bool {
    let rep = settle(None);
    let m = Matrix::build();
    rep.total == CORPUS
        && m.slots() == CORPUS * TIER_COUNT
        && m.goods() == CORPUS
        && rep.tier_cells() == CORPUS as u32
        && m.covered()
}

/// 红档率标定：注入一维红之后分子必须掉到 13/14。
pub fn red_rate_calibrated() -> bool {
    let base = settle(None);
    let bad = match run_with(Some((2, Fault::Value))) {
        Ok(r) => r,
        Err(_) => return false,
    };
    if bad.matched >= base.matched {
        return false;
    }
    if bad.total < 1 {
        return false;
    }
    let want = (13u32 * RATE_FULL + (bad.total as u32) / 2) / (bad.total as u32);
    want == RED_RATE && RED_RATE == 929
}

/// 清单行是否自洽（表路用的逐行判据）。
fn entry_ok(e: &Entry) -> bool {
    if e.class.known() != e.known {
        return false;
    }
    if e.dangling {
        return false;
    }
    if e.exit != EXIT_OK {
        return false;
    }
    if e.installed() {
        return e.stderr_bytes == 0;
    }
    if e.reused() {
        return e.stderr_bytes == 0;
    }
    if e.skipped() {
        return e.stderr_bytes == bytes_of(msg_skip(e.name).as_str());
    }
    if e.quiet() {
        return e.stderr_bytes == 0 && !e.link_present;
    }
    false
}

/// 表路：清单十四行逐行自洽且汇总数与常量对齐。
fn table_route() -> bool {
    let mut i = 0usize;
    while i < LIST_LEN {
        let e = match LIST.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if !entry_ok(&e) {
            return false;
        }
        // 名字形态也必须能判（不然 Class 闸永远进不去）。
        if !name_shape_ok(e.name) {
            return false;
        }
        i += 1;
    }
    created_rows() == LINK_ROWS
        && reused_rows() == REUSE_ROWS
        && skip_rows() == SKIP_ROWS
        && pseudo_rows() == PSEUDO_ROWS
        && present_rows() == LINK_ROWS + REUSE_ROWS
        && LINK_ROWS + REUSE_ROWS + SKIP_ROWS + PSEUDO_ROWS == LIST_LEN as u32
}

/// 判路：八活全绿、六死各自命中自己那一维。
fn judge_route() -> bool {
    let mut all_ok = true;
    let mut i = 0usize;
    while i < ACCEPT {
        if let Some(c) = case_at(i) {
            if c.fault() != Fault::Ok {
                all_ok = false;
            }
        } else {
            all_ok = false;
        }
        i += 1;
    }
    let mut k = ACCEPT;
    while k < CORPUS {
        if let Some(c) = case_at(k) {
            if c.fault() == Fault::Ok {
                all_ok = false;
            }
            if c.fault() != expected_reject_ndx(k) {
                all_ok = false;
            }
        } else {
            all_ok = false;
        }
        k += 1;
    }
    all_ok
}

/// 渲染路：全绿且逐字等于 `RENDER_OK`。
fn render_route() -> bool {
    let rep = settle(None);
    rep.green() && rep.render().as_str() == RENDER_OK
}

/// 账本路：建账 → 冻结 → 重放比指纹。
fn ledger_route() -> bool {
    let mut l = build_ledger();
    match l.freeze() {
        Ok(h) => h != 0 && l.is_frozen() && l.replay() == Ok(h),
        Err(_) => false,
    }
}

/// 注入一路红，四路自检必须各指各的（表路 / 判路 / 渲染路 / 账本路）。
pub fn inject_four() -> [bool; 4] {
    [table_route(), judge_route(), render_route(), ledger_route()]
}

/// 八病逐格复核：每一格的现场形状必须落回它自己申报的闸/注解/码/新建数。
pub fn inject_eight() -> [bool; DEFECT_FAULTS] {
    let mut out = [false; DEFECT_FAULTS];
    let mut i = 0usize;
    while i < DEFECT_FAULTS {
        let row = match fault_row(i) {
            Some(x) => x,
            None => return out,
        };
        let a = define(&row.shape);
        let hit = defect_of(&row.shape) == row.defect
            && a.stage == row.stage
            && a.note == row.note
            && a.code as u32 == row.code
            && a.value == row.value
            && !a.consistent
            && !a.ok()
            && !a.default;
        if let Some(slot) = out.get_mut(i) {
            *slot = hit;
        }
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// 阶段闸门：Invoke→Class→Decide→Target→Emit→Code→Report
// ---------------------------------------------------------------------------

/// 一次安装降级判据的走闸状态机。
///
/// 倒退判死（`INS_ESTAGE`），封账后再走一步也判死（`INS_ESTATE`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Gate {
    /// 当前档；`None` 表示还没进过任何闸。
    current: Option<Stage>,
    /// 是否已封账。
    closed: bool,
    /// 成功走档次数。
    steps: u32,
    /// 被拒次数。
    refusals: u32,
    /// 走档轨迹指纹。
    root: u64,
}

impl Gate {
    /// 空闸门。
    pub fn new() -> Gate {
        Gate {
            current: None,
            closed: false,
            steps: 0,
            refusals: 0,
            root: FNV_OFF,
        }
    }

    /// 走一档。倒退即拒，且不改状态。
    pub fn step(&mut self, s: Stage) -> Result<(), u32> {
        if self.closed {
            self.refusals += 1;
            return Err(INS_ESTATE);
        }
        if let Some(cur) = self.current {
            if s.rank() < cur.rank() {
                self.refusals += 1;
                return Err(INS_ESTAGE);
            }
        }
        self.root = fold_word(self.root, s.rank() as u64);
        self.root = fold_str(self.root, s.name());
        self.current = Some(s);
        self.steps += 1;
        Ok(())
    }

    /// 封账：之后任何走档与再次封账都拒。
    pub fn close(&mut self) -> Result<u64, u32> {
        if self.closed {
            self.refusals += 1;
            return Err(INS_ESTATE);
        }
        if self.current.is_none() {
            return Err(INS_ESTAGE);
        }
        self.closed = true;
        self.root = fold_str(self.root, "closed");
        Ok(self.root)
    }

    /// 当前档。
    pub fn stage(&self) -> Option<Stage> {
        self.current
    }

    /// 成功走档次数。
    pub fn steps(&self) -> u32 {
        self.steps
    }

    /// 被拒次数。
    pub fn refusals(&self) -> u32 {
        self.refusals
    }

    /// 是否已封账。
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// 轨迹指纹。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 依次走一串档。
    pub fn walk(stages: &[Stage]) -> Result<Gate, u32> {
        let mut g = Gate::new();
        let mut i = 0usize;
        while i < stages.len() {
            let s = match stages.get(i) {
                Some(x) => *x,
                None => return Err(INS_ESLOT),
            };
            match g.step(s) {
                Ok(()) => {}
                Err(code) => return Err(code),
            }
            i += 1;
        }
        Ok(g)
    }

    /// 全档正序走一遍并封账。
    pub fn full_pass() -> Result<u64, u32> {
        let table = Stage::all();
        let mut g = Gate::new();
        let mut i = 0usize;
        while i < table.len() {
            let s = match table.get(i) {
                Some(x) => *x,
                None => return Err(INS_ESLOT),
            };
            match g.step(s) {
                Ok(()) => {}
                Err(code) => return Err(code),
            }
            i += 1;
        }
        g.close()
    }

    /// 呈现闸门。
    pub fn render(&self) -> String {
        let mut s = String::from("gate stage=");
        match self.current {
            Some(cur) => s.push_str(cur.name()),
            None => s.push_str("none"),
        }
        s.push_str(" steps=");
        s.push_str(dec_text(self.steps).as_str());
        s.push_str(" refusals=");
        s.push_str(dec_text(self.refusals).as_str());
        s.push_str(" closed=");
        s.push_str(bool_text(self.closed));
        s
    }
}

// ---------------------------------------------------------------------------
// 账本：逐格登记本卡的统计量与缺陷码
// ---------------------------------------------------------------------------

/// 一格台账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sample {
    /// 槽号，必须严格等于登记时的当前长度。
    pub slot: u8,
    /// 这一格对应的算子号。
    pub op: u8,
    /// 这一格**判得对不对**（不是"这一格是不是绿"）。
    ///
    /// 统计格：值等于标定值才算判对；缺陷格：码等于应落的缺陷码、值为 0 才算
    /// 判对——缺陷格判对了同样记 `true`。把这一位写成"槽是绿的"会让六格缺陷
    /// 永远记假，`audit` 的缺陷分支就退化成"码对了且自称失败"的同义反复，
    /// `err_count` 也不再是错判数。
    pub ok: bool,
    /// 错误码（统计格必须为 0）。
    pub code: u32,
    /// 统计值（缺陷格必须为 0）。
    pub value: u32,
}

impl Sample {
    /// 一格的指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_byte(h, self.slot);
        h = fold_byte(h, self.op);
        h = fold_bool(h, self.ok);
        h = fold_word(h, self.code as u64);
        fold_word(h, self.value as u64)
    }

    /// 呈现一格。
    pub fn render(&self) -> String {
        let mut s = String::from("slot ");
        s.push_str(dec_text(self.slot as u32).as_str());
        s.push_str(" op ");
        s.push_str(hex_text(self.op as u32, 2).as_str());
        s.push(' ');
        s.push_str(bool_text(self.ok));
        s.push_str(" code ");
        s.push_str(hex_text(self.code, 5).as_str());
        s.push_str(" value ");
        s.push_str(dec_text(self.value).as_str());
        s
    }
}

/// 账本：十四格 = 七统计格 + 一红档格 + 六缺陷码格。
#[derive(Clone, Copy, Debug)]
pub struct Ledger {
    slots: [Option<Sample>; SAMPLES],
    len: usize,
    rejects: usize,
    frozen: bool,
    root: u64,
}

impl Ledger {
    /// 空账。
    pub fn new() -> Ledger {
        Ledger {
            slots: [None; SAMPLES],
            len: 0,
            rejects: 0,
            frozen: false,
            root: 0,
        }
    }

    /// 登记一格；槽号错位一律拒。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(INS_ESTATE);
        }
        if self.len >= SAMPLES {
            self.rejects += 1;
            return Err(INS_ESLOT);
        }
        if s.slot as usize != self.len {
            self.rejects += 1;
            return Err(INS_ESLOT);
        }
        if let Some(cell) = self.slots.get_mut(self.len) {
            *cell = Some(s);
        }
        self.len += 1;
        Ok(())
    }

    /// 已登记格数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 空账判定。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取一格。
    pub fn sample(&self, i: usize) -> Option<Sample> {
        match self.slots.get(i) {
            Some(x) => *x,
            None => None,
        }
    }

    /// 成功格数。
    pub fn ok_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.sample(i) {
                if s.ok {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 败格数。
    pub fn err_count(&self) -> usize {
        self.len.saturating_sub(self.ok_count())
    }

    /// 拒收次数。
    pub fn rejects(&self) -> usize {
        self.rejects
    }

    /// 是否冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 冻结后的指纹；未冻结时为 0。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 统计格应有的算子号。
    fn stat_op_want(&self, i: usize) -> Option<u8> {
        if i < STAT_CELLS {
            return match STAT_OPS.get(i) {
                Some(v) => Some(*v),
                None => None,
            };
        }
        if i == STAT_CELLS {
            return Some(OPID_STAT_RED);
        }
        None
    }

    /// 逐格复核。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES {
            return Err(INS_ESLOT);
        }
        let stat_last = match STAT_CELLS.checked_add(1) {
            Some(x) => x,
            None => return Err(INS_ESLOT),
        };
        let mut i = 0usize;
        while i < stat_last {
            let s = match self.sample(i) {
                Some(x) => x,
                None => return Err(INS_ESLOT),
            };
            if !s.ok {
                return Err(INS_ESTATE);
            }
            if s.code != 0 {
                return Err(INS_ECODE);
            }
            let want_value = match VALS.get(i) {
                Some(v) => *v,
                None => return Err(INS_ESLOT),
            };
            if s.value != want_value {
                return Err(INS_EVALUE);
            }
            let want_op = match self.stat_op_want(i) {
                Some(v) => v,
                None => return Err(INS_ESLOT),
            };
            if s.op != want_op {
                return Err(INS_ECODE);
            }
            i += 1;
        }
        let mut j = 0usize;
        while j < REJECT {
            let cell = match STAT_CELLS.checked_add(1).and_then(|x| x.checked_add(j)) {
                Some(x) => x,
                None => return Err(INS_ESLOT),
            };
            let s = match self.sample(cell) {
                Some(x) => x,
                None => return Err(INS_ESLOT),
            };
            // 缺陷格判对了也是"判对"：这一位不是"槽绿"，是"对账无误"。
            if !s.ok {
                return Err(INS_ESTATE);
            }
            if s.value != 0 {
                return Err(INS_EVALUE);
            }
            let want_code = match REJECT_CODES.get(j) {
                Some(c) => *c,
                None => return Err(INS_ESLOT),
            };
            if s.code != want_code {
                return Err(INS_ECODE);
            }
            let want_op = match DEFECT_OP_BASE.checked_add(j as u8) {
                Some(x) => x,
                None => return Err(INS_ESLOT),
            };
            if s.op != want_op {
                return Err(INS_ECODE);
            }
            let mut k = j + 1usize;
            while k < REJECT {
                let other = match REJECT_CODES.get(k) {
                    Some(c) => *c,
                    None => return Err(INS_ESLOT),
                };
                if other == want_code {
                    return Err(INS_ECODE);
                }
                k += 1;
            }
            j += 1;
        }
        Ok(())
    }

    /// 第一处不合规的格子与原因（排障用，不改判定）。
    pub fn first_mismatch(&self) -> Option<(usize, &'static str)> {
        if self.len != SAMPLES {
            return Some((self.len, "len"));
        }
        let mut i = 0usize;
        while i < SAMPLES {
            let s = match self.sample(i) {
                Some(x) => x,
                None => return Some((i, "hole")),
            };
            if i <= STAT_CELLS {
                let want_value = match VALS.get(i) {
                    Some(v) => *v,
                    None => return Some((i, "slot")),
                };
                if !s.ok {
                    return Some((i, "ok-flag"));
                }
                if s.code != 0 {
                    return Some((i, "stat-code"));
                }
                if s.value != want_value {
                    return Some((i, "stat-value"));
                }
            } else {
                let j = i - (STAT_CELLS + 1);
                let want_code = match REJECT_CODES.get(j) {
                    Some(c) => *c,
                    None => return Some((i, "slot")),
                };
                if !s.ok {
                    return Some((i, "ok-flag"));
                }
                if s.value != 0 {
                    return Some((i, "defect-value"));
                }
                if s.code != want_code {
                    return Some((i, "defect-code"));
                }
            }
            i += 1;
        }
        None
    }

    /// 折叠全账得指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.len as u64);
        h = fold_word(h, self.rejects as u64);
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.sample(i) {
                h = fold_word(h, s.seal());
            }
            i += 1;
        }
        h
    }

    /// 冻结：先复核再折叠，复核不过一律拒。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(INS_ESTATE);
        }
        match self.audit() {
            Ok(()) => {}
            Err(code) => return Err(code),
        }
        let h = self.seal();
        if h == 0 {
            return Err(INS_ESEAL);
        }
        self.root = h;
        self.frozen = true;
        Ok(h)
    }

    /// 重放：以同一批格子重建账本，比指纹。
    pub fn replay(&self) -> Result<u64, u32> {
        if !self.frozen {
            return Err(INS_ESTATE);
        }
        let mut fresh = Ledger::new();
        let mut i = 0usize;
        while i < self.len {
            let s = match self.sample(i) {
                Some(x) => x,
                None => return Err(INS_ESEAL),
            };
            match fresh.register(s) {
                Ok(()) => {}
                Err(_) => return Err(INS_ESEAL),
            }
            i += 1;
        }
        let h = fresh.seal();
        if h != self.root {
            return Err(INS_ESEAL);
        }
        Ok(h)
    }

    /// 改动一格（仅未冻结时允许），用于注入红。
    pub fn poke(&mut self, i: usize, value: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(INS_ESTATE);
        }
        match self.slots.get_mut(i) {
            Some(cell) => {
                match *cell {
                    Some(ref mut s) => {
                        s.value = value;
                    }
                    None => return Err(INS_ESLOT),
                }
                Ok(())
            }
            None => Err(INS_ESLOT),
        }
    }

    /// 呈现全账。
    pub fn render(&self) -> String {
        let mut s = String::from("ledger len=");
        s.push_str(dec_text(self.len as u32).as_str());
        s.push_str(" ok=");
        s.push_str(dec_text(self.ok_count() as u32).as_str());
        s.push_str(" err=");
        s.push_str(dec_text(self.err_count() as u32).as_str());
        s.push_str(" rejects=");
        s.push_str(dec_text(self.rejects as u32).as_str());
        s.push_str(" frozen=");
        s.push_str(bool_text(self.frozen));
        s
    }
}

/// 统计表：七个统计量 + 红档率。
///
/// 前七格分别是闸数、身份档数、病种数、清单行数、跳过行数、建链行数、满一致率；
/// 第八格是本卡的红档率标定值（14 格里 13 格判对 = 929‰）。
pub const VALS: [u32; ACCEPT] = [
    STAGE_COUNT,
    CLASS_COUNT as u32,
    DEFECT_FAULTS as u32,
    LIST_LEN as u32,
    SKIP_ROWS,
    LINK_ROWS,
    RATE_FULL,
    RED_RATE,
];

/// 十四格标准台账：七统计 + 一红档 + 六缺陷码。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut i = 0usize;
    while i < STAT_CELLS {
        let op = match STAT_OPS.get(i) {
            Some(x) => *x,
            None => return l,
        };
        let value = match VALS.get(i) {
            Some(x) => *x,
            None => return l,
        };
        let s = Sample {
            slot: i as u8,
            op,
            ok: true,
            code: 0,
            value,
        };
        match l.register(s) {
            Ok(()) => {}
            Err(_) => return l,
        }
        i += 1;
    }
    let red = Sample {
        slot: STAT_CELLS as u8,
        op: OPID_STAT_RED,
        ok: true,
        code: 0,
        value: RED_RATE,
    };
    match l.register(red) {
        Ok(()) => {}
        Err(_) => return l,
    }
    let mut j = 0usize;
    while j < REJECT {
        let code = match REJECT_CODES.get(j) {
            Some(x) => *x,
            None => return l,
        };
        let op = match DEFECT_OP_BASE.checked_add(j as u8) {
            Some(x) => x,
            None => return l,
        };
        let slot = (STAT_CELLS + 1 + j) as u8;
        // 六格缺陷码是"应落"的结果：落对了就是判对，这一位记 true。
        let s = Sample {
            slot,
            op,
            ok: true,
            code,
            value: 0,
        };
        match l.register(s) {
            Ok(()) => {}
            Err(_) => return l,
        }
        j += 1;
    }
    l
}

/// 注入一维错的台账（证明复核真的在逐格看）。
pub fn build_ledger_broken(which: u8) -> Ledger {
    let mut l = build_ledger();
    let (cell, value) = match which {
        0 => match (STAT_CELLS, VALS.get(STAT_CELLS)) {
            (c, Some(v)) => (c, v + 1),
            _ => return l,
        },
        1 => match (0usize, VALS.get(0)) {
            (c, Some(v)) => (c, v + 7),
            _ => return l,
        },
        2 => match STAT_CELLS.checked_add(1) {
            Some(c) => (c, 1u32),
            None => return l,
        },
        3 => match STAT_CELLS.checked_add(3) {
            Some(c) => (c, 9u32),
            None => return l,
        },
        _ => return l,
    };
    match l.poke(cell, value) {
        Ok(()) => {}
        Err(_) => {}
    }
    l
}

// ---------------------------------------------------------------------------
// 十三条判据谓词
// ---------------------------------------------------------------------------

/// 01 名称与段基址。
fn name_ok() -> bool {
    if SEG_BASE != 0x6d250 {
        return false;
    }
    if ANCHOR_F11671_SEG.checked_add(ANCHOR_F11671_GAP) != Some(SEG_BASE) {
        return false;
    }
    if SEG_BASE.checked_add(ANCHOR_F11673_GAP) != Some(ANCHOR_F11673_SEG) {
        return false;
    }
    if ANCHOR_F11674_SEG != 0x6d270 || ANCHOR_F11664_SEG != 0x6d1a0 {
        return false;
    }
    if CORPUS != SAMPLES || CORPUS != ACCEPT + REJECT {
        return false;
    }
    if LIST_LEN != CORPUS {
        return false;
    }
    if TIER_COUNT != Tier::all().len() {
        return false;
    }
    if STAGE_COUNT != STAGE_LAST_RANK || Stage::all().len() as u32 != STAGE_COUNT {
        return false;
    }
    if CLASS_COUNT != Class::all().len() {
        return false;
    }
    if INVOKE_COUNT != Invoke::all().len() || INVOKE_JUDGED >= INVOKE_COUNT {
        return false;
    }
    if LINK_COUNT != LinkState::all().len() {
        return false;
    }
    if DEFECT_COUNT != Defect::all().len() || DEFECT_FAULTS + 1 != DEFECT_COUNT {
        return false;
    }
    if NOTE_COUNT != Note::all().len() || Note::COUNT != NOTE_COUNT {
        return false;
    }
    if MESSAGE_CELLS != MSG_TABLE.len() || MESSAGE_CELLS != MsgKind::all().len() {
        return false;
    }
    ALL_CODES.len() == 13 && FAULT_TABLE.len() == DEFECT_FAULTS
}

/// 02 十三码互异且偏移精确。
fn codes_ok() -> bool {
    if ALL_CODES.len() != 13 {
        return false;
    }
    let mut i = 0usize;
    while i < ALL_CODES.len() {
        let a = match ALL_CODES.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if a <= SEG_BASE || a >= ANCHOR_F11673_SEG {
            return false;
        }
        let mut j = i + 1usize;
        while j < ALL_CODES.len() {
            match ALL_CODES.get(j) {
                Some(b) => {
                    if *b == a {
                        return false;
                    }
                }
                None => return false,
            }
            j += 1;
        }
        i += 1;
    }
    INS_ESTATE == SEG_BASE + 0x01
        && INS_ECODE == SEG_BASE + 0x02
        && INS_ESTAGE == SEG_BASE + 0x03
        && INS_EVALUE == SEG_BASE + 0x04
        && INS_ENOTE == SEG_BASE + 0x05
        && INS_EDEFAULT == SEG_BASE + 0x06
        && INS_ELINK == SEG_BASE + 0x07
        && INS_ECHAN == SEG_BASE + 0x08
        && INS_ESLOT == SEG_BASE + 0x09
        && INS_ESEAL == SEG_BASE + 0x0a
        && INS_ENOBS == SEG_BASE + 0x0b
        && INS_EMATRIX == SEG_BASE + 0x0c
        && INS_EBAL == SEG_BASE + 0x0d
}

/// 03 安装清单十四行齐且四类各行其道。
fn list_ok() -> bool {
    if LIST.len() != LIST_LEN {
        return false;
    }
    let mut i = 0usize;
    while i < LIST_LEN {
        let e = match LIST.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if e.class.known() != e.known {
            return false;
        }
        if e.exit != EXIT_OK || e.dangling {
            return false;
        }
        if !name_shape_ok(e.name) {
            return false;
        }
        if !entry_ok(&e) {
            return false;
        }
        if !(e.installed() || e.reused() || e.skipped() || e.quiet()) {
            return false;
        }
        i += 1;
    }
    // 重名那一行必须是 `cp` 的第二趟，且它既不是跳过也不是新建。
    let dup_row = match LIST.get(11) {
        Some(x) => *x,
        None => return false,
    };
    if dup_row.name != "cp" || !dup_row.reused() || dup_row.link_created {
        return false;
    }
    created_rows() == LINK_ROWS
        && reused_rows() == REUSE_ROWS
        && skip_rows() == SKIP_ROWS
        && pseudo_rows() == PSEUDO_ROWS
        && present_rows() == LINK_ROWS + REUSE_ROWS
        && LINK_ROWS + REUSE_ROWS + SKIP_ROWS + PSEUDO_ROWS == LIST_LEN as u32
}

/// 04 未名跳过：一行、stderr、字节与直接执行行完全相同。
fn skip_ok() -> bool {
    if msg_skip(NOSUCH_ONE) != "foo: applet not found\n" {
        return false;
    }
    if lines_of(msg_skip(NOSUCH_ONE).as_str()) != 1 {
        return false;
    }
    if bytes_of(msg_skip(NOSUCH_ONE).as_str()) != 22 {
        return false;
    }
    if bytes_of(msg_skip(NOSUCH_DASH).as_str()) != 25 {
        return false;
    }
    if msg_text(MsgKind::SkipOnStderr, NOSUCH_ONE) != msg_skip(NOSUCH_ONE) {
        return false;
    }
    if msg_code(MsgKind::SkipOnStderr) != EXIT_OK {
        return false;
    }
    if !msg_route(MsgKind::SkipOnStderr).is_diag() {
        return false;
    }
    // 同一串字节、两个码档：本卡存在的理由。
    if !same_bytes_two_codes(NOSUCH_ONE) {
        return false;
    }
    if msg_code(MsgKind::NotFoundExec) != EXIT_NOTFOUND {
        return false;
    }
    // 表内三条跳过行的 stderr 字节必须逐行等于文案长度。
    let mut i = 0usize;
    let mut rows = 0u32;
    while i < LIST_LEN {
        let e = match LIST.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if e.class.speaks() {
            rows = rows.saturating_add(1);
            if e.stderr_bytes != bytes_of(msg_skip(e.name).as_str()) {
                return false;
            }
            if e.exit != EXIT_OK {
                return false;
            }
            if e.link_created || e.link_present {
                return false;
            }
        }
        i += 1;
    }
    // 干净现场：落 Report，退 0，新建数 0。
    let clean = Shape {
        class: Class::NoSuch,
        name: NOSUCH_ONE,
        link: LinkState::Absent,
        skip_declared: 1,
        ..Shape::blank()
    };
    let o = define(&clean);
    if o.stage != Stage::Report || o.note != Note::SkipLogged {
        return false;
    }
    if o.code != EXIT_OK as i32 || o.value != 0 || !o.ok() {
        return false;
    }
    rows == SKIP_ROWS && skip_bytes_total() == 69
}

/// 05 "未名必无链"：伪链 / 悬空 / 陈链三档分开记。
fn link_ok() -> bool {
    // 未名侧：本趟建的链（Created / Dangling）一律伪链，新建数为 1。
    let mut c = 0usize;
    while c < CLASS_COUNT {
        let class = match Class::at(c) {
            Some(x) => x,
            None => return false,
        };
        if class.installs() {
            c += 1;
            continue;
        }
        let mut l = 0usize;
        while l < LINK_COUNT {
            let link = match LinkState::at(l) {
                Some(x) => x,
                None => return false,
            };
            let s = Shape {
                class,
                name: NOSUCH_ONE,
                link,
                skip_declared: if class.speaks() { 1 } else { 0 },
                ..Shape::blank()
            };
            let o = define(&s);
            if link.built_now() {
                if o.stage != Stage::Decide || o.note != Note::FakeLink {
                    return false;
                }
                // 新建数只认本趟真建出来的那一条：Created 记 1，Dangling 记 0。
                if o.value != link.effect() || o.ok() {
                    return false;
                }
                if link == LinkState::Created && o.value != 1 {
                    return false;
                }
                if link == LinkState::Created && o.link_invariant() {
                    return false;
                }
            } else if matches!(link, LinkState::Stale | LinkState::Reused) {
                if o.stage != Stage::Target || o.note != Note::StaleLeft {
                    return false;
                }
                if o.value != 0 || o.ok() {
                    return false;
                }
            } else {
                if o.stage != Stage::Report || o.value != 0 || !o.ok() {
                    return false;
                }
            }
            l += 1;
        }
        c += 1;
    }
    // 实名侧：缺链 / 悬空 / 陈链一律"目标不真"，落 Target。
    let mut l = 0usize;
    while l < LINK_COUNT {
        let link = match LinkState::at(l) {
            Some(x) => x,
            None => return false,
        };
        let s = Shape {
            link,
            ..Shape::blank()
        };
        let o = define(&s);
        if matches!(link, LinkState::Absent | LinkState::Dangling | LinkState::Stale) {
            if o.stage != Stage::Target || o.note != Note::TargetDangling {
                return false;
            }
            if o.ok() || o.value != 0 {
                return false;
            }
        } else if !o.ok() {
            return false;
        }
        l += 1;
    }
    // 清单里没有一条悬空链，也没有任何未名行留下链。
    let mut i = 0usize;
    while i < LIST_LEN {
        let e = match LIST.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if e.dangling {
            return false;
        }
        if e.class.speaks() && e.link_present {
            return false;
        }
        i += 1;
    }
    // 八病表里三种"链病"都在，且都破坏不变量。
    Defect::FakeLink.breaks_link() && Defect::Dangling.breaks_link() && Defect::StaleLink.breaks_link()
}

/// 06 跳过不改退出码；127 只属于直接执行路径。
fn code_ok() -> bool {
    let clean = Shape {
        class: Class::NoSuch,
        name: NOSUCH_TWO,
        link: LinkState::Absent,
        skip_declared: 1,
        ..Shape::blank()
    };
    let o = define(&clean);
    if o.code != EXIT_OK as i32 || !o.ok() {
        return false;
    }
    let bumped = Shape {
        exit_declared: 2,
        ..clean
    };
    let b = define(&bumped);
    if b.stage != Stage::Code || b.note != Note::ExitBumped {
        return false;
    }
    if b.code != 2 || b.ok() {
        return false;
    }
    let pass = Shape {
        exit_declared: EXIT_NOTFOUND,
        ..clean
    };
    let p = define(&pass);
    if p.stage != Stage::Code || p.note != Note::PassCode127 {
        return false;
    }
    if p.code != EXIT_NOTFOUND as i32 || p.ok() {
        return false;
    }
    // 缺目录：用法错，一字不落盘。
    let nodir = Shape {
        invoke: Invoke::NoDir,
        ..clean
    };
    let n = define(&nodir);
    if n.stage != Stage::Invoke || n.note != Note::UsageNoDir || n.code != EXIT_USAGE as i32 {
        return false;
    }
    // 普通运行：兜底，不进结论。
    let plain = Shape {
        invoke: Invoke::Plain,
        ..Shape::blank()
    };
    let q = define(&plain);
    if q.stage != Stage::Invoke || q.note != Note::NotInstall || !q.default {
        return false;
    }
    // 跳过行与执行行的码档必须不同（同一串字节）。
    if msg_code(MsgKind::SkipOnStderr) == msg_code(MsgKind::NotFoundExec) {
        return false;
    }
    msg_usage().as_str() == "busybox: --install: no directory specified\n"
}

/// 07 通道：跳过行只许走 stderr。
fn emit_ok() -> bool {
    let stderr = Shape {
        class: Class::NoSuch,
        name: NOSUCH_ONE,
        link: LinkState::Absent,
        skip_declared: 1,
        ..Shape::blank()
    };
    let good = define(&stderr);
    if good.stage != Stage::Report || good.note != Note::SkipLogged || !good.ok() {
        return false;
    }
    let misrouted = Shape {
        line: LineRoute::Stdout,
        ..stderr
    };
    let bad = define(&misrouted);
    if bad.stage != Stage::Emit || bad.note != Note::LineOnStdout {
        return false;
    }
    if bad.consistent || bad.ok() {
        return false;
    }
    // 通道判断本身：stderr 是诊断流，stdout 不是。
    if !LineRoute::Stderr.is_diag() || LineRoute::Stdout.is_diag() {
        return false;
    }
    // 两条流的字一模一样，只有通道不同（所以这一维只能靠通道判）。
    let a = msg_text(MsgKind::SkipOnStderr, NOSUCH_ONE);
    let b = msg_text(MsgKind::SkipOnStdout, NOSUCH_ONE);
    if a.as_str() != b.as_str() {
        return false;
    }
    if !msg_route(MsgKind::SkipOnStdout).is_diag() {
        return true;
    }
    false
}

/// 08 语料十四槽互校。
fn corpus_ok() -> bool {
    let mut i = 0usize;
    while i < CORPUS {
        let c = match case_at(i) {
            Some(x) => x,
            None => return false,
        };
        let a = c.actual();
        let want_stage = match WANT_STAGE.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let want_code = match WANT_CODE.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let want_value = match WANT_VALUE.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let want_note = match WANT_NOTE.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let want_default = match WANT_DEFAULT.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let want_success = match WANT_SUCCESS.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if a.stage != want_stage
            || a.code as u32 != want_code
            || a.value != want_value
            || a.note != want_note
            || a.default != want_default
            || a.ok() != want_success
        {
            return false;
        }
        if i >= ACCEPT {
            let ndx = match WANT_NDX.get(i) {
                Some(x) => *x,
                None => return false,
            };
            let k = &c.claim;
            let mut offs = 0usize;
            if a.ok() != k.success() {
                offs += 1;
            }
            if a.stage != k.stage {
                offs += 1;
            }
            if a.code != k.code {
                offs += 1;
            }
            if a.value != k.value {
                offs += 1;
            }
            if a.note != k.note {
                offs += 1;
            }
            if a.default != k.default {
                offs += 1;
            }
            if offs != 1 {
                return false;
            }
            if fault_ndx(c.fault()) != ndx {
                return false;
            }
        }
        i += 1;
    }
    let mut t = 0usize;
    while t < CORPUS {
        let a = match WANT_TAGS.get(t) {
            Some(x) => *x,
            None => return false,
        };
        if !a.starts_with("f11672-") {
            return false;
        }
        let mut u = t + 1usize;
        while u < CORPUS {
            if let Some(b) = WANT_TAGS.get(u) {
                if *b == a {
                    return false;
                }
            }
            u += 1;
        }
        t += 1;
    }
    WANT_STAGE.len() == CORPUS
        && WANT_CODE.len() == CORPUS
        && WANT_VALUE.len() == CORPUS
        && WANT_NOTE.len() == CORPUS
        && WANT_DEFAULT.len() == CORPUS
        && WANT_SUCCESS.len() == CORPUS
        && WANT_NDX.len() == CORPUS
}

/// 09 判序六维互斥。
fn judge_ok() -> bool {
    let mut good = Case::blank();
    good.shape = Shape::blank();
    good.claim = Claim {
        success: true,
        stage: Stage::Report,
        code: EXIT_OK as i32,
        value: 1,
        note: Note::LinkMade,
        default: false,
    };
    if judge(&good) != Fault::Ok {
        return false;
    }
    // 同时坏 Estate 与 Stage：必须报 Estate。
    let mut both = good;
    both.claim.success = false;
    both.claim.stage = Stage::Decide;
    if judge(&both) != Fault::Estate {
        return false;
    }
    // 同时坏 Stage 与 Code：必须报 Stage。
    let mut sc = good;
    sc.claim.stage = Stage::Emit;
    sc.claim.code = 2;
    if judge(&sc) != Fault::Stage {
        return false;
    }
    // 同时坏 Code 与 Value：必须报 Code。
    let mut cv = good;
    cv.claim.code = 2;
    cv.claim.value = 9;
    if judge(&cv) != Fault::Code {
        return false;
    }
    // 同时坏 Value 与 Note：必须报 Value。
    let mut vn = good;
    vn.claim.value = 9;
    vn.claim.note = Note::PseudoQuiet;
    if judge(&vn) != Fault::Value {
        return false;
    }
    // 同时坏 Note 与 Default：必须报 Note。
    let mut nd = good;
    nd.claim.note = Note::PseudoQuiet;
    nd.claim.default = true;
    if judge(&nd) != Fault::Note {
        return false;
    }
    // 只坏 Default：报 Default。
    let mut d = good;
    d.claim.default = true;
    if judge(&d) != Fault::Default {
        return false;
    }
    // 现场真落兜底时，怎么申报都不许当结论。
    let mut fallback = Case::blank();
    fallback.shape = Shape::blank().with_invoke(Invoke::Plain);
    fallback.claim = Claim {
        success: true,
        stage: Stage::Invoke,
        code: EXIT_OK as i32,
        value: 0,
        note: Note::NotInstall,
        default: true,
    };
    judge(&fallback) == Fault::Default
}

/// 10 四档矩阵覆盖。
fn matrix_ok() -> bool {
    let m = Matrix::build();
    if m.slots() != CORPUS * TIER_COUNT {
        return false;
    }
    if m.goods() != CORPUS {
        return false;
    }
    if m.red_total() + m.goods() != m.slots() {
        return false;
    }
    if !m.covered() || m.perfect_rows() != 0 {
        return false;
    }
    let want = [
        (Tier::Linked, 4usize),
        (Tier::Untouched, 7usize),
        (Tier::Faulted, 2usize),
        (Tier::Fallback, 1usize),
    ];
    let mut i = 0usize;
    while i < want.len() {
        let (tier, good) = match want.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let row = match m.row(tier) {
            Some(r) => r,
            None => return false,
        };
        if row.good != good || row.total != CORPUS {
            return false;
        }
        i += 1;
    }
    // 抽样归档：
    let made = define(&Shape::blank());
    if Tier::of(&made) != Tier::Linked {
        return false;
    }
    let skipped = define(&Shape {
        class: Class::NoSuch,
        name: NOSUCH_ONE,
        link: LinkState::Absent,
        skip_declared: 1,
        ..Shape::blank()
    });
    if Tier::of(&skipped) != Tier::Untouched {
        return false;
    }
    let fake = define(&Shape {
        class: Class::NoSuch,
        name: NOSUCH_ONE,
        link: LinkState::Created,
        skip_declared: 1,
        ..Shape::blank()
    });
    if Tier::of(&fake) != Tier::Faulted {
        return false;
    }
    let plain = define(&Shape {
        invoke: Invoke::Plain,
        ..Shape::blank()
    });
    Tier::of(&plain) == Tier::Fallback
}

/// 11 全绿渲染逐字。
fn render_ok() -> bool {
    match run() {
        Ok(rep) => rep.green() && rep.render().as_str() == RENDER_OK && rep.rate() == RATE_FULL,
        Err(_) => false,
    }
}

/// 12 阶段闸门不倒退。
fn gate_ok() -> bool {
    let root = match Gate::full_pass() {
        Ok(h) => h,
        Err(_) => return false,
    };
    if root == 0 {
        return false;
    }
    let mut g = Gate::new();
    if g.step(Stage::Target) != Ok(()) {
        return false;
    }
    if g.step(Stage::Decide) != Err(INS_ESTAGE) {
        return false;
    }
    if g.stage() != Some(Stage::Target) || g.refusals() != 1 {
        return false;
    }
    let mut closed = match Gate::walk(&Stage::all()) {
        Ok(x) => x,
        Err(_) => return false,
    };
    if closed.close().is_err() {
        return false;
    }
    if closed.step(Stage::Report) != Err(INS_ESTATE) {
        return false;
    }
    if closed.close() != Err(INS_ESTATE) {
        return false;
    }
    let mut empty = Gate::new();
    empty.close() == Err(INS_ESTAGE) && closed.is_closed()
}

/// 13 冻结重放四注入（账本路本体，四注入在下面那条判据里合取）。
fn ledger_ok() -> bool {
    let mut l = build_ledger();
    if l.len() != SAMPLES || l.audit() != Ok(()) {
        return false;
    }
    // 十四格全部"判对"（八统计 + 一红档 + 六缺陷码，缺陷格是应落码）。
    if l.ok_count() != SAMPLES || l.err_count() != 0 {
        return false;
    }
    if l.first_mismatch().is_some() {
        return false;
    }
    let h = match l.freeze() {
        Ok(x) => x,
        Err(_) => return false,
    };
    if h == 0 || !l.is_frozen() || l.replay() != Ok(h) {
        return false;
    }
    let mut w = 0u8;
    while w < 4u8 {
        let broken = build_ledger_broken(w);
        if broken.audit().is_err() != true {
            return false;
        }
        w += 1;
    }
    if build_ledger_broken(9).audit() != Ok(()) {
        return false;
    }
    let mut wrong = Ledger::new();
    let stray = Sample {
        slot: 3,
        op: OP_INVOKE,
        ok: true,
        code: 0,
        value: 1,
    };
    wrong.register(stray) == Err(INS_ESLOT) && wrong.rejects() == 1 && wrong.is_empty()
}

/// 申报表第一格的标签。
pub fn head_tag() -> &'static str {
    match WANT_TAGS.get(0) {
        Some(x) => *x,
        None => "f11672-ls-install-soft",
    }
}

/// 统计表第一格的值。
pub fn head_val() -> u32 {
    match VALS.get(0) {
        Some(x) => *x,
        None => 0,
    }
}

/// 汇总：十三条判据，一条不多一条不少。
pub fn run_unx11672_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11672_install_skip");
    s.add("INS01-名称与段基址", name_ok(), "static");
    s.add("INS02-十三码互异", codes_ok(), "static");
    s.add("INS03-安装清单十四行", list_ok(), "static");
    s.add("INS04-未名跳过留一行", skip_ok(), "static");
    s.add("INS05-不建伪链不悬空", link_ok(), "static");
    s.add("INS06-跳过不改退出码", code_ok(), "static");
    s.add("INS07-通道只走stderr", emit_ok(), "static");
    s.add("INS08-语料十四槽互校", corpus_ok(), "static");
    s.add("INS09-判序六维互斥", judge_ok(), "static");
    s.add("INS10-四档矩阵覆盖", matrix_ok(), "static");
    s.add("INS11-全绿渲染逐字", render_ok(), "static");
    s.add("INS12-阶段闸门不倒退", gate_ok(), "static");
    s.add(
        "INS13-冻结重放四注入",
        {
            let mut l = build_ledger();
            let audited = l.audit() == Ok(());
            let frozen = match l.freeze() {
                Ok(r) => r != 0 && l.is_frozen() && l.replay() == Ok(r),
                Err(_) => false,
            };
            let four = inject_four();
            let mut all_four = true;
            let mut i = 0usize;
            while i < four.len() {
                if let Some(x) = four.get(i) {
                    if !*x {
                        all_four = false;
                    }
                }
                i += 1;
            }
            let eight = inject_eight();
            let mut all_eight = true;
            let mut k = 0usize;
            while k < eight.len() {
                if let Some(x) = eight.get(k) {
                    if !*x {
                        all_eight = false;
                    }
                }
                k += 1;
            }
            audited && frozen && ledger_ok() && all_four && all_eight
        },
        "static",
    );
    s
}

// ---------------------------------------------------------------------------
// 单测：逐条钉住本卡的每一个数、每一道闸、每一个码
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 夹具取不到时的出口（只在单测里出现，生产路径一概不 panic）。
    fn panic_free_false() -> ! {
        unreachable!("test fixture slot out of range")
    }

    // --- 段基址与锚点 ------------------------------------------------------

    #[test]
    fn t01_seg_base_is_pinned() {
        assert_eq!(SEG_BASE, 0x6d250);
        assert_eq!(seg_text(SEG_BASE).as_str(), "0x06d250");
    }

    #[test]
    fn t02_anchor_gaps_line_up() {
        assert_eq!(ANCHOR_F11671_SEG + ANCHOR_F11671_GAP, SEG_BASE);
        assert_eq!(SEG_BASE + ANCHOR_F11673_GAP, ANCHOR_F11673_SEG);
        assert_eq!(ANCHOR_F11673_SEG, 0x6d260);
        assert_eq!(ANCHOR_F11674_SEG, 0x6d270);
        assert!(ANCHOR_F11664_SEG < SEG_BASE);
        assert!(ANCHOR_F11584_SEG < SEG_BASE);
    }

    #[test]
    fn t03_thirteen_codes_are_sequential() {
        let mut i = 0usize;
        while i < 13 {
            if let Some(c) = ALL_CODES.get(i) {
                assert_eq!(*c, SEG_BASE + 1 + i as u32);
            } else {
                assert!(false);
            }
            i += 1;
        }
    }

    #[test]
    fn t04_thirteen_codes_are_distinct() {
        let mut i = 0usize;
        while i < 13 {
            let mut k = i + 1;
            while k < 13 {
                if let (Some(a), Some(b)) = (ALL_CODES.get(i), ALL_CODES.get(k)) {
                    assert!(a != b);
                }
                k += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn t05_scale_constants_agree() {
        assert_eq!(CORPUS, 14);
        assert_eq!(SAMPLES, 14);
        assert_eq!(ACCEPT, 8);
        assert_eq!(REJECT, 6);
        assert_eq!(TIER_COUNT, 4);
        assert_eq!(ACCEPT + REJECT, CORPUS);
        assert_eq!(SAMPLES, CORPUS);
        assert_eq!(LIST_LEN, CORPUS);
        assert_eq!(STAT_CELLS + 1 + REJECT, SAMPLES);
    }

    // --- FNV-1a 折叠 ------------------------------------------------------

    #[test]
    fn t06_fnv_known_vector() {
        assert_eq!(fold_byte(FNV_OFF, b'a'), 0xaf63_dc4c_8601_ec8c);
    }

    #[test]
    fn t07_fnv_empty_is_offset() {
        assert_eq!(fold_str(FNV_OFF, ""), FNV_OFF);
        assert_eq!(fold_word(FNV_OFF, 0), {
            let mut h = FNV_OFF;
            let mut i = 0u32;
            while i < 8 {
                h = fold_byte(h, 0);
                i += 1;
            }
            h
        });
    }

    #[test]
    fn t08_fnv_distinguishes_names() {
        let a = fold_str(FNV_OFF, "ls");
        let b = fold_str(FNV_OFF, "cp");
        let c = fold_str(FNV_OFF, "sl");
        assert!(a != b);
        assert!(a != c);
    }

    #[test]
    fn t09_fnv_order_matters() {
        let mut h1 = FNV_OFF;
        h1 = fold_byte(h1, 1);
        h1 = fold_byte(h1, 2);
        let mut h2 = FNV_OFF;
        h2 = fold_byte(h2, 2);
        h2 = fold_byte(h2, 1);
        assert!(h1 != h2);
    }

    #[test]
    fn t10_fold_i32_sign_preserved() {
        assert_eq!(fold_i32(FNV_OFF, -1), fold_word(FNV_OFF, 0xffff_ffff));
        assert!(fold_i32(FNV_OFF, -1) != fold_i32(FNV_OFF, 1));
    }

    #[test]
    fn t11_fold_bool_is_one_byte() {
        assert_eq!(fold_bool(FNV_OFF, true), fold_byte(FNV_OFF, 1));
        assert_eq!(fold_bool(FNV_OFF, false), fold_byte(FNV_OFF, 0));
        assert!(fold_bool(FNV_OFF, true) != fold_bool(FNV_OFF, false));
    }

    // --- 文本助手（无分配式拼接） ----------------------------------------

    #[test]
    fn t12_dec_text_basics() {
        assert_eq!(dec_text(0).as_str(), "0");
        assert_eq!(dec_text(7).as_str(), "7");
        assert_eq!(dec_text(929).as_str(), "929");
        assert_eq!(dec_text(4294967295).as_str(), "4294967295");
        assert_eq!(u32_text(14).as_str(), "14");
    }

    #[test]
    fn t13_hex_and_seg_text() {
        assert_eq!(hex_text(0, 5).as_str(), "00000");
        assert_eq!(hex_text(0xff, 2).as_str(), "ff");
        assert_eq!(hex_text(0x6d250, 6).as_str(), "06d250");
        assert_eq!(seg_text(0x6d250).as_str(), "0x06d250");
    }

    #[test]
    fn t14_i32_text_negative() {
        assert_eq!(i32_text(-1).as_str(), "-1");
        assert_eq!(i32_text(-127).as_str(), "-127");
        assert_eq!(i32_text(0).as_str(), "0");
        assert_eq!(i32_text(127).as_str(), "127");
    }

    #[test]
    fn t15_permille_text() {
        assert_eq!(permille_text(929).as_str(), "929permille");
        assert_eq!(permille_text(RATE_FULL).as_str(), "1000permille");
        assert_eq!(permille_text(0).as_str(), "0permille");
    }

    #[test]
    fn t16_lines_and_bytes() {
        assert_eq!(lines_of(""), 0);
        assert_eq!(lines_of("a\nb\n"), 2);
        assert_eq!(lines_of("no newline"), 0);
        assert_eq!(bytes_of("abc"), 3);
        assert_eq!(bytes_of(NOT_FOUND_TAIL), 18);
    }

    #[test]
    fn t17_link_path_shape() {
        assert_eq!(link_path("ls").as_str(), "/bin/ls");
        assert_eq!(link_path(NOSUCH_DASH).as_str(), "/bin/ls-zzz");
        assert_eq!(INSTALL_DIR, "/bin");
    }

    #[test]
    fn t18_skip_message_is_notfound_bytes() {
        let m = msg_skip(NOSUCH_ONE);
        assert_eq!(m.as_str(), "foo: applet not found\n");
        assert_eq!(bytes_of(m.as_str()), 22);
        assert_eq!(bytes_of(msg_skip(NOSUCH_DASH).as_str()), 25);
        assert_eq!(lines_of(m.as_str()), 1);
    }

    #[test]
    fn t19_usage_and_create_messages() {
        assert_eq!(
            msg_usage().as_str(),
            "busybox: --install: no directory specified\n"
        );
        assert_eq!(
            msg_create("ls").as_str(),
            "busybox: can't create /bin/ls: File exists\n"
        );
        assert!(msg_quiet().is_empty());
    }

    #[test]
    fn t20_msg_cells_route_and_code() {
        assert_eq!(msg_code(MsgKind::SkipOnStderr), EXIT_OK);
        assert_eq!(msg_code(MsgKind::SkipOnStdout), EXIT_OK);
        assert_eq!(msg_code(MsgKind::NotFoundExec), EXIT_NOTFOUND);
        assert_eq!(msg_route(MsgKind::SkipOnStderr), LineRoute::Stderr);
        assert_eq!(msg_route(MsgKind::SkipOnStdout), LineRoute::Stdout);
        assert_eq!(msg_route(MsgKind::NotFoundExec), LineRoute::Stderr);
        assert!(msg_text(MsgKind::QuietPseudo, "x").is_empty());
    }

    #[test]
    fn t21_msg_table_is_six_cells() {
        assert_eq!(MESSAGE_CELLS, 6);
        let mut i = 0usize;
        while i < MESSAGE_CELLS {
            let k = match MsgKind::at(i) {
                Some(x) => x,
                None => panic_free_false(),
            };
            assert!(msg_cell(k).is_some());
            i += 1;
        }
        assert!(MsgKind::at(MESSAGE_CELLS).is_none());
    }

    #[test]
    fn t22_same_bytes_two_codes() {
        assert!(same_bytes_two_codes(NOSUCH_ONE));
        assert!(same_bytes_two_codes(NOSUCH_DASH));
        assert_eq!(
            bytes_of(msg_text(MsgKind::SkipOnStderr, "foo").as_str()),
            bytes_of(msg_text(MsgKind::NotFoundExec, "foo").as_str())
        );
    }

    #[test]
    fn t23_subst_replaces_placeholder() {
        assert_eq!(
            msg_text(MsgKind::SkipOnStderr, "foo").as_str(),
            "foo: applet not found\n"
        );
        assert_eq!(subst("plain", "x").as_str(), "plain");
        assert_eq!(subst("{N}!{N}", "a").as_str(), "a!a");
    }

    // --- 域模型：调用形态 --------------------------------------------------

    #[test]
    fn t24_invoke_table_roundtrip() {
        let all = Invoke::all();
        assert_eq!(all.len(), INVOKE_COUNT);
        let mut i = 0usize;
        while i < all.len() {
            if let Some(v) = all.get(i) {
                assert_eq!(v.index(), i);
                assert_eq!(Invoke::at(i), Some(*v));
            }
            i += 1;
        }
        assert!(Invoke::at(INVOKE_COUNT).is_none());
    }

    #[test]
    fn t25_invoke_predicates() {
        assert!(Invoke::Soft.is_install());
        assert!(Invoke::Hard.is_install());
        assert!(!Invoke::NoDir.is_install());
        assert!(!Invoke::Plain.is_install());
        assert!(Invoke::Soft.is_soft());
        assert!(!Invoke::Hard.is_soft());
        assert!(!Invoke::Plain.judged());
        assert!(!Invoke::NoDir.has_dir());
        assert!(Invoke::Soft.has_dir());
        assert_eq!(INVOKE_JUDGED, 3);
    }

    // --- 域模型：名字身份 --------------------------------------------------

    #[test]
    fn t26_class_table_roundtrip() {
        let all = Class::all();
        assert_eq!(all.len(), CLASS_COUNT);
        let mut i = 0usize;
        while i < all.len() {
            if let Some(c) = all.get(i) {
                assert_eq!(c.index(), i);
                assert_eq!(Class::at(i), Some(*c));
            }
            i += 1;
        }
        assert!(Class::at(CLASS_COUNT).is_none());
    }

    #[test]
    fn t27_class_predicates() {
        assert!(Class::Real.known());
        assert!(Class::Alias.known());
        assert!(Class::Pseudo.known());
        assert!(!Class::NoSuch.known());
        assert!(Class::Real.installs());
        assert!(Class::Alias.installs());
        assert!(!Class::Pseudo.installs());
        assert!(!Class::NoSuch.installs());
        assert!(Class::NoSuch.speaks());
        assert!(!Class::Real.speaks());
        assert!(Class::Pseudo.quiet());
        assert!(!Class::NoSuch.quiet());
    }

    // --- 域模型：链接状态 --------------------------------------------------

    #[test]
    fn t28_link_state_table_roundtrip() {
        let all = LinkState::all();
        assert_eq!(all.len(), LINK_COUNT);
        let mut i = 0usize;
        while i < all.len() {
            if let Some(l) = all.get(i) {
                assert_eq!(l.index(), i);
                assert_eq!(LinkState::at(i), Some(*l));
            }
            i += 1;
        }
        assert!(LinkState::at(LINK_COUNT).is_none());
    }

    #[test]
    fn t29_link_state_effect_and_truth() {
        assert_eq!(LinkState::Created.effect(), 1);
        assert_eq!(LinkState::Reused.effect(), 0);
        assert_eq!(LinkState::Dangling.effect(), 0);
        assert!(LinkState::Created.target_real());
        assert!(LinkState::Reused.target_real());
        assert!(!LinkState::Dangling.target_real());
        assert!(LinkState::Dangling.built_now());
        assert!(!LinkState::Stale.built_now());
        assert!(!LinkState::Absent.exists());
        assert!(LinkState::Stale.exists());
    }

    // --- 域模型：日志通道 --------------------------------------------------

    #[test]
    fn t30_route_is_diag_only_stderr() {
        assert!(LineRoute::Stderr.is_diag());
        assert!(!LineRoute::Stdout.is_diag());
        assert_eq!(LineRoute::all().len(), 2);
        assert_eq!(LineRoute::Stderr.index(), 0);
        assert_eq!(LineRoute::Stdout.index(), 1);
    }

    // --- 域模型：八病 ------------------------------------------------------

    #[test]
    fn t31_defect_table_roundtrip() {
        let all = Defect::all();
        assert_eq!(all.len(), DEFECT_COUNT);
        let mut i = 0usize;
        while i < all.len() {
            if let Some(d) = all.get(i) {
                assert_eq!(d.index(), i);
                assert_eq!(Defect::at(i), Some(*d));
            }
            i += 1;
        }
        assert!(Defect::at(DEFECT_COUNT).is_none());
    }

    #[test]
    fn t32_defect_flags() {
        assert!(!Defect::None.is_fault());
        assert!(Defect::FakeLink.is_fault());
        assert!(Defect::FakeLink.breaks_link());
        assert!(Defect::Dangling.breaks_link());
        assert!(Defect::StaleLink.breaks_link());
        assert!(!Defect::SkipExit.breaks_link());
        assert!(Defect::SkipExit.is_code());
        assert!(Defect::Code127.is_code());
        assert!(!Defect::Misroute.is_code());
        assert!(Defect::Misroute.is_emit());
        assert!(!Defect::Duplicate.is_emit());
    }

    // --- 域模型：注解档 ----------------------------------------------------

    #[test]
    fn t33_note_table_counts() {
        let all = Note::all();
        assert_eq!(all.len(), NOTE_COUNT);
        let mut live = 0u32;
        let mut defect = 0u32;
        let mut defaulted = 0u32;
        let mut i = 0usize;
        while i < all.len() {
            if let Some(n) = all.get(i) {
                assert_eq!(n.index(), i);
                if n.live() {
                    live += 1;
                }
                if n.defect() {
                    defect += 1;
                }
                if n.defaulted() {
                    defaulted += 1;
                }
            }
            i += 1;
        }
        assert_eq!(live, 5);
        assert_eq!(defect, DEFECT_FAULTS as u32);
        assert_eq!(defaulted, 1);
    }

    #[test]
    fn t34_note_skip_green_is_two_cells() {
        assert!(Note::SkipLogged.skip_green());
        assert!(Note::PseudoQuiet.skip_green());
        assert!(!Note::LinkMade.skip_green());
        assert!(!Note::FakeLink.skip_green());
        assert!(Note::LinkMade.links());
        assert!(Note::AliasLink.links());
        assert!(!Note::AlreadyLinked.links());
        assert!(Note::SkipLogged.has_line());
        assert!(!Note::PseudoQuiet.has_line());
    }

    #[test]
    fn t35_note_unknown_side_invariant_side() {
        assert!(Note::SkipLogged.on_unknown_side());
        assert!(Note::StaleLeft.on_unknown_side());
        assert!(Note::FakeLink.on_unknown_side());
        assert!(!Note::LinkMade.on_unknown_side());
        assert!(!Note::AlreadyLinked.on_unknown_side());
    }

    // --- 域模型：阶段 ------------------------------------------------------

    #[test]
    fn t36_stage_ranks_are_monotone() {
        let all = Stage::all();
        assert_eq!(all.len(), STAGE_COUNT as usize);
        let mut i = 0usize;
        while i < all.len() {
            if let Some(s) = all.get(i) {
                assert_eq!(s.rank(), 1 + i as u32);
                assert_eq!(Stage::from_rank(s.rank()), Some(*s));
            }
            i += 1;
        }
        assert_eq!(STAGE_LAST_RANK, STAGE_COUNT);
        assert!(Stage::from_rank(0).is_none());
        assert!(Stage::from_rank(STAGE_LAST_RANK + 1).is_none());
    }

    #[test]
    fn t37_stage_disk_split() {
        assert!(Stage::Invoke.before_disk());
        assert!(Stage::Decide.before_disk());
        assert!(!Stage::Target.before_disk());
        assert!(Stage::Emit.after_disk());
        assert!(Stage::Report.after_disk());
        assert!(!Stage::Class.after_disk());
    }

    // --- 域模型：缺陷维 ----------------------------------------------------

    #[test]
    fn t38_fault_order_is_fixed() {
        assert_eq!(REJECT_FAULTS, [
            Fault::Estate,
            Fault::Stage,
            Fault::Code,
            Fault::Value,
            Fault::Note,
            Fault::Default,
        ]);
        let mut i = 0usize;
        while i < REJECT {
            if let Some(f) = REJECT_FAULTS.get(i) {
                assert_eq!(f.order(), i);
                assert_eq!(fault_ndx(*f), i);
                assert_eq!(ndx_fault(i), Some(*f));
            }
            i += 1;
        }
        assert_eq!(Fault::Ok.order(), 6);
        assert_eq!(ndx_fault(6), Some(Fault::Ok));
        assert!(ndx_fault(7).is_none());
    }

    #[test]
    fn t39_fault_codes_match_table() {
        let mut i = 0usize;
        while i < REJECT {
            if let (Some(f), Some(c)) = (REJECT_FAULTS.get(i), REJECT_CODES.get(i)) {
                assert_eq!(f.code(), *c);
                assert_ne!(*c, 0);
            }
            i += 1;
        }
        assert_eq!(Fault::Ok.code(), 0);
        assert_eq!(Fault::Estate.code(), INS_ESTATE);
        assert_eq!(Fault::Default.code(), INS_EDEFAULT);
    }

    #[test]
    fn t40_fault_short_names() {
        assert_eq!(fault_short(Fault::Ok), "OK");
        assert_eq!(fault_short(Fault::Estate), "ESTATE");
        assert_eq!(fault_short(Fault::Stage), "ESTAGE");
        assert_eq!(fault_short(Fault::Code), "ECODE");
        assert_eq!(fault_short(Fault::Value), "EVALUE");
        assert_eq!(fault_short(Fault::Note), "ENOTE");
        assert_eq!(fault_short(Fault::Default), "EDEFAUL");
    }

    // --- 安装清单 ----------------------------------------------------------

    #[test]
    fn t41_list_row_counts() {
        assert_eq!(created_rows(), LINK_ROWS);
        assert_eq!(created_rows(), 8);
        assert_eq!(present_rows(), 9);
        assert_eq!(skip_rows(), SKIP_ROWS);
        assert_eq!(skip_rows(), 3);
        assert_eq!(pseudo_rows(), PSEUDO_ROWS);
        assert_eq!(pseudo_rows(), 2);
        assert_eq!(reused_rows(), REUSE_ROWS);
        assert_eq!(reused_rows(), 1);
        assert_eq!(skip_bytes_total(), 69);
    }

    #[test]
    fn t42_list_classes_agree_with_known_flag() {
        let mut i = 0usize;
        while i < LIST_LEN {
            let e = match LIST.get(i) {
                Some(x) => *x,
                None => panic_free_false(),
            };
            assert_eq!(e.class.known(), e.known);
            assert!(!e.dangling);
            assert_eq!(e.exit, EXIT_OK);
            i += 1;
        }
        assert!(entry_at(LIST_LEN).is_none());
    }

    #[test]
    fn t43_list_every_row_is_self_consistent() {
        let mut i = 0usize;
        while i < LIST_LEN {
            let e = match LIST.get(i) {
                Some(x) => *x,
                None => panic_free_false(),
            };
            assert!(entry_ok(&e));
            assert!(name_shape_ok(e.name));
            i += 1;
        }
    }

    #[test]
    fn t44_list_skips_leave_one_line_each() {
        let mut i = 0usize;
        while i < LIST_LEN {
            let e = match LIST.get(i) {
                Some(x) => *x,
                None => panic_free_false(),
            };
            if e.class.speaks() {
                assert!(e.skipped());
                assert!(!e.link_created);
                assert!(!e.link_present);
                assert_eq!(e.stderr_bytes, bytes_of(msg_skip(e.name).as_str()));
            }
            i += 1;
        }
    }

    #[test]
    fn t45_list_quiet_pseudo_rows() {
        let mut i = 0usize;
        while i < LIST_LEN {
            let e = match LIST.get(i) {
                Some(x) => *x,
                None => panic_free_false(),
            };
            if e.class.quiet() {
                assert!(e.quiet());
                assert_eq!(e.stderr_bytes, 0);
                assert!(!e.link_created);
                assert!(!e.link_present);
            }
            i += 1;
        }
    }

    #[test]
    fn t46_list_reused_row_is_cp() {
        let mut found = false;
        let mut i = 0usize;
        while i < LIST_LEN {
            let e = match LIST.get(i) {
                Some(x) => *x,
                None => panic_free_false(),
            };
            if e.reused() {
                found = true;
                assert_eq!(e.name, "cp");
                assert!(e.link_present);
                assert!(!e.link_created);
            }
            i += 1;
        }
        assert!(found);
    }

    #[test]
    fn t47_entry_render_is_pinned() {
        let e = match LIST.get(0) {
            Some(x) => *x,
            None => panic_free_false(),
        };
        assert_eq!(
            e.render().as_str(),
            "entry ls class=real link=true exit=0 bytes=0 dangling=false"
        );
    }

    #[test]
    fn t48_entry_seal_changes_with_exit() {
        let e = match LIST.get(0) {
            Some(x) => *x,
            None => panic_free_false(),
        };
        let mut f = e;
        f.exit = 2;
        assert!(e.seal() != f.seal());
        assert_eq!(e.seal(), e.seal());
    }

    // --- 语料与期望表 ------------------------------------------------------

    #[test]
    fn t49_corpus_slots_all_present() {
        let mut i = 0usize;
        while i < CORPUS {
            assert!(case_at(i).is_some());
            assert!(SHAPES.get(i).is_some());
            assert!(CLAIMS.get(i).is_some());
            i += 1;
        }
        assert!(case_at(CORPUS).is_none());
        assert_eq!(cases().len(), CORPUS);
    }

    #[test]
    fn t50_accept_slots_are_all_green() {
        let mut i = 0usize;
        while i < ACCEPT {
            let c = match case_at(i) {
                Some(x) => x,
                None => panic_free_false(),
            };
            assert_eq!(c.fault(), Fault::Ok);
            assert!(c.ok());
            i += 1;
        }
    }

    #[test]
    fn t51_reject_slots_hit_their_own_dimension() {
        let mut i = ACCEPT;
        while i < CORPUS {
            let c = match case_at(i) {
                Some(x) => x,
                None => panic_free_false(),
            };
            assert_ne!(c.fault(), Fault::Ok);
            assert_eq!(c.fault(), expected_reject_ndx(i));
            assert_eq!(c.fault(), REJECT_FAULTS[i - ACCEPT]);
            i += 1;
        }
    }

    #[test]
    fn t52_want_tables_match_live_corpus() {
        let mut i = 0usize;
        while i < CORPUS {
            let c = match case_at(i) {
                Some(x) => x,
                None => panic_free_false(),
            };
            let a = c.actual();
            assert_eq!(a.stage, WANT_STAGE[i]);
            assert_eq!(a.code as u32, WANT_CODE[i]);
            assert_eq!(a.value, WANT_VALUE[i]);
            assert_eq!(a.note, WANT_NOTE[i]);
            assert_eq!(a.default, WANT_DEFAULT[i]);
            assert_eq!(a.ok(), WANT_SUCCESS[i]);
            assert_eq!(fault_ndx(c.fault()), WANT_NDX[i]);
            assert_eq!(c.tag, WANT_TAGS[i]);
            i += 1;
        }
    }

    #[test]
    fn t53_tags_all_carry_card_id() {
        let mut i = 0usize;
        while i < CORPUS {
            if let Some(t) = WANT_TAGS.get(i) {
                assert!(t.starts_with("f11672-"));
            }
            i += 1;
        }
        assert_eq!(head_tag(), WANT_TAGS[0]);
    }

    #[test]
    fn t54_expected_lookup_bounds() {
        assert_eq!(expected(0), Some(Fault::Ok));
        assert_eq!(expected(ACCEPT), Some(Fault::Estate));
        assert_eq!(expected(CORPUS - 1), Some(Fault::Default));
        assert!(expected(CORPUS).is_none());
        assert_eq!(cover(CORPUS), Ok(()));
        assert_eq!(cover(CORPUS + 1), Err(INS_ESLOT));
    }

    #[test]
    fn t55_claim_table_shapes() {
        let mut i = 0usize;
        while i < CORPUS {
            let c = match case_at(i) {
                Some(x) => x,
                None => panic_free_false(),
            };
            assert_eq!(c.claim.success, c.claim.success());
            assert_eq!(c.claim.value <= 2, true);
            i += 1;
        }
        assert_eq!(GOOD_CLAIM.stage, Stage::Report);
        assert_eq!(GOOD_CLAIM.code, 0);
        assert_eq!(GOOD_CLAIM.value, 1);
    }

    // --- 判据本体：七道闸 ------------------------------------------------

    #[test]
    fn t56_blank_shape_installs() {
        let a = define(&Shape::blank());
        assert_eq!(a.stage, Stage::Report);
        assert_eq!(a.code, 0);
        assert_eq!(a.value, 1);
        assert_eq!(a.note, Note::LinkMade);
        assert!(!a.default);
        assert!(a.consistent);
        assert!(a.ok());
        assert_eq!(a.exit(), EXIT_OK);
        assert_eq!(Tier::of(&a), Tier::Linked);
    }

    #[test]
    fn t57_unknown_name_skip_is_green() {
        let s = Shape {
            class: Class::NoSuch,
            name: NOSUCH_ONE,
            link: LinkState::Absent,
            skip_declared: 1,
            ..Shape::blank()
        };
        let a = define(&s);
        assert_eq!(a.stage, Stage::Report);
        assert_eq!(a.code, 0);
        assert_eq!(a.value, 0);
        assert_eq!(a.note, Note::SkipLogged);
        assert!(a.ok());
        assert!(a.link_invariant());
        assert_eq!(Tier::of(&a), Tier::Untouched);
    }

    #[test]
    fn t58_no_dir_is_usage_error() {
        let a = define(&Shape::blank().with_invoke(Invoke::NoDir));
        assert_eq!(a.stage, Stage::Invoke);
        assert_eq!(a.code, EXIT_USAGE as i32);
        assert_eq!(a.note, Note::UsageNoDir);
        assert!(!a.consistent);
        assert!(!a.ok());
        assert_eq!(a.exit(), EXIT_USAGE);
    }

    #[test]
    fn t59_plain_run_is_fallback() {
        let a = define(&Shape::blank().with_invoke(Invoke::Plain));
        assert!(a.default);
        assert_eq!(a.stage, Stage::Invoke);
        assert_eq!(a.note, Note::NotInstall);
        assert_eq!(Tier::of(&a), Tier::Fallback);
    }

    #[test]
    fn t60_bad_names_die_at_class_gate() {
        let names = [NAME_EMPTY, NAME_SLASH, NAME_LONG];
        let mut i = 0usize;
        while i < names.len() {
            if let Some(n) = names.get(i) {
                let a = define(&Shape::blank().with_name(n));
                assert_eq!(a.stage, Stage::Class);
                assert_eq!(a.note, Note::BadName);
                assert_eq!(a.code, EXIT_USAGE as i32);
                assert!(!a.ok());
            }
            i += 1;
        }
        assert!(!name_shape_ok(NAME_EMPTY));
        assert!(!name_shape_ok(NAME_SLASH));
        assert!(!name_shape_ok(NAME_LONG));
        assert!(name_shape_ok("ls"));
        assert!(name_shape_ok(NOSUCH_DASH));
    }

    #[test]
    fn t61_fake_link_dies_at_decide_gate() {
        let s = Shape {
            class: Class::NoSuch,
            name: NOSUCH_ONE,
            link: LinkState::Created,
            skip_declared: 1,
            ..Shape::blank()
        };
        let a = define(&s);
        assert_eq!(a.stage, Stage::Decide);
        assert_eq!(a.note, Note::FakeLink);
        assert_eq!(a.value, 1);
        assert!(!a.consistent);
        assert!(!a.link_invariant());
        assert_eq!(defect_of(&s), Defect::FakeLink);
    }

    #[test]
    fn t62_misroute_dies_at_emit_gate() {
        let s = Shape {
            class: Class::NoSuch,
            name: NOSUCH_ONE,
            link: LinkState::Absent,
            line: LineRoute::Stdout,
            skip_declared: 1,
            ..Shape::blank()
        };
        let a = define(&s);
        assert_eq!(a.stage, Stage::Emit);
        assert_eq!(a.note, Note::LineOnStdout);
        assert_eq!(a.code, 0);
        assert_eq!(defect_of(&s), Defect::Misroute);
    }

    #[test]
    fn t63_code_127_dies_at_code_gate() {
        let s = Shape {
            class: Class::NoSuch,
            name: NOSUCH_ONE,
            link: LinkState::Absent,
            exit_declared: EXIT_NOTFOUND,
            skip_declared: 1,
            ..Shape::blank()
        };
        let a = define(&s);
        assert_eq!(a.stage, Stage::Code);
        assert_eq!(a.note, Note::PassCode127);
        assert_eq!(a.code, EXIT_NOTFOUND as i32);
        assert_eq!(defect_of(&s), Defect::Code127);
    }

    #[test]
    fn t64_bumped_exit_dies_at_code_gate() {
        let s = Shape {
            class: Class::NoSuch,
            name: NOSUCH_ONE,
            link: LinkState::Absent,
            exit_declared: 2,
            skip_declared: 1,
            ..Shape::blank()
        };
        let a = define(&s);
        assert_eq!(a.stage, Stage::Code);
        assert_eq!(a.note, Note::ExitBumped);
        assert_eq!(a.code, 2);
        assert_eq!(defect_of(&s), Defect::SkipExit);
    }

    #[test]
    fn t65_count_drift_dies_at_report_gate() {
        let s = Shape {
            class: Class::NoSuch,
            name: NOSUCH_ONE,
            link: LinkState::Absent,
            skip_declared: 0,
            ..Shape::blank()
        };
        let a = define(&s);
        assert_eq!(a.stage, Stage::Report);
        assert_eq!(a.note, Note::CountDrift);
        assert_eq!(a.value, 0);
        assert_eq!(defect_of(&s), Defect::SkipCount);
    }

    #[test]
    fn t66_dup_collapse_dies_at_decide_gate() {
        let s = Shape {
            name: "cp",
            link: LinkState::Absent,
            dup: true,
            ..Shape::blank()
        };
        let a = define(&s);
        assert_eq!(a.stage, Stage::Decide);
        assert_eq!(a.note, Note::DupCollapsed);
        assert!(!a.consistent);
        assert_eq!(defect_of(&s), Defect::Duplicate);
    }

    #[test]
    fn t67_known_dangling_dies_at_target_gate() {
        let s = Shape {
            name: "ls",
            link: LinkState::Dangling,
            ..Shape::blank()
        };
        let a = define(&s);
        assert_eq!(a.stage, Stage::Target);
        assert_eq!(a.note, Note::TargetDangling);
        assert_eq!(a.value, 0);
        assert_eq!(defect_of(&s), Defect::Dangling);
    }

    #[test]
    fn t68_unknown_stale_link_dies_at_target_gate() {
        let s = Shape {
            class: Class::NoSuch,
            name: NOSUCH_DASH,
            link: LinkState::Stale,
            skip_declared: 1,
            ..Shape::blank()
        };
        let a = define(&s);
        assert_eq!(a.stage, Stage::Target);
        assert_eq!(a.note, Note::StaleLeft);
        assert_eq!(a.value, 0);
        assert_eq!(defect_of(&s), Defect::StaleLink);
    }

    #[test]
    fn t69_unknown_reused_link_is_stale_too() {
        let s = Shape {
            class: Class::NoSuch,
            name: NOSUCH_TWO,
            link: LinkState::Reused,
            skip_declared: 1,
            ..Shape::blank()
        };
        let a = define(&s);
        assert_eq!(a.stage, Stage::Target);
        assert_eq!(a.note, Note::StaleLeft);
        assert_eq!(a.value, 0);
    }

    #[test]
    fn t70_pseudo_with_link_is_fake() {
        let s = Shape {
            class: Class::Pseudo,
            name: MULTICALL_NAME,
            link: LinkState::Created,
            ..Shape::blank()
        };
        let a = define(&s);
        assert_eq!(a.stage, Stage::Decide);
        assert_eq!(a.note, Note::FakeLink);
        assert_eq!(a.value, 1);
    }

    #[test]
    fn t71_pseudo_quiet_shape_is_green() {
        let s = Shape {
            class: Class::Pseudo,
            name: MULTICALL_NAME,
            link: LinkState::Absent,
            ..Shape::blank()
        };
        let a = define(&s);
        assert_eq!(a.stage, Stage::Report);
        assert_eq!(a.note, Note::PseudoQuiet);
        assert!(a.ok());
        assert_eq!(Tier::of(&a), Tier::Untouched);
    }

    #[test]
    fn t72_hard_link_install_same_code() {
        let a = define(&Shape::blank().with_invoke(Invoke::Hard));
        assert_eq!(a.code, 0);
        assert_eq!(a.note, Note::LinkMade);
        assert!(a.ok());
    }

    #[test]
    fn t73_skip_declared_must_equal_want() {
        let mut s = Shape::blank().with_link(LinkState::Absent);
        assert_eq!(s.skip_want(), 0);
        s.class = Class::NoSuch;
        assert_eq!(s.skip_want(), 1);
        s.skip_declared = 1;
        assert!(s.valid());
        s.skip_declared = 0;
        let a = define(&s);
        assert_eq!(a.note, Note::CountDrift);
    }

    #[test]
    fn t74_invalid_shape_is_never_green() {
        let s = Shape::blank().with_exit(EXIT_NOTFOUND + 1);
        assert!(!s.valid());
        let a = define(&s);
        assert!(!a.consistent);
        assert!(!a.ok());
    }

    #[test]
    fn t75_shape_seal_tracks_every_dimension() {
        let base = Shape::blank();
        let mut moved = 0u32;
        let probes = [
            base.with_invoke(Invoke::Hard).seal(),
            base.with_class(Class::NoSuch).seal(),
            base.with_name("cp").seal(),
            base.with_link(LinkState::Absent).seal(),
            base.with_line(LineRoute::Stdout).seal(),
            base.with_exit(2).seal(),
            base.with_skip(1).seal(),
        ];
        let mut i = 0usize;
        while i < probes.len() {
            if let Some(h) = probes.get(i) {
                if *h != base.seal() {
                    moved += 1;
                }
            }
            i += 1;
        }
        assert_eq!(moved, 7);
        assert_eq!(base.seal(), Shape::blank().seal());
    }

    #[test]
    fn t76_outcome_predicates() {
        let a = Outcome::blank();
        assert!(a.default);
        assert!(a.anchored());
        assert!(!a.refused());
        assert!(!a.link_invariant() || a.value == 0);
        let red = Outcome::make(Stage::Code, 127, 0, Note::PassCode127, false, false);
        assert!(red.failed());
        assert!(red.refused());
        assert!(!red.ok());
        assert_eq!(red.exit(), EXIT_NOTFOUND);
        assert!(red.seal() != 0);
    }

    #[test]
    fn t77_claim_and_case_seals() {
        let c = Claim::make(true, Stage::Report, 0, 1, Note::LinkMade, false);
        assert!(c.success());
        assert!(c.seal() != 0);
        assert!(c.seal() != Claim::blank().seal());
        let case = Case::blank();
        assert_eq!(case.tag(), "f11672-blank");
        assert!(case.seal() != 0);
        assert_eq!(case.fault(), Fault::Estate);
    }

    // --- 判序：六维互斥 --------------------------------------------------

    #[test]
    fn t78_estate_wins_over_everything() {
        let case = Case {
            shape: Shape::blank(),
            claim: Claim::make(false, Stage::Invoke, 5, 9, Note::SkipLogged, true),
            tag: "t78",
        };
        assert_eq!(judge(&case), Fault::Estate);
    }

    #[test]
    fn t79_stage_wins_over_code() {
        let case = Case {
            shape: Shape::blank(),
            claim: Claim::make(true, Stage::Emit, 5, 9, Note::SkipLogged, true),
            tag: "t79",
        };
        assert_eq!(judge(&case), Fault::Stage);
    }

    #[test]
    fn t80_code_wins_over_value() {
        let case = Case {
            shape: Shape::blank(),
            claim: Claim::make(true, Stage::Report, 5, 9, Note::SkipLogged, true),
            tag: "t80",
        };
        assert_eq!(judge(&case), Fault::Code);
    }

    #[test]
    fn t81_value_wins_over_note() {
        let case = Case {
            shape: Shape::blank(),
            claim: Claim::make(true, Stage::Report, 0, 9, Note::SkipLogged, true),
            tag: "t81",
        };
        assert_eq!(judge(&case), Fault::Value);
    }

    #[test]
    fn t82_note_wins_over_default() {
        let case = Case {
            shape: Shape::blank(),
            claim: Claim::make(true, Stage::Report, 0, 1, Note::SkipLogged, true),
            tag: "t82",
        };
        assert_eq!(judge(&case), Fault::Note);
    }

    #[test]
    fn t83_default_is_judged_both_ways() {
        let claimed_fallback = Case {
            shape: Shape::blank(),
            claim: Claim::make(true, Stage::Report, 0, 1, Note::LinkMade, true),
            tag: "t83a",
        };
        assert_eq!(judge(&claimed_fallback), Fault::Default);
        let missed_fallback = Case {
            shape: Shape::blank().with_invoke(Invoke::Plain),
            claim: Claim::make(true, Stage::Invoke, 0, 0, Note::NotInstall, false),
            tag: "t83b",
        };
        assert_eq!(judge(&missed_fallback), Fault::Default);
    }

    #[test]
    fn t84_exact_claim_is_ok() {
        let case = Case {
            shape: Shape::blank(),
            claim: Claim::make(true, Stage::Report, 0, 1, Note::LinkMade, false),
            tag: "t84",
        };
        assert_eq!(judge(&case), Fault::Ok);
        assert_eq!(case.fault(), judge(&case));
    }

    // --- 八病表 ------------------------------------------------------------

    #[test]
    fn t85_fault_table_rows_land_where_declared() {
        let out = inject_eight();
        assert_eq!(out, [true; DEFECT_FAULTS]);
    }

    #[test]
    fn t86_fault_rows_cover_all_eight_defects() {
        let mut i = 0usize;
        while i < DEFECT_FAULTS {
            let row = match fault_row(i) {
                Some(x) => x,
                None => panic_free_false(),
            };
            assert_eq!(row.defect.index(), i + 1);
            assert_eq!(defect_of(&row.shape), row.defect);
            let a = define(&row.shape);
            assert_eq!(a.stage, row.stage);
            assert_eq!(a.note, row.note);
            assert_eq!(a.code as u32, row.code);
            assert_eq!(a.value, row.value);
            assert!(!a.consistent);
            i += 1;
        }
        assert!(fault_row(DEFECT_FAULTS).is_none());
    }

    #[test]
    fn t87_defect_of_green_shape_is_none() {
        assert_eq!(defect_of(&Shape::blank()), Defect::None);
        let skip = Shape {
            class: Class::NoSuch,
            name: NOSUCH_ONE,
            link: LinkState::Absent,
            skip_declared: 1,
            ..Shape::blank()
        };
        assert_eq!(defect_of(&skip), Defect::None);
    }

    // --- 四档矩阵 ----------------------------------------------------------

    #[test]
    fn t88_tier_of_covers_four_cells() {
        assert_eq!(Tier::of(&define(&Shape::blank())), Tier::Linked);
        let skip = Shape {
            class: Class::NoSuch,
            name: NOSUCH_ONE,
            link: LinkState::Absent,
            skip_declared: 1,
            ..Shape::blank()
        };
        assert_eq!(Tier::of(&define(&skip)), Tier::Untouched);
        let fake = Shape {
            class: Class::NoSuch,
            name: NOSUCH_ONE,
            link: LinkState::Created,
            skip_declared: 1,
            ..Shape::blank()
        };
        assert_eq!(Tier::of(&define(&fake)), Tier::Faulted);
        assert_eq!(
            Tier::of(&define(&Shape::blank().with_invoke(Invoke::Plain))),
            Tier::Fallback
        );
        assert!(Tier::Linked.has_effect());
        assert!(Tier::Untouched.clean());
        assert!(!Tier::Faulted.clean());
    }

    #[test]
    fn t89_matrix_rows_scan_whole_corpus() {
        let m = Matrix::build();
        assert_eq!(m.slots(), CORPUS * TIER_COUNT);
        assert_eq!(m.goods(), CORPUS);
        assert_eq!(m.red_total(), 42);
        assert_eq!(m.perfect_rows(), 0);
        assert!(m.covered());
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(t) = Tier::all().get(i) {
                let r = match m.row(*t) {
                    Some(x) => x,
                    None => panic_free_false(),
                };
                assert_eq!(r.total, CORPUS);
                assert_eq!(r.good, r.good);
                i += 1;
            } else {
                panic_free_false();
            }
        }
    }

    #[test]
    fn t90_matrix_row_counts_are_pinned() {
        let m = Matrix::build();
        assert_eq!(m.row(Tier::Linked).map(|r| r.good), Some(4));
        assert_eq!(m.row(Tier::Untouched).map(|r| r.good), Some(7));
        assert_eq!(m.row(Tier::Faulted).map(|r| r.good), Some(2));
        assert_eq!(m.row(Tier::Fallback).map(|r| r.good), Some(1));
    }

    #[test]
    fn t91_matrix_render_is_pinned() {
        let m = Matrix::build();
        assert_eq!(
            m.render().as_str(),
            "matrix linked=4/14 untouched=7/14 faulted=2/14 fallback=1/14"
        );
    }

    #[test]
    fn t92_row_render_and_reds() {
        let r = Row {
            tier: Tier::Linked,
            total: 14,
            good: 4,
        };
        assert_eq!(r.render().as_str(), "linked=4/14");
        assert_eq!(r.red(), 10);
        assert!(!r.perfect());
        let p = Row {
            tier: Tier::Fallback,
            total: 1,
            good: 1,
        };
        assert!(p.perfect());
        assert_eq!(p.red(), 0);
    }

    // --- 报告 --------------------------------------------------------------

    #[test]
    fn t93_run_is_all_green() {
        let rep = match run() {
            Ok(x) => x,
            Err(_) => panic_free_false(),
        };
        assert!(rep.green());
        assert_eq!(rep.total, 14);
        assert_eq!(rep.accept, 8);
        assert_eq!(rep.reject, 6);
        assert_eq!(rep.linked, 4);
        assert_eq!(rep.untouched, 7);
        assert_eq!(rep.faulted, 2);
        assert_eq!(rep.fallback, 1);
        assert_eq!(rep.matched, 14);
        assert_eq!(rep.rate(), RATE_FULL);
        assert_eq!(rep.render().as_str(), RENDER_OK);
        assert_eq!(rep.reds(), 0);
        assert_eq!(rep.gate(), Ok(()));
    }

    #[test]
    fn t94_empty_report_is_not_green() {
        let rep = empty_report();
        assert!(!rep.green());
        assert_eq!(rep.rate(), 0);
        assert_eq!(rep.gate(), Err(INS_ENOBS));
        assert_eq!(empty_report().total, 0);
    }

    #[test]
    fn t95_rate_rounding_is_half_up() {
        let rep = Report {
            total: 14,
            accept: 8,
            reject: 6,
            linked: 4,
            untouched: 7,
            faulted: 2,
            fallback: 1,
            matched: 13,
        };
        assert_eq!(rep.rate(), 929);
        assert_eq!(rep.rate(), RED_RATE);
        assert_eq!(rep.reds(), 1);
        assert!(!rep.green());
        assert_eq!(rep.gate(), Err(INS_EBAL));
    }

    #[test]
    fn t96_report_gate_checks_matrix_cells() {
        let rep = Report {
            total: 14,
            accept: 8,
            reject: 6,
            linked: 4,
            untouched: 7,
            faulted: 1,
            fallback: 1,
            matched: 14,
        };
        assert_eq!(rep.tier_cells(), 13);
        assert_eq!(rep.gate(), Err(INS_EMATRIX));
        let over = Report {
            total: 14,
            accept: 8,
            reject: 6,
            linked: 4,
            untouched: 7,
            faulted: 2,
            fallback: 1,
            matched: 15,
        };
        assert_eq!(over.gate(), Err(INS_EBAL));
    }

    #[test]
    fn t97_run_with_none_matches_run() {
        assert_eq!(run_with(None), run());
        assert_eq!(settle(None), match run() {
            Ok(r) => r,
            Err(_) => empty_report(),
        });
    }

    #[test]
    fn t98_run_with_out_of_range_slot() {
        assert_eq!(run_with(Some((CORPUS, Fault::Value))), Err(INS_ESLOT));
        assert_eq!(run_with(Some((CORPUS + 1, Fault::Ok))), Err(INS_ESLOT));
        assert_eq!(run_with(Some((0, Fault::Ok))), run());
    }

    #[test]
    fn t99_injection_drops_rate_to_calibrated_red() {
        let rep = match run_with(Some((2, Fault::Value))) {
            Ok(x) => x,
            Err(_) => panic_free_false(),
        };
        assert_eq!(rep.matched, 13);
        assert_eq!(rep.rate(), RED_RATE);
        assert!(red_rate_calibrated());
        assert!(report_agrees_with_matrix());
    }

    #[test]
    fn t100_matrix_and_report_agree() {
        let m = Matrix::build();
        assert_eq!(m.row(Tier::Linked).map(|r| r.good), Some(4));
        assert_eq!(m.slots(), CORPUS * TIER_COUNT);
        assert!(m.covered());
        assert!(report_agrees_with_matrix());
        assert_eq!(head_val(), STAGE_COUNT);
    }

    // --- 四注入 ------------------------------------------------------------

    #[test]
    fn t101_four_routes_are_all_green() {
        assert_eq!(inject_four(), [true, true, true, true]);
    }

    #[test]
    fn t102_table_route_is_green() {
        assert!(table_route());
        assert!(judge_route());
        assert!(render_route());
        assert!(ledger_route());
    }

    // --- 阶段闸门 ----------------------------------------------------------

    #[test]
    fn t103_gate_new_is_empty() {
        let mut g = Gate::new();
        assert_eq!(g.stage(), None);
        assert_eq!(g.steps(), 0);
        assert_eq!(g.refusals(), 0);
        assert!(!g.is_closed());
        assert_eq!(g.close(), Err(INS_ESTAGE));
        assert_eq!(g.render().as_str(), "gate stage=none steps=0 refusals=0 closed=false");
    }

    #[test]
    fn t104_gate_advances_monotonically() {
        let mut g = Gate::new();
        assert_eq!(g.step(Stage::Invoke), Ok(()));
        assert_eq!(g.step(Stage::Class), Ok(()));
        assert_eq!(g.step(Stage::Decide), Ok(()));
        assert_eq!(g.step(Stage::Target), Ok(()));
        assert_eq!(g.step(Stage::Emit), Ok(()));
        assert_eq!(g.step(Stage::Code), Ok(()));
        assert_eq!(g.step(Stage::Report), Ok(()));
        assert_eq!(g.steps(), 7);
        assert_eq!(g.stage(), Some(Stage::Report));
        assert_eq!(g.refusals(), 0);
        assert!(g.root() != 0);
    }

    #[test]
    fn t105_gate_regression_is_refused() {
        let mut g = Gate::new();
        assert_eq!(g.step(Stage::Target), Ok(()));
        assert_eq!(g.step(Stage::Decide), Err(INS_ESTAGE));
        assert_eq!(g.stage(), Some(Stage::Target));
        assert_eq!(g.steps(), 1);
        assert_eq!(g.refusals(), 1);
        let mut h = Gate::new();
        assert_eq!(h.step(Stage::Report), Ok(()));
        assert_eq!(h.step(Stage::Invoke), Err(INS_ESTAGE));
        assert_eq!(h.refusals(), 1);
    }

    #[test]
    fn t106_gate_after_close_is_refused() {
        let mut g = Gate::new();
        assert_eq!(g.step(Stage::Invoke), Ok(()));
        let root = match g.close() {
            Ok(x) => x,
            Err(_) => panic_free_false(),
        };
        assert!(root != 0);
        assert!(g.is_closed());
        assert_eq!(g.close(), Err(INS_ESTATE));
        assert_eq!(g.step(Stage::Report), Err(INS_ESTATE));
        assert_eq!(g.refusals(), 2);
        assert_eq!(g.root(), root);
    }

    #[test]
    fn t107_gate_walk_and_full_pass() {
        let g = match Gate::walk(&Stage::all()) {
            Ok(x) => x,
            Err(_) => panic_free_false(),
        };
        assert_eq!(g.steps(), 7);
        assert!(!g.is_closed());
        let bad = Gate::walk(&[Stage::Report, Stage::Invoke]);
        assert_eq!(bad, Err(INS_ESTAGE));
        let full = Gate::full_pass();
        match full {
            Ok(h) => assert!(h != 0),
            Err(_) => panic_free_false(),
        }
        assert!(gate_ok());
    }

    #[test]
    fn t108_gate_render_after_steps() {
        let mut g = Gate::new();
        assert_eq!(g.step(Stage::Invoke), Ok(()));
        assert_eq!(g.step(Stage::Class), Ok(()));
        assert_eq!(
            g.render().as_str(),
            "gate stage=class steps=2 refusals=0 closed=false"
        );
    }

    // --- 台账 --------------------------------------------------------------

    #[test]
    fn t109_vals_table_is_eight_cells() {
        assert_eq!(VALS.len(), ACCEPT);
        assert_eq!(VALS[0], STAGE_COUNT);
        assert_eq!(VALS[1], CLASS_COUNT as u32);
        assert_eq!(VALS[2], DEFECT_FAULTS as u32);
        assert_eq!(VALS[3], LIST_LEN as u32);
        assert_eq!(VALS[4], SKIP_ROWS);
        assert_eq!(VALS[5], LINK_ROWS);
        assert_eq!(VALS[6], RATE_FULL);
        assert_eq!(VALS[7], RED_RATE);
        assert_eq!(RED_RATE, 929);
    }

    #[test]
    fn t110_ledger_builds_fourteen_cells() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert!(!l.is_empty());
        // 六格缺陷码是"应落"：判对了也记 ok，故十四格全判对、零错判。
        assert_eq!(l.ok_count(), SAMPLES);
        assert_eq!(l.err_count(), 0);
        assert_eq!(l.rejects(), 0);
        assert!(!l.is_frozen());
        assert_eq!(l.root(), 0);
        assert_eq!(l.audit(), Ok(()));
        assert_eq!(l.first_mismatch(), None);
        assert_eq!(l.ok_count(), STAT_CELLS + 1 + REJECT);
    }

    #[test]
    fn t111_ledger_cells_have_pinned_ops() {
        let l = build_ledger();
        let mut i = 0usize;
        while i < STAT_CELLS {
            let s = match l.sample(i) {
                Some(x) => x,
                None => panic_free_false(),
            };
            assert_eq!(s.slot as usize, i);
            assert_eq!(s.op, STAT_OPS[i]);
            assert!(s.ok);
            assert_eq!(s.code, 0);
            assert_eq!(s.value, VALS[i]);
            i += 1;
        }
        let red = match l.sample(STAT_CELLS) {
            Some(x) => x,
            None => panic_free_false(),
        };
        assert_eq!(red.op, OPID_STAT_RED);
        assert_eq!(red.value, RED_RATE);
        let mut j = 0usize;
        while j < REJECT {
            let cell = STAT_CELLS + 1 + j;
            let s = match l.sample(cell) {
                Some(x) => x,
                None => panic_free_false(),
            };
            // 缺陷格：码对上、值为 0 就是"判对"，ok 必须为真。
            assert!(s.ok);
            assert_eq!(s.value, 0);
            assert_eq!(s.code, REJECT_CODES[j]);
            assert_eq!(s.op, DEFECT_OP_BASE + j as u8);
            j += 1;
        }
    }

    #[test]
    fn t112_ledger_seal_is_deterministic() {
        let a = build_ledger();
        let b = build_ledger();
        assert_eq!(a.seal(), b.seal());
        assert!(a.seal() != 0);
    }

    #[test]
    fn t113_ledger_register_rejects_wrong_slot() {
        let mut l = Ledger::new();
        let stray = Sample {
            slot: 3,
            op: OP_INVOKE,
            ok: true,
            code: 0,
            value: 1,
        };
        assert_eq!(l.register(stray), Err(INS_ESLOT));
        assert!(l.is_empty());
        assert_eq!(l.rejects(), 1);
        assert_eq!(Ledger::new().audit(), Err(INS_ESLOT));
        assert_eq!(Ledger::new().first_mismatch(), Some((0, "len")));
        assert_eq!(Ledger::new().replay(), Err(INS_ESTATE));
        assert_eq!(Ledger::new().poke(0, 1), Err(INS_ESLOT));
    }

    #[test]
    fn t114_ledger_freeze_and_replay() {
        let mut l = build_ledger();
        let h = match l.freeze() {
            Ok(x) => x,
            Err(_) => panic_free_false(),
        };
        assert!(h != 0);
        assert!(l.is_frozen());
        assert_eq!(l.root(), h);
        assert_eq!(l.replay(), Ok(h));
        assert_eq!(l.freeze(), Err(INS_ESTATE));
        let extra = Sample {
            slot: 0,
            op: OP_INVOKE,
            ok: true,
            code: 0,
            value: 0,
        };
        assert_eq!(l.register(extra), Err(INS_ESTATE));
        assert_eq!(l.poke(0, 7), Err(INS_ESTATE));
        assert_eq!(l.rejects(), 1);
    }

    #[test]
    fn t115_broken_ledgers_are_caught() {
        let mut w = 0u8;
        while w < 4u8 {
            let l = build_ledger_broken(w);
            assert_eq!(l.audit(), Err(INS_EVALUE));
            assert!(l.first_mismatch().is_some());
            w += 1;
        }
        assert_eq!(build_ledger_broken(9).audit(), Ok(()));
        assert_eq!(build_ledger_broken(9).first_mismatch(), None);
    }

    #[test]
    fn t116_ledger_render_is_pinned() {
        let l = build_ledger();
        assert_eq!(
            l.render().as_str(),
            "ledger len=14 ok=14 err=0 rejects=0 frozen=false"
        );
        let s = match l.sample(0) {
            Some(x) => x,
            None => panic_free_false(),
        };
        assert_eq!(
            s.render().as_str(),
            "slot 0 op 01 true code 00000 value 7"
        );
    }

    #[test]
    fn t117_sample_seal_changes_with_value() {
        let a = Sample {
            slot: 0,
            op: OP_INVOKE,
            ok: true,
            code: 0,
            value: 7,
        };
        let mut b = a;
        b.value = 8;
        assert!(a.seal() != b.seal());
        assert_eq!(a.seal(), a.seal());
    }

    // --- 十三条汇总 --------------------------------------------------------

    #[test]
    fn t118_checkset_has_thirteen_items() {
        let cs = run_unx11672_checks();
        assert_eq!(cs.len(), 13);
        assert!(cs.all_passed());
        assert_eq!(cs.tally(), (13, 0));
        assert_eq!(cs.passed_count(), 13);
        assert_eq!(cs.failed_count(), 0);
        assert!(!cs.is_empty());
    }

    #[test]
    fn t119_every_check_name_carries_prefix() {
        let cs = run_unx11672_checks();
        let mut i = 0usize;
        while i < cs.len() {
            let c = match cs.get(i) {
                Some(x) => x,
                None => panic_free_false(),
            };
            assert!(c.name.starts_with("INS"));
            assert!(c.passed);
            assert_eq!(c.detail, "static");
            i += 1;
        }
    }

    #[test]
    fn t120_last_check_is_frozen_replay() {
        let cs = run_unx11672_checks();
        let last = match cs.get(12) {
            Some(x) => x,
            None => panic_free_false(),
        };
        assert!(last.name.ends_with("-冻结重放四注入"));
        assert!(last.passed);
        let first = match cs.get(0) {
            Some(x) => x,
            None => panic_free_false(),
        };
        assert_eq!(first.name, "INS01-名称与段基址");
    }

    #[test]
    fn t121_each_predicate_is_green() {
        assert!(name_ok());
        assert!(codes_ok());
        assert!(list_ok());
        assert!(skip_ok());
        assert!(link_ok());
        assert!(code_ok());
        assert!(emit_ok());
        assert!(corpus_ok());
        assert!(judge_ok());
        assert!(matrix_ok());
        assert!(render_ok());
        assert!(gate_ok());
        assert!(ledger_ok());
    }

    #[test]
    fn t122_checkset_render_smoke() {
        let cs = run_unx11672_checks();
        let mut buf = [0u8; 2048];
        let n = cs.render(&mut buf);
        assert!(n > 0);
        assert!(n <= buf.len());
    }

    // --- 账本 ok 位语义：判对，不是"槽绿" --------------------------------

    #[test]
    fn t123_ledger_defect_cells_are_judged_as_expected() {
        let l = build_ledger();
        let mut j = 0usize;
        while j < REJECT {
            let cell = STAT_CELLS + 1 + j;
            let s = match l.sample(cell) {
                Some(x) => x,
                None => panic_free_false(),
            };
            // 缺陷格落的是应落的缺陷码：它"判对了"，ok 必须为真。
            assert!(s.ok);
            assert_eq!(s.code, REJECT_CODES[j]);
            assert_eq!(s.value, 0);
            assert_eq!(s.op, DEFECT_OP_BASE + j as u8);
            j += 1;
        }
        assert_eq!(l.ok_count(), SAMPLES);
        assert_eq!(l.err_count(), 0);
        assert_eq!(l.first_mismatch(), None);
    }

    #[test]
    fn t124_ledger_audit_catches_ok_flag_flip() {
        // 统计格 ok 位翻假：首格即报 ok-flag。
        let mut a = build_ledger();
        if let Some(Some(s)) = a.slots.get_mut(0) {
            s.ok = false;
        }
        assert_eq!(a.audit(), Err(INS_ESTATE));
        assert_eq!(a.first_mismatch(), Some((0, "ok-flag")));
        // 缺陷格 ok 位翻假：第 8 格（六缺陷首格）报 ok-flag。
        let mut b = build_ledger();
        let defect = STAT_CELLS + 1;
        if let Some(Some(s)) = b.slots.get_mut(defect) {
            s.ok = false;
        }
        assert_eq!(b.audit(), Err(INS_ESTATE));
        assert_eq!(b.first_mismatch(), Some((defect, "ok-flag")));
    }

    #[test]
    fn t125_skip_message_bytes_are_pinned() {
        // 这三条字节数是 LIST.stderr_bytes 与 skip_ok 的共用标定，
        // 任何一处 +1/-1 都会让"跳过留一行"的账当场对不上。
        assert_eq!(bytes_of(msg_skip(NOSUCH_ONE).as_str()), 22);
        assert_eq!(bytes_of(msg_skip(NOSUCH_TWO).as_str()), 22);
        assert_eq!(bytes_of(msg_skip(NOSUCH_DASH).as_str()), 25);
        assert_eq!(skip_bytes_total(), 22 + 22 + 25);
        assert_eq!(skip_bytes_total(), 69);
        let mut i = 0usize;
        while i < LIST_LEN {
            let e = match LIST.get(i) {
                Some(x) => *x,
                None => panic_free_false(),
            };
            if e.class.speaks() {
                assert_eq!(e.stderr_bytes, bytes_of(msg_skip(e.name).as_str()));
            }
            i += 1;
        }
    }

    #[test]
    fn t126_manifest_order_and_segments() {
        // 重名那一行必须落在第 12 行（下标 11），且是 cp 的第二趟。
        let dup = match LIST.get(11) {
            Some(x) => *x,
            None => panic_free_false(),
        };
        assert_eq!(dup.name, "cp");
        assert!(dup.reused());
        // 除重名 cp 外，清单里的名字两两互异（四条路径各归各的段）。
        let mut i = 0usize;
        while i < LIST_LEN {
            let a = match LIST.get(i) {
                Some(x) => *x,
                None => panic_free_false(),
            };
            let mut k = i + 1usize;
            while k < LIST_LEN {
                if let Some(b) = LIST.get(k) {
                    if a.name == b.name {
                        assert_eq!(a.name, "cp");
                        assert!(a.link_created != b.link_created);
                    }
                }
                k += 1;
            }
            i += 1;
        }
        // 四段之和恰好铺满十四行，一段不多一段不少。
        assert_eq!(
            LINK_ROWS + REUSE_ROWS + SKIP_ROWS + PSEUDO_ROWS,
            LIST_LEN as u32
        );
        assert_eq!(LINK_ROWS, 8);
        assert_eq!(REUSE_ROWS, 1);
        assert_eq!(SKIP_ROWS, 3);
        assert_eq!(PSEUDO_ROWS, 2);
    }
}
