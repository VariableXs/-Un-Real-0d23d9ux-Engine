//! UNX-F11687 mv 跨盘 ENOSPC 部分复制回滚判据
//!
//! 跨设备的 `mv` 不是一次 `rename(2)`，而是一条**先复制、后删除**的两段式
//! 事务：`open(dst, O_WRONLY|O_CREAT|O_EXCL)` → 逐块 `read/write` → `fsync`
//! → `close` → `unlink(src)`。这两段之间没有内核事务保护，任何一个字节写失败，
//! 事务就停在"半成品"状态上。本卡判的就是这个半成品状态**怎么收场**：
//!
//! ```text
//! $ mv /mnt/a/blob /mnt/b/blob
//! mv: cannot create regular file '/mnt/b/blob': No space left on device
//! $ echo $?
//! 1
//! $ ls /mnt/a/blob          # 源必须原样在
//! /mnt/a/blob
//! $ ls /mnt/b/blob          # 目的地的半成品必须已经不在
//! ls: cannot access '/mnt/b/blob': No such file or directory
//! ```
//!
//! 四件事必须同时成立，缺一即判死：
//!
//! * **源保留**：复制失败后源文件一个字节都不能少、一个字节都不能多；`mv`
//!   只有在目的侧完整落盘并校验通过之后才允许 `unlink(src)`。
//! * **半成品回滚**：目的侧已经写进去的那部分必须被 `unlink` 掉。留下一个
//!   `Partial{n}` 是本卡的头号红——用户看不见的半截文件比明确失败更危险。
//! * **预存目标不毁**：目的路径在操作前就存在时，失败必须让它**原样幸存**；
//!   不许先截断再复制（那会把旧内容先毁掉再报 ENOSPC）。
//! * **文案与退出码**：GNU coreutils 的原文是
//!   `mv: cannot create regular file '<dst>': No space left on device`，退出 1；
//!   busybox 走 `mv: can't create '<dst>': No space left on device`，退出 1。
//!   两式都要能落在本卡的注记面上，不许把 errno 文案改写成别的说法。
//!
//! ## 闸门次序（这条纵线为什么不能倒）
//!
//! 九道闸，秩 1..9，一次只许前进；倒退即 `MV_STAGE`，封账后再动即
//! `MV_ESTATE`：
//!
//! 1. **Check**（秩 1）——命令行成立：源与目标两个操作数都在、没有 `-t` 之类
//!    的用法错。用法错一字节都不会读，谈不上回滚。
//! 2. **Source**（秩 2）——源可读、可 `stat`。源都打不开时，"源保留"是一个
//!    空命题，必须先立住这一闸。
//! 3. **Scope**（秩 3）——同盘还是跨盘。同盘走 `rename(2)`，是元数据操作
//!    （本卡只用它做边界对照，真正判 rename 的是 F11690）；跨盘才进入
//!    copy-then-unlink 的事务面。把同盘认成跨盘，就会凭空造出一场复制；
//!    把跨盘认成同盘，就会把 EXDEV 当成成功——两个方向都判死。
//! 4. **Dest**（秩 4）——目的地现状：不存在、已存在、是目录。预存目标是
//!    本卡"不毁旧物"维度的输入；这一闸不产出事实，后面就无从判"是否被毁"。
//! 5. **Copy**（秩 5）——建目的文件、复制数据。ENOSPC 就在这一闸的写面上
//!    浮出；它是本卡唯一的失败发源地。
//! 6. **Write**（秩 6）——写面结算：写进去多少字节、错误是哪一条 errno。
//!    `value` 维（已复制字节数）在这一闸定稿。
//! 7. **Verify**（秩 7）——完整性校验：字节数一致、`fsync` 成功。只有这一
//!    闸通过，后面才允许删源；跳过校验直接删源是本卡最贵的伪造。
//! 8. **Unlink**（秩 8）——删源。它是事务的**提交点**：源一旦删除，任何后续
//!    失败都无法回滚成"没发生过"。所以 Unlink 必须排在 Verify 之后，次序
//!    倒了，回滚语义就整体失效。
//! 9. **Exit**（秩 9）——退出码与文案结算。1 = 失败（含已正确回滚的 ENOSPC），
//!    0 = 成功，2 = 用法错。回滚正确不代表退出 0，这两件事必须分开记。
//!
//! 次序不能倒的三条硬理由：Dest 先于 Copy（不知道目的地原先有没有东西，
//! 就无法判"旧物是否被毁"）；Verify 先于 Unlink（先删源再校验等于把源当成了
//! 可再生的缓存）；Write 先于 Exit（退出码是写面事实的折算，不是独立事实）。
//!
//! ## 关键差异（本卡深度所在，谁判死谁放行）
//!
//! * **与 F11686（sed -i 原件保护）分层**：sed -i 的"原件"是**输入**，它的
//!   回滚是"临时文件清掉、原件不动"；mv 的"源"同时是输入与待删对象，它的
//!   回滚是"目的半成品清掉、源不动"。前者保护的是被读的文件，后者保护的是
//!   **将被删的文件**——所以 mv 多出一条"删源时机"判据（Verify 先于 Unlink），
//!   sed -i 没有这一条。
//! * **与 F11685（sort 临时文件 ENOSPC）分层**：sort 的临时文件是自己造的
//!   私有对象，删掉即回滚；mv 的目的文件落在**用户指定的路径**上，可能覆盖
//!   一个预存文件，所以 mv 多出"预存目标不毁"这一维，sort 不判这一维。
//! * **与 F11690（rename ENOSPC）分层**：同盘 `rename(2)` 不复制数据、不分配
//!   数据块，正常路径上根本不会有 ENOSPC。F11690 判"把 EXDEV 谎报成 ENOSPC"
//!   这条假标签；本卡判"跨盘 fallback 真的复制到一半没空间"之后怎么收场。
//!   一张卡管**不该有的 ENOSPC**，一张卡管**真的有了 ENOSPC 之后的回滚**。
//! * **与 F11694（文案逐字）分层**：文案卡判 `strerror(28)` 的逐字拼写；本卡
//!   只判 mv 这一句的**归属**（前缀 + 目标路径 + 尾巴），文案内容归文案卡。
//! * **与 F11696（释放后重试恢复）分层**：本卡到"回滚完成、退出 1"为止；
//!   清空间后重跑是否成功、数据是否完整，是恢复面卡的正判据，本卡不许代判。
//! * **回滚 vs 未创建**：`Dest` 从 `Absent` 出发、写失败后仍是 `Absent`，与
//!   `Partial{n}` 被删回 `Absent`，在**终态**上同形，但在事故面不同。本卡用
//!   `rollback` 位把两者分开记，终态判据只认"绝不 Left-Partial"，不认过程。
//! * **红档率钉在 929**：与全批统一，`VALS` 第八格是 929；它标定"14 格里
//!   恰好中 13 格"的千分率，使注入一格红之后的结算立刻露出，不被整除掩盖。
//!
//! 探针一律在 `/tmp/bx-sandbox/` 内建模；本卡从不写宿主真实库路径。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 本卡段基址（全仓唯一，VX5 分配表给定，不许改）。
pub const SEG_BASE: u32 = 0x6d330;

// ---------------------------------------------------------------------------
// 联签锚：本卡与哪些卡共用同一条上游事实
// ---------------------------------------------------------------------------

/// 上游卡 F11686（sed -i 原件保护）的段基址：同批 B25 的前一张。
pub const ANCHOR_F11686_SEG: u32 = 0x6d320;
/// F11686 与本卡的段差（0x6d330 - 0x6d320 = 0x10）。
pub const ANCHOR_F11686_GAP: u32 = 0x10;

/// 上游卡 F11685（sort 临时文件）的段基址：临时文件回滚的母版。
pub const ANCHOR_F11685_SEG: u32 = 0x6d310;
/// F11685 与本卡的段差（0x6d330 - 0x6d310 = 0x20）。
pub const ANCHOR_F11685_GAP: u32 = 0x20;

/// 文案卡 F11694（No space left on device 逐字）的段基址。
pub const ANCHOR_F11694_SEG: u32 = 0x6d3d0;
/// F11694 与本卡的段差（0x6d3d0 - 0x6d330 = 0xa0）。
pub const ANCHOR_F11694_GAP: u32 = 0xa0;

/// 双机快照卡 F11699 的段基址：本卡的结果要进它的一致率分母。
pub const ANCHOR_F11699_SEG: u32 = 0x6d430;
/// F11699 与本卡的段差（0x6d430 - 0x6d330 = 0x100）。
pub const ANCHOR_F11699_GAP: u32 = 0x100;

/// 上游 errno：`ENOENT`——源不存在。
pub const ANCHOR_ENOENT: u32 = 2;
/// 上游 errno：`EACCES`——源不可读或目标目录不可写。
pub const ANCHOR_EACCES: u32 = 13;
/// 上游 errno：`EXDEV`——跨设备，rename 的固定回执（F11690 的正判据）。
pub const ANCHOR_EXDEV: u32 = 18;
/// 上游 errno：`ENOSPC`——本卡的靶心错误，Linux 固定为 28。
pub const ANCHOR_ENOSPC: u32 = 28;
/// strerror(28) 的逐字文本（与 F11694 同源）。
pub const ANCHOR_ENOSPC_TEXT: &str = "No space left on device";

// ---------------------------------------------------------------------------
// 规模常量（契约给定，逐卡一致）
// ---------------------------------------------------------------------------

/// 语料槽数。
pub const CORPUS: usize = 14;
/// 账本槽数。
pub const SAMPLES: usize = 14;
/// 正例槽数。
pub const ACCEPT: usize = 8;
/// 反例槽数。
pub const REJECT: usize = 6;
/// 矩阵档位数。
pub const TIER_COUNT: usize = 4;
/// 闸门数（九道）。
pub const STAGE_COUNT: u32 = 9;
/// 最末闸的秩。
pub const STAGE_LAST_RANK: u32 = 9;
/// 账本统计格数。
pub const STAT_CELLS: usize = 7;
/// 红档率千分比（全批统一钉值，14 格中 13 格的千分率）。
pub const RED_RATE: u32 = 929;
/// 满一致千分比。
pub const RATE_FULL: u32 = 1000;
/// 族内场景数：六条回滚 + 六条完好。
pub const FAMILY_SCENES: usize = 12;
/// 回滚场景数（ENOSPC 正确收场）。
pub const ROLLBACK_SCENES: usize = 6;
/// 完好场景数（完整复制 / 同盘 / 预存保留）。
pub const INTACT_SCENES: usize = 6;
/// 统计格对应的算子号。
pub const STAT_OPS: [u8; STAT_CELLS] = [1, 2, 3, 4, 5, 6, 7];
/// 红档格在账本里的算子号。
pub const OPID_STAT_RED: u8 = 0x08;
/// 六个缺陷码格的算子号起点。
pub const DEFECT_OP_BASE: u8 = 0x21;

// ---------------------------------------------------------------------------
// 磁盘与文案侧常量
// ---------------------------------------------------------------------------

/// 一个文件系统块的字节数（本卡所有规模都按块对齐建模）。
pub const BLOCK: u32 = 4096;
/// 半块：用来造"部分复制"的最小单位。
pub const HALF_BLOCK: u32 = 2048;
/// 沙盒前缀——本卡所有探针现场都在这里建模。
pub const SANDBOX_PREFIX: &str = "/tmp/bx-sandbox/";
/// 探针源相对路径。
pub const SRC_REL: &str = "a/blob";
/// 探针目的相对路径。
pub const DST_REL: &str = "b/blob";
/// 退出码：成功。
pub const EXIT_OK: u32 = 0;
/// 退出码：运行期失败（含已正确回滚的 ENOSPC）。
pub const EXIT_FAIL: u32 = 1;
/// 退出码：用法错。
pub const EXIT_USAGE: u32 = 2;
/// 未判档：本卡绝不允许把它当结论交付。
pub const EXIT_UNJUDGED: u32 = 99;
/// 退出码档位数。
pub const EXIT_RANKS: usize = 4;
/// 重复实现一次满一致，做交叉校验。
pub const MATCH_FULL: u32 = 1000;

// ---------------------------------------------------------------------------
// 十三错误码（前缀 MV = move）
// ---------------------------------------------------------------------------

/// 01 槽位/用法：注入槽越界、账本跳号、命令行操作数缺失。
pub const MV_SLOT: u32 = SEG_BASE + 0x01;
/// 02 成败互报：源已删却报成功，或回滚完成却报失败为成功（Estate 维出口）。
pub const MV_ESTATE: u32 = SEG_BASE + 0x02;
/// 03 阶段不合：回滚发生点与申报的阶段不一致（Stage 维出口）。
pub const MV_STAGE: u32 = SEG_BASE + 0x03;
/// 04 码档不合：把 `MV_ROLLBACK` 的现场申报成无码（Code 维出口）。
pub const MV_CODE: u32 = SEG_BASE + 0x04;
/// 05 数值不合：已复制字节数虚报（Value 维出口）。
pub const MV_VALUE: u32 = SEG_BASE + 0x05;
/// 06 注记不合：预存保留申报成回滚，或回滚申报成保留（Note 维出口）。
pub const MV_NOTE: u32 = SEG_BASE + 0x06;
/// 07 兜底档：本可判定的现场被丢进未判档（Default 维出口）。
pub const MV_DEFAULT: u32 = SEG_BASE + 0x07;
/// 08 指纹不符：账本折叠前后不一致，或目的地状态迁移非法。
pub const MV_SEAL: u32 = SEG_BASE + 0x08;
/// 09 矩阵覆盖缺口：四档里有档位没有任何落点。
pub const MV_MATRIX: u32 = SEG_BASE + 0x09;
/// 10 一致率不足：族内场景未全绿。
pub const MV_RATE: u32 = SEG_BASE + 0x0a;
/// 11 回滚违约：目的侧留下 `Partial{n}`——本卡的头号红。
pub const MV_ROLLBACK: u32 = SEG_BASE + 0x0b;
/// 12 旧物被毁：预存目标在失败路径上被截断/覆盖。
pub const MV_CLOBBER: u32 = SEG_BASE + 0x0c;
/// 13 源件丢失：失败路径上源被删或被改写。
pub const MV_SOURCE: u32 = SEG_BASE + 0x0d;

/// 十三码全表，交叉校验用。
pub const ALL_CODES: [u32; 13] = [
    MV_SLOT,
    MV_ESTATE,
    MV_STAGE,
    MV_CODE,
    MV_VALUE,
    MV_NOTE,
    MV_DEFAULT,
    MV_SEAL,
    MV_MATRIX,
    MV_RATE,
    MV_ROLLBACK,
    MV_CLOBBER,
    MV_SOURCE,
];

/// 反例六维对应的码（申报对判用，次序固定 Estate→…→Default）。
pub const REJECT_CODES: [u32; REJECT] = [
    MV_ESTATE,
    MV_STAGE,
    MV_CODE,
    MV_VALUE,
    MV_NOTE,
    MV_DEFAULT,
];

/// 正例八格统计数：族场景 / 回滚 / 完好 / 预存保留 / 文案逐字 / 残留 / 一致率 /
/// 红档率（第八格必须是 `RED_RATE`）。
pub const VALS: [u32; ACCEPT] = [12, 6, 6, 1, 12, 0, 1000, 929];

// ---------------------------------------------------------------------------
// FNV-1a 指纹
// ---------------------------------------------------------------------------

/// FNV-1a 初始偏移量。
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 素数。
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折一字节进哈希。
fn fold_byte(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(FNV_PRIME)
}

/// 折一个 u64（按小端逐字节展开，不受宿主字节序影响）。
fn fold_word(h: u64, w: u64) -> u64 {
    let mut v = w;
    let mut acc = h;
    let mut i = 0usize;
    while i < 8 {
        let b = (v & 0xff) as u8;
        acc = fold_byte(acc, b);
        v = v >> 8;
        i += 1;
    }
    acc
}

