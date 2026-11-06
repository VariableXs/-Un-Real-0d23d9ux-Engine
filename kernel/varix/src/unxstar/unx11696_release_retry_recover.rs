//! UNX-F11696 空间释放后重试恢复判据（正判据面）
//!
//! 一张盘写满的时候，运维的第一反应从来不是"换盘"，而是**腾地方再写一次**：
//! `rm` 掉一个 filler、把临时件 `truncate -s 0`、或者 `> big.log` 清空，然后
//! 原封不动地再跑一遍同一条命令。这条动作链是**两刀**，不是一刀：
//!
//! ```text
//! dd: error writing 'srv/data/archive.bin': No space left on device
//! rm -f srv/data/filler.img
//! dd if=payload.bin of=srv/data/archive.bin bs=4096
//! 8192 bytes (8.2 kB, 8.0 KiB) copied, 0.0012 s, 6.8 MB/s
//! ```
//!
//! 第一刀失败、第二刀成功，这就是"重试恢复"。本卡判的**不是**第一刀为什么
//! 失败（那是 F11681 总锚与 F11695 双码对照档的活），而是**第二刀凭什么算
//! 成功**。这个"凭什么"有四根柱子，缺一根整个结论就不成立：
//!
//! 1. **前提柱：第一刀必须真是 ENOSPC（28）**。如果第一刀的 errno 是 EDQUOT
//!    （122），那么"删盘上的件"这个动作**根本不解决任何东西**——配额账按主体
//!    累计，删自己的件能不能回额度取决于别人有没有占用；更常见的情形是额度
//!    是硬上限顶死，删多少都回不来。于是"释放→重试"这条链在 EDQUOT 上不是
//!    "没恢复成功"，而是**前提就是假的**：把 122 误记成 28 再去删件，运维会在
//!    同一个错误上原地打转，删掉的却是别人的数据。
//! 2. **释放柱：释放量必须真把缺口填平**。`freed_before == false` 意味着根本
//!    没腾地方就重试，第二刀必然还是 `-1/28`（本卡的 `norelease-enospc` 槽）；
//!    更阴的是"腾了但不够"：`filler` 只删了 4096 而缺口 8192，第二刀会**先
//!    写进去 4096 字节再撞 ENOSPC**（`shortfree-partial` 槽）——现场留下一个
//!    半截文件，`ls -l` 看着像"部分成功"，这才是最容易被记成"恢复了一半"的
//!    假绿。释放量必须按 `free_before + filler >= payload` 显式算，不许目测。
//! 3. **精确柱：字节必须逐个对得上**。恢复的判据不是"第二刀退出码 0"，而是
//!    "第二刀的产物与第一刀想要的 payload **同一份字节**"。于是同一 payload、
//!    同一路径、同一最终长度三件事必须同时成立：`retry_payload == payload`
//!    （同一 payload）、`path` 两刀一致（同一路径）、`retry_size == payload`
//!    （同一最终长度）。只比 `retry_size == payload` 不比 payload 本身，就会
//!    放过"换了一份等长的 payload 重试"这种事故（`drift-payload` 槽）；只看
//!    尺寸不看内容比较有没有做，就会放过"尺寸对但没比过"的宣称
//!    （`unproven-nofold` 槽）。
//! 4. **记账柱：重试不是"第二次失败"，释放块也不许双计**。一条命令失败一次、
//!    腾地方后成功，账上必须是 **1 次失败 + 1 次恢复**；把重试记成"新的失败"
//!    会让失败率翻倍（`booked-retry-fail`），把 `filler` 释放的块在"已释放"
//!    与"已回账"两处各记一次会凭空多出 8192 字节可用（`booked-double`）。
//!    记账柱是**恢复面**与**统计面**的分界：前三条柱子管"真的恢复了没有"，
//!    这一条管"恢复有没有被如实记账"。
//!
//! 现场文案必须逐字钉住，因为排障顺序永远是"先看那半行字，再决定删什么"：
//!
//! ```text
//! dd: error writing 'srv/data/archive.bin': No space left on device
//! cp: error writing 'mnt/full/half.bin': No space left on device
//! truncate: failed to truncate 'var/tmp/scratch.tmp' at 0 bytes: No space left on device
//! tee: mnt/full/twice.bin: No space left on device
//! cmp: EOF on mnt/full/drift.bin after byte 4096, in line 1
//! ```
//!
//! 其中 `No space left on device` 是 glibc `strerror(28)` 的原文，一个字都
//! 不许动；工具前缀体例（`dd: error writing`、`cp: error writing`、`tee: `、
//! `truncate: failed to truncate`）跨场景**只换路径不换体例**——这正是"同一
//! 条命令两刀、只差一次成功"在真实日志里唯一的抓手。本卡的 `msg_*` 因此按
//! **动作**分族（删 filler / 截断临时件 / 重试写 / 字节对照 / 记账），而不是
//! 按 errno 分族：errno 在本卡永远是 28（前提柱已排掉 122），变化的是动作。
//!
//! ## 闸门次序（这条纵线为什么不能倒）
//!
//! `arg → premise → release → retry → exact → book → done`，七段，秩从 0 起算，
//! 只能前进。次序的理由逐段说明：
//!
//! 1. `arg`（秩 0）——入参与几何本身。`payload == 0`、`first_written > payload`、
//!    `filler`/`free_before` 越上界、`retry_size` 越上界、errno 不在合法集、
//!    `booked` 不在四值集，一律 `EINVAL`（22）短路。它排第一是因为**后面每一
//!    闸都要拿这些数算**：坏数进去算出来的"恢复成立"全是幻觉。本卡另有一条
//!    只有恢复域才有的几何自洽：第一刀既然报了 ENOSPC，那么**首刀之前的可用
//!    量必须小于 payload**（`free_before < payload`）——盘上明明够 8192 却说
//!    "没空间"，这不是 ENOSPC 而是读数坏了，必须在 arg 段就判死，不许带着
//!    自相矛盾的几何往 release 段算释放量。
//! 2. `premise`（秩 1）——首刀 errno 必须是 28。它不是"第一个物理闸"，而是
//!    **整条恢复链成立与否的前提**：首刀是 122 时，第 2/3/4 根柱子讨论的
//!    "腾多少地方""重试写多少字节"全是空谈。它必须排在 release 之前，因为
//!    "该不该释放"取决于"这次的失败是不是空间问题"。
//! 3. `release`（秩 2）——释放闸。先看 `freed_before`（有没有腾），再看
//!    `free_before + filler` 能不能盖住 `payload`（腾得够不够）。两问必须按此
//!    序：没腾的时候谈"腾得够不够"没有意义（0 + 0 永远不够，但那不是"腾少了"
//!    而是"没腾"），两者的现场文案与修法完全不同（前者去删件，后者再多删一个）。
//! 4. `retry`（秩 3）——重试闸。先比 `retry_payload` 与 `payload`（同一 payload），
//!    再看 `retry_code`（第二刀成没成）。次序不能倒：payload 都换了，第二刀
//!    成不成已经与"恢复"无关，此时若先判 retry_code 会得出"重试成功"的假绿。
//!    第二刀仍失败时，按 `retry_size` 与 `first_written` 的关系分两档：文件还
//!    留着首刀写的字节是 `still-full`；文件被截断到比首刀更短是 `truncated-left`
//!    ——后者是**数据丢失**，比"没恢复"严重一档。
//! 5. `exact`（秩 4）——字节精确闸。`retry_size == payload` 且 `proven == true`
//!    才算"同一份字节"。这一闸必须在 retry 之后：只有第二刀真成功了，才谈得上
//!    "成功的产物对不对"。它也是本卡区别于"退出码判据"的分水岭——退出码 0 只
//!    说明写完了，不说明写对。
//! 6. `book`（秩 5）——记账闸。恢复成立之后才查账：`booked` 必须是
//!    `BOOK_RECOVER`（一次失败 + 一次恢复，释放块只算一次）。排在 exact 之后
//!    是刻意的：**先证明恢复是真的，再证明账是对的**；反过来先查账，会出现
//!    "账记成恢复了所以算恢复"的循环论证。
//! 7. `done`（秩 6）——全闸通过，落恢复对照档。
//!
//! ## 关键差异（本卡深度所在）
//!
//! * **与 F11695（ENOSPC / EDQUOT 双码对照档，段基址 `0x6d3e0`，位距 `0x10`）
//!   的边界**：F11695 判的是**一刀之内**码是谁、为什么，配额软硬上限与宽限期
//!   全在它那边建模；本卡只从它那里取一条硬事实——"删别人的件救不了 EDQUOT"
//!   ——并把这条事实做成 `premise` 闸：首刀报 122 时，"释放→重试"整条链判死，
//!   且判死理由不是"重试失败"而是"前提不成立"。两卡段位差 `0x10`，本卡用
//!   `PREV_F11695_SEG` 与 `PREV_GAP` 署名，由 `anchor_ok()` 复算对齐，不 import。
//! * **与 F11694（ENOSPC 文案逐字，同批前置）的边界**：F11694 只钉
//!   `No space left on device` 这一句跨工具一致；本卡一个字的 strerror 都不新
//!   立判据，只判**动作前缀**（`dd`/`cp`/`tee`/`truncate`/`cmp` 五个动作各一版）
//!   与"同一体例只换路径"这一结构事实。
//! * **与 F11681（ENOSPC 总锚，段基址 `0x6d130`，位距 `0x2c0`）的边界**：总锚
//!   判那一次 `write()` 的 (返回, errno) 成对；本卡判的是**第二次** `write()`
//!   的产物与账。总锚给本卡供给"28 的真值与 strerror 原文"，本卡给总锚回供
//!   "重试这一刀不许被记成新的失败"这条记账约束。
//! * **与 F11697（`dd conv=noerror` 对 ENOSPC 的档位判据，段基址 `0x6d410`，
//!   位距 `0x20`）的边界**：F11697 判"同一条命令内部吞掉错误继续跑"的档位；
//!   本卡判"两条命令之间靠外部动作（rm/truncate）腾出空间"的恢复。前者不换
//!   payload 也不换命令，后者换的正是**盘上的占用**。两卡唯一共享的事实是
//!   "ENOSPC 之后还可以再写"，本卡把这条事实落成 `retry` 闸。
//! * **本卡的排他职责**：① 前提柱（首刀必须 28，122 是前提破断而不是重试失败）；
//!   ② 释放柱（没腾 / 腾不够两档分立，后者带"部分写"字节数）；
//!   ③ 精确柱（同一 payload + 同一路径 + 同一最终长度 + 做过字节比较，四件套）；
//!   ④ 记账柱（重试不记新失败、恢复必须入账、释放块不许双计）；
//!   ⑤ 七段纵线只许前进，倒退一律阶段错。
//! * **谁判死谁放行**：首刀 28、真腾了且腾够、重试同 payload、长度精确且比过
//!   字节、账上记成"一次失败 + 一次恢复"→ 放行并落恢复对照档；首刀非 28、
//!   没腾/腾不够、换 payload、长度不符、没比字节、记成新失败或双计 → 一律判死。
//!
//! 判据六维：结束阶段（gate）、errno（code）、最终字节（value）、形态注解
//! （note）、恢复样本档位（default：该现场是否构成一条可入账的"释放→重试"
//! 对照样本）、成败申报（estate）。判序固定：状态 → 阶段 → errno → 字节 →
//! 注解 → 档位。
//!
//! 语料 14 槽：8 槽申报正确（恢复 3、释放面 2、精确面 2、记账面 1），6 槽复刻
//! 同一 `norelease` 现场、各挂一维错报（estate/stage/code/value/note/default），
//! 其中 `code-swap` 那一格就是把 28 报成 122——本卡前提柱要抓的头号缺陷。
//! 账本 14 格 = 7 统计 + 1 红档率（929）+ 6 缺陷码，冻结后重放同指纹。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，`audit` 逐格
//! 对账，冻结后 `replay` 重建同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址（F11696）。全仓唯一，不许改。
pub const SEG_BASE: u32 = 0x6d3f0;

/// 段内零位：本卡的放行标记（不占错误码位，`TAG_OK == SEG_BASE`）。
pub const TAG_OK: u32 = SEG_BASE;

/// 联签署名：直接前置卡（F11695 双码对照档）段基址。
pub const PREV_F11695_SEG: u32 = 0x6d3e0;

/// 联签署名：本卡与前卡的段位差（0x10）。
pub const PREV_GAP: u32 = SEG_BASE - PREV_F11695_SEG;

/// 联签署名：ENOSPC 总锚（F11681）段基址。
pub const ANCHOR_F11681_SEG: u32 = 0x6d130;

/// 联签署名：本卡与总锚的段位差（0x2c0）。
pub const ANCHOR_GAP: u32 = SEG_BASE - ANCHOR_F11681_SEG;

/// 联签署名：直接后继卡（F11697 dd conv=noerror 档位）段基址。
pub const NEXT_F11697_SEG: u32 = 0x6d410;

/// 联签署名：本卡与后继卡的段位差（0x20）。
pub const NEXT_GAP: u32 = NEXT_F11697_SEG - SEG_BASE;

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

/// 形状维数（本卡输入的全部维度，逐维都要参与判定）。
pub const SHAPE_DIMS: usize = 13;

// ---------------------------------------------------------------------------
// errno（glibc/Linux 真值，本卡只用这四个）
// ---------------------------------------------------------------------------

/// 设备无空间（strerror: `No space left on device`）——本卡唯一合法的首刀码。
pub const ERR_ENOSPC: i32 = 28;

/// 配额超限（strerror: `Quota exceeded`）——出现在首刀即为前提破断。
pub const ERR_EDQUOT: i32 = 122;

/// 入参非法（strerror: `Invalid argument`）。
pub const ERR_EINVAL: i32 = 22;

/// 成功时 errno 应停放的零值。
pub const ERR_NONE: i32 = 0;

/// 合法 errno 集合大小（0、28、122、22 四个）。
pub const ERRNO_SET_SIZE: usize = 4;

/// 单次 payload 的合法上界（越过即 `EINVAL`）。
pub const PAYLOAD_MAX: u32 = 0x0100_0000;

/// 释放量的合法上界（`filler` 与 `free_before` 共用）。
pub const FREE_MAX: u32 = 0x0040_0000;

// ---------------------------------------------------------------------------
// 记账四值（重试这一刀在账上算几次）
// ---------------------------------------------------------------------------

/// 没有可记的重试（首刀即死，或释放/重试闸已判死）。
pub const BOOK_NONE: u8 = 0;

/// 如实记账：1 次失败 + 1 次恢复，释放块只算一次。
pub const BOOK_RECOVER: u8 = 1;

/// 把重试记成第二次失败（失败率凭空翻倍）。
pub const BOOK_NEWFAIL: u8 = 2;

/// 恢复记了，但释放块被"已释放"与"已回账"各记一次（可用量凭空翻倍）。
pub const BOOK_DOUBLE: u8 = 3;

/// 记账值集合大小（四值）。
pub const BOOK_SET_SIZE: usize = 4;

// ---------------------------------------------------------------------------
// 错误码（13 个，SEG_BASE + 0x01..0x0d）
// ---------------------------------------------------------------------------

/// 阶段申报不符。
pub const RR_ESTAGE: u32 = SEG_BASE + 0x01;

/// errno 申报不符。
pub const RR_ECODE: u32 = SEG_BASE + 0x02;

/// 最终字节申报不符。
pub const RR_EVALUE: u32 = SEG_BASE + 0x03;

/// 注解申报不符。
pub const RR_ENOTE: u32 = SEG_BASE + 0x04;

/// 恢复样本档位申报不符。
pub const RR_EDEFAULT: u32 = SEG_BASE + 0x05;

/// 字节精确性破断（结构维：同一 payload / 同一路径 / 同一长度四件套缺一）。
pub const RR_EEXACT: u32 = SEG_BASE + 0x06;

/// 成败互报（重试成功被报成失败，或反之）。
pub const RR_ESTATE: u32 = SEG_BASE + 0x07;

/// 计数不符。
pub const RR_ECOUNT: u32 = SEG_BASE + 0x08;

/// 槽位非法。
pub const RR_ESLOT: u32 = SEG_BASE + 0x09;

/// 指纹不符。
pub const RR_ESEAL: u32 = SEG_BASE + 0x0a;

/// 无观测。
pub const RR_ENOBS: u32 = SEG_BASE + 0x0b;

/// 矩阵不平。
pub const RR_EMATRIX: u32 = SEG_BASE + 0x0c;

/// 账本不平。
pub const RR_EBAL: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码（账本格归属的闸门）
// ---------------------------------------------------------------------------

/// 入参闸。
pub const OP_ARG: u8 = 0x01;

/// 释放闸（删 filler）。
pub const OP_RELEASE: u8 = 0x02;

/// 释放闸（截断临时件）。
pub const OP_TRUNC: u8 = 0x03;

/// 重试闸。
pub const OP_RETRY: u8 = 0x04;

/// 字节精确闸。
pub const OP_EXACT: u8 = 0x05;

/// 记账闸。
pub const OP_BOOK: u8 = 0x06;

/// 语料全跑。
pub const OP_CORPUS: u8 = 0x07;

/// 账本重放。
pub const OP_REPLAY: u8 = 0x08;

// ---------------------------------------------------------------------------
// FNV-1a 指纹
// ---------------------------------------------------------------------------

/// FNV 偏移基。
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV 素数。
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个字节（每折一字节乘一次素数）。
fn fold_byte(h: u64, b: u8) -> u64 {
    let mut v = h ^ (b as u64);
    let mut k = 0usize;
    while k < 8 {
        v = v.wrapping_mul(FNV_PRIME);
        k += 1;
    }
    v
}

/// 折入一个字（大端序逐字节，定宽 8 字节，保证字段位置敏感）。
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

/// 折入一个字符串（逐字节，空串也要折入长度）。
fn fold_str(h: u64, s: &str) -> u64 {
    let bytes = s.as_bytes();
    let mut v = fold_word(h, bytes.len() as u64);
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
// 数字/布尔文本（无分配器递归、无格式化宏）
// ---------------------------------------------------------------------------

/// 十进制文本（u32）。
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

/// 定宽小写十六进制文本（段基址与码位打印用）。
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

/// 有符号十进制文本（errno 打印用，负数带 `-`）。
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

/// 千分率文本。
fn rate_text(v: u32) -> String {
    dec_text(v)
}

/// errno 的短名（detail 行与指纹共用的稳定拼写）。
pub fn code_name(code: i32) -> &'static str {
    match code {
        ERR_NONE => "none",
        ERR_ENOSPC => "nospc",
        ERR_EDQUOT => "dquot",
        ERR_EINVAL => "arg",
        _ => "unknown",
    }
}

/// 记账值的短名。
pub fn book_name(b: u8) -> &'static str {
    match b {
        BOOK_NONE => "none",
        BOOK_RECOVER => "recover",
        BOOK_NEWFAIL => "newfail",
        BOOK_DOUBLE => "double",
        _ => "unknown",
    }
}

// ---------------------------------------------------------------------------
// 文案（coreutils / dd / cmp 表面逐字；动作分族）
// ---------------------------------------------------------------------------

/// strerror(errno) 的 glibc 原文（本卡涉及的四个值）。
pub fn strerror(code: i32) -> &'static str {
    match code {
        ERR_ENOSPC => "No space left on device",
        ERR_EDQUOT => "Quota exceeded",
        ERR_EINVAL => "Invalid argument",
        ERR_NONE => "",
        _ => "Unknown error",
    }
}

/// `dd` 首刀失败体例：`dd: error writing '<路径>': <strerror>`。
pub fn msg_dd_write(path: &str) -> String {
    let mut s = String::from("dd: error writing '");
    s.push_str(path);
    s.push_str("': ");
    s.push_str(strerror(ERR_ENOSPC));
    s
}

/// `cp` 首刀失败体例（同一 strerror，只换动作前缀——"同一体例只换路径"）。
pub fn msg_cp_write(path: &str) -> String {
    let mut s = String::from("cp: error writing '");
    s.push_str(path);
    s.push_str("': ");
    s.push_str(strerror(ERR_ENOSPC));
    s
}

/// `tee` 体例：`tee: <路径>: <strerror>`（注意 tee 不加引号，这是真实差异）。
pub fn msg_tee_write(path: &str) -> String {
    let mut s = String::from("tee: ");
    s.push_str(path);
    s.push_str(": ");
    s.push_str(strerror(ERR_ENOSPC));
    s
}

/// `truncate` 体例（腾地方的另一半：把临时件截成 0）。
pub fn msg_truncate(path: &str, len: u32) -> String {
    let mut s = String::from("truncate: failed to truncate '");
    s.push_str(path);
    s.push_str("' at ");
    s.push_str(dec_text(len).as_str());
    s.push_str(" bytes: ");
    s.push_str(strerror(ERR_ENOSPC));
    s
}

/// `cmp` 的 EOF 体例（字节精确柱的反面：第二刀产物比 payload 短）。
pub fn msg_cmp_eof(path: &str, n: u32) -> String {
    let mut s = String::from("cmp: EOF on ");
    s.push_str(path);
    s.push_str(" after byte ");
    s.push_str(dec_text(n).as_str());
    s.push_str(", in line 1");
    s
}

/// `cmp` 的相同体例（字节精确柱的正身：两刀产物逐字节一致）。
pub fn msg_cmp_same(a: &str, b: &str) -> String {
    let mut s = String::new();
    s.push_str(a);
    s.push_str(" and ");
    s.push_str(b);
    s.push_str(" are identical");
    s
}

/// `rm -v` 的释放行（腾地方的现场：删掉 filler）。
pub fn msg_rm_filler(path: &str) -> String {
    let mut s = String::from("removed '");
    s.push_str(path);
    s.push_str("'");
    s
}

/// 释放量审计行（本卡自研：把"腾了多少、缺口多少"摆在一行里）。
pub fn msg_release_line(freed: u32, need: u32) -> String {
    let mut s = String::from("released ");
    s.push_str(dec_text(freed).as_str());
    s.push_str(" bytes for a ");
    s.push_str(dec_text(need).as_str());
    s.push_str("-byte payload");
    s
}

/// 未释放就重试的现场（第二刀必然还是 ENOSPC）。
pub fn msg_no_release(path: &str, free: u32) -> String {
    let mut s = String::from("retry '");
    s.push_str(path);
    s.push_str("' without release (free ");
    s.push_str(dec_text(free).as_str());
    s.push_str("): ");
    s.push_str(strerror(ERR_ENOSPC));
    s
}

/// 释放不足的现场（第二刀部分写入后撞 ENOSPC）。
pub fn msg_short_free(path: &str, need: u32, have: u32) -> String {
    let mut s = String::from("retry '");
    s.push_str(path);
    s.push_str("' short of space: wrote ");
    s.push_str(dec_text(have).as_str());
    s.push_str(" of ");
    s.push_str(dec_text(need).as_str());
    s.push_str(" bytes: ");
    s.push_str(strerror(ERR_ENOSPC));
    s
}

/// payload 漂移的现场（换了 payload 再重试）。
pub fn msg_payload_drift(path: &str, want: u32, got: u32) -> String {
    let mut s = String::from("retry '");
    s.push_str(path);
    s.push_str("' payload drift: requested ");
    s.push_str(dec_text(got).as_str());
    s.push_str(", first attempt ");
    s.push_str(dec_text(want).as_str());
    s
}

/// 第二刀仍失败且文件留着首刀字节的现场。
pub fn msg_still_full(path: &str, size: u32) -> String {
    let mut s = String::from("retry '");
    s.push_str(path);
    s.push_str("' still failing, file left at ");
    s.push_str(dec_text(size).as_str());
    s.push_str(" bytes: ");
    s.push_str(strerror(ERR_ENOSPC));
    s
}

/// 第二刀失败且文件被截短的现场（数据丢失）。
pub fn msg_truncated_left(path: &str, want: u32, got: u32) -> String {
    let mut s = String::from("retry '");
    s.push_str(path);
    s.push_str("' failed, file truncated to ");
    s.push_str(dec_text(got).as_str());
    s.push_str(" of ");
    s.push_str(dec_text(want).as_str());
    s.push_str(" bytes: ");
    s.push_str(strerror(ERR_ENOSPC));
    s
}

