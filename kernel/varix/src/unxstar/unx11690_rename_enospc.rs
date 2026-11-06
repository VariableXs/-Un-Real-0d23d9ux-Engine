//! UNX-F11690 rename ENOSPC 判据
//!
//! `rename(2)` 是同设备、同文件系统内的一条**纯元数据**操作：它把目录项从
//! 一个名字挪到另一个名字，不复制数据块、不改 inode 号、不改文件长度。于是
//! 本卡的第一条事实是反直觉的：
//!
//! > **同盘 rename 不会因为磁盘满而失败。**
//!
//! 满了的盘仍然能把 `a` 改名成 `b`——因为这件事一个数据块都不需要分配。
//! 能失败的是另外几条：`EXDEV`（跨设备，内核根本不做）、`ENOENT`（源/父目录
//! 不存在）、`EACCES`（父目录不可写）、`ENOTDIR`/`EISDIR`（目标类型冲突）。
//! `ENOSPC` 不在这个名单里。所以当某个工具在 rename 这条路径上打印
//! `No space left on device` 时，只有两种可能：
//!
//! 1. 它其实走的是**跨盘 fallback**（`rename` 返回 `EXDEV` 之后，工具退化成
//!    copy-then-unlink）。此时 ENOSPC 属于那次**复制**，不属于 rename；把两件
//!    事混成一句"rename 失败：磁盘满"，就是贴错了标签。
//! 2. 它压根没调 rename，或者把别人的 errno 抄了过来。这就是**假标签**。
//!
//! 本卡判的正是这张标签：
//!
//! ```text
//! $ mv /tmp/bx-sandbox/a/x /tmp/bx-sandbox/a/y     # 同盘
//! $ echo $?                                        # 0，且绝不出现 ENOSPC
//! 0
//! $ rename /mnt/a/x /mnt/b/y                       # 跨盘
//! rename: Invalid cross-device link                # EXDEV，交给 fallback
//! ```
//!
//! 同盘报 ENOSPC → 判死（`RN_FALSE`）；跨盘把 fallback 的 ENOSPC 记在 rename
//! 头上 → 同样判死；跨盘 rename 如实报 `EXDEV` 并交给 fallback → 放行。三类
//! 现场的边界就是本卡的全部深度。
//!
//! ## 闸门次序（这条纵线为什么不能倒）
//!
//! 八道闸，秩 1..8，一次只许前进；倒退 → `RN_STAGE`，封账后再动 → `RN_ESTATE`：
//!
//! 1. **Check**（秩 1）——两个操作数都在、没有 `-t`/`-T` 之类的用法错。
//! 2. **Resolve**（秩 2）——源与目标路径可解析：父目录存在、名字合法。
//!    Resolve 不产出事实，后面就不知道"失败该记在谁头上"。
//! 3. **Scope**（秩 3）——同设备还是跨设备。**这条闸是本卡的命门**：同设备
//!    走 rename，跨设备必然 EXDEV；把 Scope 判错，后面所有归因都错。
//! 4. **Atomic**（秩 4）——VFS 的 rename 本体：同设备在这一闸一次提交，
//!    要么旧名在、要么新名在，不存在"半改名"。
//! 5. **Journal**（秩 5）——元数据日志落盘。它可能失败（EIO），但**不会**
//!    因为数据区满而失败；ENOSPC 在这一闸出现即为假标签。
//! 6. **Fallback**（秩 6）——只有跨设备才会进入：`EXDEV` 之后由工具做
//!    copy-then-unlink。ENOSPC 只可能在这一闸产生，并且必须记在 fallback 上。
//! 7. **Report**（秩 7）——errno 与文案归属。同设备的 errno 集与跨设备
//!    fallback 的 errno 集是**两个集合**，交集里没有 ENOSPC。
//! 8. **Exit**（秩 8）——退出码：0 = 改名/复制成功，1 = 失败（含 EXDEV 未
//!    fallback、fallback ENOSPC），2 = 用法错。
//!
//! 次序为什么不能倒：Scope 必须早于 Atomic（不然无法解释 EXDEV 从哪来）；
//! Atomic 必须早于 Journal（先改目录项再记日志是文件系统自己的次序，判据面
//! 照抄，不许把日志失败当成改名失败）；Fallback 必须早于 Report（先有 fallback
//! 的事实，才谈得上把 ENOSPC 归给它）。
//!
//! ## 关键差异（本卡深度所在，谁判死谁放行）
//!
//! * **与 F11687（mv 跨盘 ENOSPC 回滚）分层**：F11687 判"fallback 复制到一半
//!   没空间之后怎么收场"（源保留、半成品回滚、预存目标不毁）；本卡判"这条
//!   ENOSPC 到底该不该记在 rename 头上"。一句话：F11687 管**真的有 ENOSPC**，
//!   本卡管**不该有的 ENOSPC 被伪造出来**。两卡共享 `EXDEV` 这个上游事实，
//!   但本卡到"归因"为止，不回滚、不删源。
//! * **与 F11689（df 0 avail）分层**：df 卡判"旁观者看到的 avail 为 0"；
//!   本卡判"改名的人在 avail 为 0 时仍然成功"。同一块满盘，一个读数、一个
//!   行为，df=0 与 rename 成功可以同时成立，这正是本卡要钉的反直觉。
//! * **与 F11694（文案逐字）分层**：文案卡判 `strerror(28)` 的拼写；本卡判
//!   这句拼写**被贴到了哪条路径上**。文案对、归属错，仍是本卡的红。
//! * **与 F11691（fsync ENOSPC）分层**：fsync 卡的 ENOSPC 来自延迟分配的
//!   回写；rename 不写数据，因此本卡与 fsync 卡在 errno 面上同数、在成因面
//!   上互斥。把 rename 的 ENOSPC 解释成"回写延迟"，就是把元数据操作当成了
//!   数据操作。
//! * **原子性 vs 归因**：本卡只判"没有半改名"（Atomic 闸）与"errno 归属"
//!   （Report 闸）。半改名在真实内核里不会发生，所以本卡的 `RN_ATOMIC` 是
//!   一条**防伪造**码：任何声称"改名改了一半"的现场，都是观测层伪造。
//! * **假标签必须显式成档**：`FalseEnospc` 不许并进 `FallbackEnospc`。前者
//!   是谎报（红），后者是如实归因（绿）。两档在 `Tier::of` 里分属
//!   `FalseLabel` 与 `Fallback`，任何合并都会让本卡失去判据价值。
//! * **红档率钉在 929**：与全批统一，`VALS` 第八格是 929，用来让注入一格红
//!   之后的结算立刻露出。
//!
//! 探针一律在 `/tmp/bx-sandbox/` 内建模；本卡从不写宿主真实库路径。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 本卡段基址（全仓唯一，VX5 分配表给定，不许改）。
pub const SEG_BASE: u32 = 0x6d370;

// ---------------------------------------------------------------------------
// 联签锚：本卡与哪些卡共用同一条上游事实
// ---------------------------------------------------------------------------

/// 上游卡 F11689（df 0 avail 呈现）的段基址：同批 B25 的前一张。
pub const ANCHOR_F11689_SEG: u32 = 0x6d360;
/// F11689 与本卡的段差（0x6d370 - 0x6d360 = 0x10）。
pub const ANCHOR_F11689_GAP: u32 = 0x10;

/// 下游卡 F11691（fsync 与 ENOSPC）的段基址。
pub const ANCHOR_F11691_SEG: u32 = 0x6d390;
/// F11691 与本卡的段差（0x6d390 - 0x6d370 = 0x20）。
pub const ANCHOR_F11691_GAP: u32 = 0x20;

/// 回滚卡 F11687（mv 跨盘 ENOSPC）的段基址：fallback 面的归属方。
pub const ANCHOR_F11687_SEG: u32 = 0x6d330;
/// F11687 与本卡的段差（0x6d370 - 0x6d330 = 0x40）。
pub const ANCHOR_F11687_GAP: u32 = 0x40;

/// 双机快照卡 F11699 的段基址。
pub const ANCHOR_F11699_SEG: u32 = 0x6d430;
/// F11699 与本卡的段差（0x6d430 - 0x6d370 = 0xc0）。
pub const ANCHOR_F11699_GAP: u32 = 0xc0;

/// 上游 errno：`ENOENT`。
pub const ANCHOR_ENOENT: u32 = 2;
/// 上游 errno：`EACCES`。
pub const ANCHOR_EACCES: u32 = 13;
/// 上游 errno：`EXDEV`——跨设备 rename 的固定回执。
pub const ANCHOR_EXDEV: u32 = 18;
/// 上游 errno：`ENOSPC`——本卡判"不该出现在 rename 面"的那一条。
pub const ANCHOR_ENOSPC: u32 = 28;
/// `strerror(18)` 的逐字文本。
pub const ANCHOR_EXDEV_TEXT: &str = "Invalid cross-device link";
/// `strerror(28)` 的逐字文本。
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
/// 闸门数（八道）。
pub const STAGE_COUNT: u32 = 8;
/// 最末闸的秩。
pub const STAGE_LAST_RANK: u32 = 8;
/// 账本统计格数。
pub const STAT_CELLS: usize = 7;
/// 红档率千分比（全批统一钉值）。
pub const RED_RATE: u32 = 929;
/// 满一致千分比。
pub const RATE_FULL: u32 = 1000;
/// 族内场景数：四条同盘 + 八条跨盘。
pub const FAMILY_SCENES: usize = 12;
/// 同盘场景数。
pub const ATOMIC_SCENES: usize = 4;
/// 跨盘场景数。
pub const FALLBACK_SCENES: usize = 8;
/// fallback 里撞 ENOSPC 的场景数。
pub const ENOSPC_SCENES: usize = 3;
/// 统计格对应的算子号。
pub const STAT_OPS: [u8; STAT_CELLS] = [1, 2, 3, 4, 5, 6, 7];
/// 红档格在账本里的算子号。
pub const OPID_STAT_RED: u8 = 0x08;
/// 六个缺陷码格的算子号起点。
pub const DEFECT_OP_BASE: u8 = 0x31;

// ---------------------------------------------------------------------------
// 目录项与文案侧常量
// ---------------------------------------------------------------------------

/// 一个文件系统块的字节数。
pub const BLOCK: u32 = 4096;
/// 半块。
pub const HALF_BLOCK: u32 = 2048;
/// 沙盒前缀。
pub const SANDBOX_PREFIX: &str = "/tmp/bx-sandbox/";
/// 同盘源路径。
pub const SRC_SAME: &str = "/tmp/bx-sandbox/a/x";
/// 同盘目标路径。
pub const DST_SAME: &str = "/tmp/bx-sandbox/a/y";
/// 跨盘目标路径。
pub const DST_CROSS: &str = "/tmp/bx-sandbox/b/y";
/// 退出码：成功。
pub const EXIT_OK: u32 = 0;
/// 退出码：失败。
pub const EXIT_FAIL: u32 = 1;
/// 退出码：用法错。
pub const EXIT_USAGE: u32 = 2;
/// 未判档。
pub const EXIT_UNJUDGED: u32 = 99;
/// 退出码档位数。
pub const EXIT_RANKS: usize = 4;
/// 满一致（矩阵侧独立复写）。
pub const MATCH_FULL: u32 = 1000;

// ---------------------------------------------------------------------------
// 十三错误码（前缀 RN = rename）
// ---------------------------------------------------------------------------

/// 01 槽位/用法：注入槽越界、账本跳号、操作数缺失。
pub const RN_SLOT: u32 = SEG_BASE + 0x01;
/// 02 成败互报（Estate 维出口）。
pub const RN_ESTATE: u32 = SEG_BASE + 0x02;
/// 03 阶段不合（Stage 维出口）。
pub const RN_STAGE: u32 = SEG_BASE + 0x03;
/// 04 码档不合（Code 维出口）。
pub const RN_CODE: u32 = SEG_BASE + 0x04;
/// 05 数值不合（Value 维出口）。
pub const RN_VALUE: u32 = SEG_BASE + 0x05;
/// 06 注记不合（Note 维出口）。
pub const RN_NOTE: u32 = SEG_BASE + 0x06;
/// 07 兜底档（Default 维出口）。
pub const RN_DEFAULT: u32 = SEG_BASE + 0x07;
/// 08 指纹不符。
pub const RN_SEAL: u32 = SEG_BASE + 0x08;
/// 09 矩阵覆盖缺口。
pub const RN_MATRIX: u32 = SEG_BASE + 0x09;
/// 10 一致率不足。
pub const RN_RATE: u32 = SEG_BASE + 0x0a;
/// 11 假 ENOSPC 标签：rename 面报了本不属于它的空间错误。
pub const RN_FALSE: u32 = SEG_BASE + 0x0b;
/// 12 原子性被伪造：现场声称"改名改了一半"。
pub const RN_ATOMIC: u32 = SEG_BASE + 0x0c;
/// 13 EXDEV 误用：同设备报跨设备，或跨设备把 EXDEV 吞掉。
pub const RN_XDEV: u32 = SEG_BASE + 0x0d;

/// 十三码全表。
pub const ALL_CODES: [u32; 13] = [
    RN_SLOT,
    RN_ESTATE,
    RN_STAGE,
    RN_CODE,
    RN_VALUE,
    RN_NOTE,
    RN_DEFAULT,
    RN_SEAL,
    RN_MATRIX,
    RN_RATE,
    RN_FALSE,
    RN_ATOMIC,
    RN_XDEV,
];

/// 反例六维对应的码。
pub const REJECT_CODES: [u32; REJECT] = [
    RN_ESTATE,
    RN_STAGE,
    RN_CODE,
    RN_VALUE,
    RN_NOTE,
    RN_DEFAULT,
];

/// 正例八格统计数：族场景 / 同盘 / 跨盘 / fallback ENOSPC / 文案逐字 /
/// 假标签 / 一致率 / 红档率。
pub const VALS: [u32; ACCEPT] = [12, 4, 8, 3, 12, 0, 1000, 929];

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

/// 折一个 u64。
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

/// 折一个 i32。
fn fold_i32(h: u64, v: i32) -> u64 {
    fold_word(h, v as u32 as u64)
}