/// 折一条字符串。
fn fold_str(h: u64, s: &str) -> u64 {
    let mut acc = h;
    for b in s.as_bytes() {
        acc = fold_byte(acc, *b);
    }
    acc
}

/// 折一个 i32（errno 走这条）。
fn fold_i32(h: u64, v: i32) -> u64 {
    fold_word(h, v as u32 as u64)
}

/// 折一个布尔。
fn fold_bool(h: u64, v: bool) -> u64 {
    fold_byte(h, if v { 1 } else { 0 })
}

// ---------------------------------------------------------------------------
// 数字/布尔文本（不递归、不格式化宏）
// ---------------------------------------------------------------------------

/// u32 → 十进制文本。
fn dec_text(v: u32) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 10];
    let mut n = v;
    let mut i = 10usize;
    while n > 0 && i > 0 {
        let d = (n % 10) as u8;
        n = n / 10;
        i -= 1;
        buf[i] = b'0' + d;
    }
    let mut s = String::new();
    while i < 10 {
        if let Some(c) = buf.get(i) {
            s.push(*c as char);
        }
        i += 1;
    }
    s
}

/// u32 → 固定宽度十六进制文本（高位补 0，宽度上限 8）。
fn hex_text(v: u32, width: usize) -> String {
    let digits = b"0123456789abcdef";
    let mut s = String::new();
    let w = if width > 8 { 8 } else { width };
    let mut i = w;
    while i > 0 {
        let shift = (i - 1) * 4;
        let ndx = ((v >> shift) & 0xf) as usize;
        match digits.get(ndx) {
            Some(c) => s.push(*c as char),
            None => s.push('0'),
        }
        i -= 1;
    }
    s
}

/// u32 → 文本（与 `dec_text` 同口径，单独命名处供调用方自述意图）。
pub fn u32_text(v: u32) -> String {
    dec_text(v)
}

/// i32 → 文本（负数带 `-`，不依赖格式化宏）。
fn i32_text(v: i32) -> String {
    if v < 0 {
        let mut s = String::from("-");
        let mag = (v as i64).unsigned_abs();
        s.push_str(dec_text(mag as u32).as_str());
        s
    } else {
        dec_text(v as u32)
    }
}

/// 布尔 → 文本。
fn bool_text(v: bool) -> String {
    if v {
        String::from("true")
    } else {
        String::from("false")
    }
}

/// 段址 → `0x` 十六进制文本。
pub fn seg_text(v: u32) -> String {
    let mut s = String::from("0x");
    s.push_str(hex_text(v, 6).as_str());
    s
}

/// 千分比 → 文本。
pub fn permille_text(v: u32) -> String {
    let mut s = dec_text(v);
    s.push_str("permille");
    s
}

// ---------------------------------------------------------------------------
// 逐字文案（GNU coreutils 与 busybox 两式）
// ---------------------------------------------------------------------------

/// GNU mv 的 ENOSPC 前缀（`cannot create regular file`）。
pub const T_GNU_PREFIX: &str = "mv: cannot create regular file '";
/// busybox mv 的 ENOSPC 前缀（`can't create`）。
pub const T_BB_PREFIX: &str = "mv: can't create '";
/// 引号后半段。
pub const T_QUOTE_TAIL: &str = "': ";
/// 源删除失败（不属于本卡靶心，但注记面要能认）。
pub const T_REMOVE_PREFIX: &str = "mv: cannot remove '";
/// 复制完成但校验短尾（完整性问题）。
pub const T_SHORT_PREFIX: &str = "mv: error writing '";
/// 空串常量。
pub const TEXT_EMPTY: &str = "";

/// 拼一条 `前缀 + 路径 + 尾巴` 的报错文本。
pub fn join_text(prefix: &str, path: &str, tail: &str) -> String {
    let mut s = String::from(prefix);
    s.push_str(path);
    s.push_str(tail);
    s
}

/// GNU mv 的 ENOSPC 原文。
pub fn msg_mv_enospc_gnu(dst: &str) -> String {
    join_text(T_GNU_PREFIX, dst, T_QUOTE_TAIL)
}

/// 给上面那句补上 errno 文本，得到完整一行。
pub fn msg_mv_enospc(dst: &str) -> String {
    let mut s = msg_mv_enospc_gnu(dst);
    s.push_str(ANCHOR_ENOSPC_TEXT);
    s
}

/// busybox mv 的 ENOSPC 原文。
pub fn msg_mv_enospc_busybox(dst: &str) -> String {
    let mut s = join_text(T_BB_PREFIX, dst, T_QUOTE_TAIL);
    s.push_str(ANCHOR_ENOSPC_TEXT);
    s
}

/// 源删除失败原文。
pub fn msg_mv_remove(src: &str) -> String {
    let mut s = join_text(T_REMOVE_PREFIX, src, T_QUOTE_TAIL);
    s.push_str("Permission denied");
    s
}

/// 写面短尾原文（目的侧写不足）。
pub fn msg_mv_short(dst: &str) -> String {
    let mut s = join_text(T_SHORT_PREFIX, dst, T_QUOTE_TAIL);
    s.push_str(ANCHOR_ENOSPC_TEXT);
    s
}

/// 用法错原文（缺操作数）。
pub fn msg_usage_missing() -> String {
    String::from("mv: missing destination file operand after 'a/blob'")
}

/// 用法错原文（多余操作数）。
pub fn msg_usage_extra() -> String {
    String::from("mv: target 'b/blob' is not a directory")
}

/// 回滚成功的呈现行。
pub fn msg_rollback(dst: &str) -> String {
    let mut s = String::from("rolled back partial ");
    s.push_str(dst);
    s
}

/// 预存目标幸存的呈现行。
pub fn msg_kept(dst: &str) -> String {
    let mut s = String::from("kept pre-existing ");
    s.push_str(dst);
    s
}

/// 半成品残留的呈现行（本卡的红）。
pub fn msg_residue(dst: &str, bytes: u32) -> String {
    let mut s = String::from("left partial ");
    s.push_str(dst);
    s.push(' ');
    s.push_str(dec_text(bytes).as_str());
    s.push_str(" bytes");
    s
}

/// 旧物被毁的呈现行。
pub fn msg_clobbered(dst: &str) -> String {
    let mut s = String::from("clobbered pre-existing ");
    s.push_str(dst);
    s
}

/// 源件丢失的呈现行。
pub fn msg_source_lost(src: &str) -> String {
    let mut s = String::from("lost source ");
    s.push_str(src);
    s
}

// ---------------------------------------------------------------------------
// 闸门：九道
// ---------------------------------------------------------------------------

/// 本卡的九道闸。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// 秩 1：命令行成立。
    Check,
    /// 秩 2：源可读。
    Source,
    /// 秩 3：同盘/跨盘判定。
    Scope,
    /// 秩 4：目的地现状。
    Dest,
    /// 秩 5：建目的文件并复制。
    Copy,
    /// 秩 6：写面结算（字节数 + errno）。
    Write,
    /// 秩 7：完整性校验。
    Verify,
    /// 秩 8：删源（事务提交点）。
    Unlink,
    /// 秩 9：退出码与文案。
    Exit,
}

impl Stage {
    /// 九道闸全表（正序）。
    pub const ALL: [Stage; 9] = [
        Stage::Check,
        Stage::Source,
        Stage::Scope,
        Stage::Dest,
        Stage::Copy,
        Stage::Write,
        Stage::Verify,
        Stage::Unlink,
        Stage::Exit,
    ];

    /// 秩（1..=9）。
    pub fn rank(&self) -> u32 {
        match self {
            Stage::Check => 1,
            Stage::Source => 2,
            Stage::Scope => 3,
            Stage::Dest => 4,
            Stage::Copy => 5,
            Stage::Write => 6,
            Stage::Verify => 7,
            Stage::Unlink => 8,
            Stage::Exit => 9,
        }
    }

    /// 短名。
    pub fn slug(&self) -> &'static str {
        match self {
            Stage::Check => "check",
            Stage::Source => "source",
            Stage::Scope => "scope",
            Stage::Dest => "dest",
            Stage::Copy => "copy",
            Stage::Write => "write",
            Stage::Verify => "verify",
            Stage::Unlink => "unlink",
            Stage::Exit => "exit",
        }
    }

    /// 中文名（判据名与诊断共用）。
    pub fn cn(&self) -> &'static str {
        match self {
            Stage::Check => "命令行闸",
            Stage::Source => "源闸",
            Stage::Scope => "跨盘闸",
            Stage::Dest => "目的闸",
            Stage::Copy => "复制闸",
            Stage::Write => "写面闸",
            Stage::Verify => "校验闸",
            Stage::Unlink => "删源闸",
            Stage::Exit => "退出闸",
        }
    }

    /// 由秩还原（0 与 10 以上不合法）。
    pub fn from_rank(r: u32) -> Option<Stage> {
        match r {
            1 => Some(Stage::Check),
            2 => Some(Stage::Source),
            3 => Some(Stage::Scope),
            4 => Some(Stage::Dest),
            5 => Some(Stage::Copy),
            6 => Some(Stage::Write),
            7 => Some(Stage::Verify),
            8 => Some(Stage::Unlink),
            9 => Some(Stage::Exit),
            _ => None,
        }
    }
}

/// 盘间关系：跨盘复制 vs 同盘改名。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Route {
    /// 跨设备：copy-then-unlink。
    CrossDev,
    /// 同设备：rename(2)，元数据操作。
    SameDev,
}

impl Route {
    /// 短名。
    pub fn slug(&self) -> &'static str {
        match self {
            Route::CrossDev => "crossdev",
            Route::SameDev => "samedev",
        }
    }

    /// 是否复制数据（只有跨盘才复制）。
    pub fn copies_data(&self) -> bool {
        matches!(self, Route::CrossDev)
    }

    /// 该路线是否可能产生 ENOSPC（同盘 rename 不分配数据块）。
    pub fn can_enospc(&self) -> bool {
        matches!(self, Route::CrossDev)
    }
}

/// 目的地状态机：不存在 → 部分 → 完整；失败时部分必须退回不存在。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Destination {
    /// 目的地不存在（合法的初始态，也是回滚后的终态）。
    Absent,
    /// 已经写进去 `n` 字节的半成品（本卡禁止它成为终态）。
    Partial(u32),
    /// 完整落盘 `n` 字节。
    Complete(u32),
}

impl Destination {
    /// 已承载的字节数。
    pub fn bytes(&self) -> u32 {
        match *self {
            Destination::Absent => 0,
            Destination::Partial(n) => n,
            Destination::Complete(n) => n,
        }
    }

    /// 是否不存在。
    pub fn is_absent(&self) -> bool {
        matches!(*self, Destination::Absent)
    }

    /// 是否半成品。
    pub fn is_partial(&self) -> bool {
        matches!(*self, Destination::Partial(_))
    }

    /// 是否完整。
    pub fn is_complete(&self) -> bool {
        matches!(*self, Destination::Complete(_))
    }

    /// 状态标签（不带字节数）。
    pub fn tag(&self) -> &'static str {
        match *self {
            Destination::Absent => "absent",
            Destination::Partial(_) => "partial",
            Destination::Complete(_) => "complete",
        }
    }

    /// 是否是从该状态出发的合法迁移：
    /// Absent→Partial/Complete，Partial→Partial/Complete/Absent（回滚），
    /// Complete→Complete/Absent（被删）。其余一概非法。
    pub fn may_step_to(&self, next: Destination) -> bool {
        match (*self, next) {
            (Destination::Absent, Destination::Absent) => true,
            (Destination::Absent, Destination::Partial(_)) => true,
            (Destination::Absent, Destination::Complete(_)) => true,
            (Destination::Partial(_), Destination::Partial(_)) => true,
            (Destination::Partial(_), Destination::Complete(_)) => true,
            (Destination::Partial(_), Destination::Absent) => true,
            (Destination::Complete(_), Destination::Complete(_)) => true,
            (Destination::Complete(_), Destination::Absent) => true,
            _ => false,
        }
    }

    /// 折叠进指纹。
    pub fn fold(&self, h: u64) -> u64 {
        let x = fold_str(h, self.tag());
        fold_word(x, self.bytes() as u64)
    }
}

/// 注记：一条现场最终落在哪一种收场。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Note {
    /// 完整复制成功（目的侧全新）。
    Copied,
    /// ENOSPC 后把半成品删掉，源保留（本卡的正例靶心）。
    RolledBack,
    /// ENOSPC 后预存目标原样幸存。
    ExistingKept,
    /// 同盘 rename，根本不复制（边界档）。
    RenameNoCopy,
    /// 半成品留在目的侧（头号红）。
    PartialLeft,
    /// 预存目标被截断/毁掉。
    Clobbered,
    /// 源在失败路径上被删掉或改写。
    SourceLost,
    /// 用法错。
    Usage,
    /// 未判档。
    Nil,
}

impl Note {
    /// 短名。
    pub fn slug(&self) -> &'static str {
        match self {
            Note::Copied => "copied",
            Note::RolledBack => "rolled-back",
            Note::ExistingKept => "existing-kept",
            Note::RenameNoCopy => "rename-no-copy",
            Note::PartialLeft => "partial-left",
            Note::Clobbered => "clobbered",
            Note::SourceLost => "source-lost",
            Note::Usage => "usage",
            Note::Nil => "nil",
        }
    }

    /// 中文名。
    pub fn cn(&self) -> &'static str {
        match self {
            Note::Copied => "完整复制",
            Note::RolledBack => "半成品回滚",
            Note::ExistingKept => "预存保留",
            Note::RenameNoCopy => "同盘改名",
            Note::PartialLeft => "半成品残留",
            Note::Clobbered => "旧物被毁",
            Note::SourceLost => "源件丢失",
            Note::Usage => "用法错",
            Note::Nil => "未判",
        }
    }

    /// 次序（用于指纹与表内比较）。
    pub fn rank(&self) -> u32 {
        match self {
            Note::Copied => 1,
            Note::RolledBack => 2,
            Note::ExistingKept => 3,
            Note::RenameNoCopy => 4,
            Note::PartialLeft => 5,
            Note::Clobbered => 6,
            Note::SourceLost => 7,
            Note::Usage => 8,
            Note::Nil => 9,
        }
    }

    /// 由次序还原。
    pub fn from_rank(r: u32) -> Option<Note> {
        match r {
            1 => Some(Note::Copied),
            2 => Some(Note::RolledBack),
            3 => Some(Note::ExistingKept),
            4 => Some(Note::RenameNoCopy),
            5 => Some(Note::PartialLeft),
            6 => Some(Note::Clobbered),
            7 => Some(Note::SourceLost),
            8 => Some(Note::Usage),
            9 => Some(Note::Nil),
            _ => None,
        }
    }

    /// 该注记是否保住了源与旧物（本卡的"干净收场"）。
    pub fn is_clean(&self) -> bool {
        match self {
            Note::Copied | Note::RolledBack | Note::ExistingKept | Note::RenameNoCopy => true,
            _ => false,
        }
    }

    /// 该注记是否是一次回滚（含预存保留）。
    pub fn is_rollback(&self) -> bool {
        matches!(self, Note::RolledBack | Note::ExistingKept)
    }
}

// ---------------------------------------------------------------------------
// 判定维
// ---------------------------------------------------------------------------

/// 判定维序（judge 的六维 + Ok）。次序固定：
/// Estate → Stage → Code → Value → Note → Default → Ok。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fault {
    /// 全合。
    Ok,
    /// 成败互报（最贵：源已丢却报成功，或回滚完成却被报成成功）。
    Estate,
    /// 阶段不合。
    Stage,
    /// 码档不合。
    Code,
    /// 已复制字节数不合。
    Value,
    /// 注记不合。
    Note,
    /// 兜底档不合。
    Default,
}