/// 退出码 0 但长度不符的现场。
pub fn msg_size_short(path: &str, want: u32, got: u32) -> String {
    let mut s = String::from("retry '");
    s.push_str(path);
    s.push_str("' reported ok but size is ");
    s.push_str(dec_text(got).as_str());
    s.push_str(", expected ");
    s.push_str(dec_text(want).as_str());
    s
}

/// 未做字节比较就宣称恢复的现场。
pub fn msg_unproven(path: &str, size: u32) -> String {
    let mut s = String::from("recovery claimed for '");
    s.push_str(path);
    s.push_str("' without byte-exact comparison (size ");
    s.push_str(dec_text(size).as_str());
    s.push_str(")");
    s
}

/// 首刀不是 ENOSPC 的前提破断现场。
pub fn msg_wrong_first(code: i32) -> String {
    let mut s = String::from("recovery premise broken: first attempt failed with ");
    s.push_str(strerror(code));
    s.push_str(" (expected ");
    s.push_str(strerror(ERR_ENOSPC));
    s.push(')');
    s
}

/// 重试被记成新失败的记账现场。
pub fn msg_booked_fail(failures: u32) -> String {
    let mut s = String::from("1 operation booked as ");
    s.push_str(dec_text(failures).as_str());
    s.push_str(" failures (expected 1)");
    s
}

/// 恢复没有入账的记账现场。
pub fn msg_booked_loose(path: &str) -> String {
    let mut s = String::from("recovery of '");
    s.push_str(path);
    s.push_str("' not booked (0 recoveries recorded)");
    s
}

/// 释放块被双计的记账现场。
pub fn msg_booked_double(freed: u32, credited: u32) -> String {
    let mut s = String::from("freed block counted twice: ");
    s.push_str(dec_text(credited).as_str());
    s.push_str(" of ");
    s.push_str(dec_text(freed).as_str());
    s.push_str(" bytes credited");
    s
}

/// 入参非法文案。
pub fn msg_arg_bad(field: &str, v: u32) -> String {
    let mut s = String::from("invalid ");
    s.push_str(field);
    s.push(' ');
    s.push_str(dec_text(v).as_str());
    s.push_str(": ");
    s.push_str(strerror(ERR_EINVAL));
    s
}

/// `df` 表头逐字（六列，宽度即真实输出）。
pub fn msg_df_header() -> &'static str {
    "Filesystem      1K-blocks    Used Available Use% Mounted on\n"
}

/// `df` 数据行：本卡用它钉"释放之后 available 变大了多少"。
pub fn msg_df_row(fs: &str, blocks: u32, used: u32, avail: u32, mount: &str) -> String {
    let pct = if blocks == 0 {
        100u32
    } else {
        (used.saturating_mul(100)).saturating_div(blocks)
    };
    let mut s = String::from(fs);
    s.push_str("      ");
    s.push_str(dec_text(blocks).as_str());
    s.push_str("      ");
    s.push_str(dec_text(used).as_str());
    s.push_str("      ");
    s.push_str(dec_text(avail).as_str());
    s.push(' ');
    s.push_str(dec_text(pct).as_str());
    s.push_str("% ");
    s.push_str(mount);
    s.push('\n');
    s
}

/// `stat -c %s` 的产出（最终长度这一维的现场来源）。
pub fn msg_stat_size(path: &str, size: u32) -> String {
    let mut s = String::from(dec_text(size).as_str());
    s.push(' ');
    s.push_str(path);
    s.push('\n');
    s
}

// ---------------------------------------------------------------------------
// 枚举：闸门阶段（本卡纵线的七段）
// ---------------------------------------------------------------------------

/// 恢复纵线的七段。秩从 0 起算，只能前进。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// 入参与几何合法性（`EINVAL`）。
    Arg,
    /// 前提：首刀必须是 ENOSPC。
    Premise,
    /// 释放：腾没腾、腾够不够。
    Release,
    /// 重试：同一 payload、第二刀成没成。
    Retry,
    /// 字节精确：同一最终长度且做过逐字节比较。
    Exact,
    /// 记账：重试不记新失败、恢复入账、释放块不双计。
    Book,
    /// 全闸通过。
    Done,
}

impl Stage {
    /// 次序（纵线不可倒）。
    pub fn order(&self) -> u8 {
        match self {
            Stage::Arg => 0,
            Stage::Premise => 1,
            Stage::Release => 2,
            Stage::Retry => 3,
            Stage::Exact => 4,
            Stage::Book => 5,
            Stage::Done => 6,
        }
    }

    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Stage::Arg => "arg",
            Stage::Premise => "premise",
            Stage::Release => "release",
            Stage::Retry => "retry",
            Stage::Exact => "exact",
            Stage::Book => "book",
            Stage::Done => "done",
        }
    }

    /// 是否失败点（Done 之外都是某道闸的短路）。
    pub fn failure(&self) -> bool {
        !matches!(self, Stage::Done)
    }

    /// 是否空间侧闸门（release/retry 的红都是"没写成"）。
    pub fn space_side(&self) -> bool {
        match self {
            Stage::Release | Stage::Retry => true,
            Stage::Arg | Stage::Premise | Stage::Exact | Stage::Book | Stage::Done => false,
        }
    }

    /// 是否结构/记账侧（arg/premise/exact/book：判定自身成不成立）。
    pub fn structural(&self) -> bool {
        match self {
            Stage::Arg | Stage::Premise | Stage::Exact | Stage::Book => true,
            Stage::Release | Stage::Retry | Stage::Done => false,
        }
    }

    /// 是否已进入"腾完地方再写"的阶段（release 及其后）。
    pub fn after_release(&self) -> bool {
        self.order() >= Stage::Release.order()
    }
}

/// 结果注（现场形态的分类；13 态互斥）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Note {
    /// 恢复成立：首刀 28、腾够、同 payload、长度精确且比过字节、账记一次。
    Recovered,
    /// 入参非法：`-1/22`，恢复根本没开始。
    ArgBad,
    /// 前提破断：首刀不是 ENOSPC（EDQUOT 误判），删件动作无从谈起。
    WrongFirst,
    /// 没腾地方就重试：第二刀仍 `-1/28`。
    NoRelease,
    /// 腾了但不够：第二刀部分写入后 `-1/28`。
    ShortFree,
    /// 换了 payload 再重试：尺寸对不上（同一 payload 这条不成立）。
    PayloadDrift,
    /// 第二刀仍失败，文件留着首刀写的字节。
    StillFull,
    /// 第二刀失败且文件被截得比首刀更短（数据丢失）。
    TruncatedLeft,
    /// 退出码 0 但最终长度与 payload 不符。
    SizeShort,
    /// 长度对但没做逐字节比较就宣称恢复。
    Unproven,
    /// 重试被记成第二次失败。
    BookedRetryFail,
    /// 恢复根本没入账。
    BookedLoose,
    /// 释放块被双计（已释放 + 已回账各一次）。
    BookedDouble,
}

impl Note {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Note::Recovered => "recovered",
            Note::ArgBad => "arg-bad",
            Note::WrongFirst => "wrong-first",
            Note::NoRelease => "norelease",
            Note::ShortFree => "short-free",
            Note::PayloadDrift => "payload-drift",
            Note::StillFull => "still-full",
            Note::TruncatedLeft => "truncated-left",
            Note::SizeShort => "size-short",
            Note::Unproven => "unproven",
            Note::BookedRetryFail => "booked-retry-fail",
            Note::BookedLoose => "booked-loose",
            Note::BookedDouble => "booked-double",
        }
    }

    /// 是否成功态注解（本卡只有 Recovered 一个）。
    pub fn success(&self) -> bool {
        matches!(self, Note::Recovered)
    }

    /// 是否前提面（首刀码不对）。
    pub fn premise_cause(&self) -> bool {
        matches!(self, Note::WrongFirst)
    }

    /// 是否释放面（没腾 / 腾不够）。
    pub fn release_cause(&self) -> bool {
        matches!(self, Note::NoRelease | Note::ShortFree)
    }

    /// 是否重试面（第二刀本身出问题）。
    pub fn retry_cause(&self) -> bool {
        matches!(self, Note::StillFull | Note::TruncatedLeft)
    }

    /// 是否精确面（长度/字节对不上）。
    pub fn drift_cause(&self) -> bool {
        matches!(self, Note::PayloadDrift | Note::SizeShort | Note::Unproven)
    }

    /// 是否记账面（账记错了）。
    pub fn book_cause(&self) -> bool {
        matches!(
            self,
            Note::BookedRetryFail | Note::BookedLoose | Note::BookedDouble
        )
    }

    /// 是否结构面（入参自毁：既非空间亦非记账）。
    pub fn structural(&self) -> bool {
        matches!(self, Note::ArgBad)
    }

    /// 是否"恢复没做成"这一大类（含入参、前提、释放、重试四面）。
    pub fn stalled(&self) -> bool {
        self.structural() || self.premise_cause() || self.release_cause() || self.retry_cause()
    }

    /// 是否落在四类之一（互斥完备，供逐格分类校验）。
    pub fn classes(&self) -> u8 {
        self.success() as u8
            + self.stalled() as u8
            + self.drift_cause() as u8
            + self.book_cause() as u8
    }

    /// 是否"必须由释放量解释"的注解（空间面两格）。
    pub fn space_cause(&self) -> bool {
        self.release_cause() || self.retry_cause()
    }

    /// 是否"字节没锚住"的注解（精确面三格）。
    pub fn exact_cause(&self) -> bool {
        self.drift_cause()
    }
}

/// 结果档位（四档互斥，由实际结果投影）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// 放行档：恢复成立。
    Pass,
    /// 停摆档：恢复没做成（含入参短路与前提破断）。
    Stall,
    /// 漂移档：写完了但字节锚不住。
    Drift,
    /// 记账档：恢复了但账记错（含结构面）。
    Book,
}

impl Tier {
    /// 由结果投影。
    pub fn of(o: &Outcome) -> Tier {
        if o.ok() {
            Tier::Pass
        } else if o.note.stalled() {
            Tier::Stall
        } else if o.note.drift_cause() {
            Tier::Drift
        } else {
            Tier::Book
        }
    }

    /// 次序。
    pub fn index(&self) -> usize {
        match self {
            Tier::Pass => 0,
            Tier::Stall => 1,
            Tier::Drift => 2,
            Tier::Book => 3,
        }
    }

    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Tier::Pass => "pass",
            Tier::Stall => "stall",
            Tier::Drift => "drift",
            Tier::Book => "book",
        }
    }

    /// 是否成功档。
    pub fn success(&self) -> bool {
        matches!(self, Tier::Pass)
    }

    /// 是否停摆档。
    pub fn stall(&self) -> bool {
        matches!(self, Tier::Stall)
    }

    /// 是否漂移档。
    pub fn drift(&self) -> bool {
        matches!(self, Tier::Drift)
    }

    /// 是否记账档。
    pub fn book(&self) -> bool {
        matches!(self, Tier::Book)
    }

    /// 是否失败三面之一（stall/drift/book：恢复面的三张反面）。
    pub fn failure_face(&self) -> bool {
        !self.success()
    }

    /// 四档枚举表（渲染与遍历用）。
    pub fn all() -> [Tier; TIER_COUNT] {
        [Tier::Pass, Tier::Stall, Tier::Drift, Tier::Book]
    }
}

/// 申报缺陷。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// 一致。
    Ok,
    /// 阶段错。
    Stage,
    /// errno 错。
    Code,
    /// 最终字节错。
    Value,
    /// 注解错。
    Note,
    /// 恢复样本档位错。
    Default,
    /// 字节精确性破断（结构维）。
    Exact,
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
            Fault::Stage => RR_ESTAGE,
            Fault::Code => RR_ECODE,
            Fault::Value => RR_EVALUE,
            Fault::Note => RR_ENOTE,
            Fault::Default => RR_EDEFAULT,
            Fault::Exact => RR_EEXACT,
            Fault::Estate => RR_ESTATE,
            Fault::Count => RR_ECOUNT,
            Fault::Slot => RR_ESLOT,
            Fault::Seal => RR_ESEAL,
            Fault::Nobs => RR_ENOBS,
            Fault::Matrix => RR_EMATRIX,
            Fault::Bal => RR_EBAL,
        }
    }

    /// 是否非零。
    pub fn alive(&self) -> bool {
        !matches!(self, Fault::Ok)
    }
}

/// 短名。
pub fn fault_short(f: Fault) -> &'static str {
    match f {
        Fault::Ok => "ok",
        Fault::Stage => "stage",
        Fault::Code => "code",
        Fault::Value => "value",
        Fault::Note => "note",
        Fault::Default => "default",
        Fault::Exact => "exact",
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
// 释放量与记账的几何
// ---------------------------------------------------------------------------

/// 一次"缺口 vs 腾出来的空间"的对账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fit {
    /// 缺口（payload 字节）。
    pub need: u32,
    /// 可用于重试的字节（`free_before + filler`）。
    pub have: u32,
}

impl Fit {
    /// 构造。
    pub const fn new(need: u32, have: u32) -> Fit {
        Fit { need, have }
    }

    /// 是否盖住缺口。
    pub fn covers(&self) -> bool {
        self.have >= self.need
    }

    /// 还差多少字节（盖住时为 0）。
    pub fn short_by(&self) -> u32 {
        self.need.saturating_sub(self.have)
    }

    /// 覆盖率（百万分位；need 为 0 时按 0 处理，不除零）。
    pub fn ratio_ppm(&self) -> u32 {
        if self.need == 0 {
            return 0;
        }
        let h = self.have.min(self.need) as u64;
        let n = self.need as u64;
        ((h * 1_000_000) / n) as u32
    }

    /// 是否自洽（两个读数都在上界内且无溢出）。
    pub fn well_formed(&self) -> bool {
        self.need > 0 && self.need <= PAYLOAD_MAX && self.have <= FREE_MAX.saturating_mul(2)
    }

    /// 渲染一行对账。
    pub fn render(&self) -> String {
        let mut s = String::from("fit need=");
        s.push_str(dec_text(self.need).as_str());
        s.push_str(" have=");
        s.push_str(dec_text(self.have).as_str());
        s.push_str(" short=");
        s.push_str(dec_text(self.short_by()).as_str());
        s.push_str(" ratio=");
        s.push_str(dec_text(self.ratio_ppm()).as_str());
        s.push_str("ppm\n");
        s
    }
}

/// 一次释放动作的几何（本卡的"腾地方"表）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReleaseGeom {
    /// 标签（唯一）。
    pub tag: &'static str,
    /// payload 字节。
    pub payload: u32,
    /// 首刀之前的可用字节。
    pub free_before: u32,
    /// 释放出来的字节。
    pub filler: u32,
    /// 首刀已经写进去的字节（0 表示首刀一字节没落）。
    pub first_written: u32,
}

impl ReleaseGeom {
    /// 空白几何。
    pub const fn blank() -> ReleaseGeom {
        ReleaseGeom {
            tag: "",
            payload: 0,
            free_before: 0,
            filler: 0,
            first_written: 0,
        }
    }

    /// 对账。
    pub fn fit(&self) -> Fit {
        Fit::new(self.payload, self.free_before.saturating_add(self.filler))
    }

    /// 是否自洽（首刀写进去的字节不超过 payload，且首刀 ENOSPC 时可用量必不足）。
    pub fn well_formed(&self) -> bool {
        if self.tag.is_empty() {
            return false;
        }
        if self.payload == 0 || self.payload > PAYLOAD_MAX {
            return false;
        }
        if self.first_written > self.payload {
            return false;
        }
        if self.filler > FREE_MAX || self.free_before > FREE_MAX {
            return false;
        }
        // 首刀既然报 ENOSPC，首刀之前的可用量必须小于 payload。
        self.free_before < self.payload
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_str(h, self.tag);
        h = fold_word(h, self.payload as u64);
        h = fold_word(h, self.free_before as u64);
        h = fold_word(h, self.filler as u64);
        fold_word(h, self.first_written as u64)
    }
}

/// 四条释放几何：删 filler 刚好够、截断临时件补齐、首刀部分写后补齐、腾不够。
pub fn release_geoms() -> [ReleaseGeom; 4] {
    [
        ReleaseGeom {
            tag: "filler-covers",
            payload: 8192,
            free_before: 0,
            filler: 8192,
            first_written: 0,
        },
        ReleaseGeom {
            tag: "truncate-tops-up",
            payload: 4096,
            free_before: 1024,
            filler: 3072,
            first_written: 0,
        },
        ReleaseGeom {
            tag: "partial-first-splice",
            payload: 12288,
            free_before: 0,
            filler: 12288,
            first_written: 4096,
        },
        ReleaseGeom {
            tag: "short-by-half",
            payload: 8192,
            free_before: 0,
            filler: 4096,
            first_written: 0,
        },
    ]
}

// ---------------------------------------------------------------------------
// 形状（本卡输入的全部 13 维）
// ---------------------------------------------------------------------------
//
// 13 维逐条对应卡面语料的九元组，并把"账"与"证"拆成显式的两维：
//
//  1 tag            槽标签（唯一）
//  2 path           两刀共用的目标路径（同一路径这一维的载体）
//  3 payload        首刀想要的字节数（同一 payload 这一维的载体）
//  4 free_before    首刀之前的可用字节（盘上本来有多少）
//  5 filler         释放出来的字节（删 filler / 截断临时件）
//  6 first_code     首刀 errno（必须是 28）
//  7 first_written  首刀已经落盘的字节（部分写）
//  8 freed_before   重试之前到底腾没腾
//  9 retry_payload  重试这一刀请求的字节数（必须等于 payload）
// 10 retry_code     重试 errno（0 表示第二刀成了）
// 11 retry_size     重试之后的最终长度（必须等于 payload）
// 12 booked         这一刀在账上算几次（四值）
// 13 proven         有没有做过逐字节比较（同一份字节这一维的载体）

/// 一次"写满 → 腾地方 → 重试"的观测形状。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shape {
    /// 槽标签（唯一）。
    pub tag: &'static str,
    /// 目标路径（两刀共用）。
    pub path: &'static str,
    /// 首刀请求的字节数。
    pub payload: u32,
    /// 首刀之前的可用字节。
    pub free_before: u32,
    /// 释放出来的字节。
    pub filler: u32,
    /// 首刀 errno。
    pub first_code: i32,
    /// 首刀已落盘字节。
    pub first_written: u32,
    /// 重试之前是否腾过地方。
    pub freed_before: bool,
    /// 重试请求的字节数。
    pub retry_payload: u32,
    /// 重试 errno。
    pub retry_code: i32,
    /// 重试后的最终长度。
    pub retry_size: u32,
    /// 记账几值。
    pub booked: u8,
    /// 是否做过逐字节比较。
    pub proven: bool,
}

impl Shape {
    /// 默认形状：删 filler 刚够盖住 8192 的缺口，重试成功且比过字节。
    pub const fn blank() -> Shape {
        Shape {
            tag: "",
            path: "srv/data/archive.bin",
            payload: 8192,
            free_before: 0,
            filler: 8192,
            first_code: ERR_ENOSPC,
            first_written: 0,
            freed_before: true,
            retry_payload: 8192,
            retry_code: ERR_NONE,
            retry_size: 8192,
            booked: BOOK_RECOVER,
            proven: true,
        }
    }

    /// 释放量对账副本。
    pub fn geom(&self) -> ReleaseGeom {
        ReleaseGeom {
            tag: self.tag,
            payload: self.payload,
            free_before: self.free_before,
            filler: self.filler,
            first_written: self.first_written,
        }
    }

    /// 缺口 vs 腾出来的空间。
    pub fn fit(&self) -> Fit {
        Fit::new(self.payload, self.free_before.saturating_add(self.filler))
    }

    /// 缺口还差多少。
    pub fn shortfall(&self) -> u32 {
        self.fit().short_by()
    }

    /// 换 payload 的副本。
    pub fn with_payload(&self, v: u32) -> Shape {
        Shape { payload: v, ..*self }
    }

    /// 换首刀之前可用量的副本。
    pub fn with_free_before(&self, v: u32) -> Shape {
        Shape {
            free_before: v,
            ..*self
        }
    }

    /// 换释放量的副本。
    pub fn with_filler(&self, v: u32) -> Shape {
        Shape { filler: v, ..*self }
    }

    /// 换首刀 errno 的副本。
    pub fn with_first_code(&self, v: i32) -> Shape {
        Shape {
            first_code: v,
            ..*self
        }
    }

    /// 换首刀已落盘字节的副本。
    pub fn with_first_written(&self, v: u32) -> Shape {
        Shape {
            first_written: v,
            ..*self
        }
    }

    /// 换"腾没腾"的副本。
    pub fn with_freed(&self, v: bool) -> Shape {
        Shape {
            freed_before: v,
            ..*self
        }
    }

    /// 换重试 payload 的副本。
    pub fn with_retry_payload(&self, v: u32) -> Shape {
        Shape {
            retry_payload: v,
            ..*self
        }
    }

    /// 换重试 errno 的副本。
    pub fn with_retry_code(&self, v: i32) -> Shape {
        Shape {
            retry_code: v,
            ..*self
        }
    }

    /// 换最终长度的副本。
    pub fn with_retry_size(&self, v: u32) -> Shape {
        Shape {
            retry_size: v,
            ..*self
        }
    }

    /// 换记账值的副本。
    pub fn with_booked(&self, v: u8) -> Shape {
        Shape { booked: v, ..*self }
    }

    /// 换"比没比字节"的副本。
    pub fn with_proven(&self, v: bool) -> Shape {
        Shape { proven: v, ..*self }
    }

    /// 换路径的副本。
    pub fn with_path(&self, v: &'static str) -> Shape {
        Shape { path: v, ..*self }
    }

    /// errno 是否在合法集内。
    pub fn errno_legal(&self) -> bool {
        let a = self.first_code;
        let b = self.retry_code;
        (a == ERR_NONE || a == ERR_ENOSPC || a == ERR_EDQUOT || a == ERR_EINVAL)
            && (b == ERR_NONE || b == ERR_ENOSPC || b == ERR_EDQUOT || b == ERR_EINVAL)
    }

    /// 记账值是否在四值集内。
    pub fn booked_legal(&self) -> bool {
        self.booked < BOOK_SET_SIZE as u8
    }

    /// 入参是否合法（第一闸的判据）。
    pub fn arg_legal(&self) -> bool {
        self.illegal_field().is_none()
    }

    /// 第一个非法入参：返回（字段名, 读数）。次序固定，保证文案可复算。
    pub fn illegal_field(&self) -> Option<(&'static str, u32)> {
        if self.payload == 0 {
            return Some(("payload", 0));
        }
        if self.payload > PAYLOAD_MAX {
            return Some(("payload", self.payload));
        }
        if self.first_written > self.payload {
            return Some(("first_written", self.first_written));
        }
        if self.filler > FREE_MAX {
            return Some(("filler", self.filler));
        }
        if self.free_before > FREE_MAX {
            return Some(("free_before", self.free_before));
        }
        if self.retry_payload > PAYLOAD_MAX {
            return Some(("retry_payload", self.retry_payload));
        }
        if self.retry_size > PAYLOAD_MAX {
            return Some(("retry_size", self.retry_size));
        }
        if !self.errno_legal() {
            if self.first_code != ERR_NONE
                && self.first_code != ERR_ENOSPC
                && self.first_code != ERR_EDQUOT
                && self.first_code != ERR_EINVAL
            {
                return Some(("first_code", self.first_code as u32));
            }
            return Some(("retry_code", self.retry_code as u32));
        }
        if !self.booked_legal() {
            return Some(("booked", self.booked as u32));
        }
        // 首刀既报 ENOSPC，则首刀之前可用量必须小于 payload——否则读数自相矛盾。
        if self.first_code == ERR_ENOSPC && self.free_before >= self.payload {
            return Some(("free_before", self.free_before));
        }
        None
    }

    /// 形状是否合法可判（含释放几何自洽）。
    pub fn valid(&self) -> bool {
        if !self.arg_legal() {
            return false;
        }
        self.geom().well_formed()
    }