/// 折一个布尔。
fn fold_bool(h: u64, v: bool) -> u64 {
    fold_byte(h, if v { 1 } else { 0 })
}

// ---------------------------------------------------------------------------
// 数字/布尔文本
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

/// u32 → 固定宽度十六进制文本。
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

/// u32 → 文本。
pub fn u32_text(v: u32) -> String {
    dec_text(v)
}

/// i32 → 文本。
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

/// 段址 → 文本。
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
// 逐字文案
// ---------------------------------------------------------------------------

/// mv 的 rename 失败行前缀（`cannot move`）。
pub const T_MOVE_PREFIX: &str = "mv: cannot move '";
/// mv 的 fallback 建件失败行前缀。
pub const T_CREATE_PREFIX: &str = "mv: cannot create regular file '";
/// 两段路径之间的连接词。
pub const T_TO: &str = "' to '";
/// 引号尾巴。
pub const T_QUOTE_TAIL: &str = "': ";
/// rename 工具自己的失败行前缀。
pub const T_RENAME_PREFIX: &str = "rename: ";
/// mv -v 的成功行前缀。
pub const T_RENAMED_PREFIX: &str = "renamed '";
/// 空串。
pub const TEXT_EMPTY: &str = "";

/// 拼一条 `前缀 + 路径 + 尾巴` 的文本。
pub fn join_text(prefix: &str, path: &str, tail: &str) -> String {
    let mut s = String::from(prefix);
    s.push_str(path);
    s.push_str(tail);
    s
}

/// 同盘 rename 行：`mv: cannot move '<src>' to '<dst>': <tail>`。
pub fn msg_move(src: &str, dst: &str, tail: &str) -> String {
    let mut s = String::from(T_MOVE_PREFIX);
    s.push_str(src);
    s.push_str(T_TO);
    s.push_str(dst);
    s.push_str(T_QUOTE_TAIL);
    s.push_str(tail);
    s
}

/// 假标签原文：同盘 rename 报 ENOSPC（本卡的红）。
pub fn msg_false_enospc(src: &str, dst: &str) -> String {
    msg_move(src, dst, ANCHOR_ENOSPC_TEXT)
}

/// 跨设备 rename 的如实回执。
pub fn msg_exdev(src: &str, dst: &str) -> String {
    msg_move(src, dst, ANCHOR_EXDEV_TEXT)
}

/// 裸 rename 工具的回执。
pub fn msg_rename_exdev() -> String {
    let mut s = String::from(T_RENAME_PREFIX);
    s.push_str(ANCHOR_EXDEV_TEXT);
    s
}

/// 跨盘 fallback 的建件失败行（ENOSPC 的正确归属）。
pub fn msg_fallback_enospc(dst: &str) -> String {
    let mut s = String::from(T_CREATE_PREFIX);
    s.push_str(dst);
    s.push_str(T_QUOTE_TAIL);
    s.push_str(ANCHOR_ENOSPC_TEXT);
    s
}

/// mv -v 的成功行。
pub fn msg_renamed(src: &str, dst: &str) -> String {
    let mut s = String::from(T_RENAMED_PREFIX);
    s.push_str(src);
    s.push_str(T_TO);
    s.push_str(dst);
    s.push('\'');
    s
}

/// 用法错原文。
pub fn msg_usage_missing() -> String {
    String::from("rename: missing operand")
}

/// 用法错原文（多余操作数）。
pub fn msg_usage_extra() -> String {
    String::from("rename: extra operand '/tmp/bx-sandbox/b/y'")
}

/// 同盘成功的呈现行。
pub fn msg_atomic_done(src: &str, dst: &str) -> String {
    let mut s = String::from("atomic rename ");
    s.push_str(src);
    s.push_str(" -> ");
    s.push_str(dst);
    s
}

/// fallback ENOSPC 的呈现行（正确归因）。
pub fn msg_fallback_attributed(dst: &str, bytes: u32) -> String {
    let mut s = String::from("fallback enospc at ");
    s.push_str(dst);
    s.push(' ');
    s.push_str(dec_text(bytes).as_str());
    s.push_str(" bytes");
    s
}

/// 假标签的呈现行。
pub fn msg_false_label(src: &str, dst: &str) -> String {
    let mut s = String::from("false enospc label on ");
    s.push_str(src);
    s.push_str(" -> ");
    s.push_str(dst);
    s
}

// ---------------------------------------------------------------------------
// 闸门：八道
// ---------------------------------------------------------------------------

/// 本卡的八道闸。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// 秩 1：命令行成立。
    Check,
    /// 秩 2：源与目标路径可解析。
    Resolve,
    /// 秩 3：同设备/跨设备。
    Scope,
    /// 秩 4：rename 本体（同设备在此提交）。
    Atomic,
    /// 秩 5：元数据日志。
    Journal,
    /// 秩 6：跨设备 fallback（copy-then-unlink）。
    Fallback,
    /// 秩 7：errno 与文案归属。
    Report,
    /// 秩 8：退出码。
    Exit,
}

impl Stage {
    /// 八道闸全表。
    pub const ALL: [Stage; 8] = [
        Stage::Check,
        Stage::Resolve,
        Stage::Scope,
        Stage::Atomic,
        Stage::Journal,
        Stage::Fallback,
        Stage::Report,
        Stage::Exit,
    ];

    /// 秩（1..=8）。
    pub fn rank(&self) -> u32 {
        match self {
            Stage::Check => 1,
            Stage::Resolve => 2,
            Stage::Scope => 3,
            Stage::Atomic => 4,
            Stage::Journal => 5,
            Stage::Fallback => 6,
            Stage::Report => 7,
            Stage::Exit => 8,
        }
    }

    /// 短名。
    pub fn slug(&self) -> &'static str {
        match self {
            Stage::Check => "check",
            Stage::Resolve => "resolve",
            Stage::Scope => "scope",
            Stage::Atomic => "atomic",
            Stage::Journal => "journal",
            Stage::Fallback => "fallback",
            Stage::Report => "report",
            Stage::Exit => "exit",
        }
    }

    /// 中文名。
    pub fn cn(&self) -> &'static str {
        match self {
            Stage::Check => "命令行闸",
            Stage::Resolve => "解析闸",
            Stage::Scope => "同跨盘闸",
            Stage::Atomic => "原子闸",
            Stage::Journal => "日志闸",
            Stage::Fallback => "退化闸",
            Stage::Report => "归因闸",
            Stage::Exit => "退出闸",
        }
    }

    /// 由秩还原。
    pub fn from_rank(r: u32) -> Option<Stage> {
        match r {
            1 => Some(Stage::Check),
            2 => Some(Stage::Resolve),
            3 => Some(Stage::Scope),
            4 => Some(Stage::Atomic),
            5 => Some(Stage::Journal),
            6 => Some(Stage::Fallback),
            7 => Some(Stage::Report),
            8 => Some(Stage::Exit),
            _ => None,
        }
    }

    /// 该闸是否允许产生 ENOSPC（只有 fallback 允许）。
    pub fn may_enospc(&self) -> bool {
        matches!(*self, Stage::Fallback)
    }
}

/// 盘间关系。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RenameLine {
    /// 同设备：rename(2) 一次提交。
    SameDev,
    /// 跨设备：rename 必报 EXDEV。
    CrossDev,
}

impl RenameLine {
    /// 四档全表（两档，命名与上游一致）。
    pub const ALL: [RenameLine; 2] = [RenameLine::SameDev, RenameLine::CrossDev];

    /// 短名。
    pub fn slug(&self) -> &'static str {
        match self {
            RenameLine::SameDev => "samedev",
            RenameLine::CrossDev => "crossdev",
        }
    }

    /// 中文名。
    pub fn cn(&self) -> &'static str {
        match self {
            RenameLine::SameDev => "同盘线",
            RenameLine::CrossDev => "跨盘线",
        }
    }

    /// 该线 rename 本体是否可能产生 ENOSPC。
    pub fn rename_can_enospc(&self) -> bool {
        match self {
            RenameLine::SameDev => false,
            RenameLine::CrossDev => false,
        }
    }

    /// 该线是否必然先撞 EXDEV。
    pub fn expects_exdev(&self) -> bool {
        matches!(*self, RenameLine::CrossDev)
    }

    /// 该线是否需要 fallback。
    pub fn needs_fallback(&self) -> bool {
        matches!(*self, RenameLine::CrossDev)
    }
}

/// 现场报出的 errno 档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Errno {
    /// 无错误（成功）。
    None,
    /// `ENOSPC`。
    Enospc,
    /// `EXDEV`。
    Exdev,
    /// `EACCES`。
    Eacces,
    /// `ENOENT`。
    Enoent,
}

impl Errno {
    /// 五档全表。
    pub const ALL: [Errno; 5] = [
        Errno::None,
        Errno::Enospc,
        Errno::Exdev,
        Errno::Eacces,
        Errno::Enoent,
    ];

    /// 短名。
    pub fn slug(&self) -> &'static str {
        match self {
            Errno::None => "none",
            Errno::Enospc => "enospc",
            Errno::Exdev => "exdev",
            Errno::Eacces => "eacces",
            Errno::Enoent => "enoent",
        }
    }

    /// errno 数值（`None` = 0）。
    pub fn errno(&self) -> u32 {
        match self {
            Errno::None => 0,
            Errno::Enospc => ANCHOR_ENOSPC,
            Errno::Exdev => ANCHOR_EXDEV,
            Errno::Eacces => ANCHOR_EACCES,
            Errno::Enoent => ANCHOR_ENOENT,
        }
    }

    /// 逐字文本。
    pub fn text(&self) -> &'static str {
        match self {
            Errno::None => "",
            Errno::Enospc => ANCHOR_ENOSPC_TEXT,
            Errno::Exdev => ANCHOR_EXDEV_TEXT,
            Errno::Eacces => "Permission denied",
            Errno::Enoent => "No such file or directory",
        }
    }

    /// 是否是空间类错误。
    pub fn is_space(&self) -> bool {
        matches!(*self, Errno::Enospc)
    }

    /// 由 errno 还原。
    pub fn from_errno(e: u32) -> Option<Errno> {
        match e {
            0 => Some(Errno::None),
            28 => Some(Errno::Enospc),
            18 => Some(Errno::Exdev),
            13 => Some(Errno::Eacces),
            2 => Some(Errno::Enoent),
            _ => None,
        }
    }
}

/// 注记：一条现场最终落在哪一种收场。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Note {
    /// 同盘 rename 成功（元数据一次提交）。
    MetadataOnly,
    /// 跨盘 rename 如实报 `EXDEV`，工具没有 fallback。
    ExdevOnly,
    /// 跨盘 fallback 复制成功。
    FallbackDone,
    /// 跨盘 fallback 撞 ENOSPC，归因正确（错在复制，不在 rename）。
    FallbackEnospc,
    /// rename 面报了 ENOSPC——假标签（本卡的头号红）。
    FalseEnospc,
    /// 同盘报了 EXDEV——同样不可能，假标签。
    FalseExdev,
    /// 半改名：源与目标一个在、一个不在，原子性被伪造。
    HalfMove,
    /// 用法错。
    Usage,
    /// 未判档。
    Nil,
}

impl Note {
    /// 短名。
    pub fn slug(&self) -> &'static str {
        match self {
            Note::MetadataOnly => "metadata-only",
            Note::ExdevOnly => "exdev-only",
            Note::FallbackDone => "fallback-done",
            Note::FallbackEnospc => "fallback-enospc",
            Note::FalseEnospc => "false-enospc",
            Note::FalseExdev => "false-exdev",
            Note::HalfMove => "half-move",
            Note::Usage => "usage",
            Note::Nil => "nil",
        }
    }

    /// 中文名。
    pub fn cn(&self) -> &'static str {
        match self {
            Note::MetadataOnly => "纯元数据",
            Note::ExdevOnly => "仅跨设备",
            Note::FallbackDone => "退化完成",
            Note::FallbackEnospc => "退化满盘",
            Note::FalseEnospc => "假满盘标签",
            Note::FalseExdev => "假跨设备标签",
            Note::HalfMove => "半改名",
            Note::Usage => "用法错",
            Note::Nil => "未判",
        }
    }

    /// 次序。
    pub fn rank(&self) -> u32 {
        match self {
            Note::MetadataOnly => 1,
            Note::ExdevOnly => 2,
            Note::FallbackDone => 3,
            Note::FallbackEnospc => 4,
            Note::FalseEnospc => 5,
            Note::FalseExdev => 6,
            Note::HalfMove => 7,
            Note::Usage => 8,
            Note::Nil => 9,
        }
    }

    /// 由次序还原。
    pub fn from_rank(r: u32) -> Option<Note> {
        match r {
            1 => Some(Note::MetadataOnly),
            2 => Some(Note::ExdevOnly),
            3 => Some(Note::FallbackDone),
            4 => Some(Note::FallbackEnospc),
            5 => Some(Note::FalseEnospc),
            6 => Some(Note::FalseExdev),
            7 => Some(Note::HalfMove),
            8 => Some(Note::Usage),
            9 => Some(Note::Nil),
            _ => None,
        }
    }

    /// 这条注记是否把 ENOSPC 贴在了 rename 头上。
    pub fn labels_rename(&self) -> bool {
        matches!(*self, Note::FalseEnospc)
    }

    /// 是否是正确归因的绿档收场。
    pub fn is_sound(&self) -> bool {
        matches!(
            *self,
            Note::MetadataOnly | Note::ExdevOnly | Note::FallbackDone | Note::FallbackEnospc
        )
    }
}

// ---------------------------------------------------------------------------
// 判定维
// ---------------------------------------------------------------------------

/// 判定维序：Estate → Stage → Code → Value → Note → Default → Ok。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fault {
    /// 全合。
    Ok,
    /// 成败互报。
    Estate,
    /// 阶段不合。
    Stage,
    /// 码档不合。
    Code,
    /// 数值不合。
    Value,
    /// 注记不合。
    Note,
    /// 兜底档不合。
    Default,
}

impl Fault {
    /// 七格全表。
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