impl Fault {
    /// 七格全表（含 Ok）。
    pub const ALL: [Fault; 7] = [
        Fault::Ok,
        Fault::Estate,
        Fault::Stage,
        Fault::Code,
        Fault::Value,
        Fault::Note,
        Fault::Default,
    ];

    /// 秩（Ok = 0）。
    pub fn rank(&self) -> u32 {
        match self {
            Fault::Ok => 0,
            Fault::Estate => 1,
            Fault::Stage => 2,
            Fault::Code => 3,
            Fault::Value => 4,
            Fault::Note => 5,
            Fault::Default => 6,
        }
    }

    /// 短名。
    pub fn slug(&self) -> &'static str {
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

    /// 由秩还原。
    pub fn from_rank(r: u32) -> Option<Fault> {
        match r {
            0 => Some(Fault::Ok),
            1 => Some(Fault::Estate),
            2 => Some(Fault::Stage),
            3 => Some(Fault::Code),
            4 => Some(Fault::Value),
            5 => Some(Fault::Note),
            6 => Some(Fault::Default),
            _ => None,
        }
    }

    /// 本缺陷维对应的错误码（judge 命中维的出口）。
    pub fn code(&self) -> u32 {
        match self {
            Fault::Ok => 0,
            Fault::Estate => MV_ESTATE,
            Fault::Stage => MV_STAGE,
            Fault::Code => MV_CODE,
            Fault::Value => MV_VALUE,
            Fault::Note => MV_NOTE,
            Fault::Default => MV_DEFAULT,
        }
    }

    /// 反例六维（固定次序）。
    pub fn reject_faults() -> [Fault; REJECT] {
        [
            Fault::Estate,
            Fault::Stage,
            Fault::Code,
            Fault::Value,
            Fault::Note,
            Fault::Default,
        ]
    }

    /// 是否真缺陷。
    pub fn is_fault(&self) -> bool {
        !matches!(self, Fault::Ok)
    }

    /// 一句话诊断。
    pub fn why(&self) -> &'static str {
        match self {
            Fault::Ok => "六维全合",
            Fault::Estate => "失败被报成成功，或成功被报成失败",
            Fault::Stage => "回滚发生在写面/删源闸，却申报成别的阶段",
            Fault::Code => "半成品残留的现场被申报成无码",
            Fault::Value => "已复制字节数虚报",
            Fault::Note => "预存保留与回滚两种收场互认",
            Fault::Default => "可判定的现场被丢进未判档",
        }
    }
}

/// 反例六维表（与 `REJECT_CODES` 一一对应）。
pub const REJECT_FAULTS: [Fault; REJECT] = [
    Fault::Estate,
    Fault::Stage,
    Fault::Code,
    Fault::Value,
    Fault::Note,
    Fault::Default,
];

// ---------------------------------------------------------------------------
// 四档：回滚 / 完好 / 残留 / 违诺
// ---------------------------------------------------------------------------

/// 目的地收场四档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    /// 回滚完成：半成品已删，源保留。
    Rollback,
    /// 现场完好：完整复制、同盘改名、预存保留。
    Intact,
    /// 残留：半成品留在目的侧（红）。
    Residue,
    /// 违诺：旧物被毁、源丢失、用法错（红）。
    Breach,
}

impl Tier {
    /// 四档全表。
    pub const ALL: [Tier; 4] = [Tier::Rollback, Tier::Intact, Tier::Residue, Tier::Breach];

    /// 秩（1..=4）。
    pub fn rank(&self) -> u32 {
        match self {
            Tier::Rollback => 1,
            Tier::Intact => 2,
            Tier::Residue => 3,
            Tier::Breach => 4,
        }
    }

    /// 短名。
    pub fn slug(&self) -> &'static str {
        match self {
            Tier::Rollback => "rollback",
            Tier::Intact => "intact",
            Tier::Residue => "residue",
            Tier::Breach => "breach",
        }
    }

    /// 中文名。
    pub fn cn(&self) -> &'static str {
        match self {
            Tier::Rollback => "回滚档",
            Tier::Intact => "完好档",
            Tier::Residue => "残留档",
            Tier::Breach => "违诺档",
        }
    }

    /// 由秩还原。
    pub fn from_rank(r: u32) -> Option<Tier> {
        match r {
            1 => Some(Tier::Rollback),
            2 => Some(Tier::Intact),
            3 => Some(Tier::Residue),
            4 => Some(Tier::Breach),
            _ => None,
        }
    }

    /// 绿档只有两档：回滚与完好。
    pub fn is_green(&self) -> bool {
        matches!(*self, Tier::Rollback | Tier::Intact)
    }

    /// 由一次结果归档。次序固定：先看兜底，再看残留，再看旧物/源，最后按注记。
    pub fn of(o: &Outcome) -> Tier {
        if o.default || o.note == Note::Nil || o.note == Note::Usage {
            return Tier::Breach;
        }
        if o.note == Note::PartialLeft {
            return Tier::Residue;
        }
        if o.note == Note::Clobbered || o.note == Note::SourceLost {
            return Tier::Breach;
        }
        if o.note == Note::RolledBack {
            return Tier::Rollback;
        }
        Tier::Intact
    }
}

// ---------------------------------------------------------------------------
// 域模型：Shape / Outcome / Claim / Case
// ---------------------------------------------------------------------------

/// 本卡输入的全部维度——一次 `mv` 的完整现场。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Shape {
    /// 盘间关系。
    pub route: Route,
    /// 源文件字节数。
    pub src_len: u32,
    /// 目的盘当前可用字节数。
    pub free: u32,
    /// 源可读（stat/open 成功）。
    pub src_ok: bool,
    /// 事故后源是否已不存在（观察位）。
    pub src_removed: bool,
    /// 操作前目的地状态。
    pub dest0: Destination,
    /// 操作后目的地状态（观察位）。
    pub dest1: Destination,
    /// 预存目标的内容是否幸存（dest0 为 Complete 时有意义）。
    pub preexisting_ok: bool,
    /// 命令行是否非法。
    pub bad_call: bool,
    /// 观察到的退出码。
    pub exit: u32,
    /// 兜底档标记。
    pub default: bool,
}

impl Shape {
    /// 一条干净的跨盘完整复制。
    pub fn clean(len: u32) -> Shape {
        Shape {
            route: Route::CrossDev,
            src_len: len,
            free: len,
            src_ok: true,
            src_removed: true,
            dest0: Destination::Absent,
            dest1: Destination::Complete(len),
            preexisting_ok: true,
            bad_call: false,
            exit: EXIT_OK,
            default: false,
        }
    }

    /// 一条跨盘 ENOSPC 且已正确回滚的现场。
    pub fn rollback(len: u32, free: u32) -> Shape {
        Shape {
            route: Route::CrossDev,
            src_len: len,
            free,
            src_ok: true,
            src_removed: false,
            dest0: Destination::Absent,
            dest1: Destination::Absent,
            preexisting_ok: true,
            bad_call: false,
            exit: EXIT_FAIL,
            default: false,
        }
    }

    /// 跨盘空间是否不足（只有跨盘且源长于可用空间才算）。
    pub fn enospc(&self) -> bool {
        self.route.copies_data() && self.src_len > self.free
    }

    /// 已复制的字节数：空间够就是全长，不够就是可用空间（写满即停）。
    pub fn copied(&self) -> u32 {
        if self.src_len <= self.free {
            self.src_len
        } else {
            self.free
        }
    }

    /// 源是否在事故里被错误删除（失败路径上删源）。
    pub fn source_at_risk(&self) -> bool {
        self.src_removed && (self.bad_call || self.enospc() || self.exit != EXIT_OK)
    }

    /// 目的终态是否留了半成品。
    pub fn left_partial(&self) -> bool {
        self.dest1.is_partial()
    }

    /// 预存目标是否被毁：只有**失败**路径才谈得上毁物；完整覆盖是合法行为。
    pub fn clobbered(&self) -> bool {
        self.dest0.is_complete() && !self.preexisting_ok && self.enospc()
    }

    /// 域合法性：状态迁移必须合法、字节数必须能闭合。
    pub fn domain_ok(&self) -> bool {
        if !self.dest0.may_step_to(self.dest1) {
            return false;
        }
        if self.src_len > BLOCK.saturating_mul(64) {
            return false;
        }
        if self.free > BLOCK.saturating_mul(64) {
            return false;
        }
        if self.bad_call {
            return self.exit == EXIT_USAGE && self.dest0 == self.dest1;
        }
        if self.route == Route::SameDev {
            // 同盘 rename 不复制数据，字节数关系不在本卡判。
            return true;
        }
        true
    }

    /// 形状指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_str(h, self.route.slug());
        h = fold_word(h, self.src_len as u64);
        h = fold_word(h, self.free as u64);
        h = fold_bool(h, self.src_ok);
        h = fold_bool(h, self.src_removed);
        h = self.dest0.fold(h);
        h = self.dest1.fold(h);
        h = fold_bool(h, self.preexisting_ok);
        h = fold_bool(h, self.bad_call);
        h = fold_word(h, self.exit as u64);
        h = fold_bool(h, self.default);
        h
    }
}

/// 事实面（判据的应然结论）。五维与 `Claim` 一一对应，外加一个显式成败位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Outcome {
    /// 纵线落点阶段。
    pub stage: Stage,
    /// 归因码（0 = 现场没有违诺；非 0 = 本卡某条不变量被破）。
    pub code: u32,
    /// 已复制字节数。
    pub value: u32,
    /// 收场注记。
    pub note: Note,
    /// 兜底档。
    pub default: bool,
    /// 命令本身是否成功（退出码 0）。回滚正确但是失败，这一位必须是 false。
    pub ok: bool,
}

impl Outcome {
    /// 构造。
    pub const fn new(
        stage: Stage,
        code: u32,
        value: u32,
        note: Note,
        default: bool,
        ok: bool,
    ) -> Outcome {
        Outcome {
            stage,
            code,
            value,
            note,
            default,
            ok,
        }
    }

    /// 成功收场（退出 0）。
    fn pass(stage: Stage, value: u32, note: Note) -> Outcome {
        Outcome::new(stage, 0, value, note, false, true)
    }

    /// 失败收场（退出非 0 且现场无违诺），例如正确回滚。
    fn fail_clean(stage: Stage, value: u32, note: Note) -> Outcome {
        Outcome::new(stage, 0, value, note, false, false)
    }

    /// 违诺收场：现场破了本卡的不变量。
    fn breach(stage: Stage, code: u32, value: u32, note: Note, default: bool) -> Outcome {
        Outcome::new(stage, code, value, note, default, false)
    }

    /// 是否成功。
    pub fn ok(&self) -> bool {
        self.ok
    }

    /// 是否失败。
    pub fn failed(&self) -> bool {
        !self.ok
    }

    /// 是否发生了回滚。
    pub fn rolled_back(&self) -> bool {
        self.note.is_rollback()
    }

    /// 是否破了不变量。
    pub fn breached(&self) -> bool {
        self.code != 0
    }

    /// 折叠六维进指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.stage.rank() as u64);
        h = fold_word(h, self.code as u64);
        h = fold_word(h, self.value as u64);
        h = fold_str(h, self.note.slug());
        h = fold_bool(h, self.default);
        h = fold_bool(h, self.ok);
        h
    }

    /// 一行渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("stage=");
        s.push_str(self.stage.slug());
        s.push_str(" code=");
        s.push_str(seg_text(self.code).as_str());
        s.push_str(" copied=");
        s.push_str(dec_text(self.value).as_str());
        s.push_str(" note=");
        s.push_str(self.note.slug());
        s.push_str(" ok=");
        s.push_str(bool_text(self.ok).as_str());
        s
    }
}

/// 申报面（被复核的那份结论）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Claim {
    /// 反例槽的缺陷标签（正例也带，便于逐槽指认）。
    pub tag: &'static str,
    /// 申报的成败。
    pub success: bool,
    /// 申报的落点阶段。
    pub stage: Stage,
    /// 申报的归因码。
    pub code: u32,
    /// 申报的已复制字节数。
    pub value: u32,
    /// 申报的收场注记。
    pub note: Note,
    /// 申报的兜底位。
    pub default: bool,
}

impl Claim {
    /// 构造。
    pub const fn new(
        tag: &'static str,
        success: bool,
        stage: Stage,
        code: u32,
        value: u32,
        note: Note,
        default: bool,
    ) -> Claim {
        Claim {
            tag,
            success,
            stage,
            code,
            value,
            note,
            default,
        }
    }

    /// 申报的成败位。
    pub fn success(&self) -> bool {
        self.success
    }

    /// 由事实面照抄一份申报（活槽用；死槽在此之后只翻一维）。
    pub fn from_outcome(actual: &Outcome, success: bool, tag: &'static str) -> Claim {
        Claim::new(
            tag,
            success,
            actual.stage,
            actual.code,
            actual.value,
            actual.note,
            actual.default,
        )
    }

    /// 与事实面的六维差异掩码（bit0=Estate…bit5=Default）。
    pub fn diff_mask(&self, actual: &Outcome) -> u32 {
        let mut m: u32 = 0;
        if actual.ok() != self.success {
            m |= 0x01;
        }
        if actual.stage != self.stage {
            m |= 0x02;
        }
        if actual.code != self.code {
            m |= 0x04;
        }
        if actual.value != self.value {
            m |= 0x08;
        }
        if actual.note != self.note {
            m |= 0x10;
        }
        if actual.default != self.default {
            m |= 0x20;
        }
        m
    }

    /// 折叠申报七维。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_str(h, self.tag);
        h = fold_bool(h, self.success);
        h = fold_word(h, self.stage.rank() as u64);
        h = fold_word(h, self.code as u64);
        h = fold_word(h, self.value as u64);
        h = fold_str(h, self.note.slug());
        h = fold_bool(h, self.default);
        h
    }

    /// 一行渲染。
    pub fn render(&self) -> String {
        let mut s = String::from(self.tag);
        s.push_str(" ok=");
        s.push_str(bool_text(self.success).as_str());
        s.push(' ');
        s.push_str(self.stage.slug());
        s.push(' ');
        s.push_str(seg_text(self.code).as_str());
        s.push(' ');
        s.push_str(dec_text(self.value).as_str());
        s.push(' ');
        s.push_str(self.note.slug());
        s
    }
}

/// 一条语料：现场 + 申报。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Case {
    /// 现场。
    pub shape: Shape,
    /// 申报。
    pub claim: Claim,
}

impl Case {
    /// 构造。
    pub const fn new(shape: Shape, claim: Claim) -> Case {
        Case { shape, claim }
    }

    /// 真实结果。
    pub fn actual(&self) -> Outcome {
        define(&self.shape)
    }

    /// 缺陷标签。
    pub fn tag(&self) -> &'static str {
        self.claim.tag
    }

    /// 判定。
    pub fn fault(&self) -> Fault {
        judge(self)
    }

    /// 是否合格。
    pub fn ok(&self) -> bool {
        self.fault() == Fault::Ok
    }

    /// 语料指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.shape.seal());
        h = fold_word(h, self.claim.seal());
        h
    }
}

// ---------------------------------------------------------------------------
// 主判据纵线：一次 mv 如何一步步走到收场
// ---------------------------------------------------------------------------