    /// 指纹：对全部 13 维敏感。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_str(h, self.tag);
        h = fold_str(h, self.path);
        h = fold_word(h, self.payload as u64);
        h = fold_word(h, self.free_before as u64);
        h = fold_word(h, self.filler as u64);
        h = fold_word(h, self.first_code as i64 as u64);
        h = fold_word(h, self.first_written as u64);
        h = fold_word(h, self.freed_before as u64);
        h = fold_word(h, self.retry_payload as u64);
        h = fold_word(h, self.retry_code as i64 as u64);
        h = fold_word(h, self.retry_size as u64);
        h = fold_word(h, self.booked as u64);
        fold_word(h, self.proven as u64)
    }
}

// ---------------------------------------------------------------------------
// 结果
// ---------------------------------------------------------------------------

/// 一次判定的结果（六维）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// 结束阶段。
    pub stage: Stage,
    /// errno（0 表示第二刀成了；前提破断时为首刀码）。
    pub code: i32,
    /// 最终字节（恢复成立时等于 payload；失败时是实际留下的字节）。
    pub value: u32,
    /// 形态注解。
    pub note: Note,
    /// 恢复样本档位：该现场是否构成一条可入账的"释放→重试"对照样本。
    pub default: bool,
}

impl Outcome {
    /// 空白结果（放行态）。
    pub const fn blank() -> Outcome {
        Outcome {
            stage: Stage::Done,
            code: ERR_NONE,
            value: 0,
            note: Note::Recovered,
            default: false,
        }
    }

    /// 失败结果。
    const fn fail(stage: Stage, code: i32, value: u32, note: Note) -> Outcome {
        Outcome {
            stage,
            code,
            value,
            note,
            default: false,
        }
    }

    /// 成功结果（唯一放行态）。
    const fn pass(value: u32) -> Outcome {
        Outcome {
            stage: Stage::Done,
            code: ERR_NONE,
            value,
            note: Note::Recovered,
            default: true,
        }
    }

    /// 是否成功（恢复成立）。
    pub fn ok(&self) -> bool {
        self.stage == Stage::Done && self.code == ERR_NONE && self.note.success()
    }

    /// 是否失败。
    pub fn failed(&self) -> bool {
        !self.ok()
    }

    /// 是否恢复样本（失败侧一律不是）。
    pub fn recovered(&self) -> bool {
        self.ok() && self.default
    }

    /// 结果的码族文本。
    pub fn family(&self) -> &'static str {
        code_name(self.code)
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.stage.order() as u64);
        h = fold_word(h, self.code as i64 as u64);
        h = fold_word(h, self.value as u64);
        h = fold_str(h, self.note.name());
        fold_word(h, self.default as u64)
    }
}

// ---------------------------------------------------------------------------
// 定义纵线（arg → premise → release → retry → exact → book → done）
// ---------------------------------------------------------------------------

/// 判定一次"释放→重试"现场。
///
/// 每一闸只在上一闸放行后执行。`premise` 排在 `release` 之前，是因为
/// **"该不该腾地方"取决于"这次失败是不是空间问题"**：首刀报 EDQUOT 时，
/// 腾多少都回不来额度。`exact` 排在 `book` 之前，是因为**先证明恢复是真的、
/// 再证明账是对的**；反过来先查账会得出"账记成恢复了所以算恢复"的循环。
pub fn define(s: &Shape) -> Outcome {
    // ---- arg ----
    if let Some((field, v)) = s.illegal_field() {
        let _ = field;
        let _ = v;
        return Outcome::fail(Stage::Arg, ERR_EINVAL, 0, Note::ArgBad);
    }

    // ---- premise ----
    if s.first_code != ERR_ENOSPC {
        return Outcome::fail(Stage::Premise, s.first_code, 0, Note::WrongFirst);
    }

    // ---- release ----
    if !s.freed_before {
        // 没腾地方就重试：第二刀写得进去的只有首刀之前的余量。
        return Outcome::fail(Stage::Release, ERR_ENOSPC, s.free_before, Note::NoRelease);
    }
    let fit = s.fit();
    if !fit.covers() {
        // 腾了但不够：第二刀先写满余量再撞 ENOSPC，留下半截文件。
        return Outcome::fail(Stage::Release, ERR_ENOSPC, fit.have, Note::ShortFree);
    }

    // ---- retry ----
    if s.retry_payload != s.payload {
        // 换了 payload：第二刀成不成已经与"恢复"无关。
        return Outcome::fail(
            Stage::Retry,
            s.retry_code,
            s.retry_size,
            Note::PayloadDrift,
        );
    }
    if s.retry_code != ERR_NONE {
        if s.retry_size < s.first_written {
            // 比首刀还短：文件被截断了，这是数据丢失。
            return Outcome::fail(
                Stage::Retry,
                s.retry_code,
                s.retry_size,
                Note::TruncatedLeft,
            );
        }
        return Outcome::fail(Stage::Retry, s.retry_code, s.retry_size, Note::StillFull);
    }

    // ---- exact ----
    if s.retry_size != s.payload {
        // 退出码 0 但长度不符：写完了不等于写对了。
        return Outcome::fail(Stage::Exact, ERR_NONE, s.retry_size, Note::SizeShort);
    }
    if !s.proven {
        // 长度对但没比过字节：宣称恢复不成立。
        return Outcome::fail(Stage::Exact, ERR_NONE, s.retry_size, Note::Unproven);
    }

    // ---- book ----
    match s.booked {
        BOOK_RECOVER => Outcome::pass(s.payload),
        BOOK_NEWFAIL => Outcome::fail(
            Stage::Book,
            ERR_NONE,
            s.payload,
            Note::BookedRetryFail,
        ),
        BOOK_DOUBLE => Outcome::fail(Stage::Book, ERR_NONE, s.payload, Note::BookedDouble),
        // 恢复到账上一个字都没记：等价于"没恢复"。
        _ => Outcome::fail(Stage::Book, ERR_NONE, s.payload, Note::BookedLoose),
    }
}

/// 现场结束阶段（供联签与交叉校验复算）。
pub fn stage_of(s: &Shape) -> Stage {
    define(s).stage
}

/// 现场结束码（同上）。
pub fn code_of(s: &Shape) -> i32 {
    define(s).code
}

/// 现场最终字节（同上）。
pub fn value_of(s: &Shape) -> u32 {
    define(s).value
}

/// 现场文案（note → 真实文本；恢复成立时为空串）。
pub fn message(s: &Shape, o: &Outcome) -> String {
    match o.note {
        Note::Recovered => String::new(),
        Note::ArgBad => match s.illegal_field() {
            Some((field, v)) => msg_arg_bad(field, v),
            None => msg_arg_bad("shape", 0),
        },
        Note::WrongFirst => msg_wrong_first(s.first_code),
        Note::NoRelease => msg_no_release(s.path, s.free_before),
        Note::ShortFree => msg_short_free(s.path, s.payload, s.fit().have),
        Note::PayloadDrift => msg_payload_drift(s.path, s.payload, s.retry_payload),
        Note::StillFull => msg_still_full(s.path, s.retry_size),
        Note::TruncatedLeft => msg_truncated_left(s.path, s.first_written, s.retry_size),
        Note::SizeShort => msg_size_short(s.path, s.payload, s.retry_size),
        Note::Unproven => msg_unproven(s.path, s.retry_size),
        Note::BookedRetryFail => msg_booked_fail(2),
        Note::BookedLoose => msg_booked_loose(s.path),
        Note::BookedDouble => {
            msg_booked_double(s.filler, s.filler.saturating_mul(2))
        }
    }
}

/// 一句话结论（人读口径，判据不落账）。
pub fn verdict(o: &Outcome) -> &'static str {
    if o.ok() {
        return "recovered";
    }
    match o.note {
        Note::Recovered => "recovered",
        Note::ArgBad => "invalid argument",
        Note::WrongFirst => "premise broken",
        Note::NoRelease => "no release",
        Note::ShortFree => "short release",
        Note::PayloadDrift => "payload drift",
        Note::StillFull => "still full",
        Note::TruncatedLeft => "truncated",
        Note::SizeShort => "size short",
        Note::Unproven => "unproven",
        Note::BookedRetryFail => "retry booked as failure",
        Note::BookedLoose => "recovery not booked",
        Note::BookedDouble => "freed block double counted",
    }
}

// ---------------------------------------------------------------------------
// 申报与用例
// ---------------------------------------------------------------------------

/// 一槽申报。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Claim {
    /// 结束阶段。
    pub stage: Stage,
    /// errno。
    pub code: i32,
    /// 最终字节。
    pub value: u32,
    /// 形态注解。
    pub note: Note,
    /// 恢复样本档位。
    pub default: bool,
}

impl Claim {
    /// 空白申报。
    pub const fn blank() -> Claim {
        Claim {
            stage: Stage::Done,
            code: ERR_NONE,
            value: 0,
            note: Note::Recovered,
            default: false,
        }
    }

    /// 是否申报"恢复成功"（码归零且注解属成功态）。
    pub fn success(&self) -> bool {
        self.code == ERR_NONE && self.note.success()
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
        Case {
            shape: Shape::blank(),
            claim: Claim::blank(),
        }
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

/// 逐槽释义：一槽一行，十三维取值与六维结果全部落在文本里。
pub fn detail(c: &Case) -> String {
    let o = c.actual();
    let msg = message(&c.shape, &o);
    let mut s = String::from(c.shape.path);
    s.push_str(" payload=");
    s.push_str(dec_text(c.shape.payload).as_str());
    s.push_str(" free=");
    s.push_str(dec_text(c.shape.free_before).as_str());
    s.push_str(" freed=");
    s.push_str(dec_text(c.shape.filler).as_str());
    s.push_str(" first=");
    s.push_str(code_name(c.shape.first_code));
    s.push_str(" wrote=");
    s.push_str(dec_text(c.shape.first_written).as_str());
    s.push_str(" retry=");
    s.push_str(code_name(c.shape.retry_code));
    s.push_str(" size=");
    s.push_str(dec_text(c.shape.retry_size).as_str());
    s.push_str(" booked=");
    s.push_str(book_name(c.shape.booked));
    s.push_str(" stage=");
    s.push_str(o.stage.name());
    s.push_str(" errno=");
    s.push_str(i32_text(o.code).as_str());
    s.push_str(" ret=");
    s.push_str(dec_text(o.value).as_str());
    s.push_str(" pair=");
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

// ---------------------------------------------------------------------------
// 语料（14 槽：8 活 + 6 死）
// ---------------------------------------------------------------------------

/// 8 个可正确申报的形状 + 6 个复刻形状（复刻自 `norelease-enospc`）。
///
/// 每个形状都写全 13 维，不用 `..Shape::blank()`：本卡的立卡理由是"恢复成立
/// 靠四根柱子"，任何一维被默认值悄悄填掉，红档就说不清是被哪一维判死的。
pub const SHAPES: [Shape; CORPUS] = [
    // 0 —— 删 filler 刚好盖住缺口：恢复的规范形态（同一 payload、同一路径、
    //      同一最终长度、比过字节、账记一次）。
    Shape {
        tag: "recover-filler",
        path: "srv/data/archive.bin",
        payload: 8192,
        free_before: 0,
        filler: 8192,
        first_code: ERR_ENOSPC,
        first_written: 0,
        freed_before: true,
        retry_payload: 8192,
        retry_code: ERR_NONE,
        retry_size: 8192,
        booked: BOOK_RECOVER,
        proven: true,
    },
    // 1 —— 截断临时件补齐：盘上本来还有 1024，截 3072 凑够 4096。
    Shape {
        tag: "recover-truncate",
        path: "var/tmp/scratch.tmp",
        payload: 4096,
        free_before: 1024,
        filler: 3072,
        first_code: ERR_ENOSPC,
        first_written: 0,
        freed_before: true,
        retry_payload: 4096,
        retry_code: ERR_NONE,
        retry_size: 4096,
        booked: BOOK_RECOVER,
        proven: true,
    },
    // 2 —— 首刀已经部分写了 4096，腾够 12288 后重试补齐：字节精确柱的正身
    //      （前半截来自首刀，后半截来自重试，比较必须逐字节做）。
    Shape {
        tag: "recover-partial",
        path: "home/alice/vm.img",
        payload: 12288,
        free_before: 0,
        filler: 12288,
        first_code: ERR_ENOSPC,
        first_written: 4096,
        freed_before: true,
        retry_payload: 12288,
        retry_code: ERR_NONE,
        retry_size: 12288,
        booked: BOOK_RECOVER,
        proven: true,
    },
    // 3 —— 一字节没腾就重试：第二刀必然还是 ENOSPC（六死槽的复刻源）。
    Shape {
        tag: "norelease-enospc",
        path: "mnt/full/big.bin",
        payload: 8192,
        free_before: 0,
        filler: 8192,
        first_code: ERR_ENOSPC,
        first_written: 0,
        freed_before: false,
        retry_payload: 8192,
        retry_code: ERR_ENOSPC,
        retry_size: 0,
        booked: BOOK_NONE,
        proven: false,
    },
    // 4 —— 腾了但只腾了一半：第二刀先写 4096 再撞 ENOSPC，留下半截文件。
    Shape {
        tag: "shortfree-partial",
        path: "mnt/full/half.bin",
        payload: 8192,
        free_before: 0,
        filler: 4096,
        first_code: ERR_ENOSPC,
        first_written: 0,
        freed_before: true,
        retry_payload: 8192,
        retry_code: ERR_ENOSPC,
        retry_size: 4096,
        booked: BOOK_NONE,
        proven: false,
    },
    // 5 —— 腾够了，但重试换了一份等长的 payload：尺寸对得上、字节不是那批。
    Shape {
        tag: "drift-payload",
        path: "mnt/full/drift.bin",
        payload: 8192,
        free_before: 0,
        filler: 8192,
        first_code: ERR_ENOSPC,
        first_written: 0,
        freed_before: true,
        retry_payload: 4096,
        retry_code: ERR_NONE,
        retry_size: 4096,
        booked: BOOK_NONE,
        proven: false,
    },
    // 6 —— 长度精确、退出码 0，但根本没做逐字节比较就宣称恢复。
    Shape {
        tag: "unproven-nofold",
        path: "mnt/full/claim.bin",
        payload: 8192,
        free_before: 0,
        filler: 8192,
        first_code: ERR_ENOSPC,
        first_written: 0,
        freed_before: true,
        retry_payload: 8192,
        retry_code: ERR_NONE,
        retry_size: 8192,
        booked: BOOK_RECOVER,
        proven: false,
    },
    // 7 —— 恢复是真的，但释放出来的 8192 被"已释放"与"已回账"各记一次。
    Shape {
        tag: "bookdouble-credit",
        path: "mnt/full/twice.bin",
        payload: 8192,
        free_before: 0,
        filler: 8192,
        first_code: ERR_ENOSPC,
        first_written: 0,
        freed_before: true,
        retry_payload: 8192,
        retry_code: ERR_NONE,
        retry_size: 8192,
        booked: BOOK_DOUBLE,
        proven: true,
    },
    // 8 —— 复刻 norelease，错报为"重试成功了"（Estate）。
    Shape {
        tag: "estate-swap",
        path: "mnt/full/big.bin",
        payload: 8192,
        free_before: 0,
        filler: 8192,
        first_code: ERR_ENOSPC,
        first_written: 0,
        freed_before: false,
        retry_payload: 8192,
        retry_code: ERR_ENOSPC,
        retry_size: 0,
        booked: BOOK_NONE,
        proven: false,
    },
    // 9 —— 复刻 norelease，把释放闸说成重试闸（Stage）。
    Shape {
        tag: "stage-swap",
        path: "mnt/full/big.bin",
        payload: 8192,
        free_before: 0,
        filler: 8192,
        first_code: ERR_ENOSPC,
        first_written: 0,
        freed_before: false,
        retry_payload: 8192,
        retry_code: ERR_ENOSPC,
        retry_size: 0,
        booked: BOOK_NONE,
        proven: false,
    },
    // 10 —— 复刻 norelease，把 28 报成 122（Code，本卡前提柱的头号缺陷）。
    Shape {
        tag: "code-swap",
        path: "mnt/full/big.bin",
        payload: 8192,
        free_before: 0,
        filler: 8192,
        first_code: ERR_ENOSPC,
        first_written: 0,
        freed_before: false,
        retry_payload: 8192,
        retry_code: ERR_ENOSPC,
        retry_size: 0,
        booked: BOOK_NONE,
        proven: false,
    },
    // 11 —— 复刻 norelease，在一笔没落盘的写上申报 4096 字节（Value）。
    Shape {
        tag: "value-swap",
        path: "mnt/full/big.bin",
        payload: 8192,
        free_before: 0,
        filler: 8192,
        first_code: ERR_ENOSPC,
        first_written: 0,
        freed_before: false,
        retry_payload: 8192,
        retry_code: ERR_ENOSPC,
        retry_size: 0,
        booked: BOOK_NONE,
        proven: false,
    },
    // 12 —— 复刻 norelease，码与阶段都对但注解写成"腾不够"（Note）。
    Shape {
        tag: "note-swap",
        path: "mnt/full/big.bin",
        payload: 8192,
        free_before: 0,
        filler: 8192,
        first_code: ERR_ENOSPC,
        first_written: 0,
        freed_before: false,
        retry_payload: 8192,
        retry_code: ERR_ENOSPC,
        retry_size: 0,
        booked: BOOK_NONE,
        proven: false,
    },
    // 13 —— 复刻 norelease，五维全对却把停摆现场申报成恢复样本（Default）。
    Shape {
        tag: "default-swap",
        path: "mnt/full/big.bin",
        payload: 8192,
        free_before: 0,
        filler: 8192,
        first_code: ERR_ENOSPC,
        first_written: 0,
        freed_before: false,
        retry_payload: 8192,
        retry_code: ERR_ENOSPC,
        retry_size: 0,
        booked: BOOK_NONE,
        proven: false,
    },
];

/// 与 `SHAPES` 一一对应的申报。
///
/// 前 8 槽逐维等于现场；后 6 槽各只错一维，错的那维依次是
/// estate/stage/code/value/note/default。
pub const CLAIMS: [Claim; CORPUS] = [
    Claim {
        stage: Stage::Done,
        code: 0,
        value: 8192,
        note: Note::Recovered,
        default: true,
    },
    Claim {
        stage: Stage::Done,
        code: 0,
        value: 4096,
        note: Note::Recovered,
        default: true,
    },
    Claim {
        stage: Stage::Done,
        code: 0,
        value: 12288,
        note: Note::Recovered,
        default: true,
    },
    Claim {
        stage: Stage::Release,
        code: 28,
        value: 0,
        note: Note::NoRelease,
        default: false,
    },
    Claim {
        stage: Stage::Release,
        code: 28,
        value: 4096,
        note: Note::ShortFree,
        default: false,
    },
    Claim {
        stage: Stage::Retry,
        code: 0,
        value: 4096,
        note: Note::PayloadDrift,
        default: false,
    },
    Claim {
        stage: Stage::Exact,
        code: 0,
        value: 8192,
        note: Note::Unproven,
        default: false,
    },
    Claim {
        stage: Stage::Book,
        code: 0,
        value: 8192,
        note: Note::BookedDouble,
        default: false,
    },
    // 8 —— 成败互报：没腾地方的第二刀被申报成"恢复成立"。
    Claim {
        stage: Stage::Done,
        code: 0,
        value: 8192,
        note: Note::Recovered,
        default: true,
    },
    // 9 —— 阶段错：码与注解都对，只把释放闸说成重试闸。
    Claim {
        stage: Stage::Retry,
        code: 28,
        value: 0,
        note: Note::NoRelease,
        default: false,
    },
    // 10 —— 码错：ENOSPC 报成 EDQUOT（28 → 122），前提柱的头号缺陷。
    Claim {
        stage: Stage::Release,
        code: 122,
        value: 0,
        note: Note::NoRelease,
        default: false,
    },
    // 11 —— 字节错：一笔 0 字节的失败申报落盘 4096。
    Claim {
        stage: Stage::Release,
        code: 28,
        value: 4096,
        note: Note::NoRelease,
        default: false,
    },
    // 12 —— 注解错：28 配"腾不够"的注解（没腾与腾不够是两件事）。
    Claim {
        stage: Stage::Release,
        code: 28,
        value: 0,
        note: Note::ShortFree,
        default: false,
    },
    // 13 —— 档位错：五维全对，却把停摆现场当成恢复样本。
    Claim {
        stage: Stage::Release,
        code: 28,
        value: 0,
        note: Note::NoRelease,
        default: true,
    },
];

/// 后 6 槽的期望缺陷。
pub const REJECT_FAULTS: [Fault; REJECT] = [
    Fault::Estate,
    Fault::Stage,
    Fault::Code,
    Fault::Value,
    Fault::Note,
    Fault::Default,
];

/// 后 6 槽的缺陷码。
pub const REJECT_CODES: [u32; REJECT] = [
    RR_ESTATE,
    RR_ESTAGE,
    RR_ECODE,
    RR_EVALUE,
    RR_ENOTE,
    RR_EDEFAULT,
];

/// 账本 8 个成功格的值（总数、成、败、恢复档、停摆档、漂移档、记账档、
/// 红档率千分位）。**第 8 格恒为红档率 929**。
pub const VALS: [u32; ACCEPT] = [14, 8, 6, 3, 2, 2, 1, 929];

/// 账本 14 格的操作码（逐格归属的闸门，`ops_match()` 复核）。
pub const LEDGER_OPS: [u8; SAMPLES] = [
    OP_CORPUS,
    OP_CORPUS,
    OP_CORPUS,
    OP_RELEASE,
    OP_RETRY,
    OP_EXACT,
    OP_BOOK,
    OP_REPLAY,
    OP_BOOK,
    OP_BOOK,
    OP_BOOK,
    OP_BOOK,
    OP_BOOK,
    OP_BOOK,
];

/// 语料表。
pub fn cases() -> [Case; CORPUS] {
    let mut out = [Case::blank(); CORPUS];
    let mut i = 0usize;
    while i < CORPUS {
        if let (Some(s), Some(c)) = (SHAPES.get(i), CLAIMS.get(i)) {
            if let Some(dst) = out.get_mut(i) {
                *dst = Case {
                    shape: *s,
                    claim: *c,
                };
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
        return Err(RR_ENOBS);
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

/// 对判：状态 → 阶段 → errno → 最终字节 → 注解 → 恢复档位。
///
/// 次序不可换。成败互报排第一，是因为**它是唯一能让"其余五维完全合理"的
/// 申报仍然致命的维**：一个把"没腾地方的第二刀"说成恢复成立的申报，后面的
/// 阶段/码/字节全按成功口径写，逐维比下去每一维都"自洽"，只有跟现场对撞才
/// 露馅。反过来，把恢复成立说成失败（同一维的反向）也在这里被抓住。
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

// ---------------------------------------------------------------------------
// 期望表（语料的第二份陈述）
// ---------------------------------------------------------------------------

/// 14 槽的期望结束阶段。
const WANT_STAGE: [Stage; CORPUS] = [
    Stage::Done,
    Stage::Done,
    Stage::Done,
    Stage::Release,
    Stage::Release,
    Stage::Retry,
    Stage::Exact,
    Stage::Book,
    Stage::Release,
    Stage::Release,
    Stage::Release,
    Stage::Release,
    Stage::Release,
    Stage::Release,
];

/// 14 槽的期望 errno。
const WANT_CODE: [i32; CORPUS] = [
    ERR_NONE,
    ERR_NONE,
    ERR_NONE,
    ERR_ENOSPC,
    ERR_ENOSPC,
    ERR_NONE,
    ERR_NONE,
    ERR_NONE,
    ERR_ENOSPC,
    ERR_ENOSPC,
    ERR_ENOSPC,
    ERR_ENOSPC,
    ERR_ENOSPC,
    ERR_ENOSPC,
];

/// 14 槽的期望最终字节。
const WANT_VALUE: [u32; CORPUS] = [
    8192, 4096, 12288, 0, 4096, 4096, 8192, 8192, 0, 0, 0, 0, 0, 0,
];

/// 14 槽的期望注解。
const WANT_NOTE: [Note; CORPUS] = [
    Note::Recovered,
    Note::Recovered,
    Note::Recovered,
    Note::NoRelease,
    Note::ShortFree,
    Note::PayloadDrift,
    Note::Unproven,
    Note::BookedDouble,
    Note::NoRelease,
    Note::NoRelease,
    Note::NoRelease,
    Note::NoRelease,
    Note::NoRelease,
    Note::NoRelease,
];

/// 14 槽的期望恢复档位。
const WANT_DEFAULT: [bool; CORPUS] = [
    true, true, true, false, false, false, false, false, false, false, false, false, false, false,
];

/// 14 槽标签。
const WANT_TAGS: [&'static str; CORPUS] = [
    "recover-filler",
    "recover-truncate",
    "recover-partial",
    "norelease-enospc",
    "shortfree-partial",
    "drift-payload",
    "unproven-nofold",
    "bookdouble-credit",
    "estate-swap",
    "stage-swap",
    "code-swap",
    "value-swap",
    "note-swap",
    "default-swap",
];

/// 14 槽的期望档位（由现场投影，语料的第三份陈述）。
const WANT_TIER: [Tier; CORPUS] = [
    Tier::Pass,
    Tier::Pass,
    Tier::Pass,
    Tier::Stall,
    Tier::Stall,
    Tier::Drift,
    Tier::Drift,
    Tier::Book,
    Tier::Stall,
    Tier::Stall,
    Tier::Stall,
    Tier::Stall,
    Tier::Stall,
    Tier::Stall,
];

/// 八格申报正确的下标。
const ACCEPT_SLOTS: [usize; ACCEPT] = [0usize, 1, 2, 3, 4, 5, 6, 7];

/// 六格申报错误的下标。
const REJECT_SLOTS: [usize; REJECT] = [8usize, 9, 10, 11, 12, 13];

/// 被复刻的主样本下标（六死槽与它现场同一）。
const REJECT_SOURCE: usize = 3;

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

    /// 该档红档数（`good > total` 的畸形行钳回 total，不许下溢）。
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
/// 分档只看**实际结果**落在哪一档，申报正确与否单独计。本卡尤其要分开：
/// 停摆档里塞了 6 个复刻红槽，如果两维混计，"停摆覆盖面"看起来会从 8 槽
/// 缩成 2 槽，覆盖面假象就出来了。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Matrix {
    /// 行（pass/stall/drift/book）。
    pub rows: [Row; TIER_COUNT],
}

impl Matrix {
    /// 空矩阵。
    pub const fn blank() -> Matrix {
        Matrix {
            rows: [Row::blank(); TIER_COUNT],
        }
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

    /// 三张反面（停摆/漂移/记账）是否都有槽——本卡的最低覆盖面：
    /// 恢复的四根柱子各至少要有一格反面证词。
    pub fn faces_covered(&self) -> bool {
        let a = match self.row(Tier::Stall) {
            Some(r) => r.total,
            None => 0,
        };
        let b = match self.row(Tier::Drift) {
            Some(r) => r.total,
            None => 0,
        };
        let c = match self.row(Tier::Book) {
            Some(r) => r.total,
            None => 0,
        };
        a > 0 && b > 0 && c > 0
    }

    /// 渲染全矩阵。
    pub fn render(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(t) = Tier::all().get(i) {
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
    /// 恢复档正确数。
    pub pass: u32,
    /// 停摆档正确数。
    pub stall: u32,
    /// 漂移档正确数。
    pub drift: u32,
    /// 记账档正确数。
    pub book: u32,
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
            Err(RR_ECOUNT)
        }
    }

    /// 恢复档合计（申报正确且恢复成立）。
    pub fn recovered_cells(&self) -> u32 {
        self.pass
    }

    /// 反面档合计（停摆 + 漂移 + 记账）。
    pub fn failure_cells(&self) -> u32 {
        self.stall + self.drift + self.book
    }

    /// 三张反面是否齐全（四根柱子都有反面证词）。
    pub fn faces(&self) -> bool {
        self.stall > 0 && self.drift > 0 && self.book > 0
    }

    /// 渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("release-retry corpus=");
        s.push_str(u32_text(self.total).as_str());
        s.push_str(" accept=");
        s.push_str(u32_text(self.accept).as_str());
        s.push_str(" reject=");
        s.push_str(u32_text(self.reject).as_str());
        s.push_str(" pass=");
        s.push_str(u32_text(self.pass).as_str());
        s.push_str(" stall=");
        s.push_str(u32_text(self.stall).as_str());
        s.push_str(" drift=");
        s.push_str(u32_text(self.drift).as_str());
        s.push_str(" book=");
        s.push_str(u32_text(self.book).as_str());
        s.push_str(" match=");
        s.push_str(u32_text(self.matched).as_str());
        s.push_str(" rate=");
        s.push_str(rate_text(self.rate()).as_str());
        s.push_str("ppm\n");
        s
    }
}

/// 全绿渲染的逐字钉版（基准机实录：本批 F11696 报表在全绿语料下的原文）。
const RENDER_OK: &str = "release-retry corpus=14 accept=8 reject=6 pass=3 stall=2 drift=2 book=1 match=14 rate=1000ppm\n";

/// 全跑。
pub fn run() -> Result<Report, u32> {
    run_with(None)
}

/// 可注入一槽缺陷的全跑。
///
/// 注入把"期望表"本身也放进被测面：强行宣称某槽犯了另一种缺陷，报表必须掉
/// matched、rate 随之离开 1000。若注入后仍报全绿，说明 matched 是硬编码的。
pub fn run_with(over: Option<(usize, Fault)>) -> Result<Report, u32> {
    if let Some((i, _)) = over {
        if i >= CORPUS {
            return Err(RR_ESLOT);
        }
    }
    let cs = cases();
    let mut total = 0u32;
    let mut accept = 0u32;
    let mut reject = 0u32;
    let mut pass = 0u32;
    let mut stall = 0u32;
    let mut drift = 0u32;
    let mut book = 0u32;
    let mut matched = 0u32;
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return Err(RR_ENOBS),
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
            None => return Err(RR_ENOBS),
        };
        total += 1;
        if f == want {
            matched += 1;
        }
        if f == Fault::Ok {
            accept += 1;
            match Tier::of(&a) {
                Tier::Pass => pass += 1,
                Tier::Stall => stall += 1,
                Tier::Drift => drift += 1,
                Tier::Book => book += 1,
            }
        } else {
            reject += 1;
        }
        i += 1;
    }
    Ok(Report {
        total,
        accept,
        reject,
        pass,
        stall,
        drift,
        book,
        matched,
    })
}

// ---------------------------------------------------------------------------
// 阶段闸门
// ---------------------------------------------------------------------------

/// 单调阶段闸门。
///
/// 观测纵线是 `arg → premise → release → retry → exact → book → done`，七段只能
/// 向前。回退（例如已经判死在释放闸，又声称走到了重试闸）说明申报者没有按闸门
/// 次序观测，而是事后从"第二刀失败了"倒推——本卡的头号风险恰好是这种倒推：
/// 看见第二次仍是 `No space left on device` 就补一个 retry 阶段、把"没腾地方"
/// 这层原因整段抹掉。
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
        Gate {
            last: -1,
            steps: 0,
            closed: false,
        }
    }

    /// 走一段。
    pub fn step(&mut self, s: Stage) -> Result<(), u32> {
        if self.closed {
            return Err(RR_ESTATE);
        }
        let o = s.order() as i32;
        if o < self.last {
            return Err(RR_ESTAGE);
        }
        self.last = o;
        self.steps = self.steps.saturating_add(1);
        Ok(())
    }

    /// 关闭，返回段数。
    pub fn close(&mut self) -> Result<u32, u32> {
        if self.closed {
            return Err(RR_ESTATE);
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    /// 槽位（必须严格递增）。
    pub slot: u8,
    /// 操作码。
    pub op: u8,
    /// 是否成功格。
    pub ok: bool,
    /// 错误码。
    pub code: u32,
    /// 观测值。
    pub value: u32,
}

impl Sample {
    /// 空白格。
    pub const fn blank() -> Sample {
        Sample {
            slot: 0,
            op: 0,
            ok: false,
            code: 0,
            value: 0,
        }
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

    /// 登记一格；槽位不连续、越界或已冻结一律拒绝并计数。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(RR_ESLOT);
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

    /// 逐格对账：槽位连续、成格值等于 `VALS`、败格码等于 `REJECT_CODES`、
    /// 败格值必归零。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != ACCEPT || self.err_count() != REJECT {
            return Err(RR_EBAL);
        }
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(RR_EBAL),
            };
            if s.slot as usize != i {
                return Err(RR_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(RR_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(RR_EVALUE);
                        }
                    }
                    None => return Err(RR_EBAL),
                }
                vi += 1;
            } else {
                if s.value != 0 {
                    return Err(RR_EVALUE);
                }
                match REJECT_CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(RR_EVALUE);
                        }
                    }
                    None => return Err(RR_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != ACCEPT || ci != REJECT {
            return Err(RR_EBAL);
        }
        Ok(())
    }

    /// 操作码逐格对账（本卡附加面）：每格归属的闸门必须与 `LEDGER_OPS` 一致。
    pub fn ops_match(&self) -> Result<(), u32> {
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(RR_EBAL),
            };
            match LEDGER_OPS.get(i) {
                Some(w) => {
                    if s.op != *w {
                        return Err(RR_ECOUNT);
                    }
                }
                None => return Err(RR_EBAL),
            }
            i += 1;
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
            return Err(RR_EBAL);
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
            Err(RR_ESEAL)
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
        Ok(v) => Sample {
            slot,
            op,
            ok: true,
            code: 0,
            value: v,
        },
        Err(c) => Sample {
            slot,
            op,
            ok: false,
            code: c,
            value: 0,
        },
    }
}