    /// 本缺陷维对应的错误码。
    pub fn code(&self) -> u32 {
        match self {
            Fault::Ok => 0,
            Fault::Estate => RN_ESTATE,
            Fault::Stage => RN_STAGE,
            Fault::Code => RN_CODE,
            Fault::Value => RN_VALUE,
            Fault::Note => RN_NOTE,
            Fault::Default => RN_DEFAULT,
        }
    }

    /// 反例六维。
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
            Fault::Stage => "fallback 的 ENOSPC 被记在 rename 面",
            Fault::Code => "假 ENOSPC 标签被申报成无码",
            Fault::Value => "fallback 已复制字节数虚报",
            Fault::Note => "退化满盘与假满盘标签互认",
            Fault::Default => "可判定的现场被丢进未判档",
        }
    }
}

/// 反例六维表。
pub const REJECT_FAULTS: [Fault; REJECT] = [
    Fault::Estate,
    Fault::Stage,
    Fault::Code,
    Fault::Value,
    Fault::Note,
    Fault::Default,
];

// ---------------------------------------------------------------------------
// 四档：原子 / 退化 / 假标签 / 空白
// ---------------------------------------------------------------------------

/// 收场四档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    /// 同盘原子改名成功。
    Atomic,
    /// 跨盘：EXDEV 如实上报或 fallback 如实收场（含 fallback ENOSPC）。
    Fallback,
    /// 假标签：rename 面报了不属于它的 ENOSPC/EXDEV。
    FalseLabel,
    /// 空白：用法错或未判档。
    Void,
}

impl Tier {
    /// 四档全表。
    pub const ALL: [Tier; 4] = [Tier::Atomic, Tier::Fallback, Tier::FalseLabel, Tier::Void];

    /// 秩。
    pub fn rank(&self) -> u32 {
        match self {
            Tier::Atomic => 1,
            Tier::Fallback => 2,
            Tier::FalseLabel => 3,
            Tier::Void => 4,
        }
    }

    /// 短名。
    pub fn slug(&self) -> &'static str {
        match self {
            Tier::Atomic => "atomic",
            Tier::Fallback => "fallback",
            Tier::FalseLabel => "false-label",
            Tier::Void => "void",
        }
    }

    /// 中文名。
    pub fn cn(&self) -> &'static str {
        match self {
            Tier::Atomic => "原子档",
            Tier::Fallback => "退化档",
            Tier::FalseLabel => "假标签档",
            Tier::Void => "空白档",
        }
    }

    /// 由秩还原。
    pub fn from_rank(r: u32) -> Option<Tier> {
        match r {
            1 => Some(Tier::Atomic),
            2 => Some(Tier::Fallback),
            3 => Some(Tier::FalseLabel),
            4 => Some(Tier::Void),
            _ => None,
        }
    }

    /// 绿档只有两档。
    pub fn is_green(&self) -> bool {
        matches!(*self, Tier::Atomic | Tier::Fallback)
    }

    /// 由一次结果归档。次序固定：假标签优先于一切"看起来成功"的收场。
    pub fn of(o: &Outcome) -> Tier {
        if o.default || o.note == Note::Nil || o.note == Note::Usage {
            return Tier::Void;
        }
        if o.note == Note::FalseEnospc
            || o.note == Note::FalseExdev
            || o.note == Note::HalfMove
        {
            return Tier::FalseLabel;
        }
        if o.note == Note::MetadataOnly {
            return Tier::Atomic;
        }
        Tier::Fallback
    }
}

// ---------------------------------------------------------------------------
// 域模型：Shape / Outcome / Claim / Case
// ---------------------------------------------------------------------------

/// 本卡输入的全部维度——一次 rename 的完整现场。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Shape {
    /// 同盘线还是跨盘线。
    pub line: RenameLine,
    /// 源文件字节数（只在 fallback 里有意义）。
    pub src_len: u32,
    /// 目的盘可用字节数（只在 fallback 里有意义）。
    pub free: u32,
    /// 目标名是否已存在。
    pub dst_exists: bool,
    /// 是否同一目录内改名。
    pub same_dir: bool,
    /// 现场报出的 errno。
    pub report: Errno,
    /// 目标名是否已指向源内容（改名/复制成功）。
    pub moved: bool,
    /// 是否用过 fallback。
    pub fallback_used: bool,
    /// fallback 是否完整复制（与 moved 分开记，防伪造）。
    pub fallback_complete: bool,
    /// 源名是否已消失。
    pub src_removed: bool,
    /// 命令行是否非法。
    pub bad_call: bool,
    /// 观察到的退出码。
    pub exit: u32,
    /// 兜底档。
    pub default: bool,
}

impl Shape {
    /// 一条同盘成功的 rename。
    pub fn same_dev(len: u32) -> Shape {
        Shape {
            line: RenameLine::SameDev,
            src_len: len,
            free: 0,
            dst_exists: false,
            same_dir: true,
            report: Errno::None,
            moved: true,
            fallback_used: false,
            fallback_complete: false,
            src_removed: true,
            bad_call: false,
            exit: EXIT_OK,
            default: false,
        }
    }

    /// 一条跨盘、如实报 EXDEV 且没有 fallback 的现场。
    pub fn cross_dev_exdev() -> Shape {
        Shape {
            line: RenameLine::CrossDev,
            src_len: BLOCK,
            free: 0,
            dst_exists: false,
            same_dir: false,
            report: Errno::Exdev,
            moved: false,
            fallback_used: false,
            fallback_complete: false,
            src_removed: false,
            bad_call: false,
            exit: EXIT_FAIL,
            default: false,
        }
    }

    /// 一条跨盘 fallback 撞 ENOSPC、归因正确的现场。
    pub fn cross_dev_enospc(len: u32, free: u32) -> Shape {
        Shape {
            line: RenameLine::CrossDev,
            src_len: len,
            free,
            dst_exists: false,
            same_dir: false,
            report: Errno::Enospc,
            moved: false,
            fallback_used: true,
            fallback_complete: false,
            src_removed: false,
            bad_call: false,
            exit: EXIT_FAIL,
            default: false,
        }
    }

    /// 跨盘 fallback 是否空间不足。
    pub fn fallback_enospc(&self) -> bool {
        self.line.needs_fallback()
            && self.fallback_used
            && self.src_len > self.free
            && self.report.is_space()
    }

    /// fallback 已复制的字节数。
    pub fn copied(&self) -> u32 {
        if self.src_len <= self.free {
            self.src_len
        } else {
            self.free
        }
    }

    /// 该现场是否给 rename 贴了假标签。
    pub fn false_label(&self) -> bool {
        match self.line {
            // 同盘 rename 永远不该报 ENOSPC 或 EXDEV。
            RenameLine::SameDev => matches!(self.report, Errno::Enospc | Errno::Exdev),
            // 跨盘 rename 本体只该报 EXDEV；ENOSPC 只能来自 fallback。
            // 没走 fallback，或走了但空间明明够，这条 ENOSPC 都是假的。
            RenameLine::CrossDev => {
                self.report.is_space() && (!self.fallback_used || self.src_len <= self.free)
            }
        }
    }

    /// 域合法性：字节数能闭合、同盘不复制数据。
    pub fn domain_ok(&self) -> bool {
        if self.src_len > BLOCK.saturating_mul(64) {
            return false;
        }
        if self.free > BLOCK.saturating_mul(64) {
            return false;
        }
        if self.bad_call {
            return self.exit == EXIT_USAGE && !self.moved && !self.src_removed;
        }
        if self.line == RenameLine::SameDev {
            if self.fallback_used || self.fallback_complete {
                return false;
            }
            if self.exit == EXIT_OK && !self.moved {
                return false;
            }
            return true;
        }
        if self.fallback_complete && !self.moved {
            return false;
        }
        if self.moved && !self.src_removed {
            return false;
        }
        true
    }

    /// 形状指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_str(h, self.line.slug());
        h = fold_word(h, self.src_len as u64);
        h = fold_word(h, self.free as u64);
        h = fold_bool(h, self.dst_exists);
        h = fold_bool(h, self.same_dir);
        h = fold_str(h, self.report.slug());
        h = fold_bool(h, self.moved);
        h = fold_bool(h, self.fallback_used);
        h = fold_bool(h, self.fallback_complete);
        h = fold_bool(h, self.src_removed);
        h = fold_bool(h, self.bad_call);
        h = fold_word(h, self.exit as u64);
        h = fold_bool(h, self.default);
        h
    }
}

/// 事实面。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Outcome {
    /// 纵线落点阶段。
    pub stage: Stage,
    /// 归因码（0 = 归因正确；非 0 = 破了本卡不变量）。
    pub code: u32,
    /// 已复制字节数（同盘恒 0）。
    pub value: u32,
    /// 收场注记。
    pub note: Note,
    /// 兜底档。
    pub default: bool,
    /// 命令是否成功。
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

    /// 成功收场。
    fn pass(stage: Stage, value: u32, note: Note) -> Outcome {
        Outcome::new(stage, 0, value, note, false, true)
    }

    /// 归因正确的失败收场。
    fn fail_sound(stage: Stage, value: u32, note: Note) -> Outcome {
        Outcome::new(stage, 0, value, note, false, false)
    }

    /// 违诺收场。
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

    /// 是否是假标签。
    pub fn false_label(&self) -> bool {
        self.note.labels_rename()
    }

    /// 是否跨过了 fallback。
    pub fn via_fallback(&self) -> bool {
        matches!(self.note, Note::FallbackDone | Note::FallbackEnospc)
    }

    /// 折叠六维。
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

/// 申报面。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Claim {
    /// 反例槽标签。
    pub tag: &'static str,
    /// 申报的成败。
    pub success: bool,
    /// 申报的阶段。
    pub stage: Stage,
    /// 申报的归因码。
    pub code: u32,
    /// 申报的已复制字节数。
    pub value: u32,
    /// 申报的注记。
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

    /// 申报成败位。
    pub fn success(&self) -> bool {
        self.success
    }

    /// 由事实面照抄。
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

    /// 六维差异掩码。
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

    /// 折叠申报。
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

/// 一条语料。
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

    /// 标签。
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
// 本卡的不变量：一次 rename 的现场判定
// ---------------------------------------------------------------------------