/// 主判据纵线（Check → Source → Scope → Dest → Copy → Write → Verify →
/// Unlink → Exit）。
///
/// 顺序不可调。任何一闸的"过"只表示这一维没坏：Dest 不知道目的地原先有没有
/// 东西，就没法判旧物是否被毁；Verify 不先走，Unlink 就不许发生；写面没结算，
/// 退出码就是凭空编的。
pub fn define(s: &Shape) -> Outcome {
    // 1 Check：用法错一字节都不读。
    if s.bad_call {
        return Outcome::breach(Stage::Check, MV_SLOT, 0, Note::Usage, false);
    }
    // 2 Source：源打不开时"源保留"是空命题。
    if !s.src_ok {
        return Outcome::breach(Stage::Source, MV_SOURCE, 0, Note::SourceLost, false);
    }
    // 3 Scope：同盘走 rename，不复制数据，也不会 ENOSPC。
    if s.route == Route::SameDev {
        if s.exit == EXIT_OK && s.dest1.is_complete() {
            return Outcome::pass(Stage::Unlink, 0, Note::RenameNoCopy);
        }
        return Outcome::breach(Stage::Scope, MV_STAGE, 0, Note::Nil, s.default);
    }
    // 4 Dest：预存目标在失败路径上被毁，先于写面结算判死。
    if s.clobbered() {
        return Outcome::breach(Stage::Write, MV_CLOBBER, s.copied(), Note::Clobbered, s.default);
    }
    // 5 Copy / 6 Write：失败路径上删源是本卡最贵的伪造。
    if s.source_at_risk() {
        return Outcome::breach(Stage::Unlink, MV_SOURCE, s.copied(), Note::SourceLost, s.default);
    }
    // 兜底档：本可判定的现场被丢进未判。
    if s.default {
        return Outcome::new(Stage::Exit, 0, s.copied(), Note::Nil, true, false);
    }
    // 空间够：必须完整落盘且字节数对齐，才允许判"完整复制"。
    if !s.enospc() {
        if s.exit == EXIT_OK && s.dest1.is_complete() && s.dest1.bytes() == s.src_len {
            return Outcome::pass(Stage::Verify, s.src_len, Note::Copied);
        }
        return Outcome::breach(Stage::Verify, MV_STAGE, s.copied(), Note::Nil, false);
    }
    // 7 Verify / 8 Unlink / 9 Exit：ENOSPC 路径。
    if s.exit != EXIT_FAIL {
        // 写满盘却报成功（或报用法错），退出码与事实不符。
        return Outcome::breach(Stage::Exit, MV_STAGE, s.copied(), Note::Nil, false);
    }
    if s.dest1.is_partial() {
        // 头号红：半成品留在用户指定路径上。
        return Outcome::breach(Stage::Write, MV_ROLLBACK, s.copied(), Note::PartialLeft, false);
    }
    if s.dest0.is_complete() {
        if s.preexisting_ok && s.dest1.is_complete() && s.dest1.bytes() == s.dest0.bytes() {
            // 预存目标原样幸存：回滚正确，退出 1。
            return Outcome::fail_clean(Stage::Exit, s.copied(), Note::ExistingKept);
        }
        return Outcome::breach(Stage::Write, MV_CLOBBER, s.copied(), Note::Clobbered, false);
    }
    if s.dest1.is_absent() {
        // 半成品已删、源未动：本卡的靶心正例。
        return Outcome::fail_clean(Stage::Exit, s.copied(), Note::RolledBack);
    }
    Outcome::breach(Stage::Verify, MV_ROLLBACK, s.copied(), Note::Nil, false)
}

/// 缺陷维 → 序位。
pub fn fault_ndx(f: Fault) -> usize {
    match f {
        Fault::Estate => 0,
        Fault::Stage => 1,
        Fault::Code => 2,
        Fault::Value => 3,
        Fault::Note => 4,
        Fault::Default => 5,
        Fault::Ok => 6,
    }
}

/// 序位 → 缺陷维。
pub fn ndx_fault(n: usize) -> Option<Fault> {
    match n {
        0 => Some(Fault::Estate),
        1 => Some(Fault::Stage),
        2 => Some(Fault::Code),
        3 => Some(Fault::Value),
        4 => Some(Fault::Note),
        5 => Some(Fault::Default),
        6 => Some(Fault::Ok),
        _ => None,
    }
}

/// 申报对判——固定次序 Estate → Stage → Code → Value → Note → Default → Ok。
///
/// Estate 最先：一条"源已经丢了"的现场被报成成功，比阶段记错严重得多；成败
/// 互报意味着后五维再准也没有意义。Default 双向判死：现场真落未判档不行，
/// 把可判定的现场谎报成未判也不行。
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
    if a.default || a.default != k.default {
        return Fault::Default;
    }
    Fault::Ok
}

/// 语料取槽。
pub fn case_at(i: usize) -> Option<Case> {
    match (SHAPES.get(i), CLAIMS.get(i)) {
        (Some(s), Some(c)) => Some(Case::new(*s, *c)),
        _ => None,
    }
}

/// 全语料（缺槽处用基型占位，绝不越界）。
pub fn cases() -> [Case; CORPUS] {
    let base_shape = match SHAPES.get(0) {
        Some(s) => *s,
        None => Shape::clean(0),
    };
    let base_claim = match CLAIMS.get(0) {
        Some(c) => *c,
        None => Claim::new("", false, Stage::Check, 0, 0, Note::Nil, false),
    };
    let mut out = [Case::new(base_shape, base_claim); CORPUS];
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
        return Err(MV_SLOT);
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

// ---------------------------------------------------------------------------
// 语料：14 槽 = 8 活 + 6 死
// ---------------------------------------------------------------------------

/// 槽 0 `mv-clean-fit`：4096 字节源，目的盘恰好 4096，完整复制。
const SHAPE_CLEAN_FIT: Shape = Shape {
    route: Route::CrossDev,
    src_len: BLOCK,
    free: BLOCK,
    src_ok: true,
    src_removed: true,
    dest0: Destination::Absent,
    dest1: Destination::Complete(BLOCK),
    preexisting_ok: true,
    bad_call: false,
    exit: EXIT_OK,
    default: false,
};

/// 槽 1 `mv-clean-slack`：8192 字节源，目的盘 12288，完整复制且有富余。
const SHAPE_CLEAN_SLACK: Shape = Shape {
    route: Route::CrossDev,
    src_len: BLOCK * 2,
    free: BLOCK * 3,
    src_ok: true,
    src_removed: true,
    dest0: Destination::Absent,
    dest1: Destination::Complete(BLOCK * 2),
    preexisting_ok: true,
    bad_call: false,
    exit: EXIT_OK,
    default: false,
};

/// 槽 2 `mv-overwrite-complete`：目的路径预存 512 字节，空间够，完整覆盖。
/// 注意：完整复制本来就要覆盖旧物，这一格不是"旧物被毁"。
const SHAPE_OVERWRITE_COMPLETE: Shape = Shape {
    route: Route::CrossDev,
    src_len: BLOCK,
    free: BLOCK,
    src_ok: true,
    src_removed: true,
    dest0: Destination::Complete(512),
    dest1: Destination::Complete(BLOCK),
    preexisting_ok: false,
    bad_call: false,
    exit: EXIT_OK,
    default: false,
};

/// 槽 3 `mv-enospc-rollback`：8192 字节源，目的盘 4096；写满后 ENOSPC，
/// 半成品被删（dst→Absent），源未动，退出 1。本卡靶心正例。
const SHAPE_ENOSPC_ROLLBACK: Shape = Shape {
    route: Route::CrossDev,
    src_len: BLOCK * 2,
    free: BLOCK,
    src_ok: true,
    src_removed: false,
    dest0: Destination::Absent,
    dest1: Destination::Absent,
    preexisting_ok: true,
    bad_call: false,
    exit: EXIT_FAIL,
    default: false,
};

/// 槽 4 `mv-enospc-keep-existing`：目的地预存 512 字节，空间不足；
/// 失败后旧内容原样幸存（字节数仍 512），退出 1。
const SHAPE_ENOSPC_KEEP: Shape = Shape {
    route: Route::CrossDev,
    src_len: BLOCK * 2,
    free: BLOCK,
    src_ok: true,
    src_removed: false,
    dest0: Destination::Complete(512),
    dest1: Destination::Complete(512),
    preexisting_ok: true,
    bad_call: false,
    exit: EXIT_FAIL,
    default: false,
};

/// 槽 5 `mv-enospc-zero`：目的盘 0 字节可用，一个字节都没写进去。
const SHAPE_ENOSPC_ZERO: Shape = Shape {
    route: Route::CrossDev,
    src_len: BLOCK * 2,
    free: 0,
    src_ok: true,
    src_removed: false,
    dest0: Destination::Absent,
    dest1: Destination::Absent,
    preexisting_ok: true,
    bad_call: false,
    exit: EXIT_FAIL,
    default: false,
};

/// 槽 6 `mv-empty-source`：0 字节源，跨盘复制退化为建空文件，成功。
const SHAPE_EMPTY_SOURCE: Shape = Shape {
    route: Route::CrossDev,
    src_len: 0,
    free: 0,
    src_ok: true,
    src_removed: true,
    dest0: Destination::Absent,
    dest1: Destination::Complete(0),
    preexisting_ok: true,
    bad_call: false,
    exit: EXIT_OK,
    default: false,
};

/// 槽 7 `mv-same-dev`：同盘 mv 就是 rename(2)，不复制数据、不碰 ENOSPC。
const SHAPE_SAME_DEV: Shape = Shape {
    route: Route::SameDev,
    src_len: BLOCK,
    free: 0,
    src_ok: true,
    src_removed: true,
    dest0: Destination::Absent,
    dest1: Destination::Complete(BLOCK),
    preexisting_ok: true,
    bad_call: false,
    exit: EXIT_OK,
    default: false,
};

/// 槽 13 `default-bucket` 的现场：事实与槽 3 相同，但被判据体系丢进未判档。
const SHAPE_DEFAULT_BUCKET: Shape = Shape {
    route: Route::CrossDev,
    src_len: BLOCK * 2,
    free: BLOCK,
    src_ok: true,
    src_removed: false,
    dest0: Destination::Absent,
    dest1: Destination::Absent,
    preexisting_ok: true,
    bad_call: false,
    exit: EXIT_FAIL,
    default: true,
};

/// 全部 14 槽的输入形态。后 6 槽的事实面整槽复用活形态，
/// 一槽只坏一维——坏的是申报，不是事实。
pub const SHAPES: [Shape; CORPUS] = [
    SHAPE_CLEAN_FIT,
    SHAPE_CLEAN_SLACK,
    SHAPE_OVERWRITE_COMPLETE,
    SHAPE_ENOSPC_ROLLBACK,
    SHAPE_ENOSPC_KEEP,
    SHAPE_ENOSPC_ZERO,
    SHAPE_EMPTY_SOURCE,
    SHAPE_SAME_DEV,
    SHAPE_ENOSPC_ROLLBACK,
    SHAPE_ENOSPC_ROLLBACK,
    Shape {
        route: Route::CrossDev,
        src_len: BLOCK * 2,
        free: BLOCK,
        src_ok: true,
        src_removed: false,
        dest0: Destination::Absent,
        dest1: Destination::Partial(BLOCK),
        preexisting_ok: true,
        bad_call: false,
        exit: EXIT_FAIL,
        default: false,
    },
    SHAPE_ENOSPC_ROLLBACK,
    SHAPE_ENOSPC_KEEP,
    SHAPE_DEFAULT_BUCKET,
];

/// 槽 0 的申报：完整复制，六维照实。
const CLAIM_CLEAN_FIT: Claim = Claim::new(
    "mv-clean-fit",
    true,
    Stage::Verify,
    0,
    BLOCK,
    Note::Copied,
    false,
);

/// 槽 1 的申报。
const CLAIM_CLEAN_SLACK: Claim = Claim::new(
    "mv-clean-slack",
    true,
    Stage::Verify,
    0,
    BLOCK * 2,
    Note::Copied,
    false,
);

/// 槽 2 的申报。
const CLAIM_OVERWRITE: Claim = Claim::new(
    "mv-overwrite-complete",
    true,
    Stage::Verify,
    0,
    BLOCK,
    Note::Copied,
    false,
);

/// 槽 3 的申报：回滚完成、源保留、退出 1。
const CLAIM_ROLLBACK: Claim = Claim::new(
    "mv-enospc-rollback",
    false,
    Stage::Exit,
    0,
    BLOCK,
    Note::RolledBack,
    false,
);

/// 槽 4 的申报：预存目标幸存。
const CLAIM_KEEP: Claim = Claim::new(
    "mv-enospc-keep-existing",
    false,
    Stage::Exit,
    0,
    BLOCK,
    Note::ExistingKept,
    false,
);

/// 槽 5 的申报：零字节回滚。
const CLAIM_ZERO: Claim = Claim::new(
    "mv-enospc-zero",
    false,
    Stage::Exit,
    0,
    0,
    Note::RolledBack,
    false,
);

/// 槽 6 的申报：空源完整复制。
const CLAIM_EMPTY: Claim = Claim::new(
    "mv-empty-source",
    true,
    Stage::Verify,
    0,
    0,
    Note::Copied,
    false,
);

/// 槽 7 的申报：同盘改名。
const CLAIM_SAME_DEV: Claim = Claim::new(
    "mv-same-dev",
    true,
    Stage::Unlink,
    0,
    0,
    Note::RenameNoCopy,
    false,
);

/// 十四格申报。前八格与 `define(SHAPES[i])` 逐维相同，后六格各坏一维。
pub const CLAIMS: [Claim; CORPUS] = [
    CLAIM_CLEAN_FIT,
    CLAIM_CLEAN_SLACK,
    CLAIM_OVERWRITE,
    CLAIM_ROLLBACK,
    CLAIM_KEEP,
    CLAIM_ZERO,
    CLAIM_EMPTY,
    CLAIM_SAME_DEV,
    // 死 1 estate-swap：事实是失败（回滚完成），申报却报成功。
    Claim::new(
        "estate-swap",
        true,
        Stage::Exit,
        0,
        BLOCK,
        Note::RolledBack,
        false,
    ),
    // 死 2 stage-skip：回滚在退出闸定稿，申报却说在写面闸。
    Claim::new(
        "stage-skip",
        false,
        Stage::Write,
        0,
        BLOCK,
        Note::RolledBack,
        false,
    ),
    // 死 3 code-swallow：半成品残留在目的侧，申报却报无码。
    Claim::new(
        "code-swallow",
        false,
        Stage::Write,
        0,
        BLOCK,
        Note::PartialLeft,
        false,
    ),
    // 死 4 value-off：已复制 4096 字节，申报 2048。
    Claim::new(
        "value-off",
        false,
        Stage::Exit,
        0,
        HALF_BLOCK,
        Note::RolledBack,
        false,
    ),
    // 死 5 note-swap：预存保留申报成回滚。
    Claim::new(
        "note-swap",
        false,
        Stage::Exit,
        0,
        BLOCK,
        Note::RolledBack,
        false,
    ),
    // 死 6 default-bucket：可判定的现场被丢进未判档。
    Claim::new(
        "default-bucket",
        false,
        Stage::Exit,
        0,
        BLOCK,
        Note::Nil,
        false,
    ),
];

// ---------------------------------------------------------------------------
// 期望表（申报表的第二种形式，任一处笔误都要露出来）
// ---------------------------------------------------------------------------

/// 期望阶段（现场真实落到的阶段，不是申报那一栏）。
pub const WANT_STAGE: [Stage; CORPUS] = [
    Stage::Verify,
    Stage::Verify,
    Stage::Verify,
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
    Stage::Verify,
    Stage::Unlink,
    Stage::Exit,
    Stage::Exit,
    Stage::Write,
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
];

/// 期望错误码（0 = 现场无违诺）。
pub const WANT_CODE: [u32; CORPUS] = [
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    MV_ROLLBACK,
    0,
    0,
    0,
];

/// 期望缺陷维序位（0..5 六维，6 = 无缺陷）。
pub const WANT_NDX: [usize; CORPUS] = [6, 6, 6, 6, 6, 6, 6, 6, 0, 1, 2, 3, 4, 5];

/// 期望注记。
pub const WANT_NOTE: [Note; CORPUS] = [
    Note::Copied,
    Note::Copied,
    Note::Copied,
    Note::RolledBack,
    Note::ExistingKept,
    Note::RolledBack,
    Note::Copied,
    Note::RenameNoCopy,
    Note::RolledBack,
    Note::RolledBack,
    Note::PartialLeft,
    Note::RolledBack,
    Note::ExistingKept,
    Note::Nil,
];

/// 期望兜底位。
pub const WANT_DEFAULT: [bool; CORPUS] = [
    false, false, false, false, false, false, false, false, false, false, false, false, false, true,
];

/// 期望短名。
pub const WANT_TAGS: [&str; CORPUS] = [
    "mv-clean-fit",
    "mv-clean-slack",
    "mv-overwrite-complete",
    "mv-enospc-rollback",
    "mv-enospc-keep-existing",
    "mv-enospc-zero",
    "mv-empty-source",
    "mv-same-dev",
    "estate-swap",
    "stage-skip",
    "code-swallow",
    "value-off",
    "note-swap",
    "default-bucket",
];

/// 期望已复制字节数。
pub const WANT_VALUE: [u32; CORPUS] = [
    BLOCK,
    BLOCK * 2,
    BLOCK,
    BLOCK,
    BLOCK,
    0,
    0,
    0,
    BLOCK,
    BLOCK,
    BLOCK,
    BLOCK,
    BLOCK,
    BLOCK,
];

/// 期望成败。
pub const WANT_SUCCESS: [bool; CORPUS] = [
    true, true, true, false, false, false, true, true, false, false, false, false, false, false,
];

/// 期望退出码。
pub const WANT_EXIT: [u32; CORPUS] = [
    EXIT_OK,
    EXIT_OK,
    EXIT_OK,
    EXIT_FAIL,
    EXIT_FAIL,
    EXIT_FAIL,
    EXIT_OK,
    EXIT_OK,
    EXIT_FAIL,
    EXIT_FAIL,
    EXIT_FAIL,
    EXIT_FAIL,
    EXIT_FAIL,
    EXIT_FAIL,
];

// ---------------------------------------------------------------------------
// 族内场景：六条回滚 + 六条完好
// ---------------------------------------------------------------------------

/// 族内统一种子（场景指纹用）。
pub const FAMILY_SEED: u32 = 0x6871_0001;

/// 第 `i` 条场景（12 条，手写，一条一种收场）。
pub fn scene_at(i: usize) -> Option<Shape> {
    match i {
        0 => Some(Shape::rollback(BLOCK * 2, BLOCK)),
        1 => Some(Shape::rollback(BLOCK * 2, 0)),
        2 => Some(Shape::rollback(BLOCK * 4, BLOCK)),
        3 => Some(Shape::rollback(BLOCK, HALF_BLOCK)),
        4 => Some(Shape::rollback(BLOCK * 3, BLOCK * 2)),
        5 => Some(Shape::rollback(BLOCK * 16, BLOCK)),
        // 预存目标在 ENOSPC 里幸存。
        6 => Some(Shape {
            route: Route::CrossDev,
            src_len: BLOCK * 2,
            free: BLOCK,
            src_ok: true,
            src_removed: false,
            dest0: Destination::Complete(512),
            dest1: Destination::Complete(512),
            preexisting_ok: true,
            bad_call: false,
            exit: EXIT_FAIL,
            default: false,
        }),
        7 => Some(Shape::clean(BLOCK)),
        // 富余空间。
        8 => Some(Shape {
            route: Route::CrossDev,
            src_len: BLOCK * 2,
            free: BLOCK * 3,
            src_ok: true,
            src_removed: true,
            dest0: Destination::Absent,
            dest1: Destination::Complete(BLOCK * 2),
            preexisting_ok: true,
            bad_call: false,
            exit: EXIT_OK,
            default: false,
        }),
        // 空源。
        9 => Some(Shape {
            route: Route::CrossDev,
            src_len: 0,
            free: 0,
            src_ok: true,
            src_removed: true,
            dest0: Destination::Absent,
            dest1: Destination::Complete(0),
            preexisting_ok: true,
            bad_call: false,
            exit: EXIT_OK,
            default: false,
        }),
        // 同盘 rename 边界档。
        10 => Some(Shape {
            route: Route::SameDev,
            src_len: BLOCK,
            free: 0,
            src_ok: true,
            src_removed: true,
            dest0: Destination::Absent,
            dest1: Destination::Complete(BLOCK),
            preexisting_ok: true,
            bad_call: false,
            exit: EXIT_OK,
            default: false,
        }),
        // 预存目标被完整覆盖（合法覆盖，不是毁物）。
        11 => Some(Shape {
            route: Route::CrossDev,
            src_len: BLOCK,
            free: BLOCK,
            src_ok: true,
            src_removed: true,
            dest0: Destination::Complete(512),
            dest1: Destination::Complete(BLOCK),
            preexisting_ok: false,
            bad_call: false,
            exit: EXIT_OK,
            default: false,
        }),
        _ => None,
    }
}

/// 全族场景。
pub fn scenes() -> [Shape; FAMILY_SCENES] {
    let mut out = [Shape::clean(0); FAMILY_SCENES];
    let mut i = 0usize;
    while i < FAMILY_SCENES {
        if let Some(s) = scene_at(i) {
            if let Some(slot) = out.get_mut(i) {
                *slot = s;
            }
        }
        i += 1;
    }
    out
}

/// 该场景的现场是否落进绿档。
pub fn scene_anchored(i: usize) -> Option<bool> {
    match scene_at(i) {
        Some(s) => Some(Tier::of(&define(&s)).is_green()),
        None => None,
    }
}

/// 该场景配一条与其实际结论相同的申报。
pub fn scene_case(i: usize) -> Option<Case> {
    let s = match scene_at(i) {
        Some(x) => x,
        None => return None,
    };
    let o = define(&s);
    Some(Case::new(
        s,
        Claim::from_outcome(&o, o.ok(), head_tag()),
    ))
}

/// 族内每条场景都必须落绿且自申报自洽（判据绿的前提）。
pub fn scenes_agree() -> bool {
    let mut i = 0usize;
    while i < FAMILY_SCENES {
        let c = match scene_case(i) {
            Some(x) => x,
            None => return false,
        };
        if !Tier::of(&c.actual()).is_green() {
            return false;
        }
        if judge(&c) != Fault::Ok {
            return false;
        }
        if c.actual().ok() != c.claim.success() {
            return false;
        }
        i += 1;
    }
    true
}

/// 族场景计数：回滚档与完好档各多少。
pub fn scene_counts() -> (usize, usize) {
    let mut rolled = 0usize;
    let mut intact = 0usize;
    let mut i = 0usize;
    while i < FAMILY_SCENES {
        if let Some(s) = scene_at(i) {
            match Tier::of(&define(&s)) {
                Tier::Rollback => rolled += 1,
                Tier::Intact => intact += 1,
                _ => {}
            }
        }
        i += 1;
    }
    (rolled, intact)
}

/// 场景指纹。
pub fn scene_seal(i: usize) -> Option<u64> {
    match scene_at(i) {
        Some(s) => {
            let mut h = fold_word(FNV_OFF, FAMILY_SEED as u64);
            h = fold_word(h, s.seal());
            Some(h)
        }
        None => None,
    }
}

/// 联签锚自洽：段差与两侧段基址必须对得上。
pub fn anchors_agree() -> bool {
    if ANCHOR_F11686_SEG.checked_add(ANCHOR_F11686_GAP) != Some(SEG_BASE) {
        return false;
    }
    if ANCHOR_F11685_SEG.checked_add(ANCHOR_F11685_GAP) != Some(SEG_BASE) {
        return false;
    }
    if SEG_BASE.checked_add(ANCHOR_F11694_GAP) != Some(ANCHOR_F11694_SEG) {
        return false;
    }
    if SEG_BASE.checked_add(ANCHOR_F11699_GAP) != Some(ANCHOR_F11699_SEG) {
        return false;
    }
    ANCHOR_ENOSPC == 28 && ANCHOR_EXDEV == 18 && ANCHOR_ENOSPC_TEXT == "No space left on device"
}

// ---------------------------------------------------------------------------
// 档位与矩阵
// ---------------------------------------------------------------------------

/// 一档的统计行：`total` 与 `good` 两维独立。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Row {
    /// 档位。
    pub tier: Tier,
    /// 该档扫过的槽数。
    pub total: usize,
    /// 该档判对的槽数。
    pub good: usize,
}