/// 建账本：7 格统计 + 1 格红档率（929）+ 6 格缺陷码。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let r = run();
    let _ = l.register(count_cell(0, OP_CORPUS, r.map(|x| x.total)));
    let _ = l.register(count_cell(1, OP_CORPUS, r.map(|x| x.accept)));
    let _ = l.register(count_cell(2, OP_CORPUS, r.map(|x| x.reject)));
    let _ = l.register(count_cell(3, OP_RELEASE, r.map(|x| x.pass)));
    let _ = l.register(count_cell(4, OP_RETRY, r.map(|x| x.stall)));
    let _ = l.register(count_cell(5, OP_EXACT, r.map(|x| x.drift)));
    let _ = l.register(count_cell(6, OP_BOOK, r.map(|x| x.book)));

    let red = run_with(Some((0, Fault::Code)));
    let _ = l.register(count_cell(7, OP_REPLAY, red.map(|x| x.rate())));

    let cs = cases();
    let mut i = 0usize;
    while i < REJECT {
        if let Some(c) = cs.get(i + ACCEPT) {
            let f = judge(c);
            let _ = l.register(Sample {
                slot: (ACCEPT + i) as u8,
                op: OP_BOOK,
                ok: false,
                code: f.code(),
                value: 0,
            });
        }
        i += 1;
    }
    l
}