/// 由现场推出事实面。
///
/// 判定次序按闸门走，前一道闸一旦定性，后面不再看：
///
/// 1. 用法错在 Check 就收场，一个字节都不读；
/// 2. 兜底档优先于一切——本可判定的现场被丢进未判，是最容易被"看起来全绿"
///    掩盖的错；
/// 3. 半改名（源已消失而目标未立，或目标已立而源仍在）在 Atomic 闸判死；
/// 4. 同盘报 EXDEV 在 Scope 闸判死：Scope 刚判完同盘，EXDEV 就是它自己的
///    反证；
/// 5. 假 ENOSPC 在 Journal（同盘）/ Report（跨盘未退化）判死：rename 本体
///    没有分配数据块的机会，这句 errno 不可能是它自己的；
/// 6. 同盘成功只落 MetadataOnly，`value` 恒 0——元数据操作不搬字节；
/// 7. 跨盘未退化：EXDEV 如实上报 → ExdevOnly；其它 errno → 未判档；
/// 8. 跨盘退化：空间不足 → FallbackEnospc（错在复制）；完整落盘 →
///    FallbackDone。
pub fn define(s: &Shape) -> Outcome {
    // 闸 1 Check：用法错先收场。
    if s.bad_call {
        return Outcome::breach(Stage::Check, RN_SLOT, 0, Note::Usage, false);
    }
    // 兜底档：可判定的现场被丢进未判。
    if s.default {
        return Outcome::new(Stage::Report, 0, s.copied(), Note::Nil, true, false);
    }
    // 闸 4 Atomic：半改名是观测层伪造，真实 VFS 不存在。
    if s.moved != s.src_removed {
        return Outcome::breach(Stage::Atomic, RN_ATOMIC, s.copied(), Note::HalfMove, false);
    }
    // 闸 3 Scope：同盘不可能 EXDEV。
    if s.line == RenameLine::SameDev && s.report == Errno::Exdev {
        return Outcome::breach(Stage::Scope, RN_XDEV, 0, Note::FalseExdev, false);
    }
    // 闸 5/7 归因：rename 面报了 ENOSPC。
    if s.false_label() {
        if s.line == RenameLine::SameDev {
            return Outcome::breach(Stage::Journal, RN_FALSE, 0, Note::FalseEnospc, false);
        }
        return Outcome::breach(Stage::Report, RN_FALSE, 0, Note::FalseEnospc, false);
    }
    // 闸 4/8 同盘：一次提交，成功即元数据完成。
    if s.line == RenameLine::SameDev {
        if s.moved && s.report == Errno::None && s.exit == EXIT_OK {
            return Outcome::pass(Stage::Exit, 0, Note::MetadataOnly);
        }
        // 同盘其它 errno（EACCES/ENOENT）如实上报，本卡不判其归属。
        return Outcome::fail_sound(Stage::Report, 0, Note::Nil);
    }
    // 闸 6 Fallback：跨盘没有退化可走时，EXDEV 是唯一正确的结论。
    if !s.fallback_used {
        if s.report == Errno::Exdev {
            return Outcome::fail_sound(Stage::Report, 0, Note::ExdevOnly);
        }
        return Outcome::fail_sound(Stage::Report, 0, Note::Nil);
    }
    // 闸 6 Fallback：退化里真的没空间。
    if s.fallback_enospc() {
        return Outcome::fail_sound(Stage::Fallback, s.copied(), Note::FallbackEnospc);
    }
    // 闸 8 Exit：退化完整落盘、源已删、退出 0。
    if s.fallback_complete
        && s.moved
        && s.src_removed
        && s.report == Errno::None
        && s.exit == EXIT_OK
    {
        return Outcome::pass(Stage::Exit, s.src_len, Note::FallbackDone);
    }
    Outcome::fail_sound(Stage::Fallback, s.copied(), Note::Nil)
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
/// Estate 最先：成败互报意味着后五维再准也没有意义。Default 双向判死：
/// 现场真落未判档不行，把可判定的现场谎报成未判也不行。
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
        None => Shape::same_dev(0),
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
        return Err(RN_SLOT);
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

/// 槽 0 `rename-samedev`：同盘改名，一个数据块都不搬。
const SHAPE_SAMEDEV: Shape = Shape {
    line: RenameLine::SameDev,
    src_len: BLOCK,
    free: 0,
    dst_exists: false,
    same_dir: true,
    report: Errno::None,
    moved: true,
    fallback_used: false,
    fallback_complete: false,
    src_removed: true,
    bad_call: false,
    exit: EXIT_OK,
    default: false,
};

/// 槽 1 `rename-overwrite`：同盘覆盖一个已存在的名字，仍是元数据操作。
const SHAPE_OVERWRITE: Shape = Shape {
    line: RenameLine::SameDev,
    src_len: BLOCK * 2,
    free: 0,
    dst_exists: true,
    same_dir: true,
    report: Errno::None,
    moved: true,
    fallback_used: false,
    fallback_complete: false,
    src_removed: true,
    bad_call: false,
    exit: EXIT_OK,
    default: false,
};

/// 槽 2 `rename-full-disk`：盘已满（avail=0），同盘改名照旧成功。本卡靶心。
const SHAPE_FULL_DISK: Shape = Shape {
    line: RenameLine::SameDev,
    src_len: BLOCK * 4,
    free: 0,
    dst_exists: false,
    same_dir: false,
    report: Errno::None,
    moved: true,
    fallback_used: false,
    fallback_complete: false,
    src_removed: true,
    bad_call: false,
    exit: EXIT_OK,
    default: false,
};

/// 槽 3 `rename-cross-exdev`：跨盘 rename 本体如实报 EXDEV，工具不退化。
const SHAPE_CROSS_EXDEV: Shape = Shape {
    line: RenameLine::CrossDev,
    src_len: BLOCK,
    free: 0,
    dst_exists: false,
    same_dir: false,
    report: Errno::Exdev,
    moved: false,
    fallback_used: false,
    fallback_complete: false,
    src_removed: false,
    bad_call: false,
    exit: EXIT_FAIL,
    default: false,
};

/// 槽 4 `rename-fallback-done`：跨盘退化完整复制并删源。
const SHAPE_FALLBACK_DONE: Shape = Shape {
    line: RenameLine::CrossDev,
    src_len: BLOCK * 2,
    free: BLOCK * 4,
    dst_exists: false,
    same_dir: false,
    report: Errno::None,
    moved: true,
    fallback_used: true,
    fallback_complete: true,
    src_removed: true,
    bad_call: false,
    exit: EXIT_OK,
    default: false,
};

/// 槽 5 `rename-fallback-enospc`：跨盘退化复制到 2048 字节处撞满盘。
const SHAPE_FALLBACK_ENOSPC: Shape = Shape {
    line: RenameLine::CrossDev,
    src_len: BLOCK * 2,
    free: HALF_BLOCK,
    dst_exists: false,
    same_dir: false,
    report: Errno::Enospc,
    moved: false,
    fallback_used: true,
    fallback_complete: false,
    src_removed: false,
    bad_call: false,
    exit: EXIT_FAIL,
    default: false,
};

/// 槽 6 `rename-false-enospc`：同盘满盘上报 ENOSPC——假标签。
const SHAPE_FALSE_SAME: Shape = Shape {
    line: RenameLine::SameDev,
    src_len: BLOCK,
    free: 0,
    dst_exists: false,
    same_dir: true,
    report: Errno::Enospc,
    moved: false,
    fallback_used: false,
    fallback_complete: false,
    src_removed: false,
    bad_call: false,
    exit: EXIT_FAIL,
    default: false,
};

/// 槽 7 `rename-cross-false-enospc`：跨盘却根本没退化，ENOSPC 无处可来。
const SHAPE_FALSE_CROSS: Shape = Shape {
    line: RenameLine::CrossDev,
    src_len: BLOCK,
    free: 0,
    dst_exists: false,
    same_dir: false,
    report: Errno::Enospc,
    moved: false,
    fallback_used: false,
    fallback_complete: false,
    src_removed: false,
    bad_call: false,
    exit: EXIT_FAIL,
    default: false,
};

/// 槽 13 的兜底型：所有维度都像"失败但没说清"。
const SHAPE_DEFAULT_BUCKET: Shape = Shape {
    line: RenameLine::SameDev,
    src_len: BLOCK,
    free: 0,
    dst_exists: false,
    same_dir: true,
    report: Errno::None,
    moved: false,
    fallback_used: false,
    fallback_complete: false,
    src_removed: false,
    bad_call: false,
    exit: EXIT_FAIL,
    default: true,
};

/// 十四槽现场。前八槽是活槽，后六槽复用活槽现场、只坏申报的一维。
pub const SHAPES: [Shape; CORPUS] = [
    SHAPE_SAMEDEV,
    SHAPE_OVERWRITE,
    SHAPE_FULL_DISK,
    SHAPE_CROSS_EXDEV,
    SHAPE_FALLBACK_DONE,
    SHAPE_FALLBACK_ENOSPC,
    SHAPE_FALSE_SAME,
    SHAPE_FALSE_CROSS,
    SHAPE_FALLBACK_ENOSPC,
    SHAPE_FALLBACK_ENOSPC,
    SHAPE_FALSE_SAME,
    SHAPE_FALLBACK_ENOSPC,
    SHAPE_FALLBACK_ENOSPC,
    SHAPE_DEFAULT_BUCKET,
];

/// 槽 0 的申报：同盘改名成功，零字节搬运。
const CLAIM_SAMEDEV: Claim = Claim::new(
    "rename-samedev",
    true,
    Stage::Exit,
    0,
    0,
    Note::MetadataOnly,
    false,
);

/// 槽 1 的申报。
const CLAIM_OVERWRITE: Claim = Claim::new(
    "rename-overwrite",
    true,
    Stage::Exit,
    0,
    0,
    Note::MetadataOnly,
    false,
);

/// 槽 2 的申报：满盘上的同盘改名仍然成功。
const CLAIM_FULL_DISK: Claim = Claim::new(
    "rename-full-disk",
    true,
    Stage::Exit,
    0,
    0,
    Note::MetadataOnly,
    false,
);

/// 槽 3 的申报：EXDEV 如实上报、退出 1。
const CLAIM_CROSS_EXDEV: Claim = Claim::new(
    "rename-cross-exdev",
    false,
    Stage::Report,
    0,
    0,
    Note::ExdevOnly,
    false,
);

/// 槽 4 的申报：退化复制 8192 字节后删源。
const CLAIM_FALLBACK_DONE: Claim = Claim::new(
    "rename-fallback-done",
    true,
    Stage::Exit,
    0,
    BLOCK * 2,
    Note::FallbackDone,
    false,
);

/// 槽 5 的申报：退化里只写出 2048 字节，错在复制。
const CLAIM_FALLBACK_ENOSPC: Claim = Claim::new(
    "rename-fallback-enospc",
    false,
    Stage::Fallback,
    0,
    HALF_BLOCK,
    Note::FallbackEnospc,
    false,
);

/// 槽 6 的申报：同盘假 ENOSPC，判死并给出 RN_FALSE。
const CLAIM_FALSE_SAME: Claim = Claim::new(
    "rename-false-enospc",
    false,
    Stage::Journal,
    RN_FALSE,
    0,
    Note::FalseEnospc,
    false,
);

/// 槽 7 的申报：跨盘未退化却报 ENOSPC，同样判死。
const CLAIM_FALSE_CROSS: Claim = Claim::new(
    "rename-cross-false-enospc",
    false,
    Stage::Report,
    RN_FALSE,
    0,
    Note::FalseEnospc,
    false,
);

/// 十四格申报。前八格与 `define(SHAPES[i])` 逐维相同，后六格各坏一维。
pub const CLAIMS: [Claim; CORPUS] = [
    CLAIM_SAMEDEV,
    CLAIM_OVERWRITE,
    CLAIM_FULL_DISK,
    CLAIM_CROSS_EXDEV,
    CLAIM_FALLBACK_DONE,
    CLAIM_FALLBACK_ENOSPC,
    CLAIM_FALSE_SAME,
    CLAIM_FALSE_CROSS,
    // 死 1 estate-swap：事实是退化满盘（失败），申报却报成功。
    Claim::new(
        "estate-swap",
        true,
        Stage::Fallback,
        0,
        HALF_BLOCK,
        Note::FallbackEnospc,
        false,
    ),
    // 死 2 stage-skip：fallback 的 ENOSPC 落在退化闸，申报却说在退出闸。
    Claim::new(
        "stage-skip",
        false,
        Stage::Exit,
        0,
        HALF_BLOCK,
        Note::FallbackEnospc,
        false,
    ),
    // 死 3 code-swallow：假标签已判 RN_FALSE，申报却报无码。
    Claim::new(
        "code-swallow",
        false,
        Stage::Journal,
        0,
        0,
        Note::FalseEnospc,
        false,
    ),
    // 死 4 value-off：已复制 2048 字节，申报 4096。
    Claim::new(
        "value-off",
        false,
        Stage::Fallback,
        0,
        BLOCK,
        Note::FallbackEnospc,
        false,
    ),
    // 死 5 note-swap：退化满盘申报成假满盘标签（红换红也必须判出来）。
    Claim::new(
        "note-swap",
        false,
        Stage::Fallback,
        0,
        HALF_BLOCK,
        Note::FalseEnospc,
        false,
    ),
    // 死 6 default-bucket：可判定的现场被丢进未判档。
    Claim::new(
        "default-bucket",
        false,
        Stage::Report,
        0,
        0,
        Note::Nil,
        false,
    ),
];

// ---------------------------------------------------------------------------
// 期望表（申报表的第二种形式，任一处笔误都要露出来）
// ---------------------------------------------------------------------------

/// 期望阶段（现场真实落到的阶段，不是申报那一栏）。
pub const WANT_STAGE: [Stage; CORPUS] = [
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
    Stage::Report,
    Stage::Exit,
    Stage::Fallback,
    Stage::Journal,
    Stage::Report,
    Stage::Fallback,
    Stage::Fallback,
    Stage::Journal,
    Stage::Fallback,
    Stage::Fallback,
    Stage::Report,
];

/// 期望错误码（0 = 现场无违诺）。
pub const WANT_CODE: [u32; CORPUS] = [
    0,
    0,
    0,
    0,
    0,
    0,
    RN_FALSE,
    RN_FALSE,
    0,
    0,
    RN_FALSE,
    0,
    0,
    0,
];

/// 期望缺陷维序位（0..5 六维，6 = 无缺陷）。
pub const WANT_NDX: [usize; CORPUS] = [6, 6, 6, 6, 6, 6, 6, 6, 0, 1, 2, 3, 4, 5];

/// 期望注记。
pub const WANT_NOTE: [Note; CORPUS] = [
    Note::MetadataOnly,
    Note::MetadataOnly,
    Note::MetadataOnly,
    Note::ExdevOnly,
    Note::FallbackDone,
    Note::FallbackEnospc,
    Note::FalseEnospc,
    Note::FalseEnospc,
    Note::FallbackEnospc,
    Note::FallbackEnospc,
    Note::FalseEnospc,
    Note::FallbackEnospc,
    Note::FallbackEnospc,
    Note::Nil,
];

/// 期望兜底位。
pub const WANT_DEFAULT: [bool; CORPUS] = [
    false, false, false, false, false, false, false, false, false, false, false, false, false, true,
];

/// 期望短名。
pub const WANT_TAGS: [&str; CORPUS] = [
    "rename-samedev",
    "rename-overwrite",
    "rename-full-disk",
    "rename-cross-exdev",
    "rename-fallback-done",
    "rename-fallback-enospc",
    "rename-false-enospc",
    "rename-cross-false-enospc",
    "estate-swap",
    "stage-skip",
    "code-swallow",
    "value-off",
    "note-swap",
    "default-bucket",
];

/// 期望已复制字节数。
pub const WANT_VALUE: [u32; CORPUS] = [
    0,
    0,
    0,
    0,
    BLOCK * 2,
    HALF_BLOCK,
    0,
    0,
    HALF_BLOCK,
    HALF_BLOCK,
    0,
    HALF_BLOCK,
    HALF_BLOCK,
    0,
];

/// 期望成败。
pub const WANT_SUCCESS: [bool; CORPUS] = [
    true, true, true, false, true, false, false, false, false, false, false, false, false, false,
];

/// 期望退出码（现场观察值）。
pub const WANT_EXIT: [u32; CORPUS] = [
    EXIT_OK,
    EXIT_OK,
    EXIT_OK,
    EXIT_FAIL,
    EXIT_OK,
    EXIT_FAIL,
    EXIT_FAIL,
    EXIT_FAIL,
    EXIT_FAIL,
    EXIT_FAIL,
    EXIT_FAIL,
    EXIT_FAIL,
    EXIT_FAIL,
    EXIT_FAIL,
];
// ---------------------------------------------------------------------------
// 族内场景：4 条同盘原子 + 8 条跨盘退化（其中 3 条撞满盘）
// ---------------------------------------------------------------------------

/// 族内统一种子（场景指纹用）。
pub const FAMILY_SEED: u32 = 0x6872_0001;

/// 第 `i` 条场景（12 条，手写，一条一种收场）。
pub fn scene_at(i: usize) -> Option<Shape> {
    match i {
        // 0 同盘普通改名。
        0 => Some(Shape::same_dev(BLOCK)),
        // 1 同盘覆盖已存在的名字。
        1 => Some(Shape {
            line: RenameLine::SameDev,
            src_len: BLOCK * 2,
            free: 0,
            dst_exists: true,
            same_dir: true,
            report: Errno::None,
            moved: true,
            fallback_used: false,
            fallback_complete: false,
            src_removed: true,
            bad_call: false,
            exit: EXIT_OK,
            default: false,
        }),
        // 2 满盘（avail=0）上的同盘改名仍然成功。
        2 => Some(Shape {
            line: RenameLine::SameDev,
            src_len: BLOCK * 4,
            free: 0,
            dst_exists: false,
            same_dir: false,
            report: Errno::None,
            moved: true,
            fallback_used: false,
            fallback_complete: false,
            src_removed: true,
            bad_call: false,
            exit: EXIT_OK,
            default: false,
        }),
        // 3 空文件改名（元数据操作连一个字节都不碰）。
        3 => Some(Shape {
            line: RenameLine::SameDev,
            src_len: 0,
            free: 0,
            dst_exists: false,
            same_dir: true,
            report: Errno::None,
            moved: true,
            fallback_used: false,
            fallback_complete: false,
            src_removed: true,
            bad_call: false,
            exit: EXIT_OK,
            default: false,
        }),
        // 4 跨盘 rename 本体报 EXDEV，工具如实上交。
        4 => Some(Shape::cross_dev_exdev()),
        // 5 跨盘退化：源与目的盘容量恰好相等。
        5 => Some(Shape {
            line: RenameLine::CrossDev,
            src_len: BLOCK,
            free: BLOCK,
            dst_exists: false,
            same_dir: false,
            report: Errno::None,
            moved: true,
            fallback_used: true,
            fallback_complete: true,
            src_removed: true,
            bad_call: false,
            exit: EXIT_OK,
            default: false,
        }),
        // 6 跨盘退化：目的盘富余一倍。
        6 => Some(Shape {
            line: RenameLine::CrossDev,
            src_len: BLOCK * 2,
            free: BLOCK * 4,
            dst_exists: false,
            same_dir: false,
            report: Errno::None,
            moved: true,
            fallback_used: true,
            fallback_complete: true,
            src_removed: true,
            bad_call: false,
            exit: EXIT_OK,
            default: false,
        }),
        // 7 跨盘退化：零字节源，不需要任何空间。
        7 => Some(Shape {
            line: RenameLine::CrossDev,
            src_len: 0,
            free: 0,
            dst_exists: false,
            same_dir: false,
            report: Errno::None,
            moved: true,
            fallback_used: true,
            fallback_complete: true,
            src_removed: true,
            bad_call: false,
            exit: EXIT_OK,
            default: false,
        }),
        // 8 跨盘退化撞满盘：目的盘 0 字节可用。
        8 => Some(Shape::cross_dev_enospc(BLOCK * 2, 0)),
        // 9 跨盘退化撞满盘：只挤出一个块的余量。
        9 => Some(Shape::cross_dev_enospc(BLOCK * 2, BLOCK)),
        // 10 跨盘退化撞满盘：余量不足半个块。
        10 => Some(Shape::cross_dev_enospc(BLOCK * 4, HALF_BLOCK)),
        // 11 跨盘退化：目的盘极富余。
        11 => Some(Shape {
            line: RenameLine::CrossDev,
            src_len: BLOCK,
            free: BLOCK * 8,
            dst_exists: false,
            same_dir: false,
            report: Errno::None,
            moved: true,
            fallback_used: true,
            fallback_complete: true,
            src_removed: true,
            bad_call: false,
            exit: EXIT_OK,
            default: false,
        }),
        _ => None,
    }
}

/// 全族场景。
pub fn scenes() -> [Shape; FAMILY_SCENES] {
    let mut out = [Shape::same_dev(0); FAMILY_SCENES];
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
    Some(Case::new(s, Claim::from_outcome(&o, o.ok(), head_tag())))
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

/// 族场景计数：同盘原子档与跨盘退化档各多少。
pub fn scene_counts() -> (usize, usize) {
    let mut atomic = 0usize;
    let mut fallback = 0usize;
    let mut i = 0usize;
    while i < FAMILY_SCENES {
        if let Some(s) = scene_at(i) {
            match Tier::of(&define(&s)) {
                Tier::Atomic => atomic += 1,
                Tier::Fallback => fallback += 1,
                _ => {}
            }
        }
        i += 1;
    }
    (atomic, fallback)
}

/// 族内撞满盘的场景数（必须等于 `ENOSPC_SCENES`）。
pub fn scene_enospc_count() -> usize {
    let mut n = 0usize;
    let mut i = 0usize;
    while i < FAMILY_SCENES {
        if let Some(s) = scene_at(i) {
            let o = define(&s);
            if o.note == Note::FallbackEnospc {
                n += 1;
            }
        }
        i += 1;
    }
    n
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
    if ANCHOR_F11689_SEG.checked_add(ANCHOR_F11689_GAP) != Some(SEG_BASE) {
        return false;
    }
    if ANCHOR_F11687_SEG.checked_add(ANCHOR_F11687_GAP) != Some(SEG_BASE) {
        return false;
    }
    if SEG_BASE.checked_add(ANCHOR_F11691_GAP) != Some(ANCHOR_F11691_SEG) {
        return false;
    }
    if SEG_BASE.checked_add(ANCHOR_F11699_GAP) != Some(ANCHOR_F11699_SEG) {
        return false;
    }
    ANCHOR_ENOENT == 2
        && ANCHOR_EACCES == 13
        && ANCHOR_EXDEV == 18
        && ANCHOR_ENOSPC == 28
        && ANCHOR_EXDEV_TEXT == "Invalid cross-device link"
        && ANCHOR_ENOSPC_TEXT == "No space left on device"
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
    /// 由语料构造。每一档都扫全部槽：活槽必须落进自己的档且判对，死槽
    /// 必须落进非绿档并命中它自己那一维；两者都不许被别的档"顺带算对"。
    pub fn build() -> Matrix {
        let table = Tier::ALL;
        let mut rows = [Row {
            tier: Tier::Atomic,
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
                    landed == tier && !tier.is_green() && c.fault() == expected_reject_ndx(i)
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
pub const RENDER_OK: &str = "rename corpus=14 accept=8 reject=6 atomic=4 fallback=8 false=0 void=0 match=12 rate=1000permille\n";

/// 一次 rename 归因结算报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Report {
    /// 结算分母（族内场景数）。
    pub total: usize,
    /// 正例槽数。
    pub accept: usize,
    /// 反例槽数。
    pub reject: usize,
    /// 同盘原子档计数。
    pub atomic: usize,
    /// 跨盘退化档计数。
    pub fallback: usize,
    /// 假标签档计数。
    pub false_label: usize,
    /// 空白档计数。
    pub void: usize,
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
            && self.false_label == 0
            && self.void == 0
            && self.rate() == RATE_FULL
            && self.accept == ACCEPT
            && self.reject == REJECT
            && self.atomic + self.fallback == FAMILY_SCENES
            && self.atomic >= ATOMIC_SCENES
            && self.fallback >= FALLBACK_SCENES
            && self.matched == FAMILY_SCENES
    }

    /// 红项计数。
    pub fn reds(&self) -> u32 {
        (self.conflicts as u32) + (self.false_label as u32) + (self.void as u32)
    }

    /// 闸门：全绿才放行。
    pub fn gate(&self) -> Result<(), u32> {
        if self.total == 0 {
            return Err(RN_MATRIX);
        }
        if self.false_label > 0 {
            return Err(RN_FALSE);
        }
        if self.void > 0 {
            return Err(RN_DEFAULT);
        }
        if self.conflicts > 0 {
            return Err(RN_NOTE);
        }
        if self.rate() < RATE_FULL {
            return Err(RN_RATE);
        }
        if self.atomic + self.fallback != FAMILY_SCENES {
            return Err(RN_MATRIX);
        }
        Ok(())
    }

    /// 四档格数。
    pub fn tier_cells(&self) -> u32 {
        (self.atomic as u32)
            + (self.fallback as u32)
            + (self.false_label as u32)
            + (self.void as u32)
    }

    /// 红档格数。
    pub fn red_cells(&self) -> u32 {
        (self.false_label as u32) + (self.void as u32) + (self.conflicts as u32)
    }

    /// 逐字渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("rename corpus=");
        s.push_str(dec_text((self.accept + self.reject) as u32).as_str());
        s.push_str(" accept=");
        s.push_str(dec_text(self.accept as u32).as_str());
        s.push_str(" reject=");
        s.push_str(dec_text(self.reject as u32).as_str());
        s.push_str(" atomic=");
        s.push_str(dec_text(self.atomic as u32).as_str());
        s.push_str(" fallback=");
        s.push_str(dec_text(self.fallback as u32).as_str());
        s.push_str(" false=");
        s.push_str(dec_text(self.false_label as u32).as_str());
        s.push_str(" void=");
        s.push_str(dec_text(self.void as u32).as_str());
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
        atomic: 0,
        fallback: 0,
        false_label: 0,
        void: 0,
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
        atomic: 0,
        fallback: 0,
        false_label: 0,
        void: 0,
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
            Tier::Atomic => {
                rep.atomic += 1;
                rep.matched += 1;
            }
            Tier::Fallback => {
                rep.fallback += 1;
                rep.matched += 1;
            }
            Tier::FalseLabel => {
                rep.false_label += 1;
                rep.conflicts += 1;
            }
            Tier::Void => {
                rep.void += 1;
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
            return Err(RN_SLOT);
        }
    }
    Ok(settle(over))
}

/// 报告与矩阵是否互相印证。
pub fn report_agrees_with_matrix() -> bool {
    let rep = settle(None);
    let m = Matrix::build();
    let atomic_cells = match m.row(Tier::Atomic) {
        Some(r) => r.good,
        None => 0,
    };
    let fallback_cells = match m.row(Tier::Fallback) {
        Some(r) => r.good,
        None => 0,
    };
    rep.atomic == ATOMIC_SCENES
        && rep.fallback == FALLBACK_SCENES
        && atomic_cells > 0
        && fallback_cells > 0
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
// 阶段闸门：Check→Resolve→Scope→Atomic→Journal→Fallback→Report→Exit
// ---------------------------------------------------------------------------

/// 一次 rename 归因结算的走闸状态机。
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
            return Err(RN_ESTATE);
        }
        if let Some(cur) = self.current {
            if s.rank() < cur.rank() {
                self.refusals += 1;
                return Err(RN_STAGE);
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
            return Err(RN_ESTATE);
        }
        if self.current.is_none() {
            return Err(RN_STAGE);
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
                None => return Err(RN_SLOT),
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
                None => return Err(RN_SLOT),
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
            return Err(RN_ESTATE);
        }
        if self.len >= SAMPLES {
            self.rejects += 1;
            return Err(RN_SLOT);
        }
        if s.slot as usize != self.len {
            self.rejects += 1;
            return Err(RN_SLOT);
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
            return Err(RN_SLOT);
        }
        let stat_last = match STAT_CELLS.checked_add(1) {
            Some(x) => x,
            None => return Err(RN_SLOT),
        };
        let mut i = 0usize;
        while i < stat_last {
            let s = match self.sample(i) {
                Some(x) => x,
                None => return Err(RN_SLOT),
            };
            if !s.ok {
                return Err(RN_ESTATE);
            }
            if s.code != 0 {
                return Err(RN_CODE);
            }
            let want_value = match VALS.get(i) {
                Some(v) => *v,
                None => return Err(RN_SLOT),
            };
            if s.value != want_value {
                return Err(RN_VALUE);
            }
            let want_op = match self.stat_op_want(i) {
                Some(v) => v,
                None => return Err(RN_SLOT),
            };
            if s.op != want_op {
                return Err(RN_CODE);
            }
            i += 1;
        }
        let mut j = 0usize;
        while j < REJECT {
            let cell = match STAT_CELLS.checked_add(1).and_then(|x| x.checked_add(j)) {
                Some(x) => x,
                None => return Err(RN_SLOT),
            };
            let s = match self.sample(cell) {
                Some(x) => x,
                None => return Err(RN_SLOT),
            };
            if s.ok {
                return Err(RN_ESTATE);
            }
            if s.value != 0 {
                return Err(RN_VALUE);
            }
            let want_code = match REJECT_CODES.get(j) {
                Some(c) => *c,
                None => return Err(RN_SLOT),
            };
            if s.code != want_code {
                return Err(RN_CODE);
            }
            let want_op = match DEFECT_OP_BASE.checked_add(j as u8) {
                Some(x) => x,
                None => return Err(RN_SLOT),
            };
            if s.op != want_op {
                return Err(RN_CODE);
            }
            let mut k = j + 1usize;
            while k < REJECT {
                if let Some(other) = REJECT_CODES.get(k) {
                    if *other == want_code {
                        return Err(RN_CODE);
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
            return Err(RN_ESTATE);
        }
        match self.audit() {
            Ok(()) => {}
            Err(code) => return Err(code),
        }
        let h = self.seal();
        if h == 0 {
            return Err(RN_SEAL);
        }
        self.root = h;
        self.frozen = true;
        Ok(h)
    }

    /// 重放：以同一批格子重建账本，比指纹。
    pub fn replay(&self) -> Result<u64, u32> {
        if !self.frozen {
            return Err(RN_ESTATE);
        }
        let mut fresh = Ledger::new();
        let mut i = 0usize;
        while i < self.len {
            let s = match self.sample(i) {
                Some(x) => x,
                None => return Err(RN_SEAL),
            };
            match fresh.register(s) {
                Ok(()) => {}
                Err(_) => return Err(RN_SEAL),
            }
            i += 1;
        }
        let h = fresh.seal();
        if h != self.root {
            return Err(RN_SEAL);
        }
        Ok(h)
    }

    /// 改动一格（仅未冻结时允许）。
    pub fn poke(&mut self, i: usize, value: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(RN_ESTATE);
        }
        match self.slots.get_mut(i) {
            Some(cell) => {
                match *cell {
                    Some(ref mut s) => {
                        s.value = value;
                    }
                    None => return Err(RN_SLOT),
                }
                Ok(())
            }
            None => Err(RN_SLOT),
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
                    if c.claim.tag
                        != match WANT_TAGS.get(i) {
                            Some(t) => *t,
                            None => "",
                        }
                    {
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
        RN_SLOT,
        a.value.wrapping_add(7),
        Note::HalfMove,
        true,
    );
    let c = Case::new(s, wrong);
    judge(&c) == Fault::Estate
}

/// 申报表第一格的标签。
pub fn head_tag() -> &'static str {
    match WANT_TAGS.get(0) {
        Some(x) => *x,
        None => "rename-samedev",
    }
}

/// 统计表第一格的值。
pub fn head_val() -> u32 {
    match VALS.get(0) {
        Some(x) => *x,
        None => 0,
    }
}

// ---------------------------------------------------------------------------
// 十三条判据谓词（全部私有，只在 run_unx11690_checks 里汇总）
// ---------------------------------------------------------------------------

/// 01 名称与段基址：常量层自洽。
fn name_ok() -> bool {
    if SEG_BASE != 0x6d370 {
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
    if STAGE_LAST_RANK != 8 {
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
    if FAMILY_SCENES != ATOMIC_SCENES + FALLBACK_SCENES {
        return false;
    }
    if ATOMIC_SCENES != 4 || FALLBACK_SCENES != 8 || ENOSPC_SCENES != 3 {
        return false;
    }
    if !SANDBOX_PREFIX.starts_with("/tmp/bx-sandbox/") {
        return false;
    }
    if EXIT_OK != 0 || EXIT_FAIL != 1 || EXIT_USAGE != 2 {
        return false;
    }
    if Stage::ALL.len() != 8 {
        return false;
    }
    let mut r = 1u32;
    while r <= STAGE_LAST_RANK {
        match Stage::from_rank(r) {
            Some(s) => {
                if s.rank() != r {
                    return false;
                }
            }
            None => return false,
        }
        r += 1;
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
        if ndx_fault(k) != Some(f) {
            return false;
        }
        k += 1;
    }
    if Fault::Ok.code() != 0 || ndx_fault(6) != Some(Fault::Ok) {
        return false;
    }
    true
}

/// 03 同盘不搬数据：同盘 rename 的 `value` 恒 0，满盘也照旧成功。
fn atomic_ok() -> bool {
    let plain = Shape::same_dev(BLOCK);
    let o = define(&plain);
    if !o.ok() || o.value != 0 || o.note != Note::MetadataOnly {
        return false;
    }
    if o.stage != Stage::Exit {
        return false;
    }
    if plain.copied() != 0 {
        return false;
    }
    // 满盘（free = 0）上的同盘改名仍然成功，且一个字节都不搬。
    let full = match scene_at(2) {
        Some(x) => x,
        None => return false,
    };
    let fo = define(&full);
    if !fo.ok() || fo.value != 0 || fo.stage != Stage::Exit {
        return false;
    }
    if full.free != 0 || full.src_len == 0 {
        return false;
    }
    // rename 本体（同盘或跨盘）都不可能产生 ENOSPC。
    if RenameLine::SameDev.rename_can_enospc() || RenameLine::CrossDev.rename_can_enospc() {
        return false;
    }
    // 只有跨盘才期待 EXDEV、才需要退化。
    if !RenameLine::CrossDev.expects_exdev() || !RenameLine::CrossDev.needs_fallback() {
        return false;
    }
    if RenameLine::SameDev.needs_fallback() || RenameLine::SameDev.expects_exdev() {
        return false;
    }
    // 同盘现场不会带退化痕迹。
    if plain.fallback_used || plain.fallback_complete {
        return false;
    }
    plain.domain_ok() && full.domain_ok()
}

/// 04 errno 归属闸：假 ENOSPC 记在 rename 面，真 ENOSPC 记在 fallback 面。
fn attribution_ok() -> bool {
    let same = match case_at(6) {
        Some(c) => c.actual(),
        None => return false,
    };
    if same.code != RN_FALSE || same.stage != Stage::Journal {
        return false;
    }
    if same.note != Note::FalseEnospc || !same.note.labels_rename() {
        return false;
    }
    let cross = match case_at(7) {
        Some(c) => c.actual(),
        None => return false,
    };
    if cross.code != RN_FALSE || cross.stage != Stage::Report {
        return false;
    }
    if !cross.note.labels_rename() {
        return false;
    }
    let fb = match case_at(5) {
        Some(c) => c.actual(),
        None => return false,
    };
    if fb.note != Note::FallbackEnospc || fb.note.labels_rename() {
        return false;
    }
    if !fb.note.is_sound() || fb.stage != Stage::Fallback {
        return false;
    }
    if fb.value != HALF_BLOCK {
        return false;
    }
    if Stage::Journal.may_enospc() || Stage::Atomic.may_enospc() {
        return false;
    }
    if !Stage::Fallback.may_enospc() {
        return false;
    }
    if !Note::MetadataOnly.is_sound() || !Note::ExdevOnly.is_sound() {
        return false;
    }
    if Note::FalseEnospc.is_sound() || Note::FalseExdev.is_sound() || Note::HalfMove.is_sound() {
        return false;
    }
    if Errno::Enospc.errno() != ANCHOR_ENOSPC || Errno::Exdev.errno() != ANCHOR_EXDEV {
        return false;
    }
    Errno::from_errno(28) == Some(Errno::Enospc)
        && Errno::from_errno(18) == Some(Errno::Exdev)
        && Errno::from_errno(0) == Some(Errno::None)
        && Errno::from_errno(7).is_none()
}

/// 05 假标签不并档：假满盘落假标签档，真退化满盘落退化档。
fn false_ok() -> bool {
    if Tier::ALL.len() != TIER_COUNT {
        return false;
    }
    let mut i = 0usize;
    while i < Tier::ALL.len() {
        let t = match Tier::ALL.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if t.rank() != i as u32 + 1 {
            return false;
        }
        if Tier::from_rank(t.rank()) != Some(t) {
            return false;
        }
        if t.is_green() != (t == Tier::Atomic || t == Tier::Fallback) {
            return false;
        }
        i += 1;
    }
    let false_same = match case_at(6) {
        Some(c) => c.actual(),
        None => return false,
    };
    let false_cross = match case_at(7) {
        Some(c) => c.actual(),
        None => return false,
    };
    let real = match case_at(5) {
        Some(c) => c.actual(),
        None => return false,
    };
    let atomic = match case_at(0) {
        Some(c) => c.actual(),
        None => return false,
    };
    let neutral = match case_at(13) {
        Some(c) => c.actual(),
        None => return false,
    };
    if Tier::of(&false_same) != Tier::FalseLabel || Tier::of(&false_cross) != Tier::FalseLabel {
        return false;
    }
    if Tier::of(&real) != Tier::Fallback {
        return false;
    }
    if Tier::of(&atomic) != Tier::Atomic {
        return false;
    }
    if Tier::of(&neutral) != Tier::Void {
        return false;
    }
    // 假标签档与退化档的秩不同，合并即失去判据价值。
    Tier::FalseLabel.rank() != Tier::Fallback.rank() && Tier::FalseLabel.slug() != Tier::Fallback.slug()
}

/// 06 原子性防伪造：半改名一定判 `RN_ATOMIC`，且正常现场绝不触发。
fn forge_ok() -> bool {
    let mut goal = Shape::same_dev(BLOCK);
    goal.src_removed = false;
    let a = define(&goal);
    if a.ok() || a.code != RN_ATOMIC || a.stage != Stage::Atomic || a.note != Note::HalfMove {
        return false;
    }
    let mut gone = Shape::same_dev(BLOCK);
    gone.moved = false;
    let b = define(&gone);
    if b.ok() || b.code != RN_ATOMIC || b.note != Note::HalfMove {
        return false;
    }
    // 两条伪造现场都不带兜底位，也不是假 ENOSPC。
    if a.default || b.default || a.note.labels_rename() {
        return false;
    }
    // 十二条族场景没有一条是半改名。
    let mut i = 0usize;
    while i < FAMILY_SCENES {
        let s = match scene_at(i) {
            Some(x) => x,
            None => return false,
        };
        if define(&s).code == RN_ATOMIC {
            return false;
        }
        if s.moved != s.src_removed {
            return false;
        }
        i += 1;
    }
    // 语料里也没有半改名。
    let mut k = 0usize;
    while k < CORPUS {
        let c = match case_at(k) {
            Some(x) => x,
            None => return false,
        };
        if c.actual().code == RN_ATOMIC {
            return false;
        }
        k += 1;
    }
    true
}

/// 07 文案逐字：ENOSPC 与 EXDEV 的两句原文都不许走形。
fn text_ok() -> bool {
    let false_line = msg_false_enospc(SRC_SAME, DST_SAME);
    if !false_line.as_str().starts_with(T_MOVE_PREFIX) {
        return false;
    }
    if !false_line.as_str().contains(ANCHOR_ENOSPC_TEXT) {
        return false;
    }
    if !false_line.as_str().contains(SRC_SAME) || !false_line.as_str().contains(DST_SAME) {
        return false;
    }
    let exdev_line = msg_exdev(SRC_SAME, DST_CROSS);
    if !exdev_line.as_str().contains(ANCHOR_EXDEV_TEXT) {
        return false;
    }
    if exdev_line.as_str() == false_line.as_str() {
        return false;
    }
    let bare = msg_rename_exdev();
    if !bare.as_str().starts_with(T_RENAME_PREFIX) || !bare.as_str().contains(ANCHOR_EXDEV_TEXT)
    {
        return false;
    }
    let fb = msg_fallback_enospc(DST_CROSS);
    if !fb.as_str().starts_with(T_CREATE_PREFIX) || !fb.as_str().contains(ANCHOR_ENOSPC_TEXT)
    {
        return false;
    }
    // 假标签与正确归属必须是两句不同的话。
    if fb.as_str() == false_line.as_str() {
        return false;
    }
    let ok_line = msg_renamed(SRC_SAME, DST_SAME);
    if !ok_line.as_str().starts_with(T_RENAMED_PREFIX) {
        return false;
    }
    if ok_line.as_str().contains(ANCHOR_ENOSPC_TEXT) {
        return false;
    }
    let joined = join_text(T_RENAME_PREFIX, ANCHOR_ENOSPC_TEXT, TEXT_EMPTY);
    if joined.as_str() != "rename: No space left on device" {
        return false;
    }
    if msg_usage_missing().as_str() != "rename: missing operand" {
        return false;
    }
    if !msg_usage_extra().as_str().contains("extra operand") {
        return false;
    }
    if !msg_atomic_done(SRC_SAME, DST_SAME).as_str().contains("atomic rename") {
        return false;
    }
    if !msg_fallback_attributed(DST_CROSS, HALF_BLOCK)
        .as_str()
        .contains("2048")
    {
        return false;
    }
    if !msg_false_label(SRC_SAME, DST_SAME)
        .as_str()
        .contains("false enospc label")
    {
        return false;
    }
    if Errno::Enospc.text() != ANCHOR_ENOSPC_TEXT || Errno::Exdev.text() != ANCHOR_EXDEV_TEXT
    {
        return false;
    }
    if Errno::None.text() != TEXT_EMPTY {
        return false;
    }
    true
}

/// 08 语料十四槽互校：申报、期望表、现场三路必须逐格对上。
fn corpus_ok() -> bool {
    if cases().len() != CORPUS {
        return false;
    }
    let mut i = 0usize;
    while i < CORPUS {
        let c = match case_at(i) {
            Some(x) => x,
            None => return false,
        };
        let a = c.actual();
        if a.stage != match WANT_STAGE.get(i) {
            Some(x) => *x,
            None => return false,
        } {
            return false;
        }
        if a.code != match WANT_CODE.get(i) {
            Some(x) => *x,
            None => return false,
        } {
            return false;
        }
        if a.note != match WANT_NOTE.get(i) {
            Some(x) => *x,
            None => return false,
        } {
            return false;
        }
        if a.default != match WANT_DEFAULT.get(i) {
            Some(x) => *x,
            None => return false,
        } {
            return false;
        }
        if a.value != match WANT_VALUE.get(i) {
            Some(x) => *x,
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
        if c.tag() != match WANT_TAGS.get(i) {
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
        if expected(i) != Some(c.fault()) {
            return false;
        }
        if c.shape.seal() == 0 || c.claim.seal() == 0 || c.seal() == 0 {
            return false;
        }
        if i < ACCEPT && c.fault() != Fault::Ok {
            return false;
        }
        if i >= ACCEPT && c.fault() == Fault::Ok {
            return false;
        }
        // 合法现场必须过域检查。
        if !c.shape.domain_ok() {
            return false;
        }
        i += 1;
    }
    cover(CORPUS).is_ok() && cover(CORPUS + 1) == Err(RN_SLOT)
}

/// 09 判序六维互斥：死槽各坏一维，掩码恰好一位。
fn judge_ok() -> bool {
    let mut k = ACCEPT;
    while k < CORPUS {
        let c = match case_at(k) {
            Some(x) => x,
            None => return false,
        };
        let f = c.fault();
        if f == Fault::Ok {
            return false;
        }
        if f != expected_reject_ndx(k) {
            return false;
        }
        let want_bit = 1u32 << fault_ndx(f);
        if c.claim.diff_mask(&c.actual()) != want_bit {
            return false;
        }
        k += 1;
    }
    let mut i = 0usize;
    while i < ACCEPT {
        let c = match case_at(i) {
            Some(x) => x,
            None => return false,
        };
        if c.fault() != Fault::Ok || c.claim.diff_mask(&c.actual()) != 0 {
            return false;
        }
        if !c.ok() {
            return false;
        }
        i += 1;
    }
    // 六个缺陷维两两不同。
    let mut a = 0usize;
    while a < REJECT {
        let fa = match Fault::reject_faults().get(a) {
            Some(x) => *x,
            None => return false,
        };
        let mut b = a + 1usize;
        while b < REJECT {
            let fb = match Fault::reject_faults().get(b) {
                Some(x) => *x,
                None => return false,
            };
            if fa.code() == fb.code() {
                return false;
            }
            b += 1;
        }
        a += 1;
    }
    judge_order_ok()
}

/// 10 四档矩阵覆盖：每档都扫全槽，四档都有落点，红格数对得上。
fn matrix_ok() -> bool {
    let m = Matrix::build();
    if m.slots() != CORPUS * TIER_COUNT {
        return false;
    }
    if !m.covered() {
        return false;
    }
    let atomic = match m.row(Tier::Atomic) {
        Some(r) => r,
        None => return false,
    };
    let fallback = match m.row(Tier::Fallback) {
        Some(r) => r,
        None => return false,
    };
    let false_label = match m.row(Tier::FalseLabel) {
        Some(r) => r,
        None => return false,
    };
    let void = match m.row(Tier::Void) {
        Some(r) => r,
        None => return false,
    };
    if atomic.total != CORPUS || fallback.total != CORPUS {
        return false;
    }
    if false_label.total != CORPUS || void.total != CORPUS {
        return false;
    }
    if atomic.good != 3 || fallback.good != 3 {
        return false;
    }
    if false_label.good != 3 || void.good != 1 {
        return false;
    }
    if m.goods() != 10 {
        return false;
    }
    if m.red_total() != CORPUS * TIER_COUNT - 10 {
        return false;
    }
    if m.perfect_rows() != 0 {
        return false;
    }
    if atomic.red() != CORPUS - 3 {
        return false;
    }
    if !m.render().as_str().contains("atomic=") {
        return false;
    }
    if !m.render().as_str().contains("false-label=") {
        return false;
    }
    report_agrees_with_matrix()
}

/// 11 全绿渲染逐字：报告一行一字不许改，空报告必须被闸门挡住。
fn render_ok() -> bool {
    let rep = match run() {
        Ok(r) => r,
        Err(_) => return false,
    };
    if !rep.green() {
        return false;
    }
    if rep.render().as_str() != RENDER_OK {
        return false;
    }
    if rep.rate() != RATE_FULL || rep.reds() != 0 {
        return false;
    }
    if rep.atomic != ATOMIC_SCENES || rep.fallback != FALLBACK_SCENES {
        return false;
    }
    if rep.false_label != 0 || rep.void != 0 {
        return false;
    }
    if rep.tier_cells() != FAMILY_SCENES as u32 {
        return false;
    }
    let empty = empty_report();
    if empty.green() || empty.gate() != Err(RN_MATRIX) {
        return false;
    }
    if empty.rate() != 0 || empty.render().as_str() == RENDER_OK {
        return false;
    }
    if run_with(Some((CORPUS, Fault::Value))) != Err(RN_SLOT) {
        return false;
    }
    match run_with(Some((2, Fault::Value))) {
        Ok(bad) => bad.conflicts == 1 && bad.matched + 1 == rep.matched,
        Err(_) => false,
    }
}

/// 12 阶段闸门不倒退：正序放行、倒退拒、封账后再动拒。
fn gate_ok() -> bool {
    if Gate::full_pass().is_err() {
        return false;
    }
    let walk = Gate::walk(&[Stage::Check, Stage::Resolve, Stage::Scope, Stage::Atomic]);
    let mut g = match walk {
        Ok(x) => x,
        Err(_) => return false,
    };
    if g.stage() != Some(Stage::Atomic) || g.steps() != 4 {
        return false;
    }
    if g.step(Stage::Journal).is_err() {
        return false;
    }
    if g.step(Stage::Scope) != Err(RN_STAGE) {
        return false;
    }
    if g.refusals() != 1 || g.stage() != Some(Stage::Journal) {
        return false;
    }
    let root = match g.close() {
        Ok(x) => x,
        Err(_) => return false,
    };
    if root == 0 || !g.is_closed() {
        return false;
    }
    if g.step(Stage::Exit) != Err(RN_ESTATE) {
        return false;
    }
    if g.close() != Err(RN_ESTATE) {
        return false;
    }
    if g.refusals() != 3 {
        return false;
    }
    let mut fresh = Gate::new();
    if fresh.close() != Err(RN_STAGE) {
        return false;
    }
    if Gate::walk(&[Stage::Exit, Stage::Check]) != Err(RN_STAGE) {
        return false;
    }
    if Gate::walk(&[Stage::Check, Stage::Check]).is_err() {
        return false;
    }
    if !g.render().as_str().contains("closed=true") {
        return false;
    }
    if Gate::new().render().as_str() != "gate stage=none steps=0 refusals=0 closed=false" {
        return false;
    }
    true
}

/// 13 之前：账本逐格复核、冻结、重放、错位拒收。
fn ledger_ok() -> bool {
    let mut l = build_ledger();
    if l.len() != SAMPLES || l.is_empty() {
        return false;
    }
    if l.audit() != Ok(()) {
        return false;
    }
    if l.ok_count() != STAT_CELLS + 1 || l.err_count() != REJECT {
        return false;
    }
    if l.first_mismatch().is_some() {
        return false;
    }
    if build_ledger_broken(0).audit() == Ok(()) {
        return false;
    }
    if build_ledger_broken(1).audit() == Ok(()) {
        return false;
    }
    if build_ledger_broken(2).audit() == Ok(()) {
        return false;
    }
    if build_ledger_broken(9).audit() != Ok(()) {
        return false;
    }
    let mut bad = Ledger::new();
    let s = Sample {
        slot: 3,
        op: 1,
        ok: true,
        code: 0,
        value: 1,
    };
    if bad.register(s) != Err(RN_SLOT) {
        return false;
    }
    if bad.rejects() != 1 {
        return false;
    }
    let h = match l.freeze() {
        Ok(x) => x,
        Err(_) => return false,
    };
    if h == 0 || l.root() != h || !l.is_frozen() {
        return false;
    }
    if l.replay() != Ok(h) {
        return false;
    }
    if l.freeze() != Err(RN_ESTATE) {
        return false;
    }
    if l.poke(0, 1) != Err(RN_ESTATE) {
        return false;
    }
    if !l.render().as_str().contains("len=14") {
        return false;
    }
    if !l.render().as_str().contains("frozen=true") {
        return false;
    }
    true
}

/// 汇总：十三条判据，一条不多一条不少。
pub fn run_unx11690_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11690_rename_enospc");
    s.add("RN01-名称与段基址", name_ok(), "static");
    s.add("RN02-十三码互异", codes_ok(), "static");
    s.add("RN03-同盘不搬数据", atomic_ok(), "static");
    s.add("RN04-errno 归属闸", attribution_ok(), "static");
    s.add("RN05-假标签不并档", false_ok(), "static");
    s.add("RN06-原子性防伪造", forge_ok(), "static");
    s.add("RN07-文案逐字", text_ok(), "static");
    s.add("RN08-语料十四槽互校", corpus_ok(), "static");
    s.add("RN09-判序六维互斥", judge_ok(), "static");
    s.add("RN10-四档矩阵覆盖", matrix_ok(), "static");
    s.add("RN11-全绿渲染逐字", render_ok(), "static");
    s.add("RN12-阶段闸门不倒退", gate_ok(), "static");
    s.add(
        "RN13-冻结重放四注入",
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
        assert_eq!(SEG_BASE, 0x6d370);
        assert_eq!(ANCHOR_F11689_SEG + ANCHOR_F11689_GAP, SEG_BASE);
        assert_eq!(ANCHOR_F11687_SEG + ANCHOR_F11687_GAP, SEG_BASE);
        assert_eq!(SEG_BASE + ANCHOR_F11691_GAP, ANCHOR_F11691_SEG);
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
        assert_eq!(VALS[1] as usize, ATOMIC_SCENES);
        assert_eq!(VALS[2] as usize, FALLBACK_SCENES);
        assert_eq!(VALS[3] as usize, ENOSPC_SCENES);
        assert_eq!(VALS[4] as usize, FAMILY_SCENES);
        assert_eq!(VALS[5], 0);
        assert_eq!(VALS[6], RATE_FULL);
        assert_eq!(VALS[7], RED_RATE);
    }

    #[test]
    fn t06_stage_ranks_run_from_one_to_eight() {
        let mut r = 1u32;
        while r <= STAGE_LAST_RANK {
            let s = Stage::from_rank(r).expect("rank");
            assert_eq!(s.rank(), r);
            assert_eq!(Stage::from_rank(r), Some(s));
            r += 1;
        }
        assert_eq!(Stage::from_rank(0), None);
        assert_eq!(Stage::from_rank(9), None);
        assert_eq!(Stage::ALL.len(), 8);
    }

    #[test]
    fn t07_note_ranks_and_slugs_are_unique() {
        let notes = [
            Note::MetadataOnly,
            Note::ExdevOnly,
            Note::FallbackDone,
            Note::FallbackEnospc,
            Note::FalseEnospc,
            Note::FalseExdev,
            Note::HalfMove,
            Note::Usage,
            Note::Nil,
        ];
        let mut i = 0usize;
        while i < notes.len() {
            assert_eq!(notes[i].rank(), i as u32 + 1);
            assert_eq!(Note::from_rank(notes[i].rank()), Some(notes[i]));
            let mut j = i + 1usize;
            while j < notes.len() {
                assert_ne!(notes[i].slug(), notes[j].slug());
                j += 1;
            }
            i += 1;
        }
        assert_eq!(Note::from_rank(0), None);
    }

    #[test]
    fn t08_rename_line_invariants() {
        assert!(!RenameLine::SameDev.rename_can_enospc());
        assert!(!RenameLine::CrossDev.rename_can_enospc());
        assert!(!RenameLine::SameDev.expects_exdev());
        assert!(RenameLine::CrossDev.expects_exdev());
        assert!(!RenameLine::SameDev.needs_fallback());
        assert!(RenameLine::CrossDev.needs_fallback());
        assert_ne!(RenameLine::SameDev.slug(), RenameLine::CrossDev.slug());
        assert_eq!(RenameLine::ALL.len(), 2);
    }

    #[test]
    fn t09_report_errno_mapping() {
        assert_eq!(Errno::None.errno(), 0);
        assert_eq!(Errno::Enospc.errno(), 28);
        assert_eq!(Errno::Exdev.errno(), 18);
        assert_eq!(Errno::Eacces.errno(), 13);
        assert_eq!(Errno::Enoent.errno(), 2);
        assert!(Errno::Enospc.is_space());
        assert!(!Errno::Exdev.is_space());
        assert_eq!(Errno::from_errno(13), Some(Errno::Eacces));
        assert_eq!(Errno::from_errno(2), Some(Errno::Enoent));
    }

    #[test]
    fn t10_fault_codes_are_the_defect_table() {
        let mut r = 1u32;
        while r <= 6 {
            let f = ndx_fault(r as usize - 1).expect("fault");
            assert_eq!(f.rank(), r);
            assert_eq!(fault_ndx(f), r as usize - 1);
            assert!(f.is_fault());
            assert_eq!(f.code(), SEG_BASE + r + 1);
            r += 1;
        }
        assert!(!Fault::Ok.is_fault());
        assert_eq!(fault_ndx(Fault::Ok), 6);
    }

    #[test]
    fn t11_tier_greenness_and_ranks() {
        assert!(Tier::Atomic.is_green());
        assert!(Tier::Fallback.is_green());
        assert!(!Tier::FalseLabel.is_green());
        assert!(!Tier::Void.is_green());
        assert_eq!(Tier::from_rank(3), Some(Tier::FalseLabel));
        assert_eq!(Tier::from_rank(5), None);
        assert_eq!(Tier::ALL.len(), 4);
    }

    #[test]
    fn t12_fold_helpers_are_not_identities() {
        let h = FNV_OFF;
        assert_eq!(fold_byte(h, b'A'), fold_str(h, "A"));
        assert_ne!(fold_word(h, 0), h);
        assert_ne!(fold_bool(h, false), fold_bool(h, true));
        assert_ne!(fold_word(h, 1), fold_word(h, 2));
        assert_ne!(fold_i32(h, -1), fold_i32(h, 1));
    }

    #[test]
    fn t13_decimal_and_hex_renderers() {
        assert_eq!(dec_text(0).as_str(), "0");
        assert_eq!(dec_text(28).as_str(), "28");
        assert_eq!(dec_text(4096).as_str(), "4096");
        assert_eq!(u32_text(929).as_str(), "929");
        assert_eq!(i32_text(-7).as_str(), "-7");
        assert_eq!(hex_text(0x0b, 2).as_str(), "0b");
        assert_eq!(hex_text(SEG_BASE, 6).as_str(), "06d370");
        assert_eq!(seg_text(RN_FALSE).as_str(), "0x06d37b");
        assert_eq!(permille_text(1000).as_str(), "1000permille");
        assert_eq!(bool_text(true).as_str(), "true");
    }

    #[test]
    fn t14_same_dev_shape_carries_no_data() {
        let s = Shape::same_dev(BLOCK);
        assert_eq!(s.line, RenameLine::SameDev);
        assert!(s.moved && s.src_removed);
        assert!(!s.fallback_used && !s.fallback_complete);
        assert_eq!(s.copied(), 0);
        assert!(s.domain_ok());
        assert!(!s.false_label());
    }

    #[test]
    fn t15_cross_dev_exdev_shape() {
        let s = Shape::cross_dev_exdev();
        assert_eq!(s.report, Errno::Exdev);
        assert!(!s.moved && !s.src_removed);
        assert!(!s.fallback_used);
        assert!(s.domain_ok());
        assert!(!s.false_label());
        assert_ne!(s.seal(), Shape::same_dev(BLOCK).seal());
    }

    #[test]
    fn t16_cross_dev_enospc_counts_copied_bytes() {
        let s = Shape::cross_dev_enospc(BLOCK * 2, HALF_BLOCK);
        assert!(s.fallback_enospc());
        assert_eq!(s.copied(), HALF_BLOCK);
        assert!(s.domain_ok());
        let rich = Shape::cross_dev_enospc(BLOCK, BLOCK * 4);
        assert!(!rich.fallback_enospc());
        assert_eq!(rich.copied(), BLOCK);
    }

    #[test]
    fn t17_false_label_definition() {
        assert!(SHAPE_FALSE_SAME.false_label());
        assert!(SHAPE_FALSE_CROSS.false_label());
        assert!(!SHAPE_FALLBACK_ENOSPC.false_label());
        assert!(!SHAPE_SAMEDEV.false_label());
        // 走了退化但空间明明够，这条 ENOSPC 也是假的。
        assert!(Shape::cross_dev_enospc(BLOCK, BLOCK).false_label());
        // 同盘 EXDEV 同样不可能。
        let mut ex = SHAPE_SAMEDEV;
        ex.report = Errno::Exdev;
        ex.moved = false;
        ex.src_removed = false;
        assert!(ex.false_label());
    }

    #[test]
    fn t18_define_same_dev_is_metadata_only() {
        let o = define(&SHAPE_SAMEDEV);
        assert!(o.ok());
        assert_eq!(o.stage, Stage::Exit);
        assert_eq!(o.value, 0);
        assert_eq!(o.note, Note::MetadataOnly);
        assert!(!o.default);
    }

    #[test]
    fn t19_full_disk_never_blocks_rename() {
        let s = scene_at(2).expect("scene 2");
        assert_eq!(s.free, 0);
        let o = define(&s);
        assert!(o.ok());
        assert_eq!(o.value, 0);
        assert_eq!(o.note, Note::MetadataOnly);
        let quiet = msg_atomic_done(SRC_SAME, DST_SAME);
        assert!(!quiet.as_str().contains(ANCHOR_ENOSPC_TEXT));
        let loud = msg_false_enospc(SRC_SAME, DST_SAME);
        assert!(loud.as_str().contains(ANCHOR_ENOSPC_TEXT));
    }

    #[test]
    fn t20_define_cross_exdev_is_sound() {
        let o = define(&SHAPE_CROSS_EXDEV);
        assert!(!o.ok());
        assert_eq!(o.stage, Stage::Report);
        assert_eq!(o.code, 0);
        assert_eq!(o.value, 0);
        assert_eq!(o.note, Note::ExdevOnly);
    }

    #[test]
    fn t21_define_fallback_done_counts_bytes() {
        let o = define(&SHAPE_FALLBACK_DONE);
        assert!(o.ok());
        assert_eq!(o.stage, Stage::Exit);
        assert_eq!(o.value, BLOCK * 2);
        assert_eq!(o.note, Note::FallbackDone);
        assert!(o.via_fallback());
    }

    #[test]
    fn t22_define_fallback_enospc_attributes_to_copy() {
        let o = define(&SHAPE_FALLBACK_ENOSPC);
        assert!(!o.ok());
        assert_eq!(o.stage, Stage::Fallback);
        assert_eq!(o.value, HALF_BLOCK);
        assert_eq!(o.note, Note::FallbackEnospc);
        assert!(!o.false_label());
        assert!(o.via_fallback());
    }

    #[test]
    fn t23_false_enospc_on_same_dev_dies_at_journal() {
        let o = define(&SHAPE_FALSE_SAME);
        assert!(!o.ok());
        assert_eq!(o.stage, Stage::Journal);
        assert_eq!(o.code, RN_FALSE);
        assert_eq!(o.note, Note::FalseEnospc);
        assert!(o.false_label());
    }

    #[test]
    fn t24_false_enospc_on_cross_dev_dies_at_report() {
        let o = define(&SHAPE_FALSE_CROSS);
        assert!(!o.ok());
        assert_eq!(o.stage, Stage::Report);
        assert_eq!(o.code, RN_FALSE);
        assert_eq!(o.note, Note::FalseEnospc);
    }

    #[test]
    fn t25_half_move_is_never_trusted() {
        let mut a = Shape::same_dev(BLOCK);
        a.src_removed = false;
        let oa = define(&a);
        assert_eq!(oa.stage, Stage::Atomic);
        assert_eq!(oa.code, RN_ATOMIC);
        assert_eq!(oa.note, Note::HalfMove);
        let mut b = Shape::same_dev(BLOCK);
        b.moved = false;
        let ob = define(&b);
        assert_eq!(ob.code, RN_ATOMIC);
        assert_eq!(ob.note, Note::HalfMove);
        assert!(!oa.ok() && !ob.ok());
    }

    #[test]
    fn t26_usage_error_stops_at_check() {
        let mut s = Shape::same_dev(BLOCK);
        s.bad_call = true;
        let o = define(&s);
        assert_eq!(o.stage, Stage::Check);
        assert_eq!(o.code, RN_SLOT);
        assert_eq!(o.note, Note::Usage);
        assert!(!o.ok());
    }

    #[test]
    fn t27_default_bucket_is_a_void_tier() {
        let o = define(&SHAPE_DEFAULT_BUCKET);
        assert!(o.default);
        assert!(!o.ok());
        assert_eq!(o.note, Note::Nil);
        assert_eq!(Tier::of(&o), Tier::Void);
    }

    #[test]
    fn t28_corpus_has_fourteen_slots() {
        assert_eq!(cases().len(), CORPUS);
        assert_eq!(SHAPES.len(), SAMPLES);
        assert_eq!(CLAIMS.len(), CORPUS);
        assert!(case_at(CORPUS).is_none());
        assert!(case_at(CORPUS + 5).is_none());
        let mut i = 0usize;
        while i < CORPUS {
            let c = case_at(i).expect("case");
            assert_eq!(c.tag(), WANT_TAGS[i]);
            assert!(!c.tag().is_empty());
            i += 1;
        }
    }

    #[test]
    fn t29_live_slots_all_pass_judgement() {
        let mut i = 0usize;
        while i < ACCEPT {
            let c = case_at(i).expect("case");
            assert_eq!(c.fault(), Fault::Ok);
            assert_eq!(c.claim.diff_mask(&c.actual()), 0);
            assert_eq!(c.actual(), define(&c.shape));
            i += 1;
        }
    }

    #[test]
    fn t30_dead_slots_each_break_one_dimension() {
        let mut k = ACCEPT;
        while k < CORPUS {
            let c = case_at(k).expect("case");
            let f = expected_reject_ndx(k);
            assert_eq!(c.fault(), f);
            assert_ne!(f, Fault::Ok);
            assert_eq!(REJECT_CODES[fault_ndx(f)], f.code());
            k += 1;
        }
    }

    #[test]
    fn t31_dead_slots_set_exactly_one_mask_bit() {
        let mut k = ACCEPT;
        while k < CORPUS {
            let c = case_at(k).expect("case");
            let f = expected_reject_ndx(k);
            assert_eq!(c.claim.diff_mask(&c.actual()), 1u32 << fault_ndx(f));
            k += 1;
        }
    }

    #[test]
    fn t32_want_tables_agree_with_corpus() {
        let mut i = 0usize;
        while i < CORPUS {
            let c = case_at(i).expect("case");
            let a = c.actual();
            assert_eq!(a.stage, WANT_STAGE[i]);
            assert_eq!(a.code, WANT_CODE[i]);
            assert_eq!(a.note, WANT_NOTE[i]);
            assert_eq!(a.value, WANT_VALUE[i]);
            assert_eq!(a.default, WANT_DEFAULT[i]);
            assert_eq!(a.ok(), WANT_SUCCESS[i]);
            assert_eq!(c.shape.exit, WANT_EXIT[i]);
            i += 1;
        }
    }

    #[test]
    fn t33_estate_outranks_every_other_dimension() {
        let s = scene_at(0).expect("scene");
        let a = define(&s);
        let wrong = Claim::new(
            "order-probe",
            !a.ok(),
            Stage::Check,
            RN_SLOT,
            a.value.wrapping_add(9),
            Note::HalfMove,
            true,
        );
        let c = Case::new(s, wrong);
        assert_eq!(judge(&c), Fault::Estate);
    }

    #[test]
    fn t34_default_is_judged_in_both_directions() {
        let actual = define(&SHAPE_DEFAULT_BUCKET);
        let c = Case::new(
            SHAPE_DEFAULT_BUCKET,
            Claim::from_outcome(&actual, false, "default-probe"),
        );
        assert_eq!(judge(&c), Fault::Default);
        let s = scene_at(0).expect("scene");
        let a = define(&s);
        let hype = Claim::new("default-hype", a.ok(), a.stage, a.code, a.value, a.note, true);
        assert_eq!(judge(&Case::new(s, hype)), Fault::Default);
    }

    #[test]
    fn t35_family_scenes_split_four_and_eight() {
        assert_eq!(scene_at(FAMILY_SCENES), None);
        assert_eq!(scene_counts(), (ATOMIC_SCENES, FALLBACK_SCENES));
        assert_eq!(scene_enospc_count(), ENOSPC_SCENES);
        assert_eq!(scenes().len(), FAMILY_SCENES);
    }

    #[test]
    fn t36_family_scenes_are_all_green() {
        assert!(scenes_agree());
        let mut i = 0usize;
        while i < FAMILY_SCENES {
            assert_eq!(scene_anchored(i), Some(true));
            let c = scene_case(i).expect("scene case");
            assert_eq!(c.fault(), Fault::Ok);
            assert_eq!(c.claim.success(), c.actual().ok());
            i += 1;
        }
    }

    #[test]
    fn t37_scene_seals_are_pairwise_distinct() {
        let mut i = 0usize;
        while i < FAMILY_SCENES {
            let hi = scene_seal(i).expect("seal");
            assert_ne!(hi, 0);
            assert_eq!(scene_seal(i), Some(hi));
            let mut j = i + 1usize;
            while j < FAMILY_SCENES {
                assert_ne!(hi, scene_seal(j).expect("seal"));
                j += 1;
            }
            i += 1;
        }
        assert_eq!(scene_seal(FAMILY_SCENES), None);
    }

    #[test]
    fn t38_matrix_scans_every_slot_in_every_tier() {
        let m = Matrix::build();
        assert_eq!(m.slots(), CORPUS * TIER_COUNT);
        assert!(m.covered());
        assert!(m.render().as_str().starts_with("matrix atomic="));
        assert_eq!(m.row(Tier::Atomic).map(|r| r.total), Some(CORPUS));
        assert_eq!(m.row(Tier::Void).map(|r| r.good), Some(1));
    }

    #[test]
    fn t39_matrix_counts_goods_and_reds() {
        let m = Matrix::build();
        assert_eq!(m.goods(), 10);
        assert_eq!(m.red_total(), CORPUS * TIER_COUNT - 10);
        assert_eq!(m.perfect_rows(), 0);
        assert_eq!(m.row(Tier::FalseLabel).map(|r| r.good), Some(3));
        assert_eq!(m.row(Tier::Fallback).map(|r| r.good), Some(3));
    }

    #[test]
    fn t40_run_is_green_and_renders_verbatim() {
        let rep = run().expect("green run");
        assert!(rep.green());
        assert_eq!(rep.render().as_str(), RENDER_OK);
        assert_eq!(rep.rate(), MATCH_FULL);
        assert_eq!(rep.reds(), 0);
        assert_eq!(rep.tier_cells(), FAMILY_SCENES as u32);
    }

    #[test]
    fn t41_empty_report_is_blocked() {
        let e = empty_report();
        assert_eq!(e.gate(), Err(RN_MATRIX));
        assert!(!e.green());
        assert_eq!(e.rate(), 0);
        assert_eq!(run_with(Some((CORPUS, Fault::Note))), Err(RN_SLOT));
    }

    #[test]
    fn t42_injected_fault_lowers_the_rate() {
        assert!(red_rate_calibrated());
        let base = run().expect("base");
        let bad = run_with(Some((2, Fault::Value))).expect("bad");
        assert_eq!(bad.conflicts, 1);
        assert!(bad.rate() < base.rate());
        assert_eq!(bad.matched + 1, base.matched);
    }

    #[test]
    fn t43_gate_forwards_and_refuses_regression() {
        let mut g = Gate::walk(&[Stage::Check, Stage::Resolve, Stage::Scope])
            .expect("walk");
        assert_eq!(g.stage(), Some(Stage::Scope));
        assert_eq!(g.step(Stage::Resolve), Err(RN_STAGE));
        assert_eq!(g.refusals(), 1);
        assert_eq!(g.step(Stage::Atomic), Ok(()));
        assert_eq!(g.steps(), 4);
        assert_ne!(g.root(), 0);
        assert!(Gate::full_pass().is_ok());
    }

    #[test]
    fn t44_gate_closes_once() {
        let mut g = Gate::new();
        assert_eq!(g.close(), Err(RN_STAGE));
        assert_eq!(g.step(Stage::Check), Ok(()));
        let h = g.close().expect("close");
        assert_ne!(h, 0);
        assert!(g.is_closed());
        assert_eq!(g.step(Stage::Exit), Err(RN_ESTATE));
        assert_eq!(g.close(), Err(RN_ESTATE));
        assert_eq!(g.refusals(), 2);
    }

    #[test]
    fn t45_ledger_registers_and_audits() {
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
    fn t46_broken_ledgers_are_caught() {
        assert_eq!(build_ledger_broken(0).audit(), Err(RN_VALUE));
        assert_eq!(build_ledger_broken(1).audit(), Err(RN_VALUE));
        assert_eq!(build_ledger_broken(2).audit(), Err(RN_VALUE));
        assert_eq!(build_ledger_broken(7).audit(), Ok(()));
        assert!(ledger_ok());
    }

    #[test]
    fn t47_frozen_ledger_refuses_pokes() {
        let mut l = build_ledger();
        let h = l.freeze().expect("freeze");
        assert_eq!(l.root(), h);
        assert_eq!(l.freeze(), Err(RN_ESTATE));
        assert_eq!(l.poke(0, 1), Err(RN_ESTATE));
        assert_eq!(l.replay(), Ok(h));
        assert!(l.render().as_str().contains("frozen=true"));
    }

    #[test]
    fn t48_ledger_rejects_misaligned_slots() {
        let mut l = Ledger::new();
        assert!(l.is_empty());
        let s = Sample {
            slot: 1,
            op: 1,
            ok: true,
            code: 0,
            value: 1,
        };
        assert_eq!(l.register(s), Err(RN_SLOT));
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
    fn t49_four_injections_are_independent() {
        let four = inject_four();
        assert_eq!(four.len(), 4);
        assert!(four[0]);
        assert!(four[1]);
        assert!(four[2]);
        assert!(four[3]);
        assert_eq!(head_tag(), "rename-samedev");
        assert_eq!(head_val(), FAMILY_SCENES as u32);
    }

    #[test]
    fn t50_all_thirteen_checks_are_green() {
        let s = run_unx11690_checks();
        assert_eq!(s.len(), 13);
        let red = s.red_items_text();
        assert!(s.all_passed(), "red: {}", red);
        assert_eq!(s.get(0).map(|c| c.name), Some("RN01-名称与段基址"));
        assert_eq!(s.get(12).map(|c| c.name), Some("RN13-冻结重放四注入"));
    }
}