impl Row {
    /// 该行是否全对。
    pub fn perfect(&self) -> bool {
        self.total == self.good
    }

    /// 该行红格数。
    pub fn red(&self) -> usize {
        if self.total > self.good {
            self.total - self.good
        } else {
            0
        }
    }

    /// 呈现一行。
    pub fn render(&self) -> String {
        let mut s = String::from(self.tier.slug());
        s.push('=');
        s.push_str(dec_text(self.good as u32).as_str());
        s.push('/');
        s.push_str(dec_text(self.total as u32).as_str());
        s
    }
}

/// 四档矩阵：横看语料槽，纵看档位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Matrix {
    rows: [Row; TIER_COUNT],
}

impl Matrix {
    /// 由语料构造。每一档都扫全部槽：正例必须落进自己的绿档，反例必须落进
    /// 非绿档并命中它自己那一维；两者都不许被别的档"顺带算对"。
    pub fn build() -> Matrix {
        let table = Tier::ALL;
        let mut rows = [Row {
            tier: Tier::Rollback,
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
                    landed == tier
                        && !tier.is_green()
                        && c.fault() == expected_reject_ndx(i)
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
        let table = Tier::ALL;
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
                if r.good == 0 {
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

/// 第 `i` 槽（反例区）期望的缺陷维。
fn expected_reject_ndx(i: usize) -> Fault {
    match expected(i) {
        Some(f) => f,
        None => Fault::Ok,
    }
}

// ---------------------------------------------------------------------------
// 结算报告
// ---------------------------------------------------------------------------

/// 全绿渲染文本（逐字钉住，改一个字都要红）。
pub const RENDER_OK: &str = "mv corpus=14 accept=8 reject=6 rollback=6 intact=6 residue=0 breach=0 match=12 rate=1000permille\n";

/// 一次 mv 回滚结算报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Report {
    /// 结算分母（族内场景数）。
    pub total: usize,
    /// 正例槽数。
    pub accept: usize,
    /// 反例槽数。
    pub reject: usize,
    /// 回滚档计数。
    pub rollback: usize,
    /// 完好档计数。
    pub intact: usize,
    /// 残留档计数。
    pub residue: usize,
    /// 违诺档计数。
    pub breach: usize,
    /// 计入分子的场景数。
    pub matched: usize,
    /// 违约计数（现场红 + 语料错判）。
    pub conflicts: usize,
}

impl Report {
    /// 一致率千分比（四舍五入到整数千分）。
    pub fn rate(&self) -> u32 {
        if self.total == 0 {
            return 0;
        }
        (self.matched as u32 * RATE_FULL + (self.total as u32) / 2) / (self.total as u32)
    }

    /// 是否全绿。
    pub fn green(&self) -> bool {
        self.conflicts == 0
            && self.residue == 0
            && self.breach == 0
            && self.rate() == RATE_FULL
            && self.accept == ACCEPT
            && self.reject == REJECT
            && self.rollback + self.intact == FAMILY_SCENES
            && self.matched == FAMILY_SCENES
    }

    /// 红项计数。
    pub fn reds(&self) -> u32 {
        (self.conflicts as u32) + (self.residue as u32) + (self.breach as u32)
    }

    /// 闸门：全绿才放行。
    pub fn gate(&self) -> Result<(), u32> {
        if self.total == 0 {
            return Err(MV_MATRIX);
        }
        if self.residue > 0 {
            return Err(MV_ROLLBACK);
        }
        if self.breach > 0 {
            return Err(MV_CLOBBER);
        }
        if self.conflicts > 0 {
            return Err(MV_NOTE);
        }
        if self.rate() < RATE_FULL {
            return Err(MV_RATE);
        }
        if self.rollback + self.intact != FAMILY_SCENES {
            return Err(MV_MATRIX);
        }
        Ok(())
    }

    /// 四档格数。
    pub fn tier_cells(&self) -> u32 {
        (self.rollback as u32) + (self.intact as u32) + (self.residue as u32) + (self.breach as u32)
    }

    /// 红档格数。
    pub fn red_cells(&self) -> u32 {
        (self.residue as u32) + (self.breach as u32) + (self.conflicts as u32)
    }

    /// 逐字渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("mv corpus=");
        s.push_str(dec_text((self.accept + self.reject) as u32).as_str());
        s.push_str(" accept=");
        s.push_str(dec_text(self.accept as u32).as_str());
        s.push_str(" reject=");
        s.push_str(dec_text(self.reject as u32).as_str());
        s.push_str(" rollback=");
        s.push_str(dec_text(self.rollback as u32).as_str());
        s.push_str(" intact=");
        s.push_str(dec_text(self.intact as u32).as_str());
        s.push_str(" residue=");
        s.push_str(dec_text(self.residue as u32).as_str());
        s.push_str(" breach=");
        s.push_str(dec_text(self.breach as u32).as_str());
        s.push_str(" match=");
        s.push_str(dec_text(self.matched as u32).as_str());
        s.push_str(" rate=");
        s.push_str(dec_text(self.rate()).as_str());
        s.push_str("permille\n");
        s
    }
}

/// 空报告（未跑任何场景，绝不与全绿混淆）。
pub fn empty_report() -> Report {
    Report {
        total: 0,
        accept: 0,
        reject: 0,
        rollback: 0,
        intact: 0,
        residue: 0,
        breach: 0,
        matched: 0,
        conflicts: 0,
    }
}

/// 由族场景与语料两路结算。
fn settle(over: Option<(usize, Fault)>) -> Report {
    let mut rep = Report {
        total: FAMILY_SCENES,
        accept: ACCEPT,
        reject: REJECT,
        rollback: 0,
        intact: 0,
        residue: 0,
        breach: 0,
        matched: 0,
        conflicts: 0,
    };
    let mut i = 0usize;
    while i < FAMILY_SCENES {
        let s = match scene_at(i) {
            Some(x) => x,
            None => return empty_report(),
        };
        match Tier::of(&define(&s)) {
            Tier::Rollback => {
                rep.rollback += 1;
                rep.matched += 1;
            }
            Tier::Intact => {
                rep.intact += 1;
                rep.matched += 1;
            }
            Tier::Residue => {
                rep.residue += 1;
                rep.conflicts += 1;
            }
            Tier::Breach => {
                rep.breach += 1;
                rep.conflicts += 1;
            }
        }
        i += 1;
    }
    let mut violations = 0usize;
    let mut k = 0usize;
    while k < CORPUS {
        let c = match case_at(k) {
            Some(x) => x,
            None => return empty_report(),
        };
        let landed = match over {
            Some((slot, fault)) => {
                if slot == k {
                    fault
                } else {
                    c.fault()
                }
            }
            None => c.fault(),
        };
        if k < ACCEPT {
            if landed != Fault::Ok {
                violations += 1;
            }
        } else if landed != expected_reject_ndx(k) {
            violations += 1;
        }
        k += 1;
    }
    rep.matched = rep.matched.saturating_sub(violations);
    rep.conflicts += violations;
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

/// 带注入的结算：越界槽给槽位码，否则返回注入后的报告。
pub fn run_with(over: Option<(usize, Fault)>) -> Result<Report, u32> {
    if let Some((slot, _)) = over {
        if slot >= CORPUS {
            return Err(MV_SLOT);
        }
    }
    Ok(settle(over))
}

/// 报告与矩阵是否互相印证。
pub fn report_agrees_with_matrix() -> bool {
    let rep = settle(None);
    let m = Matrix::build();
    let rolled_cells = match m.row(Tier::Rollback) {
        Some(r) => r.good,
        None => 0,
    };
    let intact_cells = match m.row(Tier::Intact) {
        Some(r) => r.good,
        None => 0,
    };
    rep.rollback > 0 && rep.intact > 0 && rolled_cells > 0 && intact_cells > 0
}

/// 红档率标定：注入一维红后分子必须掉下来。
pub fn red_rate_calibrated() -> bool {
    let base = settle(None);
    let bad = match run_with(Some((2, Fault::Value))) {
        Ok(r) => r,
        Err(_) => return false,
    };
    if bad.matched >= base.matched {
        return false;
    }
    RED_RATE == 929
}

// ---------------------------------------------------------------------------
// 阶段闸门：Check→…→Exit
// ---------------------------------------------------------------------------

/// 一次 mv 回滚结算的走闸状态机。
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
            return Err(MV_ESTATE);
        }
        if let Some(cur) = self.current {
            if s.rank() < cur.rank() {
                self.refusals += 1;
                return Err(MV_STAGE);
            }
        }
        self.root = fold_word(self.root, s.rank() as u64);
        self.root = fold_str(self.root, s.slug());
        self.current = Some(s);
        self.steps += 1;
        Ok(())
    }

    /// 封账：之后任何走档与再次封账都拒。
    pub fn close(&mut self) -> Result<u64, u32> {
        if self.closed {
            self.refusals += 1;
            return Err(MV_ESTATE);
        }
        if self.current.is_none() {
            return Err(MV_STAGE);
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

    /// 依次走一串档，任一步被拒就把那个码交出来。
    pub fn walk(stages: &[Stage]) -> Result<Gate, u32> {
        let mut g = Gate::new();
        let mut i = 0usize;
        while i < stages.len() {
            let s = match stages.get(i) {
                Some(x) => *x,
                None => return Err(MV_SLOT),
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
        let table = Stage::ALL;
        let mut g = Gate::new();
        let mut i = 0usize;
        while i < table.len() {
            let s = match table.get(i) {
                Some(x) => *x,
                None => return Err(MV_SLOT),
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
            Some(cur) => s.push_str(cur.slug()),
            None => s.push_str("none"),
        }
        s.push_str(" steps=");
        s.push_str(dec_text(self.steps).as_str());
        s.push_str(" refusals=");
        s.push_str(dec_text(self.refusals).as_str());
        s.push_str(" closed=");
        s.push_str(bool_text(self.closed).as_str());
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
    /// 这一格记的是成功还是败。
    pub ok: bool,
    /// 错误码（成功格必须为 0）。
    pub code: u32,
    /// 统计值（败格必须为 0）。
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
        h = fold_word(h, self.value as u64);
        h
    }

    /// 呈现一格。
    pub fn render(&self) -> String {
        let mut s = String::from("slot ");
        s.push_str(dec_text(self.slot as u32).as_str());
        s.push_str(" op ");
        s.push_str(hex_text(self.op as u32, 2).as_str());
        s.push(' ');
        s.push_str(bool_text(self.ok).as_str());
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
            return Err(MV_ESTATE);
        }
        if self.len >= SAMPLES {
            self.rejects += 1;
            return Err(MV_SLOT);
        }
        if s.slot as usize != self.len {
            self.rejects += 1;
            return Err(MV_SLOT);
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
        self.len - self.ok_count()
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

    /// 逐格复核：统计格对上 `VALS`、红档格对上 `RED_RATE`、缺陷格对上
    /// `REJECT_CODES` 且败格值必须为 0，缺陷码两两互异。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES {
            return Err(MV_SLOT);
        }
        let stat_last = match STAT_CELLS.checked_add(1) {
            Some(x) => x,
            None => return Err(MV_SLOT),
        };
        let mut i = 0usize;
        while i < stat_last {
            let s = match self.sample(i) {
                Some(x) => x,
                None => return Err(MV_SLOT),
            };
            if !s.ok {
                return Err(MV_ESTATE);
            }
            if s.code != 0 {
                return Err(MV_CODE);
            }
            let want_value = match VALS.get(i) {
                Some(v) => *v,
                None => return Err(MV_SLOT),
            };
            if s.value != want_value {
                return Err(MV_VALUE);
            }
            let want_op = match self.stat_op_want(i) {
                Some(v) => v,
                None => return Err(MV_SLOT),
            };
            if s.op != want_op {
                return Err(MV_CODE);
            }
            i += 1;
        }
        let mut j = 0usize;
        while j < REJECT {
            let cell = match STAT_CELLS.checked_add(1).and_then(|x| x.checked_add(j)) {
                Some(x) => x,
                None => return Err(MV_SLOT),
            };
            let s = match self.sample(cell) {
                Some(x) => x,
                None => return Err(MV_SLOT),
            };
            if s.ok {
                return Err(MV_ESTATE);
            }
            if s.value != 0 {
                return Err(MV_VALUE);
            }
            let want_code = match REJECT_CODES.get(j) {
                Some(c) => *c,
                None => return Err(MV_SLOT),
            };
            if s.code != want_code {
                return Err(MV_CODE);
            }
            let want_op = match DEFECT_OP_BASE.checked_add(j as u8) {
                Some(x) => x,
                None => return Err(MV_SLOT),
            };
            if s.op != want_op {
                return Err(MV_CODE);
            }
            let mut k = j + 1usize;
            while k < REJECT {
                if let Some(other) = REJECT_CODES.get(k) {
                    if *other == want_code {
                        return Err(MV_CODE);
                    }
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
            if i < STAT_CELLS + 1 {
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
                if s.ok {
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
            return Err(MV_ESTATE);
        }
        match self.audit() {
            Ok(()) => {}
            Err(code) => return Err(code),
        }
        let h = self.seal();
        if h == 0 {
            return Err(MV_SEAL);
        }
        self.root = h;
        self.frozen = true;
        Ok(h)
    }

    /// 重放：以同一批格子重建账本，比指纹。
    pub fn replay(&self) -> Result<u64, u32> {
        if !self.frozen {
            return Err(MV_ESTATE);
        }
        let mut fresh = Ledger::new();
        let mut i = 0usize;
        while i < self.len {
            let s = match self.sample(i) {
                Some(x) => x,
                None => return Err(MV_SEAL),
            };
            match fresh.register(s) {
                Ok(()) => {}
                Err(_) => return Err(MV_SEAL),
            }
            i += 1;
        }
        let h = fresh.seal();
        if h != self.root {
            return Err(MV_SEAL);
        }
        Ok(h)
    }

    /// 改动一格（仅未冻结时允许）。
    pub fn poke(&mut self, i: usize, value: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(MV_ESTATE);
        }
        match self.slots.get_mut(i) {
            Some(cell) => {
                match *cell {
                    Some(ref mut s) => {
                        s.value = value;
                    }
                    None => return Err(MV_SLOT),
                }
                Ok(())
            }
            None => Err(MV_SLOT),
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
        s.push_str(bool_text(self.frozen).as_str());
        s
    }
}

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
        let s = Sample {
            slot,
            op,
            ok: false,
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
        1 => match VALS.get(0) {
            Some(v) => (0usize, v + 7),
            None => return l,
        },
        2 => match STAT_CELLS.checked_add(1) {
            Some(c) => (c, 1u32),
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
// 四路自检
// ---------------------------------------------------------------------------

/// 注入一路红，四路自检必须各指各的。
pub fn inject_four() -> [bool; 4] {
    let table_route = {
        let mut ok = true;
        let mut i = 0usize;
        while i < CORPUS {
            match case_at(i) {
                Some(c) => {
                    if c.claim.tag != match WANT_TAGS.get(i) {
                        Some(t) => *t,
                        None => "",
                    } {
                        ok = false;
                    }
                }
                None => ok = false,
            }
            i += 1;
        }
        ok && anchors_agree() && scenes_agree()
    };
    let judge_route = {
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
        all_ok && judge_order_ok()
    };
    let render_route = {
        let rep = settle(None);
        rep.green() && rep.render().as_str() == RENDER_OK
    };
    let ledger_route = {
        let mut l = build_ledger();
        match l.freeze() {
            Ok(h) => h != 0 && l.is_frozen() && l.replay() == Ok(h),
            Err(_) => false,
        }
    };
    [table_route, judge_route, render_route, ledger_route]
}

/// judge 的 Estate 优先性自检：造一条除 Estate 外全错的申报，必须仍判 Estate。
fn judge_order_ok() -> bool {
    let s = match scene_at(0) {
        Some(x) => x,
        None => return false,
    };
    let a = define(&s);
    let wrong = Claim::new(
        "order-probe",
        !a.ok(),
        Stage::Check,
        MV_SLOT,
        a.value.wrapping_add(7),
        Note::Clobbered,
        true,
    );
    let c = Case::new(s, wrong);
    judge(&c) == Fault::Estate
}

// ---------------------------------------------------------------------------
// 十三条判据谓词（全部私有，只在 run_unx11687_checks 里汇总）
// ---------------------------------------------------------------------------

/// 01 名称与段基址：常量层自洽。
fn name_ok() -> bool {
    if SEG_BASE != 0x6d330 {
        return false;
    }
    if CORPUS != SAMPLES {
        return false;
    }
    if ACCEPT + REJECT != CORPUS {
        return false;
    }
    if TIER_COUNT != 4 {
        return false;
    }
    if STAGE_COUNT as usize != Stage::ALL.len() {
        return false;
    }
    if STAGE_LAST_RANK != 9 {
        return false;
    }
    if STAT_CELLS + 1 != ACCEPT {
        return false;
    }
    if STAT_CELLS + 1 + REJECT != SAMPLES {
        return false;
    }
    if RED_RATE != 929 || RATE_FULL != 1000 || MATCH_FULL != 1000 {
        return false;
    }
    if FAMILY_SCENES != ROLLBACK_SCENES + INTACT_SCENES {
        return false;
    }
    if ROLLBACK_SCENES != 6 || INTACT_SCENES != 6 {
        return false;
    }
    if !SANDBOX_PREFIX.starts_with("/tmp/bx-sandbox/") {
        return false;
    }
    if EXIT_OK != 0 || EXIT_FAIL != 1 || EXIT_USAGE != 2 {
        return false;
    }
    anchors_agree()
}

/// 02 十三码互异且逐字对齐段基址；反例六码与缺陷维表一一对应。
fn codes_ok() -> bool {
    if ALL_CODES.len() != 13 {
        return false;
    }
    let mut i = 0usize;
    while i < ALL_CODES.len() {
        let c = match ALL_CODES.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let want = match SEG_BASE.checked_add(i as u32 + 1) {
            Some(x) => x,
            None => return false,
        };
        if c != want {
            return false;
        }
        let mut j = i + 1usize;
        while j < ALL_CODES.len() {
            if let Some(o) = ALL_CODES.get(j) {
                if *o == c {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let faults = Fault::reject_faults();
    let mut k = 0usize;
    while k < REJECT {
        let f = match faults.get(k) {
            Some(x) => *x,
            None => return false,
        };
        let c = match REJECT_CODES.get(k) {
            Some(x) => *x,
            None => return false,
        };
        if f.code() != c {
            return false;
        }
        if REJECT_FAULTS.get(k) != Some(&f) {
            return false;
        }
        if fault_ndx(f) != k {
            return false;
        }
        k += 1;
    }
    if Fault::Ok.code() != 0 {
        return false;
    }
    true
}

/// 03 目的地状态机：三项状态、迁移合法、`Partial(0)` 不许退化成 `Absent`。
fn dest_ok() -> bool {
    let absent = Destination::Absent;
    let partial0 = Destination::Partial(0);
    let partial = Destination::Partial(HALF_BLOCK);
    let full = Destination::Complete(BLOCK);
    if !absent.is_absent() || absent.is_partial() || absent.is_complete() {
        return false;
    }
    if !partial0.is_partial() || partial0.is_absent() {
        return false;
    }
    if partial0.bytes() != 0 || partial.bytes() != HALF_BLOCK || full.bytes() != BLOCK {
        return false;
    }
    if !absent.may_step_to(partial) || !absent.may_step_to(full) {
        return false;
    }
    if !partial.may_step_to(Destination::Absent) {
        return false;
    }
    if !partial.may_step_to(full) {
        return false;
    }
    if !full.may_step_to(Destination::Absent) {
        return false;
    }
    if full.may_step_to(partial) {
        return false;
    }
    if partial.may_step_to(Destination::Partial(BLOCK * 2)) {
        // Partial→Partial 的"增长"是合法的，不是这里要判的违例。
    } else {
        return false;
    }
    if !partial.may_step_to(Destination::Partial(HALF_BLOCK + HALF_BLOCK)) {
        return false;
    }
    if partial0.fold(FNV_OFF) == absent.fold(FNV_OFF) {
        return false;
    }
    if partial.tag() == absent.tag() || full.tag() == partial.tag() {
        return false;
    }
    true
}

/// 04 回滚不变量：任何被判"回滚完成"的现场，目的侧都绝不能是 `Partial{n}`。
fn rollback_ok() -> bool {
    // 全语料：注记与终态必须闭合。
    let mut i = 0usize;
    while i < CORPUS {
        let c = match case_at(i) {
            Some(x) => x,
            None => return false,
        };
        let o = define(&c.shape);
        if o.note == Note::RolledBack && c.shape.dest1.is_partial() {
            return false;
        }
        if o.note == Note::ExistingKept && !c.shape.preexisting_ok {
            return false;
        }
        i += 1;
    }
    // 合成一条"半成品留下"的现场：必须判 PartialLeft + MV_ROLLBACK。
    let mut s = Shape::rollback(BLOCK * 2, BLOCK);
    s.dest1 = Destination::Partial(BLOCK);
    let o = define(&s);
    if o.note != Note::PartialLeft {
        return false;
    }
    if o.code != MV_ROLLBACK {
        return false;
    }
    if o.ok() {
        return false;
    }
    if Tier::of(&o) != Tier::Residue {
        return false;
    }
    // 再合成一条"半成品已被删"的同形现场：必须判回滚。
    s.dest1 = Destination::Absent;
    if define(&s).note != Note::RolledBack {
        return false;
    }
    true
}

/// 05 预存目标保护：失败不许毁旧物，成功覆盖不算毁物。
fn preexisting_ok() -> bool {
    let keep = define(&SHAPE_ENOSPC_KEEP);
    if keep.note != Note::ExistingKept {
        return false;
    }
    if keep.code != 0 || keep.ok() {
        return false;
    }
    if keep.value != BLOCK {
        return false;
    }
    if Tier::of(&keep) != Tier::Intact {
        return false;
    }
    // 旧物被截断：必须判 Clobbered。
    let mut bad = SHAPE_ENOSPC_KEEP;
    bad.preexisting_ok = false;
    let b = define(&bad);
    if b.note != Note::Clobbered || b.code != MV_CLOBBER {
        return false;
    }
    if Tier::of(&b) != Tier::Breach {
        return false;
    }
    // 空间够时的完整覆盖是合法覆盖，不是毁物。
    let ow = define(&SHAPE_OVERWRITE_COMPLETE);
    if ow.note != Note::Copied || ow.code != 0 || !ow.ok() {
        return false;
    }
    true
}

/// 06 源件保留：失败路径上源必须在；成功路径才允许删源。
fn source_ok() -> bool {
    let mut i = 0usize;
    while i < CORPUS {
        let c = match case_at(i) {
            Some(x) => x,
            None => return false,
        };
        let o = define(&c.shape);
        // 回滚现场必须保留源。
        if o.note.is_rollback() && c.shape.src_removed {
            return false;
        }
        // 成功现场必须已删源（事务提交）。
        if o.ok() && !c.shape.src_removed && c.shape.route == Route::CrossDev {
            return false;
        }
        i += 1;
    }
    // 合成一条"失败却删了源"的现场：必须判 SourceLost。
    let mut s = Shape::rollback(BLOCK * 2, BLOCK);
    s.src_removed = true;
    let o = define(&s);
    if o.note != Note::SourceLost || o.code != MV_SOURCE {
        return false;
    }
    if o.ok() {
        return false;
    }
    // 源打不开：在源闸判死。
    let mut u = Shape::rollback(BLOCK * 2, BLOCK);
    u.src_ok = false;
    let uo = define(&u);
    if uo.stage != Stage::Source || uo.code != MV_SOURCE {
        return false;
    }
    true
}

/// 07 文案逐字：GNU 与 busybox 两式必须逐字对上上游原文。
fn text_ok() -> bool {
    let dst = "/tmp/bx-sandbox/b/blob";
    let gnu = msg_mv_enospc(dst);
    if gnu.as_str() != "mv: cannot create regular file '/tmp/bx-sandbox/b/blob': No space left on device"
    {
        return false;
    }
    let bb = msg_mv_enospc_busybox(dst);
    if bb.as_str() != "mv: can't create '/tmp/bx-sandbox/b/blob': No space left on device" {
        return false;
    }
    if !T_GNU_PREFIX.starts_with("mv: cannot create regular file ") {
        return false;
    }
    if !T_BB_PREFIX.starts_with("mv: can't create ") {
        return false;
    }
    if ANCHOR_ENOSPC_TEXT != "No space left on device" {
        return false;
    }
    if msg_mv_remove("/tmp/bx-sandbox/a/blob").as_str()
        != "mv: cannot remove '/tmp/bx-sandbox/a/blob': Permission denied"
    {
        return false;
    }
    if msg_mv_short(dst).len() <= T_SHORT_PREFIX.len() {
        return false;
    }
    if msg_usage_missing().len() == 0 || msg_usage_extra().len() == 0 {
        return false;
    }
    if !msg_rollback(dst).as_str().contains("rolled back") {
        return false;
    }
    if !msg_kept(dst).as_str().contains("kept") {
        return false;
    }
    if !msg_residue(dst, BLOCK).as_str().contains("4096 bytes") {
        return false;
    }
    if !msg_clobbered(dst).as_str().contains("clobbered") {
        return false;
    }
    if !msg_source_lost("/tmp/bx-sandbox/a/blob").as_str().contains("lost source") {
        return false;
    }
    true
}

/// 08 语料十四槽互校：形状、申报、事实、期望表四路交叉。
fn corpus_ok() -> bool {
    if SHAPES.len() != CORPUS || CLAIMS.len() != CORPUS {
        return false;
    }
    if cases().len() != CORPUS {
        return false;
    }
    if case_at(CORPUS).is_some() {
        return false;
    }
    if cover(CORPUS) != Ok(()) || cover(CORPUS + 1) != Err(MV_SLOT) {
        return false;
    }
    let mut i = 0usize;
    while i < CORPUS {
        let c = match case_at(i) {
            Some(x) => x,
            None => return false,
        };
        if c.claim.tag != match WANT_TAGS.get(i) {
            Some(t) => *t,
            None => return false,
        } {
            return false;
        }
        let a = c.actual();
        if a.stage != match WANT_STAGE.get(i) {
            Some(s) => *s,
            None => return false,
        } {
            return false;
        }
        if a.code != match WANT_CODE.get(i) {
            Some(v) => *v,
            None => return false,
        } {
            return false;
        }
        if a.value != match WANT_VALUE.get(i) {
            Some(v) => *v,
            None => return false,
        } {
            return false;
        }
        if a.note != match WANT_NOTE.get(i) {
            Some(n) => *n,
            None => return false,
        } {
            return false;
        }
        if a.default != match WANT_DEFAULT.get(i) {
            Some(d) => *d,
            None => return false,
        } {
            return false;
        }
        if a.ok() != match WANT_SUCCESS.get(i) {
            Some(x) => *x,
            None => return false,
        } {
            return false;
        }
        if c.shape.exit != match WANT_EXIT.get(i) {
            Some(x) => *x,
            None => return false,
        } {
            return false;
        }
        if !c.shape.domain_ok() {
            return false;
        }
        // 标签唯一。
        let mut j = i + 1usize;
        while j < CORPUS {
            if let Some(o) = CLAIMS.get(j) {
                if o.tag == c.claim.tag {
                    return false;
                }
            }
            j += 1;
        }
        // 申报差异掩码：活槽为 0，死槽恰好一位。
        let mask = c.claim.diff_mask(&a);
        if i < ACCEPT {
            if mask != 0 {
                return false;
            }
        } else {
            if mask != (1u32 << (i - ACCEPT)) {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 09 判序六维互斥：Estate 优先、六维互斥、秩可往返。
fn judge_ok() -> bool {
    let mut i = 0usize;
    while i < CORPUS {
        let c = match case_at(i) {
            Some(x) => x,
            None => return false,
        };
        if i < ACCEPT {
            if judge(&c) != Fault::Ok {
                return false;
            }
        } else {
            if judge(&c) != expected_reject_ndx(i) {
                return false;
            }
            if judge(&c) == Fault::Ok {
                return false;
            }
        }
        i += 1;
    }
    let mut r = 0u32;
    while r <= 6 {
        let f = match Fault::from_rank(r) {
            Some(x) => x,
            None => return false,
        };
        if f.rank() != r {
            return false;
        }
        r += 1;
    }
    if Fault::from_rank(7).is_some() {
        return false;
    }
    if fault_ndx(Fault::Ok) != 6 || ndx_fault(6) != Some(Fault::Ok) {
        return false;
    }
    if ndx_fault(7).is_some() {
        return false;
    }
    judge_order_ok()
}

/// 10 四档矩阵覆盖：每档都有落点，红档只收反例。
fn matrix_ok() -> bool {
    let m = Matrix::build();
    if !m.covered() {
        return false;
    }
    if m.slots() != CORPUS * TIER_COUNT {
        return false;
    }
    let rollback = match m.row(Tier::Rollback) {
        Some(r) => r,
        None => return false,
    };
    let intact = match m.row(Tier::Intact) {
        Some(r) => r,
        None => return false,
    };
    let residue = match m.row(Tier::Residue) {
        Some(r) => r,
        None => return false,
    };
    let breach = match m.row(Tier::Breach) {
        Some(r) => r,
        None => return false,
    };
    if rollback.good != 2 || intact.good != 6 || residue.good != 1 || breach.good != 1 {
        return false;
    }
    if m.goods() != 10 {
        return false;
    }
    if m.red_total() != CORPUS * TIER_COUNT - 10 {
        return false;
    }
    if !m.render().as_str().starts_with("matrix rollback=") {
        return false;
    }
    report_agrees_with_matrix() && red_rate_calibrated()
}

/// 11 全绿渲染逐字。
fn render_ok() -> bool {
    let rep = match run() {
        Ok(r) => r,
        Err(_) => return false,
    };
    if rep.render().as_str() != RENDER_OK {
        return false;
    }
    if !rep.green() {
        return false;
    }
    if rep.reds() != 0 {
        return false;
    }
    if rep.tier_cells() != FAMILY_SCENES as u32 {
        return false;
    }
    if rep.red_cells() != 0 {
        return false;
    }
    let empty = empty_report();
    if empty.green() {
        return false;
    }
    if empty.gate() != Err(MV_MATRIX) {
        return false;
    }
    if empty.rate() != 0 {
        return false;
    }
    true
}

/// 12 阶段闸门不倒退：正序放行、倒退判死、封账后一律拒。
fn gate_ok() -> bool {
    let root = match Gate::full_pass() {
        Ok(r) => r,
        Err(_) => return false,
    };
    if root == 0 {
        return false;
    }
    let mut g = Gate::new();
    let mut i = 0usize;
    while i < Stage::ALL.len() {
        let s = match Stage::ALL.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if g.step(s) != Ok(()) {
            return false;
        }
        i += 1;
    }
    let closed = match g.close() {
        Ok(x) => x,
        Err(_) => return false,
    };
    if closed != root {
        return false;
    }
    if !g.is_closed() {
        return false;
    }
    if g.step(Stage::Exit) != Err(MV_ESTATE) {
        return false;
    }
    if g.close() != Err(MV_ESTATE) {
        return false;
    }
    if g.refusals() != 2 {
        return false;
    }
    // 倒退判死。
    let mut b = Gate::new();
    if b.step(Stage::Write) != Ok(()) {
        return false;
    }
    if b.step(Stage::Copy) != Err(MV_STAGE) {
        return false;
    }
    if b.stage() != Some(Stage::Write) {
        return false;
    }
    if b.refusals() != 1 {
        return false;
    }
    // 空闸不许封账。
    let mut e = Gate::new();
    if e.close() != Err(MV_STAGE) {
        return false;
    }
    // walk 拒绝越界与倒退。
    let ok = [Stage::Check, Stage::Source, Stage::Scope];
    if Gate::walk(&ok).is_err() {
        return false;
    }
    let bad = [Stage::Check, Stage::Exit, Stage::Scope];
    if Gate::walk(&bad) != Err(MV_STAGE) {
        return false;
    }
    if !g.render().as_str().contains("closed=true") {
        return false;
    }
    true
}

/// 13 账本：登记、复核、冻结、重放（第 13 条判据的谓词部分）。
fn ledger_ok() -> bool {
    let mut l = build_ledger();
    if l.len() != SAMPLES || l.is_empty() {
        return false;
    }
    if l.ok_count() != STAT_CELLS + 1 || l.err_count() != REJECT {
        return false;
    }
    if l.audit() != Ok(()) {
        return false;
    }
    if l.first_mismatch().is_some() {
        return false;
    }
    let h = match l.freeze() {
        Ok(x) => x,
        Err(_) => return false,
    };
    if h == 0 || !l.is_frozen() || l.root() != h {
        return false;
    }
    if l.replay() != Ok(h) {
        return false;
    }
    // 冻结后拒登记、拒 poke。
    let s = Sample {
        slot: 0,
        op: 1,
        ok: true,
        code: 0,
        value: 1,
    };
    if l.register(s) != Err(MV_ESTATE) {
        return false;
    }
    if l.poke(0, 7) != Err(MV_ESTATE) {
        return false;
    }
    // 注入一维错：复核必须当场指出来。
    let b0 = build_ledger_broken(0);
    if b0.audit() != Err(MV_VALUE) {
        return false;
    }
    let b1 = build_ledger_broken(1);
    if b1.audit() != Err(MV_VALUE) {
        return false;
    }
    let b2 = build_ledger_broken(2);
    if b2.audit() != Err(MV_VALUE) {
        return false;
    }
    if build_ledger_broken(9).audit() != Ok(()) {
        return false;
    }
    // 跳号登记：拒收并计数。
    let mut j = Ledger::new();
    let skip = Sample {
        slot: 3,
        op: 1,
        ok: true,
        code: 0,
        value: 1,
    };
    if j.register(skip) != Err(MV_SLOT) {
        return false;
    }
    if j.rejects() != 1 || j.len() != 0 {
        return false;
    }
    if j.render().as_str().len() == 0 {
        return false;
    }
    true
}

/// 申报表第一格的标签。
pub fn head_tag() -> &'static str {
    match WANT_TAGS.get(0) {
        Some(x) => *x,
        None => "mv-clean-fit",
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
pub fn run_unx11687_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11687_mv_cross_enospc_rollback");
    s.add("MV01-名称与段基址", name_ok(), "static");
    s.add("MV02-十三码互异", codes_ok(), "static");
    s.add("MV03-目的地状态机", dest_ok(), "static");
    s.add("MV04-回滚不变量", rollback_ok(), "static");
    s.add("MV05-预存目标保护", preexisting_ok(), "static");
    s.add("MV06-源件保留", source_ok(), "static");
    s.add("MV07-文案逐字", text_ok(), "static");
    s.add("MV08-语料十四槽互校", corpus_ok(), "static");
    s.add("MV09-判序六维互斥", judge_ok(), "static");
    s.add("MV10-四档矩阵覆盖", matrix_ok(), "static");
    s.add("MV11-全绿渲染逐字", render_ok(), "static");
    s.add("MV12-阶段闸门不倒退", gate_ok(), "static");
    s.add(
        "MV13-冻结重放四注入",
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
            audited && frozen && ledger_ok() && all_four
        },
        "static",
    );
    s
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t01_seg_base_is_pinned() {
        assert_eq!(SEG_BASE, 0x6d330);
        assert_eq!(ANCHOR_F11686_SEG + ANCHOR_F11686_GAP, SEG_BASE);
        assert_eq!(ANCHOR_F11685_SEG + ANCHOR_F11685_GAP, SEG_BASE);
        assert_eq!(SEG_BASE + ANCHOR_F11694_GAP, ANCHOR_F11694_SEG);
        assert_eq!(SEG_BASE + ANCHOR_F11699_GAP, ANCHOR_F11699_SEG);
        assert!(anchors_agree());
    }

    #[test]
    fn t02_thirteen_codes_live_inside_the_segment() {
        assert_eq!(ALL_CODES.len(), 13);
        let mut i = 0usize;
        while i < ALL_CODES.len() {
            assert!(ALL_CODES[i] > SEG_BASE);
            assert!(ALL_CODES[i] <= SEG_BASE + 0x0d);
            i += 1;
        }
    }

    #[test]
    fn t03_thirteen_codes_are_pairwise_distinct() {
        let mut i = 0usize;
        while i < ALL_CODES.len() {
            let mut j = i + 1usize;
            while j < ALL_CODES.len() {
                assert_ne!(ALL_CODES[i], ALL_CODES[j]);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn t04_reject_codes_follow_the_fault_order() {
        let faults = Fault::reject_faults();
        assert_eq!(faults.len(), REJECT);
        let mut k = 0usize;
        while k < REJECT {
            assert_eq!(REJECT_CODES[k], faults[k].code());
            assert_eq!(REJECT_FAULTS[k], faults[k]);
            k += 1;
        }
        assert_eq!(Fault::Ok.code(), 0);
    }

    #[test]
    fn t05_vals_cells_are_the_pinned_statistics() {
        assert_eq!(VALS[0] as usize, FAMILY_SCENES);
        assert_eq!(VALS[1] as usize, ROLLBACK_SCENES);
        assert_eq!(VALS[2] as usize, INTACT_SCENES);
        assert_eq!(VALS[3], 1);
        assert_eq!(VALS[4] as usize, FAMILY_SCENES);
        assert_eq!(VALS[5], 0);
        assert_eq!(VALS[6], RATE_FULL);
        assert_eq!(VALS[7], RED_RATE);
    }

    #[test]
    fn t06_fold_helpers_are_not_identities() {
        let h = FNV_OFF;
        assert_eq!(fold_byte(h, b'A'), fold_str(h, "A"));
        assert_ne!(fold_word(h, 0), h);
        assert_ne!(fold_bool(h, false), fold_bool(h, true));
        assert_ne!(fold_i32(h, -1), fold_i32(h, 1));
        assert_ne!(fold_word(h, 1), fold_word(h, 2));
    }

    #[test]
    fn t07_decimal_and_hex_renderers() {
        assert_eq!(dec_text(0).as_str(), "0");
        assert_eq!(dec_text(28).as_str(), "28");
        assert_eq!(dec_text(4096).as_str(), "4096");
        assert_eq!(u32_text(929).as_str(), "929");
        assert_eq!(hex_text(0x0b, 2).as_str(), "0b");
        assert_eq!(hex_text(SEG_BASE, 6).as_str(), "06d330");
        assert_eq!(hex_text(1, 0).as_str(), "");
    }

    #[test]
    fn t08_boolean_and_segment_text() {
        assert_eq!(bool_text(true).as_str(), "true");
        assert_eq!(bool_text(false).as_str(), "false");
        assert_eq!(i32_text(-28).as_str(), "-28");
        assert_eq!(i32_text(0).as_str(), "0");
        assert_eq!(seg_text(SEG_BASE).as_str(), "0x06d330");
        assert_eq!(permille_text(929).as_str(), "929permille");
    }

    #[test]
    fn t09_destination_state_machine_is_legal() {
        assert!(Destination::Absent.may_step_to(Destination::Partial(1)));
        assert!(Destination::Partial(1).may_step_to(Destination::Absent));
        assert!(Destination::Partial(1).may_step_to(Destination::Complete(2)));
        assert!(Destination::Complete(2).may_step_to(Destination::Absent));
        assert!(!Destination::Complete(2).may_step_to(Destination::Partial(1)));
        assert!(Destination::Partial(2).may_step_to(Destination::Partial(1)));
        assert!(!Destination::Complete(2).may_step_to(Destination::Partial(0)));
        assert!(dest_ok());
    }

    #[test]
    fn t10_partial_zero_is_not_absent() {
        let p = Destination::Partial(0);
        assert!(p.is_partial());
        assert!(!p.is_absent());
        assert_eq!(p.bytes(), 0);
        assert_ne!(p.fold(FNV_OFF), Destination::Absent.fold(FNV_OFF));
        assert_eq!(p.tag(), "partial");
        assert_eq!(Destination::Absent.tag(), "absent");
    }

    #[test]
    fn t11_clean_copy_moves_the_whole_file() {
        let o = define(&SHAPE_CLEAN_FIT);
        assert!(o.ok());
        assert_eq!(o.stage, Stage::Verify);
        assert_eq!(o.value, BLOCK);
        assert_eq!(o.note, Note::Copied);
        assert_eq!(o.code, 0);
        assert_eq!(Tier::of(&o), Tier::Intact);
    }

    #[test]
    fn t12_rollback_keeps_the_source() {
        let s = Shape::rollback(BLOCK * 2, BLOCK);
        let o = define(&s);
        assert!(!o.ok());
        assert_eq!(o.note, Note::RolledBack);
        assert_eq!(o.stage, Stage::Exit);
        assert_eq!(o.value, BLOCK);
        assert!(!s.src_removed);
        assert_eq!(Tier::of(&o), Tier::Rollback);
    }

    #[test]
    fn t13_a_left_partial_is_the_cardinal_sin() {
        let mut s = Shape::rollback(BLOCK * 2, BLOCK);
        s.dest1 = Destination::Partial(BLOCK);
        let o = define(&s);
        assert_eq!(o.note, Note::PartialLeft);
        assert_eq!(o.code, MV_ROLLBACK);
        assert!(!o.ok());
        assert_eq!(Tier::of(&o), Tier::Residue);
        assert!(!o.render().as_str().is_empty());
    }

    #[test]
    fn t14_preexisting_target_survives_enospc() {
        let o = define(&SHAPE_ENOSPC_KEEP);
        assert_eq!(o.note, Note::ExistingKept);
        assert_eq!(o.value, BLOCK);
        assert!(!o.ok());
        assert_eq!(o.stage, Stage::Exit);
        assert_eq!(Tier::of(&o), Tier::Intact);
        assert!(o.rolled_back());
    }

    #[test]
    fn t15_preexisting_target_clobber_is_caught() {
        let mut s = SHAPE_ENOSPC_KEEP;
        s.preexisting_ok = false;
        let o = define(&s);
        assert_eq!(o.note, Note::Clobbered);
        assert_eq!(o.code, MV_CLOBBER);
        assert_eq!(Tier::of(&o), Tier::Breach);
    }

    #[test]
    fn t16_complete_overwrite_is_not_clobber() {
        let s = SHAPE_OVERWRITE_COMPLETE;
        assert!(s.preexisting_ok == false);
        let o = define(&s);
        assert_eq!(o.note, Note::Copied);
        assert!(o.ok());
        assert_eq!(o.value, BLOCK);
    }

    #[test]
    fn t17_same_device_mv_is_rename() {
        let s = SHAPE_SAME_DEV;
        assert!(s.route.copies_data() == false);
        assert!(s.route.can_enospc() == false);
        let o = define(&s);
        assert!(o.ok());
        assert_eq!(o.stage, Stage::Unlink);
        assert_eq!(o.note, Note::RenameNoCopy);
        assert_eq!(o.value, 0);
    }

    #[test]
    fn t18_empty_source_copies_nothing() {
        let s = Shape::clean(0);
        let o = define(&s);
        assert!(o.ok());
        assert_eq!(o.value, 0);
        assert_eq!(o.note, Note::Copied);
        assert_eq!(o.stage, Stage::Verify);
    }

    #[test]
    fn t19_deleting_the_source_on_failure_is_fatal() {
        let mut s = Shape::rollback(BLOCK * 2, BLOCK);
        s.src_removed = true;
        let o = define(&s);
        assert_eq!(o.note, Note::SourceLost);
        assert_eq!(o.code, MV_SOURCE);
        assert_eq!(o.stage, Stage::Unlink);
        assert_eq!(Tier::of(&o), Tier::Breach);
    }

    #[test]
    fn t20_corpus_split_matches_the_contract() {
        assert_eq!(CORPUS, ACCEPT + REJECT);
        assert_eq!(cases().len(), CORPUS);
        assert_eq!(SHAPES.len(), CORPUS);
        assert_eq!(CLAIMS.len(), CORPUS);
        assert!(case_at(CORPUS).is_none());
        assert!(corpus_ok());
    }

    #[test]
    fn t21_corpus_tags_are_unique_and_numbered() {
        let mut i = 0usize;
        while i < CORPUS {
            let c = match case_at(i) {
                Some(x) => x,
                None => return,
            };
            assert_eq!(c.tag(), WANT_TAGS[i]);
            let mut j = i + 1usize;
            while j < CORPUS {
                assert_ne!(WANT_TAGS[i], WANT_TAGS[j]);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn t22_accept_slots_are_judged_clean() {
        let mut i = 0usize;
        while i < ACCEPT {
            let c = match case_at(i) {
                Some(x) => x,
                None => return,
            };
            assert_eq!(c.fault(), Fault::Ok);
            assert_eq!(c.claim.diff_mask(&c.actual()), 0);
            i += 1;
        }
    }

    #[test]
    fn t23_reject_slots_hit_their_own_dimension() {
        let mut k = ACCEPT;
        while k < CORPUS {
            let c = match case_at(k) {
                Some(x) => x,
                None => return,
            };
            assert_eq!(c.fault(), expected_reject_ndx(k));
            assert_eq!(c.claim.diff_mask(&c.actual()), 1u32 << (k - ACCEPT));
            k += 1;
        }
    }

    #[test]
    fn t24_want_tables_restate_the_real_outcome() {
        let mut i = 0usize;
        while i < CORPUS {
            let c = match case_at(i) {
                Some(x) => x,
                None => return,
            };
            let a = c.actual();
            assert_eq!(a.stage, WANT_STAGE[i]);
            assert_eq!(a.code, WANT_CODE[i]);
            assert_eq!(a.value, WANT_VALUE[i]);
            assert_eq!(a.note, WANT_NOTE[i]);
            assert_eq!(a.default, WANT_DEFAULT[i]);
            assert_eq!(a.ok(), WANT_SUCCESS[i]);
            assert_eq!(c.shape.exit, WANT_EXIT[i]);
            i += 1;
        }
    }

    #[test]
    fn t25_estate_wins_over_every_other_dimension() {
        let s = match scene_at(0) {
            Some(x) => x,
            None => return,
        };
        let a = define(&s);
        let c = Case::new(
            s,
            Claim::new(
                "swap",
                !a.ok(),
                Stage::Check,
                MV_SLOT,
                a.value + 1,
                Note::Clobbered,
                true,
            ),
        );
        assert_eq!(judge(&c), Fault::Estate);
        assert!(judge_order_ok());
    }

    #[test]
    fn t26_fault_rank_round_trips() {
        let mut r = 1u32;
        while r <= 6 {
            let f = match Fault::from_rank(r) {
                Some(x) => x,
                None => return,
            };
            assert_eq!(f.rank(), r);
            assert_eq!(fault_ndx(f), r as usize - 1);
            assert_eq!(ndx_fault(r as usize - 1), Some(f));
            r += 1;
        }
        assert_eq!(Fault::Ok.rank(), 0);
        assert_eq!(fault_ndx(Fault::Ok), 6);
        assert_eq!(ndx_fault(6), Some(Fault::Ok));
        assert!(Fault::from_rank(7).is_none());
        assert_eq!(Fault::Ok.code(), 0);
        assert!(!Fault::Ok.is_fault());
    }

    #[test]
    fn t27_scene_family_splits_six_and_six() {
        let (rolled, intact) = scene_counts();
        assert_eq!(rolled, ROLLBACK_SCENES);
        assert_eq!(intact, INTACT_SCENES);
        assert_eq!(scenes().len(), FAMILY_SCENES);
        assert!(scene_at(FAMILY_SCENES).is_none());
        assert!(scene_anchored(FAMILY_SCENES).is_none());
    }

    #[test]
    fn t28_every_scene_is_anchored_and_self_consistent() {
        assert!(scenes_agree());
        let mut i = 0usize;
        while i < FAMILY_SCENES {
            assert_eq!(scene_anchored(i), Some(true));
            let c = match scene_case(i) {
                Some(x) => x,
                None => return,
            };
            assert_eq!(judge(&c), Fault::Ok);
            assert_ne!(scene_seal(i), None);
            i += 1;
        }
        assert!(scene_seal(FAMILY_SCENES).is_none());
    }

    #[test]
    fn t29_matrix_covers_all_four_tiers() {
        let m = Matrix::build();
        assert!(m.covered());
        assert_eq!(m.slots(), CORPUS * TIER_COUNT);
        assert_eq!(m.goods(), 10);
        assert_eq!(m.row(Tier::Rollback).map(|r| r.good), Some(2));
        assert_eq!(m.row(Tier::Intact).map(|r| r.good), Some(6));
        assert_eq!(m.row(Tier::Residue).map(|r| r.good), Some(1));
        assert_eq!(m.row(Tier::Breach).map(|r| r.good), Some(1));
        assert!(m.render().as_str().starts_with("matrix "));
    }

    #[test]
    fn t30_report_renders_the_pinned_green_line() {
        let rep = match run() {
            Ok(r) => r,
            Err(_) => return,
        };
        assert!(rep.green());
        assert_eq!(rep.rate(), 1000);
        assert_eq!(rep.render().as_str(), RENDER_OK);
        assert_eq!(rep.gate(), Ok(()));
        assert_eq!(rep.reds(), 0);
        assert_eq!(rep.tier_cells(), FAMILY_SCENES as u32);
    }

    #[test]
    fn t31_injection_costs_the_numerator() {
        let base = match run() {
            Ok(r) => r,
            Err(_) => return,
        };
        let bad = match run_with(Some((2, Fault::Value))) {
            Ok(r) => r,
            Err(_) => return,
        };
        assert!(bad.matched < base.matched);
        assert!(bad.conflicts > base.conflicts);
        assert!(bad.rate() < 1000);
        assert!(!bad.green());
        assert_eq!(bad.gate(), Err(MV_NOTE));
        assert!(red_rate_calibrated());
    }

    #[test]
    fn t32_out_of_range_injection_is_refused() {
        assert_eq!(run_with(Some((CORPUS, Fault::Value))), Err(MV_SLOT));
        assert_eq!(run_with(Some((CORPUS + 9, Fault::Stage))), Err(MV_SLOT));
        assert!(run_with(Some((0, Fault::Ok))).is_ok());
    }

    #[test]
    fn t33_gate_walks_only_forward() {
        let root = match Gate::full_pass() {
            Ok(r) => r,
            Err(_) => return,
        };
        assert_ne!(root, 0);
        let mut g = Gate::new();
        assert_eq!(g.step(Stage::Check), Ok(()));
        assert_eq!(g.step(Stage::Check), Ok(()));
        assert_eq!(g.step(Stage::Write), Ok(()));
        assert_eq!(g.step(Stage::Dest), Err(MV_STAGE));
        assert_eq!(g.stage(), Some(Stage::Write));
        assert_eq!(g.refusals(), 1);
        assert_eq!(g.steps(), 3);
        assert!(g.render().as_str().contains("steps=3"));
    }

    #[test]
    fn t34_gate_refuses_after_close() {
        let mut g = Gate::new();
        assert_eq!(g.close(), Err(MV_STAGE));
        assert_eq!(g.step(Stage::Check), Ok(()));
        let h = match g.close() {
            Ok(x) => x,
            Err(_) => return,
        };
        assert_ne!(h, 0);
        assert!(g.is_closed());
        assert_eq!(g.step(Stage::Exit), Err(MV_ESTATE));
        assert_eq!(g.close(), Err(MV_ESTATE));
    }

    #[test]
    fn t35_ledger_registers_and_audits() {
        let mut l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), STAT_CELLS + 1);
        assert_eq!(l.err_count(), REJECT);
        assert_eq!(l.audit(), Ok(()));
        let h = match l.freeze() {
            Ok(x) => x,
            Err(_) => return,
        };
        assert_ne!(h, 0);
        assert_eq!(l.replay(), Ok(h));
    }

    #[test]
    fn t36_ledger_rejects_out_of_order_slots() {
        let mut l = Ledger::new();
        assert!(l.is_empty());
        let s = Sample {
            slot: 1,
            op: 1,
            ok: true,
            code: 0,
            value: 1,
        };
        assert_eq!(l.register(s), Err(MV_SLOT));
        assert_eq!(l.rejects(), 1);
        let first = Sample {
            slot: 0,
            op: 1,
            ok: true,
            code: 0,
            value: 1,
        };
        assert_eq!(l.register(first), Ok(()));
        assert_eq!(l.len(), 1);
    }

    #[test]
    fn t37_broken_ledgers_are_caught() {
        assert_eq!(build_ledger_broken(0).audit(), Err(MV_VALUE));
        assert_eq!(build_ledger_broken(1).audit(), Err(MV_VALUE));
        assert_eq!(build_ledger_broken(2).audit(), Err(MV_VALUE));
        assert_eq!(build_ledger_broken(7).audit(), Ok(()));
        assert_eq!(ledger_ok(), true);
    }

    #[test]
    fn t38_messages_quote_the_verbatim_kernel_text() {
        let dst = "/tmp/bx-sandbox/b/blob";
        assert_eq!(
            msg_mv_enospc(dst).as_str(),
            "mv: cannot create regular file '/tmp/bx-sandbox/b/blob': No space left on device"
        );
        assert_eq!(
            msg_mv_enospc_busybox(dst).as_str(),
            "mv: can't create '/tmp/bx-sandbox/b/blob': No space left on device"
        );
        assert!(text_ok());
    }

    #[test]
    fn t39_all_thirteen_checks_are_green() {
        let s = run_unx11687_checks();
        assert_eq!(s.len(), 13);
        let red = s.red_items_text();
        assert!(s.all_passed(), "red: {}", red);
    }

    #[test]
    fn t40_inject_four_routes_are_independent() {
        let four = inject_four();
        assert_eq!(four.len(), 4);
        assert!(four[0]);
        assert!(four[1]);
        assert!(four[2]);
        assert!(four[3]);
        assert_eq!(head_tag(), "mv-clean-fit");
        assert_eq!(head_val(), FAMILY_SCENES as u32);
    }
}