/// 建一份**故意错账**：把"停摆档正确数"多记一格。
///
/// 它的用途只有一个：证明 `audit()` 不是摆设——`VALS` 是账本的判据，错一格
/// 必须在 `RR_EVALUE` 上露出来，而不是"账本自己说自己对"。
pub fn build_ledger_broken() -> Ledger {
    let mut l = Ledger::new();
    let r = run();
    let _ = l.register(count_cell(0, OP_CORPUS, r.map(|x| x.total)));
    let _ = l.register(count_cell(1, OP_CORPUS, r.map(|x| x.accept)));
    let _ = l.register(count_cell(2, OP_CORPUS, r.map(|x| x.reject)));
    let _ = l.register(count_cell(3, OP_RELEASE, r.map(|x| x.pass)));
    // 错账点：停摆档正确数 +1（2 记成 3）。
    let _ = l.register(count_cell(4, OP_RETRY, r.map(|x| x.stall.saturating_add(1))));
    let _ = l.register(count_cell(5, OP_EXACT, r.map(|x| x.drift)));
    let _ = l.register(count_cell(6, OP_BOOK, r.map(|x| x.book)));
    let red = run_with(Some((0, Fault::Code)));
    let _ = l.register(count_cell(7, OP_REPLAY, red.map(|x| x.rate())));

    let cs = cases();
    let mut i = 0usize;
    while i < REJECT {
        if let Some(c) = cs.get(i + ACCEPT) {
            let f = judge(c);
            let _ = l.register(Sample {
                slot: (ACCEPT + i) as u8,
                op: OP_BOOK,
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

/// 四路注入：表路、判路、渲染路、账本路。
///
/// 四路各自独立，任何一路静默退化都会在这里变成 false：
/// 1. 表路——段基址与联签、枚举、释放几何、形状合法域；
/// 2. 判路——七闸纵线、字节精确规则、对判、矩阵；
/// 3. 渲染路——语料、期望表、红档、文案、释义；
/// 4. 账本路——阶段闸门、逐格对账、错账检出、冻结重放。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    if let Some(w) = out.get_mut(0) {
        *w = name_ok() && anchor_ok() && enums_ok() && geom_ok() && shape_ok();
    }
    if let Some(w) = out.get_mut(1) {
        *w = define_ok() && exact_ok() && judge_ok() && matrix_ok();
    }
    if let Some(w) = out.get_mut(2) {
        *w = corpus_ok() && slots_ok() && reds_ok() && message_ok() && detail_ok();
    }
    if let Some(w) = out.get_mut(3) {
        *w = gate_ok()
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
    }
    out
}

// ---------------------------------------------------------------------------
// 判据基元
// ---------------------------------------------------------------------------

/// 名称与段基址：常量链、尺寸、errno 真值、码名与记账值名。
fn name_ok() -> bool {
    if SEG_BASE != 0x6d3f0 {
        return false;
    }
    if TAG_OK != SEG_BASE {
        return false;
    }
    if RR_ESTAGE != SEG_BASE + 0x01
        || RR_ECODE != SEG_BASE + 0x02
        || RR_EVALUE != SEG_BASE + 0x03
        || RR_ENOTE != SEG_BASE + 0x04
        || RR_EDEFAULT != SEG_BASE + 0x05
        || RR_EEXACT != SEG_BASE + 0x06
    {
        return false;
    }
    if RR_ESTATE != SEG_BASE + 0x07
        || RR_ECOUNT != SEG_BASE + 0x08
        || RR_ESLOT != SEG_BASE + 0x09
        || RR_ESEAL != SEG_BASE + 0x0a
        || RR_ENOBS != SEG_BASE + 0x0b
        || RR_EMATRIX != SEG_BASE + 0x0c
        || RR_EBAL != SEG_BASE + 0x0d
    {
        return false;
    }
    if CORPUS != 14 || SAMPLES != 14 || ACCEPT != 8 || REJECT != 6 || TIER_COUNT != 4 {
        return false;
    }
    if SHAPE_DIMS != 13 || ACCEPT + REJECT != CORPUS || SAMPLES != CORPUS {
        return false;
    }
    // 两个码必须是 Linux/glibc 真值，恢复域不许自造号码。
    if ERR_ENOSPC != 28 || ERR_EDQUOT != 122 || ERR_EINVAL != 22 || ERR_NONE != 0 {
        return false;
    }
    if ERRNO_SET_SIZE != 4 {
        return false;
    }
    if strerror(ERR_ENOSPC) != "No space left on device" {
        return false;
    }
    if strerror(ERR_EDQUOT) != "Quota exceeded" {
        return false;
    }
    if strerror(ERR_EINVAL) != "Invalid argument" || !strerror(ERR_NONE).is_empty() {
        return false;
    }
    if strerror(9999) != "Unknown error" {
        return false;
    }
    // 记账四值：0..3，互不相同，名字唯一。
    if BOOK_SET_SIZE != 4 || BOOK_NONE != 0 {
        return false;
    }
    if BOOK_RECOVER != 1 || BOOK_NEWFAIL != 2 || BOOK_DOUBLE != 3 {
        return false;
    }
    let books = [BOOK_NONE, BOOK_RECOVER, BOOK_NEWFAIL, BOOK_DOUBLE];
    let mut i = 0usize;
    while i < books.len() {
        let a = match books.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if book_name(a) == "unknown" || book_name(a).is_empty() {
            return false;
        }
        let mut j = i + 1;
        while j < books.len() {
            if let Some(b) = books.get(j) {
                if book_name(a) == book_name(*b) {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    // 码名：四码四个短名，未知码不落空。
    if code_name(0) != "none"
        || code_name(28) != "nospc"
        || code_name(122) != "dquot"
        || code_name(22) != "arg"
        || code_name(7) != "unknown"
    {
        return false;
    }
    // 操作码：八个，值唯一且都在 1..=8。
    if OP_ARG != 1 || OP_RELEASE != 2 || OP_TRUNC != 3 || OP_RETRY != 4 {
        return false;
    }
    if OP_EXACT != 5 || OP_BOOK != 6 || OP_CORPUS != 7 || OP_REPLAY != 8 {
        return false;
    }
    if PAYLOAD_MAX != 0x0100_0000 || FREE_MAX != 0x0040_0000 {
        return false;
    }
    // FNV 基元必须可复算（同输入同输出，异输入异输出）。
    let h1 = fold_str(FNV_OFF, "release");
    let h2 = fold_str(FNV_OFF, "release");
    let h3 = fold_str(FNV_OFF, "retry");
    if h1 != h2 || h1 == h3 || h1 == FNV_OFF {
        return false;
    }
    if fold_word(FNV_OFF, 0) == fold_word(FNV_OFF, 1) {
        return false;
    }
    // 空串也要折入长度：折完不许还等于偏移基（否则"没折"与"折了空串"分不清）。
    fold_str(FNV_OFF, "") != FNV_OFF
}

/// 联签：与前置卡 / 总锚 / 后继卡的段位差、以及文案族的结构事实。
fn anchor_ok() -> bool {
    if PREV_F11695_SEG != 0x6d3e0 || PREV_GAP != 0x10 {
        return false;
    }
    if ANCHOR_F11681_SEG != 0x6d130 || ANCHOR_GAP != 0x2c0 {
        return false;
    }
    if NEXT_F11697_SEG != 0x6d410 || NEXT_GAP != 0x20 {
        return false;
    }
    if ANCHOR_F11681_SEG >= PREV_F11695_SEG || PREV_F11695_SEG >= SEG_BASE {
        return false;
    }
    if SEG_BASE >= NEXT_F11697_SEG {
        return false;
    }
    if SEG_BASE - PREV_F11695_SEG != 0x10 {
        return false;
    }
    if SEG_BASE - ANCHOR_F11681_SEG != 0x2c0 {
        return false;
    }
    if NEXT_F11697_SEG - SEG_BASE != 0x20 {
        return false;
    }
    // 段内每个码都必须严格落在 (SEG_BASE, SEG_BASE + 0x0d] 里。
    let codes: [u32; 13] = [
        RR_ESTAGE,
        RR_ECODE,
        RR_EVALUE,
        RR_ENOTE,
        RR_EDEFAULT,
        RR_EEXACT,
        RR_ESTATE,
        RR_ECOUNT,
        RR_ESLOT,
        RR_ESEAL,
        RR_ENOBS,
        RR_EMATRIX,
        RR_EBAL,
    ];
    let mut i = 0usize;
    while i < codes.len() {
        let c = match codes.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if c <= TAG_OK || c > SEG_BASE + 0x0d {
            return false;
        }
        i += 1;
    }
    // 总锚供给的码值与文案在本卡必须一字不差（同一个 28，不是两个 28）。
    if strerror(ERR_ENOSPC) != "No space left on device" {
        return false;
    }
    // 前置卡的邻居码不许与本卡首刀码同号混写。
    if ERR_ENOSPC == ERR_EDQUOT {
        return false;
    }
    // 动作分族文案：dd / cp 只差前缀，尾段逐字相同。
    let a = msg_dd_write("p.bin");
    let b = msg_cp_write("p.bin");
    if !a.starts_with("dd: error writing 'p.bin': ") || !b.starts_with("cp: error writing 'p.bin': ")
    {
        return false;
    }
    if !a.ends_with(strerror(ERR_ENOSPC)) || !b.ends_with(strerror(ERR_ENOSPC)) {
        return false;
    }
    if a.as_str() == b.as_str() {
        return false;
    }
    // tee 体例不加引号，这是真实的体例差异（不是笔误）。
    let t = msg_tee_write("p.bin");
    if !t.starts_with("tee: ") || t.contains('\'') || !t.ends_with(strerror(ERR_ENOSPC)) {
        return false;
    }
    // truncate 体例带长度，且长度随参数变化。
    let u0 = msg_truncate("t.tmp", 0);
    let u1 = msg_truncate("t.tmp", 4096);
    if u0.as_str() == u1.as_str() {
        return false;
    }
    if !u0.ends_with(strerror(ERR_ENOSPC)) || !u0.contains(" at 0 bytes: ") {
        return false;
    }
    // 后继卡（dd conv=noerror）的名义动作在本卡不建模，只署名，不许出现在判定里。
    NEXT_GAP == 0x20 && PREV_GAP < NEXT_GAP && ANCHOR_GAP > NEXT_GAP
}

/// 枚举：次序、文本唯一性、四分类互斥完备。
fn enums_ok() -> bool {
    let stages = [
        Stage::Arg,
        Stage::Premise,
        Stage::Release,
        Stage::Retry,
        Stage::Exact,
        Stage::Book,
        Stage::Done,
    ];
    if stages.len() != 7 {
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
        // 末段是唯一放行点，其余都是某道闸的短路。
        if i + 1 == stages.len() {
            if s.failure() || s.structural() {
                return false;
            }
        } else if !s.failure() {
            return false;
        }
        // 空间侧只有 release(2) 与 retry(3) 两段。
        if s.space_side() != (i == 2 || i == 3) {
            return false;
        }
        // 结构/记账侧是 arg(0)/premise(1)/exact(4)/book(5)。
        if s.structural() != (i == 0 || i == 1 || i == 4 || i == 5) {
            return false;
        }
        if s.after_release() != (i >= 2) {
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
    // 次序即本卡的实况主张：前提先于释放，释放先于重试，精确先于记账。
    if Stage::Premise.order() >= Stage::Release.order() {
        return false;
    }
    if Stage::Release.order() >= Stage::Retry.order() {
        return false;
    }
    if Stage::Retry.order() >= Stage::Exact.order() {
        return false;
    }
    if Stage::Exact.order() >= Stage::Book.order() {
        return false;
    }
    if Stage::Book.order() + 1 != Stage::Done.order() {
        return false;
    }

    let notes = [
        Note::Recovered,
        Note::ArgBad,
        Note::WrongFirst,
        Note::NoRelease,
        Note::ShortFree,
        Note::PayloadDrift,
        Note::StillFull,
        Note::TruncatedLeft,
        Note::SizeShort,
        Note::Unproven,
        Note::BookedRetryFail,
        Note::BookedLoose,
        Note::BookedDouble,
    ];
    if notes.len() != 13 {
        return false;
    }
    let mut n = 0usize;
    let mut succ = 0usize;
    let mut stall = 0usize;
    let mut drift = 0usize;
    let mut book = 0usize;
    let mut space = 0usize;
    while n < notes.len() {
        let a = match notes.get(n) {
            Some(v) => *v,
            None => return false,
        };
        if a.name().is_empty() {
            return false;
        }
        // 四分类互斥且完备：成功 / 停摆 / 漂移 / 记账，一注解只属一类。
        if a.classes() != 1 {
            return false;
        }
        if a.success() {
            succ += 1;
        }
        if a.stalled() {
            stall += 1;
        }
        if a.drift_cause() {
            drift += 1;
        }
        if a.book_cause() {
            book += 1;
        }
        if a.space_cause() {
            space += 1;
        }
        // 恢复成立的注解不可能同时属任何失败面。
        if a.success() && (a.stalled() || a.drift_cause() || a.book_cause()) {
            return false;
        }
        // 精确面与记账面不许重叠（先证恢复、再查账，两件事不许混）。
        if a.drift_cause() && a.book_cause() {
            return false;
        }
        let mut m = n + 1;
        while m < notes.len() {
            if let Some(b) = notes.get(m) {
                if a.name() == b.name() {
                    return false;
                }
            }
            m += 1;
        }
        n += 1;
    }
    if succ != 1 || stall != 6 || drift != 3 || book != 3 {
        return false;
    }
    if space != 4 || succ + stall + drift + book != 13 {
        return false;
    }
    // 释放面两格必须是"没腾/腾不够"这一对，不许互相包含。
    if !Note::NoRelease.release_cause() || !Note::ShortFree.release_cause() {
        return false;
    }
    if Note::NoRelease == Note::ShortFree {
        return false;
    }
    // 前提面只有一格：EDQUOT 误判是首刀码的唯一破断形态。
    if !Note::WrongFirst.premise_cause() || Note::WrongFirst.release_cause() {
        return false;
    }
    // 记账面三格各指一件事：新失败 / 未入账 / 双计。
    if !Note::BookedRetryFail.book_cause()
        || !Note::BookedLoose.book_cause()
        || !Note::BookedDouble.book_cause()
    {
        return false;
    }
    if Note::BookedDouble.drift_cause() || Note::BookedLoose.success() {
        return false;
    }

    let tiers = Tier::all();
    if tiers.len() != TIER_COUNT {
        return false;
    }
    let mut t = 0usize;
    while t < tiers.len() {
        let x = match tiers.get(t) {
            Some(v) => *v,
            None => return false,
        };
        if x.index() != t {
            return false;
        }
        if x.success() != (t == 0) {
            return false;
        }
        if x.stall() != (t == 1) || x.drift() != (t == 2) || x.book() != (t == 3) {
            return false;
        }
        if x.failure_face() == x.success() {
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

    let faults = [
        Fault::Ok,
        Fault::Stage,
        Fault::Code,
        Fault::Value,
        Fault::Note,
        Fault::Default,
        Fault::Exact,
        Fault::Estate,
        Fault::Count,
        Fault::Slot,
        Fault::Seal,
        Fault::Nobs,
        Fault::Matrix,
        Fault::Bal,
    ];
    if faults.len() != 14 {
        return false;
    }
    let mut seen = [0u32; 14];
    let mut f = 0usize;
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
        && Fault::Stage.code() == RR_ESTAGE
        && Fault::Exact.code() == RR_EEXACT
        && Fault::Estate.code() == RR_ESTATE
        && Fault::Bal.code() == RR_EBAL
}

/// 释放几何与对账闸：四条几何全良，逐项破坏必判死，覆盖率必须复算得出。
fn geom_ok() -> bool {
    let gs = release_geoms();
    if gs.len() != 4 {
        return false;
    }
    let mut i = 0usize;
    while i < gs.len() {
        if let Some(g) = gs.get(i) {
            if !g.well_formed() {
                return false;
            }
            if !g.fit().well_formed() {
                return false;
            }
        }
        i += 1;
    }
    let filler = match gs.get(0) {
        Some(v) => *v,
        None => return false,
    };
    let topped = match gs.get(1) {
        Some(v) => *v,
        None => return false,
    };
    let splice = match gs.get(2) {
        Some(v) => *v,
        None => return false,
    };
    let short = match gs.get(3) {
        Some(v) => *v,
        None => return false,
    };
    // 槽 0：一字节余量都没有，全靠 filler 盖住。
    if filler.fit() != Fit::new(8192, 8192) || filler.fit().short_by() != 0 {
        return false;
    }
    if filler.fit().ratio_ppm() != 1_000_000 || !filler.fit().covers() {
        return false;
    }
    // 槽 1：1024 + 3072 恰好 4096，多一字节不多、少一字节不少。
    if topped.fit().have != 4096 || !topped.fit().covers() {
        return false;
    }
    // 槽 2：首刀已落 4096，仍需整份 12288 的余量（首刀写的字节不抵缺口）。
    if splice.first_written != 4096 || splice.fit().have != 12288 {
        return false;
    }
    if !splice.fit().covers() || splice.fit().short_by() != 0 {
        return false;
    }
    // 槽 3：只腾了一半，缺口 4096、覆盖率恰 500000ppm。
    if short.fit().covers() || short.fit().short_by() != 4096 {
        return false;
    }
    if short.fit().ratio_ppm() != 500_000 {
        return false;
    }
    // 逐项破坏：每坏一处都必须 well_formed() == false。
    let mut b = filler;
    b.tag = "";
    if b.well_formed() {
        return false;
    }
    b.tag = "probe";
    b.payload = 0;
    if b.well_formed() {
        return false;
    }
    b.payload = 8192;
    b.first_written = 8193;
    if b.well_formed() {
        return false;
    }
    b.first_written = 0;
    b.filler = FREE_MAX + 1;
    if b.well_formed() {
        return false;
    }
    b.filler = 8192;
    b.free_before = 8192;
    if b.well_formed() {
        // 首刀 ENOSPC 时可用量必须小于 payload：等于也不行。
        return false;
    }
    b.free_before = 8191;
    if !b.well_formed() {
        return false;
    }
    b.free_before = 0;
    if !b.well_formed() {
        return false;
    }
    // Fit 的退化分支：缺口为 0 不许除零、也不许自称良形。
    let zero = Fit::new(0, 0);
    if zero.ratio_ppm() != 0 || zero.well_formed() || !zero.covers() {
        return false;
    }
    if zero.short_by() != 0 {
        return false;
    }
    // Fit 的对账方向：have 多一字节即覆盖，少一字节即不覆盖。
    if !Fit::new(4096, 4096).covers() || Fit::new(4096, 4095).covers() {
        return false;
    }
    if Fit::new(4096, 4095).short_by() != 1 || Fit::new(4096, 8192).short_by() != 0 {
        return false;
    }
    // 指纹对几何字段敏感。
    let s1 = filler.seal();
    let mut c = filler;
    c.filler = 4096;
    let s2 = c.seal();
    let mut d = filler;
    d.first_written = 4096;
    let s3 = d.seal();
    let mut e = filler;
    e.free_before = 1024;
    e.filler = 7168;
    let s4 = e.seal();
    s1 != s2 && s1 != s3 && s1 != s4 && s2 != s3 && s3 != s4
}

/// 形状合法域：13 维任意一维坏掉都必须被 arg 闸抓住，且次序可复算。
fn shape_ok() -> bool {
    let mut w = Shape::blank();
    w.tag = "probe";
    if !w.valid() || !w.arg_legal() {
        return false;
    }
    if w.fit() != Fit::new(8192, 8192) || w.shortfall() != 0 {
        return false;
    }
    // payload == 0 → 非法，且 illegal_field 的第一顺位就是 payload。
    match w.with_payload(0).illegal_field() {
        Some((f, v)) => {
            if f != "payload" || v != 0 {
                return false;
            }
        }
        None => return false,
    }
    // payload 越上界 → 非法。
    match w.with_payload(PAYLOAD_MAX + 1).illegal_field() {
        Some((f, _)) => {
            if f != "payload" {
                return false;
            }
        }
        None => return false,
    }
    // 首刀写的字节超过 payload → 非法。
    match w.with_first_written(8193).illegal_field() {
        Some((f, v)) => {
            if f != "first_written" || v != 8193 {
                return false;
            }
        }
        None => return false,
    }
    // 释放量越上界 → 非法。
    match w.with_filler(FREE_MAX + 1).illegal_field() {
        Some((f, _)) => {
            if f != "filler" {
                return false;
            }
        }
        None => return false,
    }
    // 首刀之前可用量越上界 → 非法。
    match w.with_free_before(FREE_MAX + 1).illegal_field() {
        Some((f, _)) => {
            if f != "free_before" {
                return false;
            }
        }
        None => return false,
    }
    // 重试 payload 越上界 → 非法（它比 retry_size 先查）。
    match w.with_retry_payload(PAYLOAD_MAX + 1).illegal_field() {
        Some((f, _)) => {
            if f != "retry_payload" {
                return false;
            }
        }
        None => return false,
    }
    // 最终长度越上界 → 非法。
    match w.with_retry_size(PAYLOAD_MAX + 1).illegal_field() {
        Some((f, _)) => {
            if f != "retry_size" {
                return false;
            }
        }
        None => return false,
    }
    // 首刀码不在合法集 → 非法。
    match w.with_first_code(99).illegal_field() {
        Some((f, v)) => {
            if f != "first_code" || v != 99 {
                return false;
            }
        }
        None => return false,
    }
    // 重试码不在合法集 → 非法（首刀码合法时由第二顺位接住）。
    match w.with_retry_code(-7).illegal_field() {
        Some((f, _)) => {
            if f != "retry_code" {
                return false;
            }
        }
        None => return false,
    }
    // 记账值越四值集 → 非法。
    match w.with_booked(4).illegal_field() {
        Some((f, v)) => {
            if f != "booked" || v != 4 {
                return false;
            }
        }
        None => return false,
    }
    // 几何自洽：首刀报 ENOSPC 却"本来就有 8192 可用"→ 读数坏了，必须判死。
    let mut contra = w;
    contra.free_before = 8192;
    if contra.valid() {
        return false;
    }
    match contra.illegal_field() {
        Some((f, v)) => {
            if f != "free_before" || v != 8192 {
                return false;
            }
        }
        None => return false,
    }
    // 首刀 122 且首刀前可用量已经够数时，同一条"ENOSPC 必不足"的几何不再适用
    // （那是配额问题，不是读数问题）：把 payload 抬高一字节便是合法读数。
    let mut dquot = contra;
    dquot.first_code = ERR_EDQUOT;
    dquot.payload = 8193;
    if !dquot.valid() || !dquot.arg_legal() {
        return false;
    }
    // with_* 副本只改目标维。
    let mut base = Shape::blank();
    base.tag = "clone-src";
    base.path = "home/alice/vm.img";
    base.payload = 4096;
    base.free_before = 1024;
    base.filler = 3072;
    base.first_written = 512;
    base.retry_size = 4096;
    base.booked = BOOK_RECOVER;
    base.proven = true;
    let c1 = base.with_payload(8192);
    if c1.payload != 8192 || c1.filler != 3072 || c1.booked != BOOK_RECOVER {
        return false;
    }
    let c2 = base.with_free_before(0);
    if c2.free_before != 0 || c2.payload != 4096 {
        return false;
    }
    let c3 = base.with_filler(4096);
    if c3.filler != 4096 || c3.free_before != 1024 {
        return false;
    }
    let c4 = base.with_first_code(ERR_EDQUOT);
    if c4.first_code != ERR_EDQUOT || c4.retry_code != ERR_NONE {
        return false;
    }
    let c5 = base.with_first_written(0);
    if c5.first_written != 0 || c5.retry_size != 4096 {
        return false;
    }
    let c6 = base.with_freed(false);
    if c6.freed_before || c6.filler != 3072 {
        return false;
    }
    let c7 = base.with_retry_payload(4096);
    if c7.retry_payload != 4096 || c7.payload != 4096 {
        return false;
    }
    let c8 = base.with_retry_code(ERR_ENOSPC);
    if c8.retry_code != ERR_ENOSPC || c8.first_code != ERR_ENOSPC {
        return false;
    }
    let c9 = base.with_retry_size(0);
    if c9.retry_size != 0 || c9.first_written != 512 {
        return false;
    }
    let c10 = base.with_booked(BOOK_DOUBLE);
    if c10.booked != BOOK_DOUBLE || c10.proven != true {
        return false;
    }
    let c11 = base.with_proven(false);
    if c11.proven || c11.booked != BOOK_RECOVER {
        return false;
    }
    let c12 = base.with_path("x.bin");
    if c12.path != "x.bin" || c12.payload != 4096 {
        return false;
    }
    // 指纹对 13 维逐维敏感：改任意一维必换指纹。
    let s1 = base.seal();
    let mut m = base;
    m.tag = "clone-dst";
    let s2 = m.seal();
    let s3 = c12.seal();
    let s4 = c1.seal();
    let s5 = c2.seal();
    let s6 = c3.seal();
    let s7 = c4.seal();
    let s8 = c5.seal();
    let s9 = c6.seal();
    let s10 = c7.seal();
    let s11 = c8.seal();
    let s12 = c9.seal();
    let s13 = c10.seal();
    let s14 = c11.seal();
    if s1 == s2 || s1 == s3 || s1 == s4 || s1 == s5 || s1 == s6 || s1 == s7 {
        return false;
    }
    if s1 == s8 || s1 == s9 || s1 == s10 || s1 == s11 || s1 == s12 || s1 == s13 || s1 == s14 {
        return false;
    }
    if s2 == s3 || s3 == s4 || s4 == s5 {
        return false;
    }
    // 十三维里每一维都要在非法域里有用武之地：逐维置坏，判死率必须 13/13。
    let mut bad = 0usize;
    if !base.with_payload(0).valid() {
        bad += 1;
    }
    if !base.with_free_before(FREE_MAX + 1).valid() {
        bad += 1;
    }
    if !base.with_filler(FREE_MAX + 1).valid() {
        bad += 1;
    }
    if !base.with_first_code(99).valid() {
        bad += 1;
    }
    if !base.with_first_written(4097).valid() {
        bad += 1;
    }
    if !base.with_retry_payload(PAYLOAD_MAX + 1).valid() {
        bad += 1;
    }
    if !base.with_retry_code(99).valid() {
        bad += 1;
    }
    if !base.with_retry_size(PAYLOAD_MAX + 1).valid() {
        bad += 1;
    }
    if !base.with_booked(4).valid() {
        bad += 1;
    }
    // tag 维：空标签在释放几何里不成立。
    let mut empty = base;
    empty.tag = "";
    if !empty.valid() {
        bad += 1;
    }
    // path/freed/proven 三维不参与 arg，只参与判定：它们置坏后 arg 仍必须放行。
    if !base.with_path("other.bin").valid() {
        return false;
    }
    if !base.with_freed(false).valid() {
        return false;
    }
    if !base.with_proven(false).valid() {
        return false;
    }
    bad == 10
}

/// 子串出现次数（不建索引，按窗口比较）。
fn count_occur(hay: &str, needle: &str) -> usize {
    let h = hay.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() || h.len() < n.len() {
        return 0;
    }
    let mut c = 0usize;
    for w in h.windows(n.len()) {
        if w == n {
            c += 1;
        }
    }
    c
}

/// 定义纵线：七闸逐段推进，先短路的段不得被后段覆盖。
fn define_ok() -> bool {
    // 闸一：入参非法短路一切——首刀码再"像"ENOSPC 也轮不到前提闸。
    let mut bad = Shape::blank();
    bad.booked = 9;
    let o = define(&bad);
    if o.stage != Stage::Arg || o.code != ERR_EINVAL || o.note != Note::ArgBad {
        return false;
    }
    if o.ok() || o.default || Tier::of(&o) != Tier::Stall {
        return false;
    }
    let o2 = define(&bad.with_first_code(ERR_EDQUOT));
    if o2.stage != Stage::Arg || o2.note != Note::ArgBad {
        return false;
    }
    // 几何自相矛盾（首刀报 ENOSPC 却本来就有整份可用量）也落 arg。
    let o3 = define(&bad.with_booked(BOOK_RECOVER).with_free_before(8192));
    if o3.stage != Stage::Arg || o3.note != Note::ArgBad {
        return false;
    }

    // 闸二：前提破断——首刀 122 时"删件重试"整条链不成立。
    let dq = Shape::blank().with_first_code(ERR_EDQUOT);
    let o = define(&dq);
    if o.stage != Stage::Premise || o.code != ERR_EDQUOT || o.note != Note::WrongFirst {
        return false;
    }
    if o.value != 0 || o.default || Tier::of(&o) != Tier::Stall {
        return false;
    }
    // 首刀成功也算前提破断：没有失败就没有"恢复"这件事。
    let zero = define(&dq.with_first_code(ERR_NONE));
    if zero.stage != Stage::Premise || zero.code != ERR_NONE || zero.note != Note::WrongFirst {
        return false;
    }
    if zero.ok() {
        return false;
    }
    if define(&dq.with_first_code(ERR_EINVAL)).note != Note::WrongFirst {
        return false;
    }
    // 前提先于释放：既 122 又没腾，也只能落 premise。
    let both = dq.with_freed(false).with_retry_code(ERR_ENOSPC);
    if define(&both).stage != Stage::Premise {
        return false;
    }

    // 闸三：释放闸——先问"腾没腾"，再问"腾够没"。
    let nr = Shape::blank()
        .with_freed(false)
        .with_retry_code(ERR_ENOSPC)
        .with_retry_size(0);
    let o = define(&nr);
    if o.stage != Stage::Release || o.code != ERR_ENOSPC || o.value != 0 {
        return false;
    }
    if o.note != Note::NoRelease || o.default {
        return false;
    }
    // 首刀之前有 1024 余量时，没腾的第二刀仍写得进去 1024。
    let nr2 = nr.with_free_before(1024);
    if define(&nr2).value != 1024 || define(&nr2).stage != Stage::Release {
        return false;
    }
    // 腾到恰好的边界：多一字节不多、少一字节不少。
    let cov = Shape::blank();
    if !define(&cov).ok() {
        return false;
    }
    let sf = cov.with_filler(8191);
    let o = define(&sf);
    if o.stage != Stage::Release || o.note != Note::ShortFree || o.value != 8191 {
        return false;
    }
    if o.code != ERR_ENOSPC || o.default || Tier::of(&o) != Tier::Stall {
        return false;
    }
    // 余量 + filler 共同盖住缺口（1024 + 7168 == 8192）。
    let mix = cov.with_free_before(1024).with_filler(7168);
    if !define(&mix).ok() {
        return false;
    }
    if define(&mix.with_filler(7167)).note != Note::ShortFree {
        return false;
    }
    // 释放先于重试：没腾 + 换 payload，也只能落 release。
    if define(&nr.with_retry_payload(4096)).stage != Stage::Release {
        return false;
    }

    // 闸四：重试闸——先比 payload，再看第二刀成没成。
    let drift = Shape::blank()
        .with_retry_payload(4096)
        .with_retry_size(4096);
    let o = define(&drift);
    if o.stage != Stage::Retry || o.note != Note::PayloadDrift || o.value != 4096 {
        return false;
    }
    if o.code != ERR_NONE || o.default || Tier::of(&o) != Tier::Drift {
        return false;
    }
    // 换 payload 时第二刀即使失败，也先落 drift（payload 维先于退出码维）。
    if define(&drift.with_retry_code(ERR_ENOSPC)).note != Note::PayloadDrift {
        return false;
    }
    // 第二刀仍失败、文件留着首刀字节 → still-full。
    let still = Shape::blank()
        .with_first_written(4096)
        .with_retry_code(ERR_ENOSPC)
        .with_retry_size(4096);
    let o = define(&still);
    if o.stage != Stage::Retry || o.note != Note::StillFull || o.code != ERR_ENOSPC {
        return false;
    }
    if o.value != 4096 {
        return false;
    }
    // 首刀 0 字节、第二刀也 0 字节：0 >= 0，仍是 still-full，不许误判成截断。
    let still0 = Shape::blank()
        .with_retry_code(ERR_ENOSPC)
        .with_retry_size(0);
    if define(&still0).note != Note::StillFull {
        return false;
    }
    // 文件被截得比首刀还短 → truncated-left（数据丢失，比"没恢复"重一档）。
    let tr = still.with_retry_size(0);
    let o = define(&tr);
    if o.stage != Stage::Retry || o.note != Note::TruncatedLeft || o.value != 0 {
        return false;
    }
    if o.code != ERR_ENOSPC || !o.note.retry_cause() {
        return false;
    }
    // 重试先于精确：换 payload 且长度也不符，只报 drift。
    if define(&drift.with_retry_size(100)).note != Note::PayloadDrift {
        return false;
    }

    // 闸五：字节精确闸——写完了不等于写对了。
    let short = Shape::blank().with_retry_size(4096);
    let o = define(&short);
    if o.stage != Stage::Exact || o.note != Note::SizeShort || o.code != ERR_NONE {
        return false;
    }
    if o.value != 4096 {
        return false;
    }
    // 长出来也算不符（截断没做、或者多写了一截）。
    let long = Shape::blank().with_retry_size(16384);
    if define(&long).note != Note::SizeShort || define(&long).value != 16384 {
        return false;
    }
    // 长度精确但没比字节 → unproven。
    let un = Shape::blank().with_proven(false);
    let o = define(&un);
    if o.stage != Stage::Exact || o.note != Note::Unproven || o.value != 8192 {
        return false;
    }
    if o.ok() || o.default || Tier::of(&o) != Tier::Drift {
        return false;
    }
    // 精确先于记账：没比字节 + 记成新失败，只报 unproven。
    if define(&un.with_booked(BOOK_NEWFAIL)).note != Note::Unproven {
        return false;
    }

    // 闸六：记账闸——恢复成立之后才查账。
    let nf = Shape::blank().with_booked(BOOK_NEWFAIL);
    let o = define(&nf);
    if o.stage != Stage::Book || o.note != Note::BookedRetryFail || o.value != 8192 {
        return false;
    }
    if o.ok() || o.default || Tier::of(&o) != Tier::Book {
        return false;
    }
    let loose = Shape::blank().with_booked(BOOK_NONE);
    if define(&loose).note != Note::BookedLoose || define(&loose).stage != Stage::Book {
        return false;
    }
    let dbl = Shape::blank().with_booked(BOOK_DOUBLE);
    if define(&dbl).note != Note::BookedDouble || define(&dbl).stage != Stage::Book {
        return false;
    }

    // 闸七：全闸通过，落恢复对照档。
    let o = define(&Shape::blank());
    if !o.ok() || o.note != Note::Recovered || o.value != 8192 || !o.default {
        return false;
    }
    if Tier::of(&o) != Tier::Pass || !o.recovered() || o.failed() {
        return false;
    }
    if o.stage != Stage::Done || o.code != ERR_NONE {
        return false;
    }
    if !o.note.success() || o.note.classes() != 1 {
        return false;
    }

    // 语料逐槽：define 是唯一入口，三个复算函数必须与之一致。
    let cs = cases();
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let a = c.actual();
        if stage_of(&c.shape) != a.stage || code_of(&c.shape) != a.code {
            return false;
        }
        if value_of(&c.shape) != a.value {
            return false;
        }
        if a.ok() != (a.stage == Stage::Done && a.code == ERR_NONE && a.note == Note::Recovered) {
            return false;
        }
        // 恢复样本档位与"真恢复"严格同界：不许出现"失败却被记为样本"。
        if a.default != a.ok() {
            return false;
        }
        // 失败侧的最终字节不许超过 payload（截断/半截都只会更小或相等）。
        if a.failed() && a.value > c.shape.payload {
            return false;
        }
        i += 1;
    }
    // verdict 文本必须与档位一致。
    if verdict(&define(&nr)) != "no release" {
        return false;
    }
    if verdict(&define(&dq)) != "premise broken" {
        return false;
    }
    if verdict(&define(&tr)) != "truncated" {
        return false;
    }
    verdict(&define(&Shape::blank())) == "recovered"
}

/// 字节精确规则：同一 payload / 同一路径 / 同一最终长度 / 做过逐字节比较。
///
/// 四件套里"同一路径"在结构上就成立——两刀共用 `Shape::path` 一个字段，
/// 本卡不允许"首刀写 A、重试写 B"，`detail` 里路径只出现一次、`msg_cmp_same`
/// 也用它做两边的同一条。其余三件套由 `define` 逐条判。
fn exact_ok() -> bool {
    let cs = cases();
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let s = c.shape;
        let o = c.actual();
        // 恢复成立 ⟺ 九件事同时成立（本卡对"恢复"的完整定义）。
        let want = s.illegal_field().is_none()
            && s.first_code == ERR_ENOSPC
            && s.freed_before
            && s.fit().covers()
            && s.retry_payload == s.payload
            && s.retry_code == ERR_NONE
            && s.retry_size == s.payload
            && s.proven
            && s.booked == BOOK_RECOVER;
        if o.ok() != want {
            return false;
        }
        // 恢复成立的槽：账必须是 recover、必须比过字节、长度必须精确。
        if o.ok()
            && (s.booked != BOOK_RECOVER || !s.proven || s.retry_size != s.payload)
        {
            return false;
        }
        i += 1;
    }
    // 同一 payload：换 payload 一律不算恢复。
    let base = Shape::blank();
    if !define(&base.with_retry_payload(8192)).ok() {
        return false;
    }
    if define(&base.with_retry_payload(8191)).note != Note::PayloadDrift {
        return false;
    }
    if define(&base.with_retry_payload(16384).with_retry_size(16384)).note
        != Note::PayloadDrift
    {
        return false;
    }
    // 同一最终长度：短一字节、长一字节都不算恢复。
    if define(&base.with_retry_size(8191)).note != Note::SizeShort {
        return false;
    }
    if define(&base.with_retry_size(8193)).note != Note::SizeShort {
        return false;
    }
    // 做过逐字节比较：没比过就不许叫恢复。
    if define(&base.with_proven(false)).note != Note::Unproven {
        return false;
    }
    // 同一路径：路径是两刀共用的一个字段——改路径不改判定，但改指纹。
    let pa = base.with_path("srv/data/a.bin");
    let pb = base.with_path("srv/data/b.bin");
    if !define(&pa).ok() || !define(&pb).ok() {
        return false;
    }
    if pa.seal() == pb.seal() {
        return false;
    }
    let ca = Case {
        shape: pa,
        claim: Claim {
            stage: Stage::Done,
            code: 0,
            value: 8192,
            note: Note::Recovered,
            default: true,
        },
    };
    if count_occur(&detail(&ca), "srv/data/a.bin") != 1 {
        return false;
    }
    // 字节比较的现场文案：两边同一条路径，逐字稳定。
    if msg_cmp_same("a.bin", "a.bin").as_str() != "a.bin and a.bin are identical" {
        return false;
    }
    if count_occur(&msg_cmp_same("a.bin", "a.bin"), "a.bin") != 2 {
        return false;
    }
    // 首刀部分写不影响"字节精确"：前半截来自首刀，比较仍要逐字节做。
    let partial = Shape::blank()
        .with_payload(12288)
        .with_filler(12288)
        .with_first_written(4096)
        .with_retry_payload(12288)
        .with_retry_size(12288);
    if !define(&partial).ok() {
        return false;
    }
    if define(&partial.with_proven(false)).note != Note::Unproven {
        return false;
    }
    // 首刀部分写超过 payload 是坏几何，不许带着它判"精确"。
    if partial.with_first_written(12289).valid() {
        return false;
    }
    // 缺口还差一字节时谈不上精确：release 闸先短路，不会走到 exact。
    let shortfit = Shape::blank()
        .with_filler(8191)
        .with_proven(false)
        .with_retry_size(4096);
    if define(&shortfit).stage != Stage::Release {
        return false;
    }
    // 截断线落在 payload 中间时，cmp 的 EOF 字节数就是最终长度。
    let eof = msg_cmp_eof("srv/data/archive.bin", 4096);
    if eof.as_str() != "cmp: EOF on srv/data/archive.bin after byte 4096, in line 1" {
        return false;
    }
    if !eof.ends_with(", in line 1") {
        return false;
    }
    true
}

/// 申报对判：六维次序 Estate → Stage → Code → Value → Note → Default，
/// 每一维单独破坏都必须被**那一维**抓住，两维同时坏时按次序让位。
fn judge_ok() -> bool {
    let cs = cases();
    // 八活槽逐维等于现场。
    let mut i = 0usize;
    while i < ACCEPT {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if judge(&c) != Fault::Ok || !c.ok() {
            return false;
        }
        let a = c.actual();
        let k = c.claim;
        if a.stage != k.stage || a.code != k.code || a.value != k.value {
            return false;
        }
        if a.note != k.note || a.default != k.default {
            return false;
        }
        i += 1;
    }
    // 六死槽各被期望的那一维抓住。
    let mut r = 0usize;
    while r < REJECT {
        let c = match cs.get(r + ACCEPT) {
            Some(v) => *v,
            None => return false,
        };
        let want = match REJECT_FAULTS.get(r) {
            Some(v) => *v,
            None => return false,
        };
        if judge(&c) != want || c.ok() {
            return false;
        }
        if expected(r + ACCEPT) != Some(want) {
            return false;
        }
        r += 1;
    }
    // Estate 优先：拿复刻源（没腾地方）造一条"其余五维全按成功口径"的申报，
    // 只有 Estate 维能让它致命；若次序错了会先报 Stage。
    let base3 = match cs.get(REJECT_SOURCE) {
        Some(v) => *v,
        None => return false,
    };
    let allwrong = Case {
        shape: base3.shape,
        claim: Claim {
            stage: Stage::Done,
            code: 0,
            value: 8192,
            note: Note::Recovered,
            default: true,
        },
    };
    if judge(&allwrong) != Fault::Estate {
        return false;
    }
    // Stage 优先于 Code：把释放闸现场报成重试闸**并且**报码 122。
    let stage_and_code = Case {
        shape: base3.shape,
        claim: Claim {
            stage: Stage::Retry,
            code: 122,
            value: 0,
            note: Note::NoRelease,
            default: false,
        },
    };
    if judge(&stage_and_code) != Fault::Stage {
        return false;
    }
    // Code 优先于 Value：码错 + 字节错同现时只报 Code。
    let code_and_value = Case {
        shape: base3.shape,
        claim: Claim {
            stage: Stage::Release,
            code: 122,
            value: 4096,
            note: Note::NoRelease,
            default: false,
        },
    };
    if judge(&code_and_value) != Fault::Code {
        return false;
    }
    // Value 优先于 Note：字节与注解双错时先报 Value。
    let value_and_note = Case {
        shape: base3.shape,
        claim: Claim {
            stage: Stage::Release,
            code: 28,
            value: 4096,
            note: Note::ShortFree,
            default: false,
        },
    };
    if judge(&value_and_note) != Fault::Value {
        return false;
    }
    // Note 优先于 Default：注解与档位双错时先报 Note。
    let note_and_default = Case {
        shape: base3.shape,
        claim: Claim {
            stage: Stage::Release,
            code: 28,
            value: 0,
            note: Note::ShortFree,
            default: true,
        },
    };
    if judge(&note_and_default) != Fault::Note {
        return false;
    }
    // 反向 Estate：把恢复成立报成"没腾地方"。
    let base0 = match cs.first() {
        Some(v) => *v,
        None => return false,
    };
    let pass_claimed_dead = Case {
        shape: base0.shape,
        claim: Claim {
            stage: Stage::Release,
            code: 28,
            value: 0,
            note: Note::NoRelease,
            default: false,
        },
    };
    if judge(&pass_claimed_dead) != Fault::Estate {
        return false;
    }
    // 期望表边界与取槽函数。
    if expected(0) != Some(Fault::Ok) || expected(ACCEPT - 1) != Some(Fault::Ok) {
        return false;
    }
    if expected(CORPUS).is_some() {
        return false;
    }
    if case_at(0).is_none() || case_at(CORPUS - 1).is_none() {
        return false;
    }
    if case_at(CORPUS).is_some() {
        return false;
    }
    if cover(CORPUS) != Ok(()) || cover(CORPUS - 1) != Err(RR_ENOBS) {
        return false;
    }
    // 六缺陷码与缺陷枚举一一对应且不落零。
    let mut z = 0usize;
    while z < REJECT {
        let f = match REJECT_FAULTS.get(z) {
            Some(v) => *v,
            None => return false,
        };
        let w = match REJECT_CODES.get(z) {
            Some(v) => *v,
            None => return false,
        };
        if f.code() != w || w == 0 || !f.alive() {
            return false;
        }
        if fault_short(f).is_empty() {
            return false;
        }
        z += 1;
    }
    // Case::actual 必须与 define 同一入口（复算一致）。
    let c0 = match cs.first() {
        Some(v) => *v,
        None => return false,
    };
    c0.actual() == define(&c0.shape) && c0.tag() == "recover-filler"
}

// ==== CHUNK J START ====

/// 统计矩阵与汇总报表：四档行数、全对档数、覆盖平、逐字渲染、注入掉 matched。
fn matrix_ok() -> bool {
    let m = Matrix::of();
    if m.slots() != CORPUS || m.goods() != ACCEPT {
        return false;
    }
    // 四档行：恢复 3/3、停摆 8/2（含 6 个复刻槽）、漂移 2/2、记账 1/1。
    if m.row(Tier::Pass) != Some(Row { total: 3, good: 3 }) {
        return false;
    }
    if m.row(Tier::Stall) != Some(Row { total: 8, good: 2 }) {
        return false;
    }
    if m.row(Tier::Drift) != Some(Row { total: 2, good: 2 }) {
        return false;
    }
    if m.row(Tier::Book) != Some(Row { total: 1, good: 1 }) {
        return false;
    }
    if m.perfect_rows() != 3 || !m.covered() || !m.faces_covered() {
        return false;
    }
    // 停摆档的 6 个红正是复刻槽：既不完美，也不许把它算成"没覆盖"。
    if let Some(r) = m.row(Tier::Stall) {
        if r.red() != REJECT || r.perfect() || r.total != ACCEPT {
            return false;
        }
    } else {
        return false;
    }
    // 行渲染逐字：red 必须是复算量，不是抄来的。
    let want_rows = "pass total=3 good=3 red=0\n\
stall total=8 good=2 red=6\n\
drift total=2 good=2 red=0\n\
book total=1 good=1 red=0\n";
    if m.render() != want_rows {
        return false;
    }
    // 畸形行：good > total 时红档钳回 total，不下溢成天文数字。
    let overflow = Row {
        total: 2,
        good: 5,
    };
    if overflow.red() != 2 || overflow.perfect() {
        return false;
    }
    if Row::blank().perfect() || Row::blank().red() != 0 {
        return false;
    }
    // 空矩阵：无槽、无全对档、无反面。
    let empty = Matrix::blank();
    if empty.slots() != 0 || empty.goods() != 0 || empty.perfect_rows() != 0 {
        return false;
    }
    if empty.covered() || empty.faces_covered() {
        return false;
    }

    // 汇总报表：与矩阵同一份语料的两种投影，逐格必须一致。
    let rep = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if rep.total != CORPUS as u32 || rep.accept != ACCEPT as u32 || rep.reject != REJECT as u32 {
        return false;
    }
    if rep.pass != 3 || rep.stall != 2 || rep.drift != 2 || rep.book != 1 {
        return false;
    }
    if rep.matched != CORPUS as u32 || rep.rate() != 1000 || !rep.green() {
        return false;
    }
    if rep.gate() != Ok(()) || !rep.faces() {
        return false;
    }
    if rep.recovered_cells() != 3 || rep.failure_cells() != 5 {
        return false;
    }
    if rep.render() != RENDER_OK {
        return false;
    }
    // 报表三分（成/败/命中）与矩阵两维（总/对）必须相容。
    if rep.accept + rep.reject != rep.total {
        return false;
    }
    if rep.pass + rep.stall + rep.drift + rep.book != rep.accept {
        return false;
    }
    if rep.pass as usize != m.goods() - (rep.stall as usize + rep.drift as usize + rep.book as usize) {
        return false;
    }

    // 千分率四舍五入：空表归零，半格进位，全中千位。
    let zero = Report {
        total: 0,
        accept: 0,
        reject: 0,
        pass: 0,
        stall: 0,
        drift: 0,
        book: 0,
        matched: 0,
    };
    if zero.rate() != 0 || zero.green() || zero.gate() != Err(RR_ECOUNT) {
        return false;
    }
    let half = Report {
        total: 2,
        accept: 1,
        reject: 1,
        pass: 1,
        stall: 0,
        drift: 0,
        book: 0,
        matched: 1,
    };
    if half.rate() != 500 || half.green() {
        return false;
    }
    let third = Report {
        total: 3,
        accept: 1,
        reject: 2,
        pass: 1,
        stall: 0,
        drift: 0,
        book: 0,
        matched: 1,
    };
    if third.rate() != 333 {
        return false;
    }

    // 越界注入：先被槽位闸挡下，绝不悄悄忽略。
    match run_with(Some((CORPUS, Fault::Code))) {
        Ok(_) => return false,
        Err(c) => {
            if c != RR_ESLOT {
                return false;
            }
        }
    }
    match run_with(Some((CORPUS + 1, Fault::Ok))) {
        Ok(_) => return false,
        Err(c) => {
            if c != RR_ESLOT {
                return false;
            }
        }
    }
    // 槽内注入（Exact 不属于任何一槽的期望缺陷）：每注入一格必掉一格命中，
    // 红档率随之从 1000 掉到 929。任何一格注入后仍全绿，说明 matched 是硬编码。
    let mut i = 0usize;
    while i < CORPUS {
        let r = match run_with(Some((i, Fault::Exact))) {
            Ok(v) => v,
            Err(_) => return false,
        };
        if r.matched != (CORPUS as u32 - 1) || r.rate() != 929 || r.green() {
            return false;
        }
        if r.gate() != Err(RR_ECOUNT) {
            return false;
        }
        i += 1;
    }
    // 在"本来就对"的槽上注入它自己的期望（Ok）：不该产生红。
    match run_with(Some((3, Fault::Ok))) {
        Ok(v) => {
            if v.matched != CORPUS as u32 || !v.green() {
                return false;
            }
        }
        Err(_) => return false,
    }
    match run_with(None) {
        Ok(v) => v.matched == CORPUS as u32 && v.rate() == 1000,
        Err(_) => false,
    }
}

/// 语料：14 槽标签、期望表五列、档位投影、失败克隆的"只错一维"。
fn corpus_ok() -> bool {
    let cs = cases();
    if cs.len() != CORPUS || SHAPES.len() != CORPUS || CLAIMS.len() != CORPUS {
        return false;
    }
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        // 标签（第三份陈述的第一列）。
        let tag = match WANT_TAGS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if c.tag() != tag || tag.is_empty() {
            return false;
        }
        // 形状合法：13 维全部在位且几何自洽（首刀既报 ENOSPC，则 free_before < payload）。
        if !c.shape.valid() || !c.shape.arg_legal() {
            return false;
        }
        if c.shape.path.is_empty() || c.shape.payload == 0 {
            return false;
        }
        if c.shape.first_code == ERR_ENOSPC && c.shape.free_before >= c.shape.payload {
            return false;
        }
        // 期望表五列逐槽对撞。
        let a = c.actual();
        let ws = match WANT_STAGE.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let wc = match WANT_CODE.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let wv = match WANT_VALUE.get(i) {
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
        let wt = match WANT_TIER.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if a.stage != ws || a.code != wc || a.value != wv || a.note != wn {
            return false;
        }
        if a.default != wd || Tier::of(&a) != wt {
            return false;
        }
        // 单入口复算：Case::actual / stage_of / code_of / value_of 不许各走一条路。
        if define(&c.shape) != a {
            return false;
        }
        if stage_of(&c.shape) != a.stage || code_of(&c.shape) != a.code {
            return false;
        }
        if value_of(&c.shape) != a.value || c.judge() != judge(&c) {
            return false;
        }
        if c.ok() != (c.judge() == Fault::Ok) {
            return false;
        }
        // 成败与注解必须互相解释：ok() 当且仅当 Done/0/Recovered。
        if a.ok() != (a.stage == Stage::Done && a.code == ERR_NONE && a.note == Note::Recovered) {
            return false;
        }
        if a.recovered() != (a.ok() && wd) {
            return false;
        }
        if a.family() != code_name(a.code) {
            return false;
        }
        i += 1;
    }

    // 前 8 槽全绿；后 6 槽各中自己那一维的缺陷，缺陷码与期望码一一对应。
    let mut k = 0usize;
    while k < ACCEPT {
        let c = match cs.get(k) {
            Some(v) => *v,
            None => return false,
        };
        if c.judge() != Fault::Ok || !c.ok() {
            return false;
        }
        // 申报与现场六维逐维相同（对判为 Ok 的展开写法，防止 judge 被写松）。
        if c.claim.stage != c.actual().stage || c.claim.code != c.actual().code {
            return false;
        }
        if c.claim.value != c.actual().value || c.claim.note != c.actual().note {
            return false;
        }
        if c.claim.default != c.actual().default {
            return false;
        }
        // 前 3 槽是恢复成立：码归零、长度等于 payload、注解 recovered、档位为真。
        // 后 5 槽是"失败申报正确"：注解属失败面、档位为假，长度是现场留下的字节。
        if c.actual().ok() {
            if c.actual().code != ERR_NONE || c.actual().value != c.shape.payload {
                return false;
            }
            if c.actual().note != Note::Recovered || !c.actual().default {
                return false;
            }
            if c.claim.stage != Stage::Done || !c.claim.success() {
                return false;
            }
        } else {
            if c.claim.success() || c.claim.default || c.actual().default {
                return false;
            }
            if c.actual().value != c.shape.retry_size {
                return false;
            }
        }
        k += 1;
    }
    let mut r = ACCEPT;
    while r < CORPUS {
        let c = match cs.get(r) {
            Some(v) => *v,
            None => return false,
        };
        let want = match REJECT_FAULTS.get(r - ACCEPT) {
            Some(v) => *v,
            None => return false,
        };
        let code = match REJECT_CODES.get(r - ACCEPT) {
            Some(v) => *v,
            None => return false,
        };
        if c.judge() != want || want == Fault::Ok || want.code() != code {
            return false;
        }
        if c.ok() {
            return false;
        }
        if expected(r) != Some(want) {
            return false;
        }
        r += 1;
    }

    // 六死槽必须是"只换标签"的复刻：12 个非标签维与复刻源逐维相同。
    let src = match cs.get(REJECT_SOURCE) {
        Some(v) => *v,
        None => return false,
    };
    if REJECT_SOURCE >= ACCEPT || src.tag() != "norelease-enospc" {
        return false;
    }
    let mut z = ACCEPT;
    while z < CORPUS {
        let c = match cs.get(z) {
            Some(v) => *v,
            None => return false,
        };
        if c.shape.path != src.shape.path
            || c.shape.payload != src.shape.payload
            || c.shape.free_before != src.shape.free_before
            || c.shape.filler != src.shape.filler
            || c.shape.first_code != src.shape.first_code
            || c.shape.first_written != src.shape.first_written
            || c.shape.freed_before != src.shape.freed_before
            || c.shape.retry_payload != src.shape.retry_payload
            || c.shape.retry_code != src.shape.retry_code
            || c.shape.retry_size != src.shape.retry_size
            || c.shape.booked != src.shape.booked
            || c.shape.proven != src.shape.proven
        {
            return false;
        }
        // 唯一不同的那一维是标签，指纹因此必须不同（形状指纹把 tag 折进去了）。
        if c.tag() == src.tag() || c.shape.seal() == src.shape.seal() {
            return false;
        }
        z += 1;
    }

    // 14 槽两两不重名、指纹不撞。
    let mut p = 0usize;
    while p < CORPUS {
        let a = match cs.get(p) {
            Some(v) => *v,
            None => return false,
        };
        let mut q = p + 1;
        while q < CORPUS {
            let b = match cs.get(q) {
                Some(v) => *v,
                None => return false,
            };
            if a.tag() == b.tag() || a.shape.seal() == b.shape.seal() {
                return false;
            }
            q += 1;
        }
        p += 1;
    }

    // 三个恢复样本各自证明一种"够"。
    let c0 = match cs.get(0) {
        Some(v) => *v,
        None => return false,
    };
    let c1 = match cs.get(1) {
        Some(v) => *v,
        None => return false,
    };
    let c2 = match cs.get(2) {
        Some(v) => *v,
        None => return false,
    };
    if c0.shape.filler != c0.shape.payload || c1.shape.filler == 0 {
        return false;
    }
    if c1.shape.free_before.saturating_add(c1.shape.filler) != c1.shape.payload {
        return false;
    }
    if c2.shape.first_written == 0 || c2.shape.first_written >= c2.shape.payload {
        return false;
    }
    if c2.shape.retry_size != c2.shape.payload || c2.shape.fit().short_by() != 0 {
        return false;
    }
    // 停摆两例：没腾与腾不够，结果字节必须分别是"首刀前余量"与"腾出来的总量"。
    let c3 = match cs.get(3) {
        Some(v) => *v,
        None => return false,
    };
    let c4 = match cs.get(4) {
        Some(v) => *v,
        None => return false,
    };
    if c3.shape.freed_before || c3.actual().value != c3.shape.free_before {
        return false;
    }
    // 没有 freed_before 这一维，filler 摆在那里也进不了账：结果仍是"没腾"。
    if !c3.shape.fit().covers() || c3.actual().note != Note::NoRelease {
        return false;
    }
    if !c4.shape.freed_before || c4.shape.fit().covers() {
        return false;
    }
    if c4.actual().value != c4.shape.fit().have || c4.shape.shortfall() == 0 {
        return false;
    }
    // 漂移、未证、双计各占一道闸，且都不落恢复档。
    let c5 = match cs.get(5) {
        Some(v) => *v,
        None => return false,
    };
    let c6 = match cs.get(6) {
        Some(v) => *v,
        None => return false,
    };
    let c7 = match cs.get(7) {
        Some(v) => *v,
        None => return false,
    };
    if c5.shape.retry_payload == c5.shape.payload || c5.actual().stage != Stage::Retry {
        return false;
    }
    if c5.actual().value != c5.shape.retry_size || c5.actual().code != ERR_NONE {
        return false;
    }
    if c6.shape.proven || c6.actual().stage != Stage::Exact || c6.actual().code != ERR_NONE {
        return false;
    }
    if c7.shape.booked != BOOK_DOUBLE || c7.actual().stage != Stage::Book {
        return false;
    }
    if c7.actual().value != c7.shape.payload || c7.actual().default {
        return false;
    }

    // 档位覆盖：放行 3、停摆 2、漂移 2、记账 1 都在申报正确的 8 槽里。
    let mut pass = 0usize;
    let mut stall = 0usize;
    let mut drift = 0usize;
    let mut book = 0usize;
    let mut n = 0usize;
    while n < ACCEPT {
        let c = match cs.get(n) {
            Some(v) => *v,
            None => return false,
        };
        match Tier::of(&c.actual()) {
            Tier::Pass => pass += 1,
            Tier::Stall => stall += 1,
            Tier::Drift => drift += 1,
            Tier::Book => book += 1,
        }
        n += 1;
    }
    if pass != 3 || stall != 2 || drift != 2 || book != 1 {
        return false;
    }
    if pass + stall + drift + book != ACCEPT {
        return false;
    }
    // 覆盖面闸与取槽边界。
    cover(CORPUS) == Ok(())
        && cover(CORPUS - 1) == Err(RR_ENOBS)
        && expected(CORPUS).is_none()
        && case_at(CORPUS).is_none()
        && case_at(0) == cs.first().copied()
}

/// 槽位表、账值表、操作码表、缺陷码链：14 格的第二份陈述。
fn slots_ok() -> bool {
    // 8 个成功格的值：语料三数 + 四档 + 红档率。
    if VALS.len() != ACCEPT || LEDGER_OPS.len() != SAMPLES {
        return false;
    }
    let rep = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let want = [
        rep.total,
        rep.accept,
        rep.reject,
        rep.pass,
        rep.stall,
        rep.drift,
        rep.book,
        0u32,
    ];
    let mut i = 0usize;
    while i < want.len() {
        let w = match want.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let g = match VALS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        // 末格专门留给红档率，不参与前七格的同一性。
        if i + 1 < ACCEPT && g != w {
            return false;
        }
        i += 1;
    }
    // 前七格就是报表七列，逐格同值（不是"拼出来像"）。
    if VALS.first() != Some(&rep.total) || VALS.get(1) != Some(&rep.accept) {
        return false;
    }
    if VALS.get(2) != Some(&rep.reject) || VALS.get(3) != Some(&rep.pass) {
        return false;
    }
    if VALS.get(4) != Some(&rep.stall) || VALS.get(5) != Some(&rep.drift) {
        return false;
    }
    // 第 8 格恒为红档率 929，且必须由"注入一格"复算，不许硬编码。
    if VALS.get(ACCEPT - 1) != Some(&929u32) {
        return false;
    }
    match run_with(Some((0, Fault::Code))) {
        Ok(v) => {
            if VALS.get(ACCEPT - 1) != Some(&v.rate()) || v.rate() != 929 {
                return false;
            }
        }
        Err(_) => return false,
    }
    // 语料三数自洽：总数 = 成 + 败，四档 = 成。
    let t = match VALS.first() {
        Some(v) => *v,
        None => return false,
    };
    let a = match VALS.get(1) {
        Some(v) => *v,
        None => return false,
    };
    let r = match VALS.get(2) {
        Some(v) => *v,
        None => return false,
    };
    if t != CORPUS as u32 || a != ACCEPT as u32 || r != REJECT as u32 || a + r != t {
        return false;
    }
    let mut tier_sum = 0u32;
    let mut j = 3usize;
    while j < ACCEPT - 1 {
        match VALS.get(j) {
            Some(v) => tier_sum = tier_sum.saturating_add(*v),
            None => return false,
        }
        j += 1;
    }
    if tier_sum != a {
        return false;
    }
    // 9 个 0 值之外，前七格互不全等（否则这一表能用一个常数糊过去）。
    let mut same = 0usize;
    let mut p = 0usize;
    while p < ACCEPT - 1 {
        if VALS.get(p) == VALS.get(0) {
            same += 1;
        }
        p += 1;
    }
    if same != 1 {
        return false;
    }

    // 操作码逐格：前 3 格语料、1 格释放、1 格重试、1 格精确、1 格记账、1 格重放、
    // 末 6 格全是记账（缺陷码）。
    let ops = [
        OP_CORPUS,
        OP_CORPUS,
        OP_CORPUS,
        OP_RELEASE,
        OP_RETRY,
        OP_EXACT,
        OP_BOOK,
        OP_REPLAY,
    ];
    let mut o = 0usize;
    while o < ops.len() {
        if LEDGER_OPS.get(o) != ops.get(o) {
            return false;
        }
        o += 1;
    }
    let mut q = ACCEPT;
    while q < SAMPLES {
        if LEDGER_OPS.get(q) != Some(&OP_BOOK) {
            return false;
        }
        q += 1;
    }

    // 缺陷码链：Estate → Stage → Code → Value → Note → Default，与枚举同序。
    let chain = [
        RR_ESTATE,
        RR_ESTAGE,
        RR_ECODE,
        RR_EVALUE,
        RR_ENOTE,
        RR_EDEFAULT,
    ];
    if chain.len() != REJECT {
        return false;
    }
    let mut c = 0usize;
    while c < REJECT {
        let w = match chain.get(c) {
            Some(v) => *v,
            None => return false,
        };
        let f = match REJECT_FAULTS.get(c) {
            Some(v) => *v,
            None => return false,
        };
        if REJECT_CODES.get(c) != Some(&w) || w == 0 {
            return false;
        }
        if f == Fault::Ok || f.code() != w {
            return false;
        }
        c += 1;
    }
    // 13 个段内码：连续、互不相同、都在 SEG_BASE+1..=SEG_BASE+0x0d。
    let codes = [
        RR_ESTAGE, RR_ECODE, RR_EVALUE, RR_ENOTE, RR_EDEFAULT, RR_EEXACT, RR_ESTATE, RR_ECOUNT,
        RR_ESLOT, RR_ESEAL, RR_ENOBS, RR_EMATRIX, RR_EBAL,
    ];
    if codes.len() != 13 {
        return false;
    }
    let mut x = 0usize;
    while x < codes.len() {
        let v = match codes.get(x) {
            Some(v) => *v,
            None => return false,
        };
        if v != SEG_BASE + (x as u32) + 1 || v == 0 {
            return false;
        }
        let mut y = x + 1;
        while y < codes.len() {
            if codes.get(y) == Some(&v) {
                return false;
            }
            y += 1;
        }
        x += 1;
    }
    // 接受/拒绝槽表互为补集。
    if ACCEPT_SLOTS.len() != ACCEPT || REJECT_SLOTS.len() != REJECT {
        return false;
    }
    let mut s = 0usize;
    while s < ACCEPT {
        if ACCEPT_SLOTS.get(s) != Some(&s) {
            return false;
        }
        s += 1;
    }
    let mut s = 0usize;
    while s < REJECT {
        if REJECT_SLOTS.get(s) != Some(&(ACCEPT + s)) {
            return false;
        }
        s += 1;
    }
    REJECT_SOURCE < ACCEPT && REJECT_SLOTS.get(REJECT - 1) == Some(&(CORPUS - 1))
}

/// 红档：全绿语料零红，逐槽注入各一红，六死槽的码各占一格。
fn reds_ok() -> bool {
    let cs = cases();
    // 归零：期望表与判路逐槽一致时红数为 0。
    let mut red = 0usize;
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let w = match expected(i) {
            Some(v) => v,
            None => return false,
        };
        if judge(&c) != w {
            red += 1;
        }
        i += 1;
    }
    if red != 0 {
        return false;
    }
    // 注入一格，必生一红（14 格逐一验过，不许有"注入不生效"的盲格）。
    let mut j = 0usize;
    while j < CORPUS {
        let r = match run_with(Some((j, Fault::Exact))) {
            Ok(v) => v,
            Err(_) => return false,
        };
        if CORPUS as u32 - r.matched != 1 {
            return false;
        }
        j += 1;
    }
    // 反向：把槽 3（本来就对）注入它自己的期望，红数仍是 0。
    match run_with(Some((3, Fault::Ok))) {
        Ok(v) => {
            if v.matched != CORPUS as u32 {
                return false;
            }
        }
        Err(_) => return false,
    }
    // "判路被悄悄修好"不会骗过期望表：把槽 10 的 122 改回 28，判路变绿、
    // 期望表照旧判它 Code，于是恰好一红。
    let mut fixed = cases();
    let mut touched = false;
    if let Some(slot) = fixed.get_mut(10) {
        slot.claim.code = ERR_ENOSPC;
        touched = true;
    }
    if !touched {
        return false;
    }
    let c10 = match fixed.get(10) {
        Some(v) => *v,
        None => return false,
    };
    if c10.judge() != Fault::Ok || expected(10) != Some(Fault::Code) {
        return false;
    }
    let mut red2 = 0usize;
    let mut k = 0usize;
    while k < CORPUS {
        let c = match fixed.get(k) {
            Some(v) => *v,
            None => return false,
        };
        let w = match expected(k) {
            Some(v) => v,
            None => return false,
        };
        if judge(&c) != w {
            red2 += 1;
        }
        k += 1;
    }
    if red2 != 1 {
        return false;
    }
    // 账本里的 6 个红格：码逐格等于缺陷码，值必归零。
    let l = build_ledger();
    if l.err_count() != REJECT || l.ok_count() != ACCEPT {
        return false;
    }
    let mut s = 0usize;
    while s < REJECT {
        let cell = match l.sample(ACCEPT + s) {
            Some(v) => v,
            None => return false,
        };
        let w = match REJECT_CODES.get(s) {
            Some(v) => *v,
            None => return false,
        };
        if cell.ok || cell.code != w || cell.value != 0 {
            return false;
        }
        if cell.op != OP_BOOK {
            return false;
        }
        s += 1;
    }
    // 四路注入各自可独立断言（不在这里回调 inject_four，避免自证循环）。
    name_ok() && define_ok() && corpus_ok() && gate_ok() && ledger_ok()
}

/// 文案：strerror/动作分族/释放审计/字节比较/记账四句逐字钉版。
fn message_ok() -> bool {
    // 四个 errno 原文。
    if strerror(ERR_ENOSPC) != "No space left on device" {
        return false;
    }
    if strerror(ERR_EDQUOT) != "Quota exceeded" || strerror(ERR_EINVAL) != "Invalid argument" {
        return false;
    }
    if !strerror(ERR_NONE).is_empty() || strerror(7) != "Unknown error" {
        return false;
    }
    // 首刀失败体例：动作前缀 + 单引号路径 + strerror。
    if msg_dd_write("srv/data/archive.bin")
        != "dd: error writing 'srv/data/archive.bin': No space left on device"
    {
        return false;
    }
    if msg_cp_write("srv/data/archive.bin")
        != "cp: error writing 'srv/data/archive.bin': No space left on device"
    {
        return false;
    }
    // tee 不带引号——真实差异，别"统一"掉。
    if msg_tee_write("mnt/full/big.bin") != "tee: mnt/full/big.bin: No space left on device" {
        return false;
    }
    if msg_truncate("var/tmp/scratch.tmp", 0)
        != "truncate: failed to truncate 'var/tmp/scratch.tmp' at 0 bytes: No space left on device"
    {
        return false;
    }
    // 释放动作与释放量审计。
    if msg_rm_filler("filler.bin") != "removed 'filler.bin'" {
        return false;
    }
    if msg_release_line(8192, 8192) != "released 8192 bytes for a 8192-byte payload" {
        return false;
    }
    if msg_release_line(0, 8192) != "released 0 bytes for a 8192-byte payload" {
        return false;
    }
    // 四种"没恢复成"的现场。
    if msg_no_release("mnt/full/big.bin", 0)
        != "retry 'mnt/full/big.bin' without release (free 0): No space left on device"
    {
        return false;
    }
    if msg_short_free("mnt/full/half.bin", 8192, 4096)
        != "retry 'mnt/full/half.bin' short of space: wrote 4096 of 8192 bytes: No space left on device"
    {
        return false;
    }
    if msg_still_full("mnt/full/big.bin", 0)
        != "retry 'mnt/full/big.bin' still failing, file left at 0 bytes: No space left on device"
    {
        return false;
    }
    if msg_truncated_left("mnt/full/big.bin", 8192, 2048)
        != "retry 'mnt/full/big.bin' failed, file truncated to 2048 of 8192 bytes: No space left on device"
    {
        return false;
    }
    // 三种"字节没锚住"的现场。
    if msg_payload_drift("mnt/full/drift.bin", 8192, 4096)
        != "retry 'mnt/full/drift.bin' payload drift: requested 4096, first attempt 8192"
    {
        return false;
    }
    if msg_size_short("p", 8192, 4096) != "retry 'p' reported ok but size is 4096, expected 8192" {
        return false;
    }
    if msg_unproven("p", 8192)
        != "recovery claimed for 'p' without byte-exact comparison (size 8192)"
    {
        return false;
    }
    // 字节比较两句：逐字节一致 vs 提前 EOF。
    if msg_cmp_same("a.bin", "b.bin") != "a.bin and b.bin are identical" {
        return false;
    }
    if msg_cmp_eof("mnt/full/half.bin", 4096)
        != "cmp: EOF on mnt/full/half.bin after byte 4096, in line 1"
    {
        return false;
    }
    // 前提破断与记账三句。
    if msg_wrong_first(ERR_EDQUOT)
        != "recovery premise broken: first attempt failed with Quota exceeded (expected No space left on device)"
    {
        return false;
    }
    if msg_booked_fail(2) != "1 operation booked as 2 failures (expected 1)" {
        return false;
    }
    if msg_booked_loose("p") != "recovery of 'p' not booked (0 recoveries recorded)" {
        return false;
    }
    if msg_booked_double(8192, 16384)
        != "freed block counted twice: 16384 of 8192 bytes credited"
    {
        return false;
    }
    // 入参非法。
    if msg_arg_bad("payload", 0) != "invalid payload 0: Invalid argument" {
        return false;
    }
    if msg_arg_bad("free_before", 8192) != "invalid free_before 8192: Invalid argument" {
        return false;
    }
    // df 表头与数据行（释放之后 available 变大的现场）。
    if msg_df_header() != "Filesystem      1K-blocks    Used Available Use% Mounted on\n" {
        return false;
    }
    if msg_df_row("tmpfs", 4096, 1024, 3072, "/mnt") != "tmpfs      4096      1024      3072 25% /mnt\n" {
        return false;
    }
    if msg_df_row("overlay", 0, 0, 0, "/") != "overlay      0      0      0 100% /\n" {
        return false;
    }
    if msg_stat_size("srv/data/archive.bin", 8192) != "8192 srv/data/archive.bin\n" {
        return false;
    }
    // 现场文案路由：恢复成立时为空串，其余必须逐槽落成对应那一句。
    let cs = cases();
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let o = c.actual();
        let m = message(&c.shape, &o);
        match o.note {
            Note::Recovered => {
                if !m.is_empty() {
                    return false;
                }
            }
            Note::NoRelease => {
                if m != msg_no_release(c.shape.path, c.shape.free_before) {
                    return false;
                }
            }
            Note::ShortFree => {
                if m != msg_short_free(c.shape.path, c.shape.payload, c.shape.fit().have) {
                    return false;
                }
            }
            Note::PayloadDrift => {
                if m != msg_payload_drift(c.shape.path, c.shape.payload, c.shape.retry_payload) {
                    return false;
                }
            }
            Note::Unproven => {
                if m != msg_unproven(c.shape.path, c.shape.retry_size) {
                    return false;
                }
            }
            Note::BookedDouble => {
                if m != msg_booked_double(c.shape.filler, c.shape.filler.saturating_mul(2)) {
                    return false;
                }
            }
            _ => {
                if m.is_empty() {
                    return false;
                }
            }
        }
        // 结论一句话必须与成败同口径。
        if o.ok() != (verdict(&o) == "recovered") {
            return false;
        }
        i += 1;
    }
    // 四死槽的专用文案不落空（本语料只用到其中几句，但每句都得能生成）。
    if msg_still_full("p", 1).is_empty() || msg_truncated_left("p", 2, 1).is_empty() {
        return false;
    }
    if msg_size_short("p", 2, 1).is_empty() || msg_booked_loose("p").is_empty() {
        return false;
    }
    !msg_booked_fail(2).is_empty() && !msg_wrong_first(ERR_EDQUOT).is_empty()
}

/// 释义：一槽一行，13 维取值与 6 维结果全落在文本里；逐字钉版三行。
fn detail_ok() -> bool {
    let cs = cases();
    if cs.len() != CORPUS {
        return false;
    }
    let keys = [
        " payload=",
        " free=",
        " freed=",
        " first=",
        " wrote=",
        " retry=",
        " size=",
        " booked=",
        " stage=",
        " errno=",
        " ret=",
        " pair=",
        " note=",
    ];
    if keys.len() != SHAPE_DIMS {
        return false;
    }
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let line = detail(&c);
        // 行首是路径，行尾恰一个换行（多一个就是拼接漏了 as_str）。
        if !line.starts_with(c.shape.path) {
            return false;
        }
        if count_occur(&line, "\n") != 1 || !line.ends_with('\n') {
            return false;
        }
        // 13 个键一个都不能少（少一个就是某一维被默认值吞了）。
        let mut k = 0usize;
        while k < keys.len() {
            let key = match keys.get(k) {
                Some(v) => *v,
                None => return false,
            };
            if count_occur(&line, key) != 1 {
                return false;
            }
            k += 1;
        }
        // 成败与 msg= 的有无必须同口径。
        let has_msg = count_occur(&line, " msg=\"") == 1;
        if has_msg != !c.actual().ok() {
            return false;
        }
        if line.contains("msg=\"\"") {
            return false;
        }
        i += 1;
    }
    // 释义只由现场（形状 + 结果）决定：8 个活槽 8 种行文；6 个死槽现场与复刻源
    // 同一，行文必须一字不差地等于源槽那一行（标签与申报不许渗进释义）。
    let mut distinct = 0usize;
    let mut p = 0usize;
    while p < ACCEPT {
        let a = match cs.get(p) {
            Some(v) => detail(v),
            None => return false,
        };
        let mut seen = false;
        let mut q = 0usize;
        while q < p {
            if let Some(b) = cs.get(q) {
                if detail(b) == a {
                    seen = true;
                }
            }
            q += 1;
        }
        if !seen {
            distinct += 1;
        }
        p += 1;
    }
    if distinct != ACCEPT {
        return false;
    }
    let src_line = match cs.get(REJECT_SOURCE) {
        Some(v) => detail(v),
        None => return false,
    };
    let mut z = ACCEPT;
    while z < CORPUS {
        let line = match cs.get(z) {
            Some(v) => detail(v),
            None => return false,
        };
        if line != src_line {
            return false;
        }
        z += 1;
    }
    // 三行逐字钉版：恢复成立、没腾就重试、换了 payload。
    let c0 = match cs.get(0) {
        Some(v) => *v,
        None => return false,
    };
    let c3 = match cs.get(3) {
        Some(v) => *v,
        None => return false,
    };
    let c5 = match cs.get(5) {
        Some(v) => *v,
        None => return false,
    };
    let line0 = detail(&c0);
    let want0 = "srv/data/archive.bin payload=8192 free=0 freed=8192 first=nospc wrote=0 retry=none size=8192 booked=recover stage=done errno=0 ret=8192 pair=true note=recovered\n";
    if line0 != want0 {
        return false;
    }
    let line3 = detail(&c3);
    let want3 = "mnt/full/big.bin payload=8192 free=0 freed=8192 first=nospc wrote=0 retry=nospc size=0 booked=none stage=release errno=28 ret=0 pair=false note=norelease msg=\"retry 'mnt/full/big.bin' without release (free 0): No space left on device\"\n";
    if line3 != want3 {
        return false;
    }
    let line5 = detail(&c5);
    let want5 = "mnt/full/drift.bin payload=8192 free=0 freed=8192 first=nospc wrote=0 retry=none size=4096 booked=none stage=retry errno=0 ret=4096 pair=false note=payload-drift msg=\"retry 'mnt/full/drift.bin' payload drift: requested 4096, first attempt 8192\"\n";
    if line5 != want5 {
        return false;
    }
    // 部分写的那一槽必须把首刀字节与最终长度分开写（4096 与 12288 同时在行里）。
    let c2 = match cs.get(2) {
        Some(v) => *v,
        None => return false,
    };
    let line2 = detail(&c2);
    if count_occur(&line2, "wrote=4096") != 1 || count_occur(&line2, "size=12288") != 1 {
        return false;
    }
    // 截断/补写两槽的 booked 拼写必须是短名（recover/double/none），不是数字。
    let c7 = match cs.get(7) {
        Some(v) => *v,
        None => return false,
    };
    let line7 = detail(&c7);
    if count_occur(&line7, "booked=double") != 1 || !line7.contains("note=booked-double") {
        return false;
    }
    // 恢复行里不许出现 msg（成功没有可报的错）。
    !line0.contains("msg=")
}

/// 阶段闸门：七段单调、回退即段错、关闸即态错。
fn gate_ok() -> bool {
    let stages = [
        Stage::Arg,
        Stage::Premise,
        Stage::Release,
        Stage::Retry,
        Stage::Exact,
        Stage::Book,
        Stage::Done,
    ];
    let names = ["arg", "premise", "release", "retry", "exact", "book", "done"];
    if stages.len() != 7 || names.len() != 7 {
        return false;
    }
    // 空闸：未走、未关。
    let mut g = Gate::new();
    if g.steps() != 0 || g.last() != -1 || g.closed() {
        return false;
    }
    // 正走一遍：七段全过，秩与步数同步。
    let mut i = 0usize;
    while i < stages.len() {
        let s = match stages.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let want_name = match names.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if s.name() != want_name {
            return false;
        }
        if g.step(s) != Ok(()) {
            return false;
        }
        if g.last() != i as i32 || g.steps() != (i as u32) + 1 {
            return false;
        }
        i += 1;
    }
    if g.close() != Ok(7) || !g.closed() || g.steps() != 7 {
        return false;
    }
    // 关闸之后：再走是态错，再关也是态错（不许"关两次算两步"）。
    if g.step(Stage::Done) != Err(RR_ESTATE) {
        return false;
    }
    if g.close() != Err(RR_ESTATE) {
        return false;
    }

    // 回退：任何一段往后走都是段错，且失败不动步数/游标（原子性）。
    let mut r = Gate::new();
    if r.step(Stage::Retry) != Ok(()) {
        return false;
    }
    if r.step(Stage::Release) != Err(RR_ESTAGE) {
        return false;
    }
    if r.steps() != 1 || r.last() != Stage::Retry.order() as i32 {
        return false;
    }
    // 同一段重复不算回退（两次观测同一闸是合法的）。
    if r.step(Stage::Retry) != Ok(()) || r.steps() != 2 {
        return false;
    }
    // 跳过中间段合法：跳读不是倒推。
    let mut jump = Gate::new();
    if jump.step(Stage::Arg) != Ok(()) || jump.step(Stage::Done) != Ok(()) {
        return false;
    }
    if jump.steps() != 2 || jump.last() != Stage::Done.order() as i32 {
        return false;
    }

    // 7×7 全对：前进全通、回退全挡。
    let mut a = 0usize;
    while a < stages.len() {
        let x = match stages.get(a) {
            Some(v) => *v,
            None => return false,
        };
        let mut b = 0usize;
        while b < stages.len() {
            let y = match stages.get(b) {
                Some(v) => *v,
                None => return false,
            };
            let mut one = Gate::new();
            if one.step(x) != Ok(()) {
                return false;
            }
            let got = one.step(y);
            let want = if y.order() < x.order() {
                Err(RR_ESTAGE)
            } else {
                Ok(())
            };
            if got != want {
                return false;
            }
            if want == Err(RR_ESTAGE) && one.steps() != 1 {
                return false;
            }
            b += 1;
        }
        a += 1;
    }

    // 语料逐槽：走完到实际结束阶段为止，步数 = 秩 + 1；Done 恰好 7 步。
    let cs = cases();
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let stop = c.actual().stage;
        let mut w = Gate::new();
        let mut k = 0usize;
        while k < stages.len() {
            let s = match stages.get(k) {
                Some(v) => *v,
                None => return false,
            };
            if w.step(s) != Ok(()) {
                return false;
            }
            if s == stop {
                break;
            }
            k += 1;
        }
        let want_steps = stop.order() as u32 + 1;
        if w.steps() != want_steps {
            return false;
        }
        if w.close() != Ok(want_steps) {
            return false;
        }
        // 已关的闸不许再走一段"补记"阶段。
        if w.step(Stage::Done) != Err(RR_ESTATE) {
            return false;
        }
        i += 1;
    }
    // 停摆/漂移/记账三档的结束阶段都不是 Done；放行档才是。
    let mut j = 0usize;
    while j < CORPUS {
        let c = match cs.get(j) {
            Some(v) => *v,
            None => return false,
        };
        let done = c.actual().stage == Stage::Done;
        if done != c.actual().ok() || done != (Tier::of(&c.actual()) == Tier::Pass) {
            return false;
        }
        j += 1;
    }
    // 每条失败路径的结束阶段都在"该闸"或更早：秩不许越过 note 归属面。
    let mut z = 0usize;
    while z < CORPUS {
        let c = match cs.get(z) {
            Some(v) => *v,
            None => return false,
        };
        let o = c.actual();
        if o.note.premise_cause() && o.stage.order() > Stage::Premise.order() {
            return false;
        }
        if o.note.release_cause() && o.stage.order() > Stage::Release.order() {
            return false;
        }
        if o.note.retry_cause() && o.stage.order() > Stage::Retry.order() {
            return false;
        }
        if o.note.drift_cause() && o.stage.order() > Stage::Exact.order() {
            return false;
        }
        if o.note.book_cause() && o.stage.order() > Stage::Book.order() {
            return false;
        }
        z += 1;
    }
    Stage::Arg.order() == 0 && Stage::Done.order() == 6
}

/// 账本：14 格严格登记、逐格对账、错账必露、冻结后拒登记、重放同指纹。
fn ledger_ok() -> bool {
    // 空账本：不是 14 格，audit 直接判死；没有红。
    let mut e = Ledger::new();
    if e.len() != 0 || !e.is_empty() || e.ok_count() != 0 || e.err_count() != 0 {
        return false;
    }
    if e.audit() != Err(RR_EBAL) {
        return false;
    }
    if e.ops_match() != Ok(()) || e.rejects() != 0 || e.is_frozen() {
        return false;
    }
    if e.sample(0).is_some() {
        return false;
    }
    // 槽位必须等于当前长度：跳格登记被拒并计数，账本不长。
    let jump = Sample {
        slot: 1,
        op: OP_CORPUS,
        ok: true,
        code: 0,
        value: 14,
    };
    if e.register(jump) != Err(RR_ESLOT) {
        return false;
    }
    if e.len() != 0 || e.rejects() != 1 {
        return false;
    }
    // 正着登记一格，再登同一格=重复，仍被拒。
    let first = count_cell(0, OP_CORPUS, Ok(14));
    if e.register(first) != Ok(()) {
        return false;
    }
    if e.len() != 1 || e.is_empty() || e.rejects() != 1 {
        return false;
    }
    let held = match e.sample(0) {
        Some(v) => v,
        None => return false,
    };
    if held.slot != 0 || held.op != OP_CORPUS || !held.ok || held.value != 14 {
        return false;
    }
    if e.sample(1).is_some() {
        return false;
    }
    if e.register(first) != Err(RR_ESLOT) || e.rejects() != 2 || e.len() != 1 {
        return false;
    }

    // 全账：14 格、8 成 6 败、逐格对账、操作码对齐。
    let mut l = build_ledger();
    if l.len() != SAMPLES || l.ok_count() != ACCEPT || l.err_count() != REJECT {
        return false;
    }
    if l.audit() != Ok(()) || l.ops_match() != Ok(()) {
        return false;
    }
    if l.rejects() != 0 || l.is_frozen() || l.seal() == 0 {
        return false;
    }
    let mut i = 0usize;
    while i < SAMPLES {
        let s = match l.sample(i) {
            Some(v) => v,
            None => return false,
        };
        if s.slot as usize != i {
            return false;
        }
        if let Some(op) = LEDGER_OPS.get(i) {
            if s.op != *op {
                return false;
            }
        } else {
            return false;
        }
        i += 1;
    }
    if l.sample(SAMPLES).is_some() {
        return false;
    }
    // 八格成功值逐格等于 VALS；末格就是红档率。
    let mut vi = 0usize;
    let mut s = 0usize;
    while s < SAMPLES {
        let cell = match l.sample(s) {
            Some(v) => v,
            None => return false,
        };
        if cell.ok {
            let w = match VALS.get(vi) {
                Some(v) => *v,
                None => return false,
            };
            if cell.value != w || cell.code != 0 {
                return false;
            }
            vi += 1;
        }
        s += 1;
    }
    if vi != ACCEPT {
        return false;
    }
    if let Some(last) = l.sample(ACCEPT - 1) {
        if last.value != 929 {
            return false;
        }
    } else {
        return false;
    }

    // 冻结：指纹 = 记账时的 seal，重放同指纹；二次冻结与冻结后登记都是红。
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if root == 0 || root != l.root() || !l.is_frozen() {
        return false;
    }
    if l.rejects() != 0 {
        return false;
    }
    // 重放必须趁账本还是原样时对指纹：此后两次拒登记会改 rejects，指纹本就该变。
    if l.replay() != Ok(root) {
        return false;
    }
    if l.freeze() != Err(RR_EBAL) || l.rejects() != 1 {
        return false;
    }
    let late = Sample {
        slot: 0,
        op: OP_BOOK,
        ok: false,
        code: RR_ESTATE,
        value: 0,
    };
    if l.register(late) != Err(RR_ESLOT) || l.rejects() != 2 || l.len() != SAMPLES {
        return false;
    }
    // 拒登记确实改了记账面：指纹不再等于冻结时的根（冻结不是摆设）。
    if l.seal() == root {
        return false;
    }

    // 错账：停摆档多记一格，audit 必须在 RR_EVALUE 上露出来；重放对不上指纹。
    let b = build_ledger_broken();
    if b.len() != SAMPLES || b.ok_count() != ACCEPT || b.err_count() != REJECT {
        return false;
    }
    if b.audit() != Err(RR_EVALUE) || b.ops_match() != Ok(()) {
        return false;
    }
    if b.seal() == build_ledger().seal() {
        return false;
    }
    if b.replay() != Err(RR_ESEAL) {
        return false;
    }
    if b.audit() == l.audit() {
        return false;
    }

    // 计数结果 → 样本：成格码归零、败格值归零。
    let good = count_cell(0, OP_ARG, Ok(7));
    if !good.ok || good.code != 0 || good.value != 7 || good.slot != 0 || good.op != OP_ARG {
        return false;
    }
    let bad = count_cell(1, OP_ARG, Err(RR_ECODE));
    if bad.ok || bad.code != RR_ECODE || bad.value != 0 || bad.slot != 1 {
        return false;
    }
    // 空账本重放必对不上（不是"空对空"也算过）。
    Ledger::new().replay() == Err(RR_ESEAL)
}

// ==== CHUNK K START ====

/// 本卡判据入口：13 项，一项一道闸，末项把冻结重放与四路注入一起收口。
pub fn run_unx11696_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11696_release_retry_recover");

    s.add(
        "RR01-名称与段基址",
        name_ok(),
        "段基址 0x6d3f0、13 个段内码偏移连续、errno 真值 28/122/22/0、记账四值与码名逐项复算",
    );
    s.add(
        "RR02-联签与相邻卡",
        anchor_ok(),
        "与 F11695(0x6d3e0) 隔 0x10、与 F11681(0x6d130) 隔 0x2c0、与 F11697(0x6d410) 隔 0x20，文案族结构事实对齐",
    );
    s.add(
        "RR03-枚举与七段闸",
        enums_ok(),
        "七段秩 0..6 单调、13 注解四分类互斥完备、四档与 14 个缺陷码一一对应",
    );
    s.add(
        "RR04-释放几何与对账",
        geom_ok(),
        "四条释放几何全良、逐项破坏必判死、缺口覆盖率百万分位可复算且不除零",
    );
    s.add(
        "RR05-形状合法域",
        shape_ok(),
        "13 维形状逐维破坏必落 Arg 闸、合法域边界（payload/写入量/释放量/errno/记账）逐条钉住",
    );
    s.add(
        "RR06-七段纵线",
        define_ok(),
        "arg→premise→release→retry→exact→book→done 每闸只在上一闸放行后执行，短路点与注解同源",
    );
    s.add(
        "RR07-字节精确对照",
        exact_ok(),
        "同一 payload、同一路径、同一最终长度且逐字节比较过；换 payload/没比字节/长度不符一律判死",
    );
    s.add(
        "RR08-申报对判",
        judge_ok(),
        "成败→阶段→errno→最终字节→注解→档位，次序不可换，六死槽各中自己那一维",
    );
    s.add(
        "RR09-矩阵与报表",
        matrix_ok(),
        "四档 3/8/2/1 行、全对档 3、覆盖平、逐字渲染钉版、注入一格必掉 matched 并掉红档率",
    );
    s.add(
        "RR10-语料与期望表",
        corpus_ok(),
        "14 槽标签唯一、期望表五列与现场对撞、六死槽只换标签的复刻、8 槽档位覆盖 3/2/2/1",
    );
    s.add(
        "RR11-槽位与红档",
        slots_ok() && reds_ok(),
        "8 个成功格值与报表七列同值、第 8 格恒为红档率 929、6 个红格码链对齐、判路被修好也骗不过期望表",
    );
    s.add(
        "RR12-文案与释义",
        message_ok() && detail_ok(),
        "strerror/动作分族/释放审计/cmp 两句/记账三句逐字钉版；释义一行 13 维 6 结果全落文本",
    );

    // 末项：阶段闸门、逐格对账、冻结重放，以及四路注入全部为真。
    let mut l = build_ledger();
    let audited = l.audit() == Ok(());
    let ops = l.ops_match() == Ok(());
    let frozen = match l.freeze() {
        Ok(r) => r != 0 && r == l.root() && l.is_frozen() && l.replay() == Ok(r),
        Err(_) => false,
    };
    let four = inject_four();
    let all_four = four == [true, true, true, true];
    s.add(
        "RR13-阶段闸门与冻结重放四注入",
        gate_ok() && ledger_ok() && audited && ops && frozen && all_four,
        "七段闸单调且关闸即态错；14 格严格登记、错账露值、冻结后拒登记、重放同指纹；表/判/渲染/账本四路注入全真",
    );

    s
}

// ==== CHUNK K END ====

// ==== CHUNK L START ====

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Shape {
        Shape::blank()
    }

    #[test]
    fn t_seg_base_exact() {
        assert_eq!(SEG_BASE, 0x6d3f0);
        assert_eq!(TAG_OK, SEG_BASE);
    }

    #[test]
    fn t_code_offsets() {
        assert_eq!(RR_ESTAGE, SEG_BASE + 0x01);
        assert_eq!(RR_ECODE, SEG_BASE + 0x02);
        assert_eq!(RR_EVALUE, SEG_BASE + 0x03);
        assert_eq!(RR_ENOTE, SEG_BASE + 0x04);
        assert_eq!(RR_EDEFAULT, SEG_BASE + 0x05);
        assert_eq!(RR_EEXACT, SEG_BASE + 0x06);
        assert_eq!(RR_ESTATE, SEG_BASE + 0x07);
        assert_eq!(RR_ECOUNT, SEG_BASE + 0x08);
        assert_eq!(RR_ESLOT, SEG_BASE + 0x09);
        assert_eq!(RR_ESEAL, SEG_BASE + 0x0a);
        assert_eq!(RR_ENOBS, SEG_BASE + 0x0b);
        assert_eq!(RR_EMATRIX, SEG_BASE + 0x0c);
        assert_eq!(RR_EBAL, SEG_BASE + 0x0d);
    }

    #[test]
    fn t_counts() {
        assert_eq!(CORPUS, 14);
        assert_eq!(SAMPLES, 14);
        assert_eq!(ACCEPT, 8);
        assert_eq!(REJECT, 6);
        assert_eq!(TIER_COUNT, 4);
        assert_eq!(SHAPE_DIMS, 13);
        assert_eq!(ACCEPT + REJECT, CORPUS);
    }

    #[test]
    fn t_errno_truth() {
        assert_eq!(ERR_ENOSPC, 28);
        assert_eq!(ERR_EDQUOT, 122);
        assert_eq!(ERR_EINVAL, 22);
        assert_eq!(ERR_NONE, 0);
        assert_eq!(strerror(ERR_ENOSPC), "No space left on device");
        assert_eq!(strerror(ERR_EDQUOT), "Quota exceeded");
    }

    #[test]
    fn t_fnv_repeatable() {
        assert_eq!(fold_str(FNV_OFF, "release"), fold_str(FNV_OFF, "release"));
        assert_ne!(fold_str(FNV_OFF, "release"), fold_str(FNV_OFF, "retry"));
        assert_ne!(fold_word(FNV_OFF, 0), fold_word(FNV_OFF, 1));
        assert_ne!(fold_str(FNV_OFF, ""), FNV_OFF);
        assert_eq!(FNV_PRIME, 0x100_0000_01b3);
    }

    #[test]
    fn t_text_helpers() {
        assert_eq!(dec_text(0), "0");
        assert_eq!(dec_text(8192), "8192");
        assert_eq!(hex_text(0x6d3f0, 5), "6d3f0");
        assert_eq!(i32_text(-1), "-1");
        assert_eq!(i32_text(28), "28");
        assert_eq!(bool_text(true), "true");
        assert_eq!(rate_text(929), "929");
    }

    #[test]
    fn t_fit_covers() {
        assert!(Fit::new(4096, 4096).covers());
        assert!(!Fit::new(4096, 4095).covers());
        assert_eq!(Fit::new(4096, 4095).short_by(), 1);
        assert_eq!(Fit::new(4096, 8192).short_by(), 0);
    }

    #[test]
    fn t_fit_ratio() {
        assert_eq!(Fit::new(4096, 4096).ratio_ppm(), 1_000_000);
        assert_eq!(Fit::new(4096, 2048).ratio_ppm(), 500_000);
        assert_eq!(Fit::new(4096, 8192).ratio_ppm(), 1_000_000);
        assert_eq!(Fit::new(0, 8192).ratio_ppm(), 0);
    }

    #[test]
    fn t_geom_well_formed() {
        let gs = release_geoms();
        assert_eq!(gs.len(), 4);
        let mut i = 0usize;
        while i < gs.len() {
            if let Some(g) = gs.get(i) {
                assert!(g.well_formed());
            }
            i += 1;
        }
    }

    #[test]
    fn t_geom_breaks() {
        let g = match release_geoms().first() {
            Some(v) => *v,
            None => return,
        };
        assert!(!ReleaseGeom { tag: "", ..g }.well_formed());
        assert!(!ReleaseGeom { payload: 0, ..g }.well_formed());
        assert!(!ReleaseGeom { free_before: 8192, ..g }.well_formed());
        assert!(!ReleaseGeom {
            first_written: 8193,
            ..g
        }
        .well_formed());
    }

    #[test]
    fn t_stage_order() {
        assert_eq!(Stage::Arg.order(), 0);
        assert_eq!(Stage::Premise.order(), 1);
        assert_eq!(Stage::Release.order(), 2);
        assert_eq!(Stage::Retry.order(), 3);
        assert_eq!(Stage::Exact.order(), 4);
        assert_eq!(Stage::Book.order(), 5);
        assert_eq!(Stage::Done.order(), 6);
        assert!(!Stage::Done.failure());
        assert!(Stage::Release.space_side());
        assert!(Stage::Book.structural());
    }

    #[test]
    fn t_note_classes() {
        assert_eq!(Note::Recovered.classes(), 1);
        assert_eq!(Note::NoRelease.classes(), 1);
        assert!(Note::ShortFree.release_cause());
        assert!(Note::Unproven.drift_cause());
        assert!(Note::BookedDouble.book_cause());
        assert!(!Note::Recovered.stalled());
    }

    #[test]
    fn t_tier_order() {
        assert_eq!(Tier::Pass.index(), 0);
        assert_eq!(Tier::Stall.index(), 1);
        assert_eq!(Tier::Drift.index(), 2);
        assert_eq!(Tier::Book.index(), 3);
        assert!(Tier::Pass.success());
        assert!(Tier::Book.failure_face());
    }

    #[test]
    fn t_define_pass() {
        let o = define(&base());
        assert_eq!(o.stage, Stage::Done);
        assert_eq!(o.code, ERR_NONE);
        assert_eq!(o.value, 8192);
        assert_eq!(o.note, Note::Recovered);
        assert!(o.ok() && o.recovered());
    }

    #[test]
    fn t_define_norelease() {
        let o = define(&base().with_freed(false));
        assert_eq!(o.stage, Stage::Release);
        assert_eq!(o.code, ERR_ENOSPC);
        assert_eq!(o.note, Note::NoRelease);
        assert!(!o.ok());
    }

    #[test]
    fn t_define_shortfree() {
        let o = define(&base().with_filler(4096));
        assert_eq!(o.stage, Stage::Release);
        assert_eq!(o.value, 4096);
        assert_eq!(o.note, Note::ShortFree);
    }

    #[test]
    fn t_define_drift() {
        let o = define(&base().with_retry_payload(4096));
        assert_eq!(o.stage, Stage::Retry);
        assert_eq!(o.code, ERR_NONE);
        assert_eq!(o.value, 4096);
        assert_eq!(o.note, Note::PayloadDrift);
    }

    #[test]
    fn t_define_unproven() {
        let o = define(&base().with_proven(false));
        assert_eq!(o.stage, Stage::Exact);
        assert_eq!(o.value, 8192);
        assert_eq!(o.note, Note::Unproven);
    }

    #[test]
    fn t_define_double() {
        let o = define(&base().with_booked(BOOK_DOUBLE));
        assert_eq!(o.stage, Stage::Book);
        assert_eq!(o.note, Note::BookedDouble);
        assert!(!o.default);
    }

    #[test]
    fn t_define_wrong_first() {
        let o = define(&base().with_first_code(ERR_EDQUOT));
        assert_eq!(o.stage, Stage::Premise);
        assert_eq!(o.code, ERR_EDQUOT);
        assert_eq!(o.note, Note::WrongFirst);
    }

    #[test]
    fn t_define_size_short() {
        let o = define(&base().with_retry_size(4096));
        assert_eq!(o.stage, Stage::Exact);
        assert_eq!(o.code, ERR_NONE);
        assert_eq!(o.note, Note::SizeShort);
    }

    #[test]
    fn t_define_truncated_left() {
        let sh = base().with_first_written(4096).with_retry_code(ERR_ENOSPC).with_retry_size(2048);
        let o = define(&sh);
        assert_eq!(o.stage, Stage::Retry);
        assert_eq!(o.note, Note::TruncatedLeft);
    }

    #[test]
    fn t_define_still_full() {
        let sh = base().with_retry_code(ERR_ENOSPC).with_retry_size(4096);
        let o = define(&sh);
        assert_eq!(o.stage, Stage::Retry);
        assert_eq!(o.note, Note::StillFull);
    }

    #[test]
    fn t_define_booked() {
        assert_eq!(define(&base().with_booked(BOOK_NEWFAIL)).note, Note::BookedRetryFail);
        assert_eq!(define(&base().with_booked(BOOK_NONE)).note, Note::BookedLoose);
    }

    #[test]
    fn t_define_arg_bad() {
        let o = define(&base().with_payload(0));
        assert_eq!(o.stage, Stage::Arg);
        assert_eq!(o.code, ERR_EINVAL);
        assert_eq!(o.note, Note::ArgBad);
    }

    #[test]
    fn t_corpus_size() {
        assert_eq!(cases().len(), CORPUS);
        assert_eq!(SHAPES.len(), CORPUS);
        assert_eq!(CLAIMS.len(), CORPUS);
    }

    #[test]
    fn t_judge_accept() {
        let cs = cases();
        let mut i = 0usize;
        while i < ACCEPT {
            if let Some(c) = cs.get(i) {
                assert_eq!(c.judge(), Fault::Ok);
            }
            i += 1;
        }
    }

    #[test]
    fn t_judge_reject() {
        let cs = cases();
        let mut i = 0usize;
        while i < REJECT {
            if let (Some(c), Some(w)) = (cs.get(ACCEPT + i), REJECT_FAULTS.get(i)) {
                assert_eq!(c.judge(), *w);
            }
            i += 1;
        }
    }

    #[test]
    fn t_judge_estate_first() {
        let c = match case_at(8) {
            Some(v) => v,
            None => return,
        };
        assert_eq!(c.judge(), Fault::Estate);
        assert!(!c.claim.success());
        assert!(!c.actual().ok());
    }

    #[test]
    fn t_matrix_counts() {
        let m = Matrix::of();
        assert_eq!(m.slots(), CORPUS);
        assert_eq!(m.goods(), ACCEPT);
        assert_eq!(m.perfect_rows(), 3);
        assert!(m.covered() && m.faces_covered());
    }

    #[test]
    fn t_report_render() {
        let r = match run() {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(r.render(), RENDER_OK);
        assert!(r.green());
        assert_eq!(r.gate(), Ok(()));
    }

    #[test]
    fn t_rate_rounding() {
        let r = Report {
            total: 14,
            accept: 8,
            reject: 6,
            pass: 3,
            stall: 2,
            drift: 2,
            book: 1,
            matched: 13,
        };
        assert_eq!(r.rate(), 929);
        let z = Report {
            total: 0,
            accept: 0,
            reject: 0,
            pass: 0,
            stall: 0,
            drift: 0,
            book: 0,
            matched: 0,
        };
        assert_eq!(z.rate(), 0);
    }

    #[test]
    fn t_run_with_slot() {
        assert_eq!(run_with(Some((CORPUS, Fault::Code))).err(), Some(RR_ESLOT));
    }

    #[test]
    fn t_ledger_audit() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), ACCEPT);
        assert_eq!(l.audit(), Ok(()));
        assert_eq!(l.ops_match(), Ok(()));
    }

    #[test]
    fn t_ledger_broken() {
        let b = build_ledger_broken();
        assert_eq!(b.audit(), Err(RR_EVALUE));
        assert_eq!(b.replay(), Err(RR_ESEAL));
    }

    #[test]
    fn t_ledger_freeze() {
        let mut l = build_ledger();
        let r = match l.freeze() {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_ne!(r, 0);
        assert!(l.is_frozen());
        assert_eq!(l.replay(), Ok(r));
        assert_eq!(l.freeze(), Err(RR_EBAL));
    }

    #[test]
    fn t_ledger_register_order() {
        let mut l = Ledger::new();
        let s = Sample {
            slot: 1,
            op: OP_CORPUS,
            ok: true,
            code: 0,
            value: 14,
        };
        assert_eq!(l.register(s), Err(RR_ESLOT));
        assert_eq!(l.rejects(), 1);
        assert_eq!(l.len(), 0);
    }

    #[test]
    fn t_gate_monotone() {
        let mut g = Gate::new();
        assert_eq!(g.step(Stage::Release), Ok(()));
        assert_eq!(g.step(Stage::Retry), Ok(()));
        assert_eq!(g.step(Stage::Premise), Err(RR_ESTAGE));
        assert_eq!(g.steps(), 2);
    }

    #[test]
    fn t_gate_close() {
        let mut g = Gate::new();
        assert_eq!(g.step(Stage::Done), Ok(()));
        assert_eq!(g.close(), Ok(1));
        assert!(g.closed());
        assert_eq!(g.step(Stage::Done), Err(RR_ESTATE));
        assert_eq!(g.close(), Err(RR_ESTATE));
    }

    #[test]
    fn t_detail_pinned() {
        let c = match case_at(3) {
            Some(v) => v,
            None => return,
        };
        let want = "mnt/full/big.bin payload=8192 free=0 freed=8192 first=nospc wrote=0 retry=nospc size=0 booked=none stage=release errno=28 ret=0 pair=false note=norelease msg=\"retry 'mnt/full/big.bin' without release (free 0): No space left on device\"\n";
        assert_eq!(detail(&c), want);
    }

    #[test]
    fn t_message_pinned() {
        assert_eq!(
            msg_no_release("p", 0),
            "retry 'p' without release (free 0): No space left on device"
        );
        assert_eq!(msg_cmp_same("a", "b"), "a and b are identical");
        assert_eq!(
            msg_unproven("p", 8192),
            "recovery claimed for 'p' without byte-exact comparison (size 8192)"
        );
    }

    #[test]
    fn t_verdict() {
        assert_eq!(verdict(&define(&base())), "recovered");
        assert_eq!(verdict(&define(&base().with_freed(false))), "no release");
        assert_eq!(verdict(&define(&base().with_proven(false))), "unproven");
    }

    #[test]
    fn t_seal_sensitive() {
        assert_eq!(base().seal(), base().seal());
        assert_ne!(base().seal(), base().with_path("x").seal());
        assert_ne!(base().seal(), base().with_proven(false).seal());
        assert_ne!(base().seal(), base().with_booked(BOOK_DOUBLE).seal());
    }

    #[test]
    fn t_inject_four() {
        assert_eq!(inject_four(), [true, true, true, true]);
    }

    #[test]
    fn t_checks_green() {
        let cs = run_unx11696_checks();
        assert_eq!(cs.len(), 13);
        assert!(cs.all_passed());
        assert_eq!(cs.red_items().1, 0);
    }

    #[test]
    fn t_corpus_clone_single_dim() {
        let cs = cases();
        let src = match cs.get(REJECT_SOURCE) {
            Some(v) => *v,
            None => return,
        };
        let mut i = ACCEPT;
        while i < CORPUS {
            if let Some(c) = cs.get(i) {
                assert_ne!(c.shape.seal(), src.shape.seal());
                assert_eq!(c.shape.payload, src.shape.payload);
                assert_eq!(c.shape.path, src.shape.path);
            }
            i += 1;
        }
    }

    #[test]
    fn t_vals_pinned() {
        assert_eq!(VALS, [14, 8, 6, 3, 2, 2, 1, 929]);
        assert_eq!(VALS.get(ACCEPT - 1), Some(&929u32));
    }
}

// ==== CHUNK L END ====

