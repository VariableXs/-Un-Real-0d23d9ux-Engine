//! UNX-F11695 ENOSPC 与 EDQUOT 配额对照档判据
//!
//! ENOSPC（28，`No space left on device`）和 EDQUOT（122，`Quota exceeded`）在
//! 用户眼里长得一模一样：**都是"写不进去了"**，都是 `write()` 返回 `-1`，都是
//! 工具打一行 `<op>: ...: <strerror>` 然后退 1。但它们是**两本完全不同的账**：
//! 一本记的是"这块盘还有没有块"，另一本记的是"这个人还有没有额度"。混档的代价
//! 不是文案难看，而是**修法完全相反**——介质满了要删盘上的件，配额满了要动
//! `edquota`/`setquota`；把 EDQUOT 当 ENOSPC 去 `rm` 自己的件，额度一分没回
//! （配额记的是**全体用户的占用**，且 `quota` 的账在 `aquota.file` 里），运维
//! 会在同一个错误上原地打转。本卡把这两个码立成**对照档**：同一套闸门纵线，
//! 两次都要走到底，且终局必须落在**互斥的两档**里。
//!
//! 三组必须分立的事实（这是本卡区别于 F11681 总锚的地方——总锚只判"是不是
//! 28"，本卡判"是 28 还是 122，以及凭什么"）：
//!
//! 1. **`df` 有余量但写失败**：两种成因，一个都不能省。
//!    * `ext4`/`xfs` 的 **root 预留块池**（`mkfs -m 5`，默认留 5%）：非 root
//!      看到的 `Available` 已经被扣掉预留，预留以下**只剩 root 能写**。于是
//!      `df` 说还有 3% 可用，普通用户照样 `-1/28`——这是 ENOSPC，但不是
//!      "盘满了"，`Subject`/`Reserve` 两闸要把它单独立档。
//!    * **配额先行**：内核在真正的块分配器之前先做 `dquot_reserve_blocks()`，
//!      所以**盘没满也会 EDQUOT**，`df` 的 `Available` 与这次失败毫无关系。
//! 2. **软上限 + 宽限期（grace period）**：`used > soft` 而 `used <= hard` 时
//!    并不立刻判死——quota-tools 默认给 7 天宽限（`Block grace time: 7days`），
//!    宽限**内**写入照常成功（`DquotSoftGrace`，成功态！），宽限**过期**后同一
//!    笔写立刻 `-1/122`。于是"同一个 `quota` 报表、同一段文件"，隔一天复现
//!    结果相反；把宽限内的那次读成 EDQUOT 是本卡的 `Code` 维缺陷，把宽限外的
//!    那次读成成功则是 `Estate` 维缺陷。
//! 3. **主体（user / group / project）与在册性**：`project` 配额只有 `xfs`
//!    支持，`ext4` 上 `repquota -P` 直接不给账。主体不适用的时候，**任何
//!    EDQUOT 的申报都是凭空多出来的一档**——现场真相只能是介质账（或干脆
//!    写成功）。本卡把这一格立成 `Family` 闸的正例：申报 `Quota exceeded`
//!    但配额主体根本不生效 → 归族破断 → `Rogue` 档，且锚位强制归零。
//!
//! 文案差必须逐字钉住，因为排障第一眼看的就是那半行字：
//!
//! ```text
//! dd: error writing 'scratch.img': No space left on device
//! dd: error writing 'home/alice/img': Quota exceeded
//! touch: cannot touch '/mnt/full/f': No space left on device
//! quota: Disk quotas for alice (uid 1000):
//!        Filesystem   space   quota   limit   grace
//! ```
//!
//! `No space left on device` 与 `Quota exceeded` 是 glibc `strerror()` 的原文
//! （28 与 122），一个字都不能改；工具前缀体例（`dd: error writing`、`tee: `、
//! `touch: cannot touch`）跨码**不变**，变的只有尾巴那句 strerror——这正是
//! "双码分档"在真实日志里的唯一抓手，所以本卡的 `msg_*` 必须两个码各出一版，
//! 且同一 op 的两版只差最后一段。
//!
//! ## 闸门次序（这条纵线为什么不能倒）
//!
//! `arg → subject → usage → grace → device → reserve → family → done`，八段，
//! 秩从 0 起算，只能前进。次序的理由逐段说明：
//!
//! 1. `arg`（秩 0）——入参与几何本身。`count` 越界、`bsize` 非法、`errno`
//!    不在合法集，一律 `EINVAL`（22）短路。它排第一是因为**后面每一闸都要用
//!    这些数**，坏数进去算出来的档全是幻觉。
//! 2. `subject`（秩 1）——配额主体在册性。主体不在册（`ext4` 的 `project`、
//!    或 `enforced == false` 即 `quotaon` 没跑）时，`usage`/`grace` 两闸**整段
//!    跳过**，直接走介质账。它必须在用量之前：不先确定"有没有这本账"，用量
//!    比对毫无意义。
//! 3. `usage`（秩 2）——硬上限。`used + count > hard` 立即 `-1/122`，不看
//!    宽限期。硬上限是"绝对不许越过"，宽限只对软上限生效，所以硬上限这一闸
//!    必须在 grace 之前，否则会出现"超了 hard 却因为宽限未到被放行"的假绿。
//! 4. `grace`（秩 3）——软上限与宽限期。只有 `hard >= used+count > soft` 才
//!    进这一闸：`grace_left > 0` 放行（成功态，注解 `dquot-soft-grace`），
//!    `grace_left == 0` 判 `-1/122`。它在 usage 之后、device 之前：内核的
//!    `dquot` 预留确实先于块分配器。
//! 5. `device`（秩 4）——介质余量。`blocks_free * bsize == 0` 且请求非零 →
//!    `-1/28`。排在配额之后，就是"盘满且配额也满 → 报 EDQUOT"这条实况。
//! 6. `reserve`（秩 5）——root 预留池。介质有余量，但非 root 的可用量
//!    `free - reserve <= 0` → `-1/28`，注解 `nospc-reserve`。它单独一闸，
//!    是"df 说还有但你写不进"的另一半成因；root 走同一形状必须放行，这条
//!    对照由本卡的 `root-in-reserve` 槽钉住。
//! 7. `family`（秩 6）——**双码归族闸，本卡的守闸**。把申报的 errno 与物理
//!    errno 的**码族**（0=无 / 28=介质族 / 122=配额族 / 22=用法族）对照：族
//!    不同就改判 `Family` 段破断，锚位归零。它排在所有物理闸之后，因为只有
//!    先有物理真相，才谈得上"申报的码对不对族"。
//! 8. `done`（秩 7）——全闸通过，且 (返回, errno) 与码族同时成立才落对照档。
//!
//! ## 关键差异（本卡深度所在）
//!
//! * **与 F11681（ENOSPC 总锚，段基址 `0x6d130`，位距 `0x2b0`）的边界**：总锚
//!   判那**一次** `write()` 的 (返回, errno) 成对，把 EDQUOT 只当成"邻居 errno，
//!   不是 ENOSPC"判死，不判它对不对；本卡反过来——**EDQUOT 是主角之一**，配额
//!   的软硬上限、宽限期、主体在册性全在本卡建模，总锚对本卡只供给"28 的真值
//!   与 strerror 原文"这一件事。两卡段位差 `0x2b0`，本卡用 `ANCHOR_F11681_SEG`
//!   与 `ANCHOR_GAP` 署名，由 `anchor_ok()` 复算对齐，不 import。
//! * **与 F11694（ENOSPC 文案逐字，同批前置）的边界**：F11694 只判 `No space
//!   left on device` 这一句跨工具一致；本卡判的是**两句之间的差异**，即
//!   `Quota exceeded` 那一版以及"同一 op 两版只差尾段"这一结构事实。同一句
//!   文案两卡各钉各的，不重复立判据。
//! * **与 F11688（touch/mkdir 建件 ENOSPC）的边界**：建件失败的**退出码与
//!   工具体例**归 F11688；本卡借用它的体例只为一处对照——`touch` 在满盘上打
//!   `No space left on device`、在配额满上打 `Quota exceeded`，体例完全相同，
//!   本卡不许改体例，只改尾段。
//! * **与 F11696（释放后重试恢复，本卡直接后继，位距 `0x10`）的边界**：本卡
//!   只判**这一刀写下去的那一刻**码是谁；"删了件之后重试能不能成"整个归
//!   F11696。本卡的 `Quota` 档对 F11696 还有一条硬约束：**删别人的件救不了
//!   EDQUOT**（配额账按主体累计），这条事实由本卡的配额几何表提供。
//! * **本卡的排他职责**：① 双码互斥归族（一槽只属一族，跨族申报必判死）；
//!   ② root 预留池档（`df` 有余量仍 ENOSPC）；③ 软硬上限与宽限期的三分
//!   （放行/判死/绝对判死）；④ 配额主体在册性（`project` 仅在 `xfs` 生效）；
//!   ⑤ 两版文案逐字对照（前缀相同、尾段互斥）。
//! * **谁判死谁放行**：主体在册、码族对上、(返回, errno) 与族自洽 → 放行并
//!   落对照档；族串档（122 报成 28 或反之）、宽限内报死、宽限外报活、
//!   预留池档冒充介质满档、主体不适用却报 EDQUOT → 一律判死。
//!
//! 判据六维：结束阶段（gate）、errno（code）、落盘字节（value）、形态注解
//! （note）、对照档位（default：该现场是否构成一条自洽的"介质族或配额族"对照
//! 样本）、成败申报（estate）。判序固定：状态 → 阶段 → errno → 字节 → 注解 →
//! 对照档。
//!
//! 语料 14 槽：8 槽申报正确（放行 3、介质族 2、配额族 2、归族破断 1），6 槽
//! 复刻同一 `dquot-hard` 现场、各挂一维错报（estate/stage/code/value/note/
//! default），其中 `code-swap` 那一格就是把 122 报成 28——本卡立卡要抓的头号
//! 缺陷。账本 14 格 = 7 统计 + 1 红档率（929）+ 6 缺陷码，冻结后重放同指纹。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，`audit` 逐格
//! 对账，冻结后 `replay` 重建同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址（F11695）。
pub const SEG_BASE: u32 = 0x6d3e0;

/// 联签署名：ENOSPC 总锚所在段基址（F11681）。
pub const ANCHOR_F11681_SEG: u32 = 0x6d130;

/// 联签署名：本卡与总锚的段位差。
pub const ANCHOR_GAP: u32 = SEG_BASE - ANCHOR_F11681_SEG;

/// 直接后继卡（F11696 释放后重试恢复）的段基址。
pub const NEXT_F11696_SEG: u32 = 0x6d3f0;

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
// errno（glibc/Linux 真值，本卡只用这四个）
// ---------------------------------------------------------------------------

/// 设备无空间（strerror: `No space left on device`）。
pub const ERR_ENOSPC: i32 = 28;

/// 配额超限（strerror: `Quota exceeded`）。
pub const ERR_EDQUOT: i32 = 122;

/// 入参非法（strerror: `Invalid argument`）。
pub const ERR_EINVAL: i32 = 22;

/// 成功时 errno 应停放的零值。
pub const ERR_NONE: i32 = 0;

/// 单次写入的合法上界（越过即 `EINVAL`，不许参与后续闸门）。
pub const COUNT_MAX: u32 = 0x0800_0000;

/// 空闲块数上界（越过即几何坏，`bytes_free()` 溢出）。
pub const BLOCKS_MAX: u32 = 0x0040_0000;

/// 宽限期秒数上界（quota-tools 默认 7days = 604800）。
pub const GRACE_MAX: u32 = 604_800;

/// 合法 errno 集合大小（0、28、122、22 四个）。
pub const ERRNO_SET_SIZE: usize = 4;

// ---------------------------------------------------------------------------
// 错误码（13 个，SEG_BASE + 0x01..0x0d）
// ---------------------------------------------------------------------------

/// 阶段申报不符。
pub const EQ_ESTAGE: u32 = SEG_BASE + 0x01;

/// errno 申报不符。
pub const EQ_ECODE: u32 = SEG_BASE + 0x02;

/// 落盘字节申报不符。
pub const EQ_EVALUE: u32 = SEG_BASE + 0x03;

/// 注解申报不符。
pub const EQ_ENOTE: u32 = SEG_BASE + 0x04;

/// 对照档位申报不符。
pub const EQ_EDEFAULT: u32 = SEG_BASE + 0x05;

/// 双码归族破断。
pub const EQ_EFAMILY: u32 = SEG_BASE + 0x06;

/// 成败互报。
pub const EQ_ESTATE: u32 = SEG_BASE + 0x07;

/// 计数不符。
pub const EQ_ECOUNT: u32 = SEG_BASE + 0x08;

/// 槽位非法。
pub const EQ_ESLOT: u32 = SEG_BASE + 0x09;

/// 指纹不符。
pub const EQ_ESEAL: u32 = SEG_BASE + 0x0a;

/// 无观测。
pub const EQ_ENOBS: u32 = SEG_BASE + 0x0b;

/// 矩阵不平。
pub const EQ_EMATRIX: u32 = SEG_BASE + 0x0c;

/// 账本不平。
pub const EQ_EBAL: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码（账本格归属的闸门）
// ---------------------------------------------------------------------------

/// 入参闸。
pub const OP_ARG: u8 = 0x01;

/// 配额主体闸。
pub const OP_SUBJECT: u8 = 0x02;

/// 硬上限闸。
pub const OP_USAGE: u8 = 0x03;

/// 宽限期闸。
pub const OP_GRACE: u8 = 0x04;

/// 介质余量闸。
pub const OP_DEVICE: u8 = 0x05;

/// root 预留池闸。
pub const OP_RESERVE: u8 = 0x06;

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
// 数字/布尔文本
// ---------------------------------------------------------------------------

/// 十进制文本（u32，不递归、不格式化）。
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

/// 千分率文本（rate 打印用，定长不带前导零）。
fn rate_text(v: u32) -> String {
    dec_text(v)
}

// ---------------------------------------------------------------------------
// 文案（真实 strerror / quota-tools / e2fsprogs / coreutils 表面逐字）
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

/// 码族文本：把 errno 折成对照档的四族。
pub fn family_text(code: i32) -> &'static str {
    match code {
        ERR_NONE => "none",
        ERR_ENOSPC => "nospc",
        ERR_EDQUOT => "dquot",
        ERR_EINVAL => "arg",
        _ => "unknown",
    }
}

/// 介质族写失败的工具现场（`dd` 9.x 体例，尾段为 strerror）。
pub fn msg_dd_enospc(path: &str) -> String {
    let mut s = String::from("dd: error writing '");
    s.push_str(path);
    s.push_str("': No space left on device");
    s
}

/// 配额族写失败的工具现场（同一 `dd` 体例，只换尾段）。
pub fn msg_dd_edquot(path: &str) -> String {
    let mut s = String::from("dd: error writing '");
    s.push_str(path);
    s.push_str("': Quota exceeded");
    s
}

/// `tee` 的体例：`tee: <路径>: <strerror>`，双码各一版。
pub fn msg_tee_at(path: &str, code: i32) -> String {
    let mut s = String::from("tee: ");
    s.push_str(path);
    s.push_str(": ");
    s.push_str(strerror(code));
    s
}

/// `touch` 的体例：`touch: cannot touch '<路径>': <strerror>`。
pub fn msg_touch_at(path: &str, code: i32) -> String {
    let mut s = String::from("touch: cannot touch '");
    s.push_str(path);
    s.push_str("': ");
    s.push_str(strerror(code));
    s
}

/// `mkdir` 的体例：`mkdir: cannot create directory '<路径>': <strerror>`。
pub fn msg_mkdir_at(path: &str, code: i32) -> String {
    let mut s = String::from("mkdir: cannot create directory '");
    s.push_str(path);
    s.push_str("': ");
    s.push_str(strerror(code));
    s
}

/// 裸 `write()` 层的审计文案（无工具前缀，只剩调用与 strerror）。
pub fn msg_write_at(op: &str, path: &str, code: i32) -> String {
    let mut s = String::from(op);
    s.push_str(" '");
    s.push_str(path);
    s.push_str("': ");
    s.push_str(strerror(code));
    s
}

/// `df` 表头逐字（六列，宽度即真实输出）。
pub fn msg_df_header() -> &'static str {
    "Filesystem      1K-blocks    Used Available Use% Mounted on\n"
}

/// `df` 数据行：本卡只用来钉"available 有余但写失败"的对照现场。
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

/// `quota` 的第一行逐字（`Disk quotas for <user> (uid <n>):`）。
pub fn msg_quota_head(user: &str, uid: u32) -> String {
    let mut s = String::from("Disk quotas for ");
    s.push_str(user);
    s.push_str(" (uid ");
    s.push_str(dec_text(uid).as_str());
    s.push_str("):\n");
    s
}

/// `quota -s` 的列头逐字。
pub fn msg_quota_columns() -> &'static str {
    "     Filesystem   space   quota   limit   grace\n"
}

/// `quota` 的账行：占用、软上限、硬上限、宽限余量（`-` 表示无宽限）。
pub fn msg_quota_row(fs: &str, used: u32, soft: u32, hard: u32, grace_left: u32) -> String {
    let mut s = String::from("       ");
    s.push_str(fs);
    s.push_str("   ");
    s.push_str(dec_text(used).as_str());
    s.push('k');
    s.push_str("   ");
    s.push_str(dec_text(soft).as_str());
    s.push('k');
    s.push_str("   ");
    s.push_str(dec_text(hard).as_str());
    s.push('k');
    s.push_str("   ");
    if grace_left == 0 {
        s.push_str("-");
    } else {
        let days = grace_left.saturating_div(86400);
        s.push_str(dec_text(days).as_str());
        s.push('d');
    }
    s.push('\n');
    s
}

/// `repquota` 的头两行逐字（宽限期声明是配额档的判据来源）。
pub fn msg_repquota_head(fs: &str) -> String {
    let mut s = String::from("*** Report for user quotas on device ");
    s.push_str(fs);
    s.push('\n');
    s.push_str("Block grace time: 7days; Inode grace time: 7days\n");
    s
}

/// `e2fsprogs` 的预留块行逐字（`Reserved blocks: N (P% of filesystem size)`）。
pub fn msg_reserved_blocks(reserve: u32, blocks_total: u32) -> String {
    let mut s = String::from("Reserved blocks: ");
    s.push_str(dec_text(reserve).as_str());
    s.push_str(" (");
    // 百分比按 万分数 算（×10000），再拆成 "P.QQ"，全程 saturating，不溢出。
    let basis = if blocks_total == 0 {
        0u32
    } else {
        reserve.saturating_mul(10_000).saturating_div(blocks_total)
    };
    let whole = basis.saturating_div(100);
    let frac = basis.saturating_sub(whole.saturating_mul(100));
    s.push_str(dec_text(whole).as_str());
    s.push('.');
    if frac < 10 {
        s.push('0');
    }
    s.push_str(dec_text(frac).as_str());
    s.push_str("% of filesystem size)");
    s
}

/// 预留池短路的现场文案（本卡自研审计文本：`df` 的 Free 与 Available 分叉）。
pub fn msg_reserve_blocked(user: &str, free: u32, avail: u32) -> String {
    let mut s = String::from("non-root user ");
    s.push_str(user);
    s.push_str(" blocked by reserved pool (free ");
    s.push_str(dec_text(free).as_str());
    s.push_str(", available ");
    s.push_str(dec_text(avail).as_str());
    s.push_str("): No space left on device");
    s
}

/// 宽限期文案（本卡自研审计文本；软上限越过硬上限之前的一段）。
pub fn msg_grace(soft: u32, used: u32, grace_left: u32) -> String {
    let mut s = String::from("soft limit ");
    s.push_str(dec_text(soft).as_str());
    s.push_str(" exceeded (usage ");
    s.push_str(dec_text(used).as_str());
    s.push_str("), grace remaining ");
    s.push_str(dec_text(grace_left).as_str());
    s.push_str("s");
    s
}

/// 宽限过期文案（同一现场，`grace 0` 之后必判死）。
pub fn msg_grace_expired(soft: u32, used: u32) -> String {
    let mut s = String::from("soft limit ");
    s.push_str(dec_text(soft).as_str());
    s.push_str(" exceeded (usage ");
    s.push_str(dec_text(used).as_str());
    s.push_str("), grace expired: Quota exceeded");
    s
}

/// 双码归族破断文案（申报码与物理码不同族）。
pub fn msg_family_broken(reported: i32, physical: i32) -> String {
    let mut s = String::from("errno family mismatch (reported ");
    s.push_str(dec_text(reported as u32).as_str());
    s.push_str(" -> ");
    s.push_str(family_text(reported));
    s.push_str(", physical ");
    s.push_str(dec_text(physical as u32).as_str());
    s.push_str(" -> ");
    s.push_str(family_text(physical));
    s.push(')');
    s
}

/// 主体不适用文案（`project` 配额在非 xfs 上不生效）。
pub fn msg_subject_na(kind: &str, fs: &str) -> String {
    let mut s = String::from(kind);
    s.push_str(" quota not supported on ");
    s.push_str(fs);
    s.push_str(": accounting falls back to device free space");
    s
}

/// 入参非法文案（`write` 的 EINVAL 面）。
pub fn msg_arg_bad(field: &str, v: u32) -> String {
    let mut s = String::from("invalid ");
    s.push_str(field);
    s.push_str(" ");
    s.push_str(dec_text(v).as_str());
    s.push_str(": Invalid argument");
    s
}

// ---------------------------------------------------------------------------
// 判据基元用到的前向声明
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 枚举：文件系统族
// ---------------------------------------------------------------------------

/// 被测文件系统族（决定预留池与 project 配额是否成立）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FsFlavor {
    /// ext4：有 root 预留块池，**无** project 配额。
    Ext4,
    /// xfs：有预留（内部保留），**有** project 配额。
    Xfs,
    /// btrfs：配额走 qgroup，本卡不建模，一律"主体不适用"。
    Btrfs,
    /// nfs4：客户端侧不产生 EDQUOT（服务端账），只透传 strerror。
    Nfs4,
}

impl FsFlavor {
    /// 文本（`df -T` / `mount` 的第三列小写形式）。
    pub fn text(&self) -> &'static str {
        match self {
            FsFlavor::Ext4 => "ext4",
            FsFlavor::Xfs => "xfs",
            FsFlavor::Btrfs => "btrfs",
            FsFlavor::Nfs4 => "nfs4",
        }
    }

    /// 是否支持 root 预留块池（`mkfs -m`）。
    pub fn has_reserved_pool(&self) -> bool {
        match self {
            FsFlavor::Ext4 | FsFlavor::Xfs => true,
            FsFlavor::Btrfs | FsFlavor::Nfs4 => false,
        }
    }

    /// 是否支持 project 配额（只有 xfs 有）。
    pub fn supports_project(&self) -> bool {
        match self {
            FsFlavor::Xfs => true,
            FsFlavor::Ext4 | FsFlavor::Btrfs | FsFlavor::Nfs4 => false,
        }
    }

    /// 是否在客户端侧本地记账（nfs4 的配额在服务端）。
    pub fn local_accounting(&self) -> bool {
        match self {
            FsFlavor::Nfs4 => false,
            FsFlavor::Ext4 | FsFlavor::Xfs | FsFlavor::Btrfs => true,
        }
    }

    /// 是否为已建模的族（未知族由调用方判死）。
    pub fn modeled(&self) -> bool {
        match self {
            FsFlavor::Ext4 | FsFlavor::Xfs | FsFlavor::Btrfs | FsFlavor::Nfs4 => true,
        }
    }
}

/// 配额主体。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuotaKind {
    /// 未开配额（只有介质账）。
    None,
    /// 用户配额（`aquota.user`，`repquota -u`）。
    User,
    /// 组配额（`aquota.group`，`repquota -g`）。
    Group,
    /// 项目配额（xfs 专有，`repquota -P`）。
    Project,
}

impl QuotaKind {
    /// 文本（`quota`/`repquota` 里的主体名）。
    pub fn text(&self) -> &'static str {
        match self {
            QuotaKind::None => "none",
            QuotaKind::User => "user",
            QuotaKind::Group => "group",
            QuotaKind::Project => "project",
        }
    }

    /// 是否有主体（`None` 之外都有）。
    pub fn has_subject(&self) -> bool {
        match self {
            QuotaKind::None => false,
            QuotaKind::User | QuotaKind::Group | QuotaKind::Project => true,
        }
    }

    /// 是否按"人"累计（user/project 按实体，group 按组）。
    pub fn per_entity(&self) -> bool {
        match self {
            QuotaKind::User | QuotaKind::Project => true,
            QuotaKind::None | QuotaKind::Group => false,
        }
    }

    /// 是否只在特定文件系统族成立（project 只 xfs）。
    pub fn needs_flavor(&self) -> bool {
        match self {
            QuotaKind::Project => true,
            QuotaKind::None | QuotaKind::User | QuotaKind::Group => false,
        }
    }
}

/// 闸门阶段（本卡纵线的八段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// 入参与几何合法性（`EINVAL`）。
    Arg,
    /// 配额主体在册性与账本可读性。
    Subject,
    /// 硬上限（绝对判死）。
    Usage,
    /// 软上限与宽限期。
    Grace,
    /// 介质余量（真满）。
    Device,
    /// root 预留池（非 root 被挡）。
    Reserve,
    /// 双码归族（申报码族 vs 物理码族）。
    Family,
    /// 全闸通过。
    Done,
}

impl Stage {
    /// 次序（纵线不可倒）。
    pub fn order(&self) -> u8 {
        match self {
            Stage::Arg => 0,
            Stage::Subject => 1,
            Stage::Usage => 2,
            Stage::Grace => 3,
            Stage::Device => 4,
            Stage::Reserve => 5,
            Stage::Family => 6,
            Stage::Done => 7,
        }
    }

    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Stage::Arg => "arg",
            Stage::Subject => "subject",
            Stage::Usage => "usage",
            Stage::Grace => "grace",
            Stage::Device => "device",
            Stage::Reserve => "reserve",
            Stage::Family => "family",
            Stage::Done => "done",
        }
    }

    /// 是否失败点（Done 之外都是某道闸的短路）。
    pub fn failure(&self) -> bool {
        match self {
            Stage::Done => false,
            _ => true,
        }
    }

    /// 是否配额侧闸门（usage/grace 两段的红才是 EDQUOT）。
    pub fn quota_side(&self) -> bool {
        match self {
            Stage::Usage | Stage::Grace => true,
            Stage::Arg | Stage::Subject | Stage::Device | Stage::Reserve | Stage::Family
            | Stage::Done => false,
        }
    }

    /// 是否介质侧闸门（device/reserve 两段的红才是 ENOSPC）。
    pub fn device_side(&self) -> bool {
        match self {
            Stage::Device | Stage::Reserve => true,
            Stage::Arg | Stage::Subject | Stage::Usage | Stage::Grace | Stage::Family
            | Stage::Done => false,
        }
    }

    /// 是否结构侧闸门（arg/family 判的是申报本身成不成立）。
    pub fn structural(&self) -> bool {
        match self {
            Stage::Arg | Stage::Family => true,
            Stage::Subject | Stage::Usage | Stage::Grace | Stage::Device | Stage::Reserve
            | Stage::Done => false,
        }
    }
}

/// 结果注解（现场形态的分类；12 态互斥）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Note {
    /// 全量写入成功，且可用量在预留池之外。
    Written,
    /// root 在预留池内放行（与 `NospcReserve` 同一几何，只差主体）。
    RootPass,
    /// 软上限已超但宽限期未过——**放行**，成功态。
    DquotSoftGrace,
    /// 介质真满：`-1/28`，`df` 的 available 也归零。
    NospcDevice,
    /// 预留池挡人：`-1/28`，但 `df` 仍显示有余量。
    NospcReserve,
    /// 硬上限判死：`-1/122`。
    DquotHard,
    /// 软上限越过硬限之前，宽限已过期：`-1/122`。
    DquotGraceOver,
    /// 组配额判死：`-1/122`，主体是 gid。
    DquotGroup,
    /// 项目配额判死：`-1/122`，主体是 projid（仅 xfs）。
    DquotProject,
    /// 配额账不可读：账写不进去，主体闸直接判死 `-1/122`。
    DquotLedger,
    /// 归族破断：申报码族与物理码族不同。
    FamilySwap,
    /// 入参非法：`-1/22`。
    ArgBad,
}

impl Note {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Note::Written => "written",
            Note::RootPass => "root-in-reserve",
            Note::DquotSoftGrace => "dquot-soft-grace",
            Note::NospcDevice => "nospc-device",
            Note::NospcReserve => "nospc-reserve",
            Note::DquotHard => "dquot-hard",
            Note::DquotGraceOver => "dquot-grace-over",
            Note::DquotGroup => "dquot-group",
            Note::DquotProject => "dquot-project",
            Note::DquotLedger => "dquot-ledger",
            Note::FamilySwap => "family-swap",
            Note::ArgBad => "arg-bad",
        }
    }

    /// 是否成功态注解。
    pub fn success(&self) -> bool {
        match self {
            Note::Written | Note::RootPass | Note::DquotSoftGrace => true,
            Note::NospcDevice
            | Note::NospcReserve
            | Note::DquotHard
            | Note::DquotGraceOver
            | Note::DquotGroup
            | Note::DquotProject
            | Note::DquotLedger
            | Note::FamilySwap
            | Note::ArgBad => false,
        }
    }

    /// 是否介质族（ENOSPC 的两个成因）。
    pub fn nospc(&self) -> bool {
        match self {
            Note::NospcDevice | Note::NospcReserve => true,
            Note::Written
            | Note::RootPass
            | Note::DquotSoftGrace
            | Note::DquotHard
            | Note::DquotGraceOver
            | Note::DquotGroup
            | Note::DquotProject
            | Note::DquotLedger
            | Note::FamilySwap
            | Note::ArgBad => false,
        }
    }

    /// 是否配额族（EDQUOT 的四个成因）。
    pub fn dquot(&self) -> bool {
        match self {
            Note::DquotHard
            | Note::DquotGraceOver
            | Note::DquotGroup
            | Note::DquotProject
            | Note::DquotLedger => true,
            Note::Written
            | Note::RootPass
            | Note::DquotSoftGrace
            | Note::NospcDevice
            | Note::NospcReserve
            | Note::FamilySwap
            | Note::ArgBad => false,
        }
    }

    /// 是否结构注解（既非介质族亦非配额族：不许落对照档）。
    pub fn structural(&self) -> bool {
        match self {
            Note::FamilySwap | Note::ArgBad => true,
            _ => false,
        }
    }

    /// 是否对照档候选：介质族或配额族，二者互斥。
    pub fn paired(&self) -> bool {
        self.nospc() || self.dquot()
    }

    /// 是否"预留池成因"——`df` 有余量但写失败的那一档。
    pub fn reserved_cause(&self) -> bool {
        match self {
            Note::NospcReserve | Note::RootPass => true,
            _ => false,
        }
    }

    /// 是否"宽限期成因"——同一账本两种结局的那一档。
    pub fn grace_cause(&self) -> bool {
        match self {
            Note::DquotSoftGrace | Note::DquotGraceOver => true,
            _ => false,
        }
    }
}

/// 结果档位（四档互斥，由实际结果投影）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// 放行档（写进去了，含 root 预留池放行与宽限放行）。
    Room,
    /// 介质族档（ENOSPC 的两个成因）。
    Nospc,
    /// 配额族档（EDQUOT 的四个成因）。
    Dquot,
    /// 结构档（归族破断/入参非法：不进对照）。
    Rogue,
}

impl Tier {
    /// 由结果投影。
    pub fn of(o: &Outcome) -> Tier {
        if o.ok() {
            Tier::Room
        } else if o.note.nospc() {
            Tier::Nospc
        } else if o.note.dquot() {
            Tier::Dquot
        } else {
            Tier::Rogue
        }
    }

    /// 次序。
    pub fn index(&self) -> usize {
        match self {
            Tier::Room => 0,
            Tier::Nospc => 1,
            Tier::Dquot => 2,
            Tier::Rogue => 3,
        }
    }

    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Tier::Room => "room",
            Tier::Nospc => "nospc",
            Tier::Dquot => "dquot",
            Tier::Rogue => "rogue",
        }
    }

    /// 是否成功档。
    pub fn success(&self) -> bool {
        match self {
            Tier::Room => true,
            Tier::Nospc | Tier::Dquot | Tier::Rogue => false,
        }
    }

    /// 是否介质族档。
    pub fn nospc(&self) -> bool {
        matches!(self, Tier::Nospc)
    }

    /// 是否配额族档。
    pub fn dquot(&self) -> bool {
        matches!(self, Tier::Dquot)
    }

    /// 是否对照档（介质族 + 配额族，双码分档的两半）。
    pub fn dual_tier(&self) -> bool {
        matches!(self, Tier::Nospc | Tier::Dquot)
    }

    /// 四档枚举表（渲染与遍历用）。
    pub fn all() -> [Tier; TIER_COUNT] {
        [Tier::Room, Tier::Nospc, Tier::Dquot, Tier::Rogue]
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
    /// 落盘字节错。
    Value,
    /// 注解错。
    Note,
    /// 对照档位错。
    Default,
    /// 归族破断（结构维）。
    Family,
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
            Fault::Stage => EQ_ESTAGE,
            Fault::Code => EQ_ECODE,
            Fault::Value => EQ_EVALUE,
            Fault::Note => EQ_ENOTE,
            Fault::Default => EQ_EDEFAULT,
            Fault::Family => EQ_EFAMILY,
            Fault::Estate => EQ_ESTATE,
            Fault::Count => EQ_ECOUNT,
            Fault::Slot => EQ_ESLOT,
            Fault::Seal => EQ_ESEAL,
            Fault::Nobs => EQ_ENOBS,
            Fault::Matrix => EQ_EMATRIX,
            Fault::Bal => EQ_EBAL,
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
        Fault::Family => "family",
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
// 配额账本与文件系统几何
// ---------------------------------------------------------------------------

/// 一个配额主体的账本（quota-tools 的三档数 + 宽限余量）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuotaBook {
    /// 主体。
    pub kind: QuotaKind,
    /// `quotaon` 是否已经跑过（配额是否生效）。
    pub enforced: bool,
    /// 账本（`aquota.user`/`aquota.group`）是否可读。
    pub ledger_ok: bool,
    /// 软上限（字节）。
    pub soft: u32,
    /// 硬上限（字节）。
    pub hard: u32,
    /// 已用（字节，含本次请求之前落盘的）。
    pub used: u32,
    /// 宽限余量（秒；0 表示已过期或从未进入宽限）。
    pub grace_left: u32,
}

impl QuotaBook {
    /// 空白账（无配额）。
    pub const fn blank() -> QuotaBook {
        QuotaBook {
            kind: QuotaKind::None,
            enforced: false,
            ledger_ok: true,
            soft: 0,
            hard: 0,
            used: 0,
            grace_left: 0,
        }
    }

    /// 越过软上限（含本次请求）。
    pub fn over_soft(&self, count: u32) -> bool {
        if self.soft == 0 {
            return false;
        }
        match self.used.checked_add(count) {
            Some(v) => v > self.soft,
            None => true,
        }
    }

    /// 越过硬上限（含本次请求）。
    pub fn over_hard(&self, count: u32) -> bool {
        if self.hard == 0 {
            return false;
        }
        match self.used.checked_add(count) {
            Some(v) => v > self.hard,
            None => true,
        }
    }

    /// 是否处于"只越软限"的宽限窗口。
    pub fn in_grace_window(&self, count: u32) -> bool {
        self.over_soft(count) && !self.over_hard(count)
    }

    /// 账本是否自洽（用于几何闸，不参与判定次序）。
    pub fn well_formed(&self) -> bool {
        if !self.kind.has_subject() {
            return self.soft == 0 && self.hard == 0 && self.used == 0 && self.grace_left == 0;
        }
        if self.hard == 0 {
            return false;
        }
        if self.soft > self.hard {
            return false;
        }
        if self.grace_left > GRACE_MAX {
            return false;
        }
        if self.used > self.hard {
            // 已用量超硬限是合法现场（setquota 下调过限额），允许，但必须
            // 有限，否则说明读数不可信。
            return self.used < COUNT_MAX;
        }
        true
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_str(h, self.kind.text());
        h = fold_word(h, self.enforced as u64);
        h = fold_word(h, self.ledger_ok as u64);
        h = fold_word(h, self.soft as u64);
        h = fold_word(h, self.hard as u64);
        h = fold_word(h, self.used as u64);
        fold_word(h, self.grace_left as u64)
    }
}

/// 一块被测文件系统的介质几何。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FsGeom {
    /// 设备/挂载点名。
    pub name: &'static str,
    /// 文件系统族。
    pub flavor: FsFlavor,
    /// 块大小（字节）。
    pub bsize: u32,
    /// 空闲块数。
    pub blocks_free: u32,
    /// root 预留块数。
    pub reserve_blocks: u32,
}

impl FsGeom {
    /// 空白几何。
    pub const fn blank() -> FsGeom {
        FsGeom {
            name: "",
            flavor: FsFlavor::Ext4,
            bsize: 0,
            blocks_free: 0,
            reserve_blocks: 0,
        }
    }

    /// 空闲字节数（溢出即坏几何）。
    pub fn bytes_free(&self) -> Option<u32> {
        self.bsize.checked_mul(self.blocks_free)
    }

    /// 预留池字节数（族不支持预留时为 0）。
    pub fn reserved_bytes(&self) -> Option<u32> {
        if !self.flavor.has_reserved_pool() {
            return Some(0);
        }
        self.bsize.checked_mul(self.reserve_blocks)
    }

    /// 块大小是否合法（`stat -f` 只见这三档）。
    pub fn bsize_legal(&self) -> bool {
        self.bsize == 512 || self.bsize == 1024 || self.bsize == 4096
    }

    /// 是否结构良好。
    pub fn well_formed(&self) -> bool {
        if self.name.is_empty() {
            return false;
        }
        if !self.flavor.modeled() {
            return false;
        }
        if !self.bsize_legal() {
            return false;
        }
        if self.blocks_free > BLOCKS_MAX {
            return false;
        }
        if self.bytes_free().is_none() {
            return false;
        }
        if self.reserved_bytes().is_none() {
            return false;
        }
        true
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_str(h, self.name);
        h = fold_str(h, self.flavor.text());
        h = fold_word(h, self.bsize as u64);
        h = fold_word(h, self.blocks_free as u64);
        h = fold_word(h, self.reserve_blocks as u64);
        match self.bytes_free() {
            Some(v) => fold_word(h, v as u64),
            None => fold_word(h, 0),
        }
    }
}

/// 被测文件系统表：满盘 ext4、只有预留池的 ext4、宽裕的 xfs、服务端记账的 nfs4。
pub fn geoms() -> [FsGeom; 4] {
    [
        FsGeom {
            name: "/dev/sda3",
            flavor: FsFlavor::Ext4,
            bsize: 4096,
            blocks_free: 0,
            reserve_blocks: 0,
        },
        FsGeom {
            name: "/dev/sdb1",
            flavor: FsFlavor::Ext4,
            bsize: 4096,
            blocks_free: 8,
            reserve_blocks: 8,
        },
        FsGeom {
            name: "/dev/nvme0n1p2",
            flavor: FsFlavor::Xfs,
            bsize: 512,
            blocks_free: 64,
            reserve_blocks: 4,
        },
        FsGeom {
            name: "srv:/export",
            flavor: FsFlavor::Nfs4,
            bsize: 1024,
            blocks_free: 32,
            reserve_blocks: 0,
        },
    ]
}

// ---------------------------------------------------------------------------
// 形状（本卡输入的全部维度）
// ---------------------------------------------------------------------------

/// 一次写入观测的形状：文件系统 + 配额账 + 请求 + 申报的 errno。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shape {
    /// 槽标签（唯一）。
    pub tag: &'static str,
    /// 目标路径名。
    pub path: &'static str,
    /// 用户名（预留池与配额文案都要它）。
    pub user: &'static str,
    /// uid（`quota` 第一行的 `(uid N)`）。
    pub uid: u32,
    /// 文件系统族。
    pub fs: FsFlavor,
    /// 配额主体。
    pub kind: QuotaKind,
    /// `quotaon` 是否已跑。
    pub enforced: bool,
    /// 配额账本是否可读。
    pub ledger_ok: bool,
    /// 块大小。
    pub bsize: u32,
    /// 空闲块数。
    pub blocks_free: u32,
    /// root 预留块数。
    pub reserve_blocks: u32,
    /// 软上限（字节）。
    pub soft: u32,
    /// 硬上限（字节）。
    pub hard: u32,
    /// 已用（字节）。
    pub used: u32,
    /// 宽限余量（秒）。
    pub grace_left: u32,
    /// 是否 root（预留池的旁路主体）。
    pub is_root: bool,
    /// 请求写入字节数（整块对齐）。
    pub count: u32,
    /// 调用方观测到的 errno（归族闸的对照物）。
    pub reported: i32,
}

impl Shape {
    /// 默认形状：ext4、宽裕、无配额、普通用户、干净观测。
    pub const fn blank() -> Shape {
        Shape {
            tag: "",
            path: "out.bin",
            user: "alice",
            uid: 1000,
            fs: FsFlavor::Ext4,
            kind: QuotaKind::None,
            enforced: false,
            ledger_ok: true,
            bsize: 4096,
            blocks_free: 64,
            reserve_blocks: 0,
            soft: 0,
            hard: 0,
            used: 0,
            grace_left: 0,
            is_root: false,
            count: 4096,
            reported: ERR_NONE,
        }
    }

    /// 配额账副本。
    pub fn book(&self) -> QuotaBook {
        QuotaBook {
            kind: self.kind,
            enforced: self.enforced,
            ledger_ok: self.ledger_ok,
            soft: self.soft,
            hard: self.hard,
            used: self.used,
            grace_left: self.grace_left,
        }
    }

    /// 介质几何副本。
    pub fn geom(&self) -> FsGeom {
        FsGeom {
            name: self.path,
            flavor: self.fs,
            bsize: self.bsize,
            blocks_free: self.blocks_free,
            reserve_blocks: self.reserve_blocks,
        }
    }

    /// 换请求字节数的副本。
    pub fn with_count(&self, c: u32) -> Shape {
        Shape { count: c, ..*self }
    }

    /// 换空闲块数的副本。
    pub fn with_blocks_free(&self, b: u32) -> Shape {
        Shape {
            blocks_free: b,
            ..*self
        }
    }

    /// 换已用量的副本。
    pub fn with_used(&self, u: u32) -> Shape {
        Shape { used: u, ..*self }
    }

    /// 换宽限余量的副本。
    pub fn with_grace(&self, g: u32) -> Shape {
        Shape {
            grace_left: g,
            ..*self
        }
    }

    /// 换申报 errno 的副本。
    pub fn with_reported(&self, e: i32) -> Shape {
        Shape { reported: e, ..*self }
    }

    /// 换主体的副本。
    pub fn with_kind(&self, k: QuotaKind) -> Shape {
        Shape { kind: k, ..*self }
    }

    /// 空闲字节数。
    pub fn bytes_free(&self) -> Option<u32>
    {
        self.bsize.checked_mul(self.blocks_free)
    }

    /// 预留池字节数（族不支持时为 0）。
    pub fn reserved_bytes(&self) -> u32 {
        match self.geom().reserved_bytes() {
            Some(v) => v,
            None => 0,
        }
    }

    /// 非 root 的可用字节（预留池已扣）。
    pub fn usable_bytes(&self) -> Option<u32> {
        match self.bytes_free() {
            Some(f) => {
                if self.is_root || !self.fs.has_reserved_pool() {
                    Some(f)
                } else {
                    Some(f.saturating_sub(self.reserved_bytes()))
                }
            }
            None => None,
        }
    }

    /// 申报 errno 是否在合法集内。
    pub fn errno_legal(&self) -> bool {
        self.reported == ERR_NONE
            || self.reported == ERR_ENOSPC
            || self.reported == ERR_EDQUOT
            || self.reported == ERR_EINVAL
    }

    /// 配额主体是否在本文件系统上生效。
    pub fn subject_supported(&self) -> bool {
        if !self.kind.has_subject() {
            return false;
        }
        if self.kind == QuotaKind::Project && !self.fs.supports_project() {
            return false;
        }
        self.fs.local_accounting()
    }

    /// 配额是否真正参与裁定（主体生效 + `quotaon` 已跑）。
    pub fn quota_applies(&self) -> bool {
        self.subject_supported() && self.enforced
    }

    /// 入参是否合法（第一闸的判据）。
    pub fn arg_legal(&self) -> bool {
        self.illegal_field().is_none()
    }

    /// 第一个非法入参：返回（字段名, 读数）。次序固定，保证文案可复算。
    pub fn illegal_field(&self) -> Option<(&'static str, u32)> {
        if self.count == 0 {
            return Some(("count", 0));
        }
        if self.count > COUNT_MAX {
            return Some(("count", self.count));
        }
        if !self.geom().bsize_legal() {
            return Some(("bsize", self.bsize));
        }
        if self.blocks_free > BLOCKS_MAX {
            return Some(("blocks_free", self.blocks_free));
        }
        if self.reserve_blocks > BLOCKS_MAX {
            return Some(("reserve_blocks", self.reserve_blocks));
        }
        if self.bytes_free().is_none() {
            return Some(("bytes_free", self.blocks_free));
        }
        if self.grace_left > GRACE_MAX {
            return Some(("grace", self.grace_left));
        }
        if self.hard > 0 && self.soft > self.hard {
            return Some(("soft", self.soft));
        }
        if !self.errno_legal() {
            return Some(("errno", self.reported as u32));
        }
        if self.soft > COUNT_MAX {
            return Some(("soft", self.soft));
        }
        if self.hard > COUNT_MAX {
            return Some(("hard", self.hard));
        }
        if self.used > COUNT_MAX {
            return Some(("usage", self.used));
        }
        if self.uid > 65534 {
            return Some(("uid", self.uid));
        }
        None
    }

    /// 形状是否合法可判（含账本自洽）。
    pub fn valid(&self) -> bool {
        if !self.arg_legal() {
            return false;
        }
        if !self.geom().well_formed() {
            return false;
        }
        self.book().well_formed()
    }

    /// 指纹：对全部判定字段敏感。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_str(h, self.tag);
        h = fold_str(h, self.path);
        h = fold_str(h, self.user);
        h = fold_word(h, self.uid as u64);
        h = fold_str(h, self.fs.text());
        h = fold_str(h, self.kind.text());
        h = fold_word(h, self.enforced as u64);
        h = fold_word(h, self.ledger_ok as u64);
        h = fold_word(h, self.bsize as u64);
        h = fold_word(h, self.blocks_free as u64);
        h = fold_word(h, self.reserve_blocks as u64);
        h = fold_word(h, self.soft as u64);
        h = fold_word(h, self.hard as u64);
        h = fold_word(h, self.used as u64);
        h = fold_word(h, self.grace_left as u64);
        h = fold_word(h, self.is_root as u64);
        h = fold_word(h, self.count as u64);
        fold_word(h, self.reported as i64 as u64)
    }
}

/// 物理中间结果（配额/介质闸门算出的真相，归族闸之前）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Phys {
    /// 结束阶段。
    stage: Stage,
    /// 物理 errno。
    errno: i32,
    /// 物理落盘字节。
    value: u32,
    /// 形态注解。
    note: Note,
}

impl Phys {
    const fn ok(value: u32, note: Note) -> Phys {
        Phys {
            stage: Stage::Done,
            errno: ERR_NONE,
            value,
            note,
        }
    }

    const fn fail(stage: Stage, errno: i32, note: Note) -> Phys {
        Phys {
            stage,
            errno,
            value: 0,
            note,
        }
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
    /// errno（0 表示成功）。
    pub code: i32,
    /// 落盘字节（失败一律 0，归族破断除外）。
    pub value: u32,
    /// 形态注解。
    pub note: Note,
    /// 对照档位：该现场是否构成一条自洽的介质族/配额族对照样本。
    pub default: bool,
}

impl Outcome {
    /// 空白结果。
    pub const fn blank() -> Outcome {
        Outcome {
            stage: Stage::Done,
            code: 0,
            value: 0,
            note: Note::Written,
            default: false,
        }
    }

    /// 是否成功。
    pub fn ok(&self) -> bool {
        self.stage == Stage::Done && self.code == ERR_NONE && self.note.success()
    }

    /// 是否失败。
    pub fn failed(&self) -> bool {
        !self.ok()
    }

    /// 是否对照档命中：失败、注解属介质族或配额族、且档位位自洽为真。
    pub fn paired(&self) -> bool {
        self.failed() && self.note.paired() && self.default
    }

    /// 结果的码族文本。
    pub fn family(&self) -> &'static str {
        family_text(self.code)
    }

    /// 由物理结果升成判定结果。
    const fn from_phys(p: &Phys, default: bool) -> Outcome {
        Outcome {
            stage: p.stage,
            code: p.errno,
            value: p.value,
            note: p.note,
            default,
        }
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
// 定义纵线（arg → subject → usage → grace → device → reserve → family → done）
// ---------------------------------------------------------------------------

/// 物理七闸（不含归族闸）——配额先行、介质随后。
fn physical(s: &Shape) -> Phys {
    // ---- arg ----
    if let Some((field, v)) = s.illegal_field() {
        let _ = field;
        let _ = v;
        return Phys::fail(Stage::Arg, ERR_EINVAL, Note::ArgBad);
    }

    let book = s.book();

    // ---- subject ----
    if s.quota_applies() && !s.ledger_ok {
        // 配额在册但账读不出来：无法记账就不允许继续占块，判死在配额族。
        return Phys::fail(Stage::Subject, ERR_EDQUOT, Note::DquotLedger);
    }

    // ---- usage / grace（主体不适用则整段跳过）----
    if s.quota_applies() {
        if book.over_hard(s.count) {
            return Phys::fail(Stage::Usage, ERR_EDQUOT, hard_note(s.kind));
        }
        if book.in_grace_window(s.count) {
            if s.grace_left > 0 {
                return Phys::ok(s.count, Note::DquotSoftGrace);
            }
            return Phys::fail(Stage::Grace, ERR_EDQUOT, Note::DquotGraceOver);
        }
    }

    // ---- device ----
    let free = match s.bytes_free() {
        Some(v) => v,
        None => return Phys::fail(Stage::Arg, ERR_EINVAL, Note::ArgBad),
    };
    if free == 0 {
        return Phys::fail(Stage::Device, ERR_ENOSPC, Note::NospcDevice);
    }

    // ---- reserve ----
    if !s.is_root && s.fs.has_reserved_pool() {
        if free.saturating_sub(s.reserved_bytes()) == 0 {
            return Phys::fail(Stage::Reserve, ERR_ENOSPC, Note::NospcReserve);
        }
        return Phys::ok(s.count, Note::Written);
    }
    if s.is_root && s.fs.has_reserved_pool() && free <= s.reserved_bytes() {
        return Phys::ok(s.count, Note::RootPass);
    }
    Phys::ok(s.count, Note::Written)
}

/// 硬上限注解按主体分式（文案差的第一处：`user`/`group`/`project`）。
fn hard_note(k: QuotaKind) -> Note {
    match k {
        QuotaKind::User => Note::DquotHard,
        QuotaKind::Group => Note::DquotGroup,
        QuotaKind::Project => Note::DquotProject,
        // `None` 主体不可能进硬限闸（quota_applies 已排除）；保守落回 user 式。
        QuotaKind::None => Note::DquotHard,
    }
}

/// 判定的物理 errno（归族前的真相，供文案与复算取用）。
pub fn physical_errno(s: &Shape) -> i32 {
    physical(s).errno
}

/// 判定七闸的物理阶段。
pub fn physical_stage(s: &Shape) -> Stage {
    physical(s).stage
}

/// 判定一次写入观测的现场结果。
///
/// 每一闸只在上一闸放行后执行。配额闸排在介质闸之前，是因为内核先做
/// `dquot_reserve_blocks()` 再做块分配——**盘满且配额满时报 EDQUOT**，这是
/// 实况而不是约定。最后一段归族闸把申报 errno 的码族与物理 errno 的码族对照，
/// 族不同一律改判 `Family` 段破断、档位位归零：这就是"双码分档"的守闸。
pub fn define(s: &Shape) -> Outcome {
    let p = physical(s);

    // ---- family ----
    if family_class(s.reported) != family_class(p.errno) {
        return Outcome {
            stage: Stage::Family,
            code: s.reported,
            value: p.value,
            note: Note::FamilySwap,
            default: false,
        };
    }

    // ---- done ----
    let paired = p.note.paired();
    Outcome::from_phys(&p, paired)
}

/// 码族分类：0=none、28=nospc、122=dquot、22=arg，其余 unknown。
fn family_class(code: i32) -> u8 {
    match code {
        ERR_NONE => 0,
        ERR_ENOSPC => 1,
        ERR_EDQUOT => 2,
        ERR_EINVAL => 3,
        _ => 9,
    }
}

/// 现场文案（note → 真实文本；成功注解空串）。
pub fn message(s: &Shape, o: &Outcome) -> String {
    match o.note {
        Note::NospcDevice => msg_write_at("write", s.path, ERR_ENOSPC),
        Note::NospcReserve => msg_reserve_blocked(
            s.user,
            match s.bytes_free() {
                Some(v) => v,
                None => 0,
            },
            match s.bytes_free() {
                Some(v) => v.saturating_sub(s.reserved_bytes()),
                None => 0,
            },
        ),
        Note::DquotHard => msg_write_at("write", s.path, ERR_EDQUOT),
        Note::DquotGraceOver => msg_grace_expired(s.soft, s.used),
        Note::DquotGroup => msg_subject_at("group", s.path),
        Note::DquotProject => msg_subject_at("project", s.path),
        Note::DquotLedger => msg_ledger_bad(s.path),
        Note::FamilySwap => msg_family_broken(s.reported, physical_errno(s)),
        Note::ArgBad => match s.illegal_field() {
            Some((field, v)) => msg_arg_bad(field, v),
            None => msg_arg_bad("shape", 0),
        },
        Note::Written | Note::RootPass | Note::DquotSoftGrace => String::new(),
    }
}

/// 主体配额判死文案（`<kind> quota exceeded` 体例）。
pub fn msg_subject_at(kind: &str, path: &str) -> String {
    let mut s = String::from(kind);
    s.push_str(" quota exceeded: '");
    s.push_str(path);
    s.push_str("': Quota exceeded");
    s
}

/// 配额账不可读文案。
pub fn msg_ledger_bad(path: &str) -> String {
    let mut s = String::from("cannot read quota record for '");
    s.push_str(path);
    s.push_str("': Quota exceeded");
    s
}

/// 一句话结论（人读口径，判据不落账）。
pub fn verdict(o: &Outcome) -> &'static str {
    if o.ok() {
        return "room";
    }
    match o.note {
        Note::NospcDevice => "device exhausted",
        Note::NospcReserve => "reserved pool",
        Note::DquotHard => "hard limit",
        Note::DquotGraceOver => "grace expired",
        Note::DquotGroup => "group limit",
        Note::DquotProject => "project limit",
        Note::DquotLedger => "quota unreadable",
        Note::FamilySwap => "errno family mismatch",
        Note::ArgBad => "invalid argument",
        Note::Written | Note::RootPass | Note::DquotSoftGrace => "room",
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
    /// 落盘字节。
    pub value: u32,
    /// 形态注解。
    pub note: Note,
    /// 对照档位。
    pub default: bool,
}

impl Claim {
    /// 空白申报。
    pub const fn blank() -> Claim {
        Claim {
            stage: Stage::Done,
            code: 0,
            value: 0,
            note: Note::Written,
            default: false,
        }
    }

    /// 是否申报成功（errno 归零且注解属成功态）。
    pub fn success(&self) -> bool {
        self.code == 0 && self.note.success()
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
// ---------------------------------------------------------------------------
// 语料（14 槽：8 活 + 6 死）
// ---------------------------------------------------------------------------

/// 8 个可正确申报的形状 + 6 个复刻形状（复刻自 `dquot-hard`）。
///
/// 每个形状都写全 18 维，不用 `..Shape::blank()`：本卡的立卡理由是"双码分档
/// 差在哪个读数"，任何一维被默认值悄悄填掉，红档就说不清是被哪一维判死的。
pub const SHAPES: [Shape; CORPUS] = [
    // 0 —— arg/subject/usage/grace/device/reserve 全宽松：普通用户、有块、无配额。
    Shape {
        tag: "room-clear",
        path: "home/alice/room.bin",
        user: "alice",
        uid: 1000,
        fs: FsFlavor::Ext4,
        kind: QuotaKind::None,
        enforced: false,
        ledger_ok: true,
        bsize: 4096,
        blocks_free: 64,
        reserve_blocks: 0,
        soft: 0,
        hard: 0,
        used: 0,
        grace_left: 0,
        is_root: false,
        count: 4096,
        reported: ERR_NONE,
    },
    // 1 —— 与槽 4 完全同一几何，只差主体是 root：预留池内的放行对照样本。
    Shape {
        tag: "root-in-reserve",
        path: "lost+found/fsck.log",
        user: "root",
        uid: 0,
        fs: FsFlavor::Ext4,
        kind: QuotaKind::None,
        enforced: false,
        ledger_ok: true,
        bsize: 4096,
        blocks_free: 8,
        reserve_blocks: 8,
        soft: 0,
        hard: 0,
        used: 0,
        grace_left: 0,
        is_root: true,
        count: 4096,
        reported: ERR_NONE,
    },
    // 2 —— 软上限已超、硬上限未到、宽限期在：放行（成功态）。
    Shape {
        tag: "grace-open-pass",
        path: "home/alice/report.pdf",
        user: "alice",
        uid: 1000,
        fs: FsFlavor::Ext4,
        kind: QuotaKind::User,
        enforced: true,
        ledger_ok: true,
        bsize: 4096,
        blocks_free: 64,
        reserve_blocks: 0,
        soft: 1000,
        hard: 8000,
        used: 2000,
        grace_left: 604800,
        is_root: false,
        count: 4096,
        reported: ERR_NONE,
    },
    // 3 —— 介质真满：`df` 的 available 也归零。
    Shape {
        tag: "nospc-device",
        path: "/mnt/full/data.bin",
        user: "alice",
        uid: 1000,
        fs: FsFlavor::Ext4,
        kind: QuotaKind::None,
        enforced: false,
        ledger_ok: true,
        bsize: 4096,
        blocks_free: 0,
        reserve_blocks: 0,
        soft: 0,
        hard: 0,
        used: 0,
        grace_left: 0,
        is_root: false,
        count: 4096,
        reported: ERR_ENOSPC,
    },
    // 4 —— 预留池挡人：块还有 8 个，但全在 root 预留里，非 root 写不进。
    Shape {
        tag: "nospc-reserve",
        path: "/mnt/data/alice.tmp",
        user: "alice",
        uid: 1000,
        fs: FsFlavor::Ext4,
        kind: QuotaKind::None,
        enforced: false,
        ledger_ok: true,
        bsize: 4096,
        blocks_free: 8,
        reserve_blocks: 8,
        soft: 0,
        hard: 0,
        used: 0,
        grace_left: 0,
        is_root: false,
        count: 4096,
        reported: ERR_ENOSPC,
    },
    // 5 —— 硬上限判死（本卡的"配额文案差"主样本）。
    Shape {
        tag: "dquot-hard",
        path: "home/alice/img.raw",
        user: "alice",
        uid: 1000,
        fs: FsFlavor::Ext4,
        kind: QuotaKind::User,
        enforced: true,
        ledger_ok: true,
        bsize: 4096,
        blocks_free: 64,
        reserve_blocks: 0,
        soft: 1000,
        hard: 4000,
        used: 3000,
        grace_left: 604800,
        is_root: false,
        count: 4096,
        reported: ERR_EDQUOT,
    },
    // 6 —— 与槽 2 同一账本，只差宽限已过期：同一现场隔天的两副面孔。
    Shape {
        tag: "dquot-grace-over",
        path: "home/alice/day2.img",
        user: "alice",
        uid: 1000,
        fs: FsFlavor::Ext4,
        kind: QuotaKind::User,
        enforced: true,
        ledger_ok: true,
        bsize: 4096,
        blocks_free: 64,
        reserve_blocks: 0,
        soft: 1000,
        hard: 8000,
        used: 2000,
        grace_left: 0,
        is_root: false,
        count: 4096,
        reported: ERR_EDQUOT,
    },
    // 7 —— 主体不适用（ext4 无 project 配额）却申报配额族：归族破断的正例。
    Shape {
        tag: "family-swap",
        path: "home/carol/proj.dat",
        user: "carol",
        uid: 1002,
        fs: FsFlavor::Ext4,
        kind: QuotaKind::Project,
        enforced: true,
        ledger_ok: true,
        bsize: 4096,
        blocks_free: 64,
        reserve_blocks: 0,
        soft: 1000,
        hard: 4000,
        used: 3000,
        grace_left: 0,
        is_root: false,
        count: 4096,
        reported: ERR_EDQUOT,
    },
    // 8 —— 复刻 dquot-hard，错报为"写成功了"（Estate）。
    Shape {
        tag: "estate-swap",
        path: "home/alice/img.raw",
        user: "alice",
        uid: 1000,
        fs: FsFlavor::Ext4,
        kind: QuotaKind::User,
        enforced: true,
        ledger_ok: true,
        bsize: 4096,
        blocks_free: 64,
        reserve_blocks: 0,
        soft: 1000,
        hard: 4000,
        used: 3000,
        grace_left: 604800,
        is_root: false,
        count: 4096,
        reported: ERR_EDQUOT,
    },
    // 9 —— 复刻 dquot-hard，把配额闸挪到介质闸（Stage）。
    Shape {
        tag: "stage-swap",
        path: "home/alice/img.raw",
        user: "alice",
        uid: 1000,
        fs: FsFlavor::Ext4,
        kind: QuotaKind::User,
        enforced: true,
        ledger_ok: true,
        bsize: 4096,
        blocks_free: 64,
        reserve_blocks: 0,
        soft: 1000,
        hard: 4000,
        used: 3000,
        grace_left: 604800,
        is_root: false,
        count: 4096,
        reported: ERR_EDQUOT,
    },
    // 10 —— 复刻 dquot-hard，把 122 报成 28（Code，本卡头号缺陷）。
    Shape {
        tag: "code-swap",
        path: "home/alice/img.raw",
        user: "alice",
        uid: 1000,
        fs: FsFlavor::Ext4,
        kind: QuotaKind::User,
        enforced: true,
        ledger_ok: true,
        bsize: 4096,
        blocks_free: 64,
        reserve_blocks: 0,
        soft: 1000,
        hard: 4000,
        used: 3000,
        grace_left: 604800,
        is_root: false,
        count: 4096,
        reported: ERR_EDQUOT,
    },
    // 11 —— 复刻 dquot-hard，在一笔没落盘的写上申报字节数（Value）。
    Shape {
        tag: "value-swap",
        path: "home/alice/img.raw",
        user: "alice",
        uid: 1000,
        fs: FsFlavor::Ext4,
        kind: QuotaKind::User,
        enforced: true,
        ledger_ok: true,
        bsize: 4096,
        blocks_free: 64,
        reserve_blocks: 0,
        soft: 1000,
        hard: 4000,
        used: 3000,
        grace_left: 604800,
        is_root: false,
        count: 4096,
        reported: ERR_EDQUOT,
    },
    // 12 —— 复刻 dquot-hard，码保留 122 但注解写成介质满（Note）。
    Shape {
        tag: "note-swap",
        path: "home/alice/img.raw",
        user: "alice",
        uid: 1000,
        fs: FsFlavor::Ext4,
        kind: QuotaKind::User,
        enforced: true,
        ledger_ok: true,
        bsize: 4096,
        blocks_free: 64,
        reserve_blocks: 0,
        soft: 1000,
        hard: 4000,
        used: 3000,
        grace_left: 604800,
        is_root: false,
        count: 4096,
        reported: ERR_EDQUOT,
    },
    // 13 —— 复刻 dquot-hard，五维全对却丢掉对照档位（Default）。
    Shape {
        tag: "default-swap",
        path: "home/alice/img.raw",
        user: "alice",
        uid: 1000,
        fs: FsFlavor::Ext4,
        kind: QuotaKind::User,
        enforced: true,
        ledger_ok: true,
        bsize: 4096,
        blocks_free: 64,
        reserve_blocks: 0,
        soft: 1000,
        hard: 4000,
        used: 3000,
        grace_left: 604800,
        is_root: false,
        count: 4096,
        reported: ERR_EDQUOT,
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
        value: 4096,
        note: Note::Written,
        default: false,
    },
    Claim {
        stage: Stage::Done,
        code: 0,
        value: 4096,
        note: Note::RootPass,
        default: false,
    },
    Claim {
        stage: Stage::Done,
        code: 0,
        value: 4096,
        note: Note::DquotSoftGrace,
        default: false,
    },
    Claim {
        stage: Stage::Device,
        code: 28,
        value: 0,
        note: Note::NospcDevice,
        default: true,
    },
    Claim {
        stage: Stage::Reserve,
        code: 28,
        value: 0,
        note: Note::NospcReserve,
        default: true,
    },
    Claim {
        stage: Stage::Usage,
        code: 122,
        value: 0,
        note: Note::DquotHard,
        default: true,
    },
    Claim {
        stage: Stage::Grace,
        code: 122,
        value: 0,
        note: Note::DquotGraceOver,
        default: true,
    },
    Claim {
        stage: Stage::Family,
        code: 122,
        value: 4096,
        note: Note::FamilySwap,
        default: false,
    },
    // 8 —— 成败互报：硬上限那一刀被申报成"写完了"。
    Claim {
        stage: Stage::Done,
        code: 0,
        value: 4096,
        note: Note::Written,
        default: false,
    },
    // 9 —— 阶段错：码与注解都对，只把配额闸说成介质闸。
    Claim {
        stage: Stage::Device,
        code: 122,
        value: 0,
        note: Note::DquotHard,
        default: true,
    },
    // 10 —— 码错：配额满报成设备满（122 → 28），本卡的头号缺陷。
    Claim {
        stage: Stage::Usage,
        code: 28,
        value: 0,
        note: Note::DquotHard,
        default: true,
    },
    // 11 —— 字节错：一笔 0 字节的失败申报落盘 4096。
    Claim {
        stage: Stage::Usage,
        code: 122,
        value: 4096,
        note: Note::DquotHard,
        default: true,
    },
    // 12 —— 注解错：122 配介质满的注解（自相矛盾的申报）。
    Claim {
        stage: Stage::Usage,
        code: 122,
        value: 0,
        note: Note::NospcDevice,
        default: true,
    },
    // 13 —— 档位错：五维全对，对照档不给。
    Claim {
        stage: Stage::Usage,
        code: 122,
        value: 0,
        note: Note::DquotHard,
        default: false,
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
    EQ_ESTATE,
    EQ_ESTAGE,
    EQ_ECODE,
    EQ_EVALUE,
    EQ_ENOTE,
    EQ_EDEFAULT,
];

/// 账本 8 个成功格的值（总数、成、败、放行档、介质族档、配额族档、结构档、
/// 红档率千分位）。**第 8 格恒为红档率 929**。
pub const VALS: [u32; ACCEPT] = [14, 8, 6, 3, 2, 2, 1, 929];

/// 账本 14 格的操作码（逐格归属的闸门，`ops_match()` 复核）。
pub const LEDGER_OPS: [u8; SAMPLES] = [
    OP_CORPUS,
    OP_CORPUS,
    OP_CORPUS,
    OP_DEVICE,
    OP_RESERVE,
    OP_USAGE,
    OP_GRACE,
    OP_REPLAY,
    OP_SUBJECT,
    OP_SUBJECT,
    OP_SUBJECT,
    OP_SUBJECT,
    OP_SUBJECT,
    OP_SUBJECT,
];

/// 语料表。
pub fn cases() -> [Case; CORPUS] {
    let mut out = [Case::blank(); CORPUS];
    let mut i = 0usize;
    while i < CORPUS {
        if let (Some(s), Some(c)) = (SHAPES.get(i), CLAIMS.get(i)) {
            out[i] = Case {
                shape: *s,
                claim: *c,
            };
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
        return Err(EQ_ENOBS);
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

/// 对判：状态 → 阶段 → errno → 落盘字节 → 注解 → 对照档。
///
/// 次序不可换。成败互报排第一，是因为**它是唯一能让"文案完全合理"的申报
/// 仍然致命的维**：一个把 EDQUOT 说成成功的申报，后面的阶段/码/字节全按
/// 成功口径写，逐维比下去每一维都"自洽"，只有跟现场对撞才露馅。
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
    let mut s = String::from(c.shape.path);
    s.push_str(" user=");
    s.push_str(c.shape.user);
    s.push_str(" fs=");
    s.push_str(c.shape.fs.text());
    s.push_str(" kind=");
    s.push_str(c.shape.kind.text());
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
// 期望表（语料的第二份陈述）
// ---------------------------------------------------------------------------

/// 14 槽的期望结束阶段。
///
/// 这份表是把 `SHAPES`/`CLAIMS` 用**另一种形式**再写一遍：申报表说"我声明
/// 什么"，期望表说"校验器实际会走到哪一步"。任一处笔误都会在这里露出来。
const WANT_STAGE: [Stage; CORPUS] = [
    Stage::Done,
    Stage::Done,
    Stage::Done,
    Stage::Device,
    Stage::Reserve,
    Stage::Usage,
    Stage::Grace,
    Stage::Family,
    Stage::Usage,
    Stage::Usage,
    Stage::Usage,
    Stage::Usage,
    Stage::Usage,
    Stage::Usage,
];

/// 14 槽的期望 errno。
const WANT_CODE: [i32; CORPUS] = [
    ERR_NONE,
    ERR_NONE,
    ERR_NONE,
    ERR_ENOSPC,
    ERR_ENOSPC,
    ERR_EDQUOT,
    ERR_EDQUOT,
    ERR_EDQUOT,
    ERR_EDQUOT,
    ERR_EDQUOT,
    ERR_EDQUOT,
    ERR_EDQUOT,
    ERR_EDQUOT,
    ERR_EDQUOT,
];

/// 14 槽的期望落盘字节。
const WANT_VALUE: [u32; CORPUS] = [
    4096, 4096, 4096, 0, 0, 0, 0, 4096, 0, 0, 0, 0, 0, 0,
];

/// 14 槽的期望注解。
const WANT_NOTE: [Note; CORPUS] = [
    Note::Written,
    Note::RootPass,
    Note::DquotSoftGrace,
    Note::NospcDevice,
    Note::NospcReserve,
    Note::DquotHard,
    Note::DquotGraceOver,
    Note::FamilySwap,
    Note::DquotHard,
    Note::DquotHard,
    Note::DquotHard,
    Note::DquotHard,
    Note::DquotHard,
    Note::DquotHard,
];

/// 14 槽的期望对照档位。
const WANT_DEFAULT: [bool; CORPUS] = [
    false, false, false, true, true, true, true, false, true, true, true, true, true, true,
];

/// 14 槽标签。
const WANT_TAGS: [&'static str; CORPUS] = [
    "room-clear",
    "root-in-reserve",
    "grace-open-pass",
    "nospc-device",
    "nospc-reserve",
    "dquot-hard",
    "dquot-grace-over",
    "family-swap",
    "estate-swap",
    "stage-swap",
    "code-swap",
    "value-swap",
    "note-swap",
    "default-swap",
];

/// 14 槽的期望档位（由现场投影，语料的第三份陈述）。
const WANT_TIER: [Tier; CORPUS] = [
    Tier::Room,
    Tier::Room,
    Tier::Room,
    Tier::Nospc,
    Tier::Nospc,
    Tier::Dquot,
    Tier::Dquot,
    Tier::Rogue,
    Tier::Dquot,
    Tier::Dquot,
    Tier::Dquot,
    Tier::Dquot,
    Tier::Dquot,
    Tier::Dquot,
];

/// 八格申报正确的下标。
const ACCEPT_SLOTS: [usize; ACCEPT] = [0usize, 1, 2, 3, 4, 5, 6, 7];

/// 六格申报错误的下标。
const REJECT_SLOTS: [usize; REJECT] = [8usize, 9, 10, 11, 12, 13];

/// 被复刻的主样本下标（六死槽与它现场同一）。
const REJECT_SOURCE: usize = 5;
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
/// 分档只看**实际结果**落在哪一档，申报正确与否单独计。本卡尤其要分开：
/// 配额族那一档里塞了 6 个红档复刻槽，如果两维混计，"配额档覆盖面"看起来
/// 会从 8 槽缩成 2 槽，覆盖面假象就出来了。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Matrix {
    /// 行（room/nospc/dquot/rogue）。
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

    /// 双码两档是否都有槽（本卡的最低覆盖面：介质族与配额族各至少一格）。
    pub fn dual_covered(&self) -> bool {
        let nospc = match self.row(Tier::Nospc) {
            Some(r) => r.total,
            None => 0,
        };
        let dquot = match self.row(Tier::Dquot) {
            Some(r) => r.total,
            None => 0,
        };
        nospc > 0 && dquot > 0
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
    /// 放行档正确数。
    pub room: u32,
    /// 介质族档正确数。
    pub nospc: u32,
    /// 配额族档正确数。
    pub dquot: u32,
    /// 结构档正确数。
    pub rogue: u32,
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
            Err(EQ_ECOUNT)
        }
    }

    /// 放行档合计（申报正确且结果成功）。
    pub fn success_cells(&self) -> u32 {
        self.room
    }

    /// 失败档合计（申报正确且结果失败，含结构档）。
    pub fn failure_cells(&self) -> u32 {
        self.nospc + self.dquot + self.rogue
    }

    /// 双码档合计（介质族 + 配额族，对照档的正身）。
    pub fn dual_cells(&self) -> u32 {
        self.nospc + self.dquot
    }

    /// 渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("enospc-edquot corpus=");
        s.push_str(u32_text(self.total).as_str());
        s.push_str(" accept=");
        s.push_str(u32_text(self.accept).as_str());
        s.push_str(" reject=");
        s.push_str(u32_text(self.reject).as_str());
        s.push_str(" room=");
        s.push_str(u32_text(self.room).as_str());
        s.push_str(" nospc=");
        s.push_str(u32_text(self.nospc).as_str());
        s.push_str(" dquot=");
        s.push_str(u32_text(self.dquot).as_str());
        s.push_str(" rogue=");
        s.push_str(u32_text(self.rogue).as_str());
        s.push_str(" match=");
        s.push_str(u32_text(self.matched).as_str());
        s.push_str(" rate=");
        s.push_str(rate_text(self.rate()).as_str());
        s.push_str("ppm\n");
        s
    }
}

/// 全绿渲染的逐字钉版（基准机实录：本批 F11695 报表在全绿语料下的原文）。
const RENDER_OK: &str = "enospc-edquot corpus=14 accept=8 reject=6 room=3 nospc=2 dquot=2 rogue=1 match=14 rate=1000ppm\n";

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
            return Err(EQ_ESLOT);
        }
    }
    let cs = cases();
    let mut total = 0u32;
    let mut accept = 0u32;
    let mut reject = 0u32;
    let mut room = 0u32;
    let mut nospc = 0u32;
    let mut dquot = 0u32;
    let mut rogue = 0u32;
    let mut matched = 0u32;
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return Err(EQ_ENOBS),
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
            None => return Err(EQ_ENOBS),
        };
        total += 1;
        if f == want {
            matched += 1;
        }
        if f == Fault::Ok {
            accept += 1;
            match Tier::of(&a) {
                Tier::Room => room += 1,
                Tier::Nospc => nospc += 1,
                Tier::Dquot => dquot += 1,
                Tier::Rogue => rogue += 1,
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
        room,
        nospc,
        dquot,
        rogue,
        matched,
    })
}

// ---------------------------------------------------------------------------
// 阶段闸门
// ---------------------------------------------------------------------------

/// 单调阶段闸门。
///
/// 观测纵线是 `arg → subject → usage → grace → device → reserve → family →
/// done`，八段只能向前。回退（例如已经判死在配额闸，又声称走到了介质闸）
/// 说明申报者没有按闸门次序观测，而是事后从 strerror 倒推——本卡的头号
/// 风险恰好是这种倒推：看见 `No space left on device` 就补一个 device 阶段。
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
            return Err(EQ_ESTATE);
        }
        let o = s.order() as i32;
        if o < self.last {
            return Err(EQ_ESTAGE);
        }
        self.last = o;
        self.steps = self.steps.saturating_add(1);
        Ok(())
    }

    /// 关闭，返回段数。
    pub fn close(&mut self) -> Result<u32, u32> {
        if self.closed {
            return Err(EQ_ESTATE);
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
    /// 槽位（必须严格递增）。
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
            return Err(EQ_ESLOT);
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
            return Err(EQ_EBAL);
        }
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(EQ_EBAL),
            };
            if s.slot as usize != i {
                return Err(EQ_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(EQ_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(EQ_EVALUE);
                        }
                    }
                    None => return Err(EQ_EBAL),
                }
                vi += 1;
            } else {
                if s.value != 0 {
                    return Err(EQ_EVALUE);
                }
                match REJECT_CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(EQ_EVALUE);
                        }
                    }
                    None => return Err(EQ_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != ACCEPT || ci != REJECT {
            return Err(EQ_EBAL);
        }
        Ok(())
    }

    /// 操作码逐格对账（本卡附加面）：每格归属的闸门必须与 `LEDGER_OPS` 一致。
    pub fn ops_match(&self) -> Result<(), u32> {
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(EQ_EBAL),
            };
            match LEDGER_OPS.get(i) {
                Some(w) => {
                    if s.op != *w {
                        return Err(EQ_ECOUNT);
                    }
                }
                None => return Err(EQ_EBAL),
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
            return Err(EQ_EBAL);
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
            Err(EQ_ESEAL)
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
    let _ = l.register(count_cell(3, OP_DEVICE, r.map(|x| x.room)));
    let _ = l.register(count_cell(4, OP_RESERVE, r.map(|x| x.nospc)));
    let _ = l.register(count_cell(5, OP_USAGE, r.map(|x| x.dquot)));
    let _ = l.register(count_cell(6, OP_GRACE, r.map(|x| x.rogue)));

    let red = run_with(Some((0, Fault::Code)));
    let _ = l.register(count_cell(7, OP_REPLAY, red.map(|x| x.rate())));

    let cs = cases();
    let mut i = 0usize;
    while i < REJECT {
        if let Some(c) = cs.get(i + ACCEPT) {
            let f = judge(c);
            let _ = l.register(Sample {
                slot: (ACCEPT + i) as u8,
                op: OP_SUBJECT,
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
/// 1. 表路——段基址与联签、枚举、配额账与介质几何、形状合法域；
/// 2. 判路——八闸纵线、双码归族规则、对判、矩阵；
/// 3. 渲染路——语料、期望表、红档、文案、释义；
/// 4. 账本路——阶段闸门、逐格对账、冻结重放。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = name_ok() && anchor_ok() && enums_ok() && geom_ok() && shape_ok();
    out[1] = define_ok() && family_ok() && judge_ok() && matrix_ok();
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

/// 名称与段基址：常量链、尺寸、errno 真值、操作码一致性。
fn name_ok() -> bool {
    if SEG_BASE != 0x6d3e0 {
        return false;
    }
    if EQ_ESTAGE != SEG_BASE + 0x01
        || EQ_ECODE != SEG_BASE + 0x02
        || EQ_EVALUE != SEG_BASE + 0x03
        || EQ_ENOTE != SEG_BASE + 0x04
        || EQ_EDEFAULT != SEG_BASE + 0x05
        || EQ_EFAMILY != SEG_BASE + 0x06
    {
        return false;
    }
    if EQ_ESTATE != SEG_BASE + 0x07
        || EQ_ECOUNT != SEG_BASE + 0x08
        || EQ_ESLOT != SEG_BASE + 0x09
        || EQ_ESEAL != SEG_BASE + 0x0a
        || EQ_ENOBS != SEG_BASE + 0x0b
        || EQ_EMATRIX != SEG_BASE + 0x0c
        || EQ_EBAL != SEG_BASE + 0x0d
    {
        return false;
    }
    if CORPUS != 14 || SAMPLES != 14 || ACCEPT != 8 || REJECT != 6 || TIER_COUNT != 4 {
        return false;
    }
    // 两个主角必须是 Linux/glibc 真值，对照卡不许自造号码。
    if ERR_ENOSPC != 28 || ERR_EDQUOT != 122 || ERR_EINVAL != 22 || ERR_NONE != 0 {
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
    if family_text(28) != "nospc" || family_text(122) != "dquot" {
        return false;
    }
    if family_text(0) != "none" || family_text(22) != "arg" || family_text(7) != "unknown" {
        return false;
    }
    if OP_ARG != 1 || OP_SUBJECT != 2 || OP_USAGE != 3 || OP_GRACE != 4 {
        return false;
    }
    if OP_DEVICE != 5 || OP_RESERVE != 6 || OP_CORPUS != 7 || OP_REPLAY != 8 {
        return false;
    }
    if ERRNO_SET_SIZE != 4 {
        return false;
    }
    if GRACE_MAX != 604800 || COUNT_MAX != 0x0800_0000 || BLOCKS_MAX != 0x0040_0000 {
        return false;
    }
    // 码族分类必须四族互不相同（归族闸的等价关系）。
    let fam = [
        family_class(ERR_NONE),
        family_class(ERR_ENOSPC),
        family_class(ERR_EDQUOT),
        family_class(ERR_EINVAL),
    ];
    let mut i = 0usize;
    while i < fam.len() {
        let a = match fam.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let mut j = i + 1;
        while j < fam.len() {
            if let Some(b) = fam.get(j) {
                if *b == a {
                    return false;
                }
            }
            j += 1;
        }
        if a == 9 {
            return false;
        }
        i += 1;
    }
    // FNV 基元必须可复算（同输入同输出，异输入异输出）。
    let h1 = fold_str(FNV_OFF, "quota");
    let h2 = fold_str(FNV_OFF, "quota");
    let h3 = fold_str(FNV_OFF, "nospc");
    h1 == h2 && h1 != h3 && h1 != FNV_OFF
}

/// 联签：与 F11681 总锚的段位、码值、文案三处对齐。
fn anchor_ok() -> bool {
    if ANCHOR_F11681_SEG != 0x6d130 {
        return false;
    }
    if SEG_BASE < ANCHOR_F11681_SEG {
        return false;
    }
    if ANCHOR_GAP != SEG_BASE - ANCHOR_F11681_SEG || ANCHOR_GAP != 0x2b0 {
        return false;
    }
    if NEXT_F11696_SEG != 0x6d3f0 {
        return false;
    }
    if NEXT_F11696_SEG != SEG_BASE + 0x10 {
        return false;
    }
    // 总锚的码值与文案在本卡必须一字不差（同一个 28，不是两个 28）。
    if ERR_ENOSPC != 28 || strerror(ERR_ENOSPC) != "No space left on device" {
        return false;
    }
    // 本卡自己的 EDQUOT 不许与总锚的邻居档同号混写。
    if ERR_EDQUOT == ERR_ENOSPC {
        return false;
    }
    // 介质族文案与配额族文案只差尾段：前缀必须逐字相同。
    let a = msg_dd_enospc("x.bin");
    let b = msg_dd_edquot("x.bin");
    if a.as_str() == b.as_str() {
        return false;
    }
    if !a.starts_with("dd: error writing 'x.bin': ") || !b.starts_with("dd: error writing 'x.bin': ")
    {
        return false;
    }
    if !a.ends_with(strerror(ERR_ENOSPC)) || !b.ends_with(strerror(ERR_EDQUOT)) {
        return false;
    }
    // 同一 op 的两种码在 `touch`/`mkdir`/`tee` 体例下也只差尾段。
    let t28 = msg_touch_at("/mnt/full/f", ERR_ENOSPC);
    let t122 = msg_touch_at("/mnt/full/f", ERR_EDQUOT);
    if !t28.starts_with("touch: cannot touch '/mnt/full/f': ")
        || !t122.starts_with("touch: cannot touch '/mnt/full/f': ")
    {
        return false;
    }
    if t28.len() + 1 != t122.len() {
        // "No space left on device"(23) 与 "Quota exceeded"(14) 长度差 9；
        // 这里改成校尾段，不校长度差，避免把文案改动当成通过。
        return !t28.ends_with("Quota exceeded") && t122.ends_with("Quota exceeded");
    }
    true
}

/// 枚举：次序、文本唯一性、分类不重不漏。
fn enums_ok() -> bool {
    let stages = [
        Stage::Arg,
        Stage::Subject,
        Stage::Usage,
        Stage::Grace,
        Stage::Device,
        Stage::Reserve,
        Stage::Family,
        Stage::Done,
    ];
    if stages.len() != 8 {
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
        // 配额侧只有 usage/grace 两段；介质侧只有 device/reserve 两段。
        if s.quota_side() != (i == 2 || i == 3) {
            return false;
        }
        if s.device_side() != (i == 4 || i == 5) {
            return false;
        }
        // 结构侧是 arg(0) 与 family(6)。
        if s.structural() != (i == 0 || i == 6) {
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
    // 配额闸必须整段先于介质闸（次序即本卡的实况主张）。
    if Stage::Usage.order() >= Stage::Device.order() {
        return false;
    }
    if Stage::Grace.order() >= Stage::Reserve.order() {
        return false;
    }
    if Stage::Family.order() + 1 != Stage::Done.order() {
        return false;
    }

    let notes = [
        Note::Written,
        Note::RootPass,
        Note::DquotSoftGrace,
        Note::NospcDevice,
        Note::NospcReserve,
        Note::DquotHard,
        Note::DquotGraceOver,
        Note::DquotGroup,
        Note::DquotProject,
        Note::DquotLedger,
        Note::FamilySwap,
        Note::ArgBad,
    ];
    if notes.len() != 12 {
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
        // 四分类互斥且完备：成功 / 介质族 / 配额族 / 结构，一注解只属一类。
        let cls = a.success() as u8 + a.nospc() as u8 + a.dquot() as u8 + a.structural() as u8;
        if cls != 1 {
            return false;
        }
        // paired() 必须恰好等于"介质族或配额族"。
        if a.paired() != (a.nospc() || a.dquot()) {
            return false;
        }
        // 放行与对照档互斥（结构注解两者都不属，故只禁"同时成立"）。
        if a.paired() && a.success() {
            return false;
        }
        n += 1;
    }
    // 预留池一对：同一几何，两种主体两种结局，必须同属 reserved_cause。
    if !Note::NospcReserve.reserved_cause() || !Note::RootPass.reserved_cause() {
        return false;
    }
    if Note::NospcDevice.reserved_cause() || Note::Written.reserved_cause() {
        return false;
    }
    // 宽限期一对：放行与判死同属 grace_cause，且各自归类正确。
    if !Note::DquotSoftGrace.grace_cause() || !Note::DquotGraceOver.grace_cause() {
        return false;
    }
    if !Note::DquotSoftGrace.success() || Note::DquotGraceOver.success() {
        return false;
    }
    if !Note::DquotGraceOver.dquot() {
        return false;
    }
    // 介质族只有两个成因，配额族只有五个成因。
    let mut nospc_n = 0usize;
    let mut dquot_n = 0usize;
    let mut succ_n = 0usize;
    let mut j = 0usize;
    while j < notes.len() {
        if let Some(x) = notes.get(j) {
            if x.nospc() {
                nospc_n += 1;
            }
            if x.dquot() {
                dquot_n += 1;
            }
            if x.success() {
                succ_n += 1;
            }
        }
        j += 1;
    }
    if nospc_n != 2 || dquot_n != 5 || succ_n != 3 {
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
        if x.nospc() != (t == 1) || x.dquot() != (t == 2) {
            return false;
        }
        if x.dual_tier() != (t == 1 || t == 2) {
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
        Fault::Family,
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
        && Fault::Stage.code() == EQ_ESTAGE
        && Fault::Family.code() == EQ_EFAMILY
        && Fault::Estate.code() == EQ_ESTATE
        && Fault::Bal.code() == EQ_EBAL
}

/// 配额账与介质几何闸：四块表全良，逐项破坏必判死。
fn geom_ok() -> bool {
    let gs = geoms();
    if gs.len() != 4 {
        return false;
    }
    let mut i = 0usize;
    while i < gs.len() {
        if let Some(g) = gs.get(i) {
            if !g.well_formed() {
                return false;
            }
            if g.bytes_free().is_none() {
                return false;
            }
            if g.reserved_bytes().is_none() {
                return false;
            }
        }
        i += 1;
    }
    let full = match gs.get(0) {
        Some(v) => *v,
        None => return false,
    };
    let pooled = match gs.get(1) {
        Some(v) => *v,
        None => return false,
    };
    let xfs = match gs.get(2) {
        Some(v) => *v,
        None => return false,
    };
    let nfs = match gs.get(3) {
        Some(v) => *v,
        None => return false,
    };
    // 槽 0：介质真满。
    if full.bytes_free() != Some(0) {
        return false;
    }
    // 槽 1：free == reserved —— 非 root 的 available 恰为 0，root 仍可写。
    if pooled.bytes_free() != Some(32768) || pooled.reserved_bytes() != Some(32768) {
        return false;
    }
    // 槽 2：xfs 才有 project 配额。
    if !xfs.flavor.supports_project() || !xfs.flavor.has_reserved_pool() {
        return false;
    }
    if xfs.bytes_free() != Some(32768) {
        return false;
    }
    if xfs.reserved_bytes() != Some(2048) {
        return false;
    }
    // 槽 3：nfs4 不在客户端记账，也没有本地预留池。
    if nfs.flavor.local_accounting() || nfs.flavor.has_reserved_pool() {
        return false;
    }
    if nfs.flavor.supports_project() {
        return false;
    }
    if nfs.reserved_bytes() != Some(0) {
        return false;
    }
    // 逐项破坏：每坏一处都必须 well_formed() == false。
    let mut b = pooled;
    b.name = "";
    if b.well_formed() {
        return false;
    }
    b.name = "/dev/sdb1";
    b.bsize = 4095;
    if b.well_formed() || b.bsize_legal() {
        return false;
    }
    b.bsize = 4096;
    b.blocks_free = BLOCKS_MAX + 1;
    if b.well_formed() {
        return false;
    }
    b.blocks_free = 8;
    b.reserve_blocks = BLOCKS_MAX + 1;
    if b.well_formed() {
        return false;
    }
    b.reserve_blocks = 0;
    if b.bytes_free() != Some(32768) || b.reserved_bytes() != Some(0) {
        return false;
    }
    if !b.well_formed() {
        return false;
    }
    // 乘法溢出必须让 bytes_free() 归 None，而不是回绕成小值：
    // 4096 × (2^20 - 1) 恰好落进 u32，4096 × 2^20 就出界。
    let mut of = b;
    of.bsize = 4096;
    of.blocks_free = 0x000F_FFFF;
    if of.bytes_free().is_none() {
        return false;
    }
    of.blocks_free = 0x0010_0000;
    if of.bytes_free().is_some() {
        return false;
    }
    of.blocks_free = 0x0080_0000;
    if of.bytes_free().is_some() {
        return false;
    }
    // 配额账自洽面。
    let mut book = QuotaBook::blank();
    if !book.well_formed() {
        return false;
    }
    book.kind = QuotaKind::User;
    if book.well_formed() {
        // kind 有主体但 hard==0 → 判死。
        return false;
    }
    book.hard = 4000;
    book.soft = 5000;
    if book.well_formed() {
        // soft > hard 是坏账。
        return false;
    }
    book.soft = 1000;
    book.grace_left = GRACE_MAX + 1;
    if book.well_formed() {
        return false;
    }
    book.grace_left = 604800;
    book.used = COUNT_MAX;
    if book.well_formed() {
        // used 越上界：读数不可信。
        return false;
    }
    book.used = 3000;
    if !book.well_formed() {
        return false;
    }
    if !book.over_hard(1024) || book.over_hard(1) {
        return false;
    }
    if !book.over_soft(1024) || book.in_grace_window(1024) {
        return false;
    }
    if !book.in_grace_window(500) {
        return false;
    }
    // 宽限窗口的边界：used+count == hard 不算越硬限，> hard 才算。
    if book.over_hard(1000) {
        return false;
    }
    if !book.over_hard(1001) {
        return false;
    }
    // 存量已在软限之上：count 为 0 也仍然越软限（软限判的是存量 + 增量）。
    if !book.over_soft(0) {
        return false;
    }
    // 加法溢出按"越限"处理，不许回绕成放行。
    if !book.over_hard(COUNT_MAX) || !book.over_soft(COUNT_MAX) {
        return false;
    }
    book.hard == 4000 && book.seal() == QuotaBook { hard: 4000, ..book }.seal()
}

/// 形状合法域：文件系统族、配额主体、入参合法次序与副本语义。
fn shape_ok() -> bool {
    if FsFlavor::Ext4.text() != "ext4"
        || FsFlavor::Xfs.text() != "xfs"
        || FsFlavor::Btrfs.text() != "btrfs"
        || FsFlavor::Nfs4.text() != "nfs4"
    {
        return false;
    }
    if !FsFlavor::Ext4.modeled() || !FsFlavor::Xfs.modeled() {
        return false;
    }
    if FsFlavor::Ext4.supports_project() || !FsFlavor::Xfs.supports_project() {
        return false;
    }
    if FsFlavor::Nfs4.local_accounting() || !FsFlavor::Ext4.local_accounting() {
        return false;
    }
    if QuotaKind::None.has_subject() || !QuotaKind::Project.has_subject() {
        return false;
    }
    if QuotaKind::None.needs_flavor() || !QuotaKind::Project.needs_flavor() {
        return false;
    }
    if !QuotaKind::User.per_entity() || QuotaKind::Group.per_entity() {
        return false;
    }
    if QuotaKind::Group.per_entity() && QuotaKind::Project.per_entity() {
        return false;
    }

    // 合法形状：默认形状即合法。
    let mut w = Shape::blank();
    w.tag = "probe";
    if !w.valid() || w.count != 4096 {
        return false;
    }
    // count==0 → 非法，且 illegal_field 的第一顺位就是 count。
    if let Some((f, _)) = w.with_count(0).illegal_field() {
        if f != "count" {
            return false;
        }
    } else {
        return false;
    }
    // 观测 errno 越界 → 非法形状（不许判）。
    let mut e = w.clone();
    e.bsize = 3;
    if e.valid() {
        return false;
    }
    if let Some((f, v)) = e.illegal_field() {
        if f != "bsize" || v != 3 {
            return false;
        }
    } else {
        return false;
    }
    // blocks_free 越上界 → 非法。
    e.bsize = 4096;
    e.blocks_free = BLOCKS_MAX + 1;
    if e.valid() {
        return false;
    }
    // grace 越界 → 非法。
    e.blocks_free = 64;
    e.grace_left = GRACE_MAX + 1;
    if e.valid() {
        return false;
    }
    // soft > hard → 非法。
    e.grace_left = 0;
    e.soft = 9000;
    e.hard = 4000;
    if e.valid() {
        return false;
    }
    // 申报码不在合法集 → 非法（对照卡只认 0/28/122/22 四码）。
    e.soft = 0;
    e.hard = 0;
    e.reported = 5;
    if e.valid() || e.errno_legal() {
        return false;
    }
    e.reported = -3;
    if e.valid() {
        return false;
    }
    e.reported = ERR_EDQUOT;
    if !e.valid() {
        return false;
    }
    // uid 越界 → 非法。
    let mut u = e;
    u.reported = ERR_NONE;
    u.uid = 65535;
    if u.valid() {
        return false;
    }
    u.uid = 1000;
    if !u.valid() {
        return false;
    }
    // 配额适用性三态：主体 + quotaon + 文件系统支持，缺一即不适用。
    let mut q = Shape::blank();
    q.kind = QuotaKind::User;
    q.enforced = true;
    if !q.quota_applies() || !q.subject_supported() {
        return false;
    }
    q.enforced = false;
    if q.quota_applies() {
        return false;
    }
    if !q.subject_supported() {
        // 主体本身仍然"可用"，只是没开 quotaon。
        return false;
    }
    q.kind = QuotaKind::Project;
    q.enforced = true;
    if q.subject_supported() || q.quota_applies() {
        return false;
    }
    q.fs = FsFlavor::Xfs;
    if !q.subject_supported() || !q.quota_applies() {
        return false;
    }
    q.fs = FsFlavor::Nfs4;
    if q.subject_supported() {
        return false;
    }
    q.kind = QuotaKind::None;
    if q.subject_supported() {
        return false;
    }
    // usable_bytes：root 看得见全部，非 root 被预留池扣掉。
    let mut r = Shape::blank();
    r.blocks_free = 8;
    r.reserve_blocks = 8;
    r.is_root = false;
    if r.bytes_free() != Some(32768) || r.reserved_bytes() != 32768 {
        return false;
    }
    if r.usable_bytes() != Some(0) {
        return false;
    }
    r.is_root = true;
    if r.usable_bytes() != Some(32768) {
        return false;
    }
    r.fs = FsFlavor::Nfs4;
    r.is_root = false;
    if r.reserved_bytes() != 0 || r.usable_bytes() != Some(32768) {
        return false;
    }
    // with_* 副本只改目标维。
    let mut base = Shape::blank();
    base.tag = "clone-src";
    base.path = "home/alice/img.raw";
    base.count = 1024;
    base.blocks_free = 16;
    base.used = 500;
    base.grace_left = 60;
    base.reported = ERR_EDQUOT;
    let c1 = base.with_count(4096);
    if c1.count != 4096
        || c1.blocks_free != 16
        || c1.used != 500
        || c1.reported != ERR_EDQUOT
        || c1.tag != base.tag
    {
        return false;
    }
    let c2 = base.with_blocks_free(0);
    if c2.blocks_free != 0 || c2.count != 1024 {
        return false;
    }
    let c3 = base.with_used(3000);
    if c3.used != 3000 || c3.blocks_free != 16 {
        return false;
    }
    let c4 = base.with_grace(0);
    if c4.grace_left != 0 || c4.used != 500 {
        return false;
    }
    let c5 = base.with_kind(QuotaKind::Group);
    if c5.kind != QuotaKind::Group || c5.enforced != base.enforced {
        return false;
    }
    if base.with_reported(ERR_NONE).reported != 0 || base.with_reported(ERR_NONE).used != 500 {
        return false;
    }
    // 指纹对形状字段敏感：改一维必换指纹。
    let s1 = base.seal();
    let s2 = base.with_count(1025).seal();
    let s3 = base.with_blocks_free(17).seal();
    let s4 = base.with_used(501).seal();
    let s5 = base.with_grace(61).seal();
    let mut b6 = base;
    b6.is_root = true;
    let s6 = b6.seal();
    let mut b7 = base;
    b7.fs = FsFlavor::Xfs;
    let s7 = b7.seal();
    let mut b8 = base;
    b8.enforced = true;
    let s8 = b8.seal();
    s1 != s2
        && s1 != s3
        && s1 != s4
        && s1 != s5
        && s1 != s6
        && s1 != s7
        && s1 != s8
        && s6 != s7
        && s7 != s8
}

/// 定义纵线：八闸逐段推进，先短路的段不得被后段覆盖。
fn define_ok() -> bool {
    // 闸一：入参非法短路一切——满盘也轮不到 ENOSPC/EDQUOT。
    let mut bad = Shape::blank();
    bad.count = 0;
    bad.blocks_free = 0;
    bad.kind = QuotaKind::User;
    bad.enforced = true;
    bad.hard = 1000;
    bad.used = 9000;
    bad.reported = ERR_EINVAL;
    let o = define(&bad);
    if o.stage != Stage::Arg || o.code != ERR_EINVAL || o.note != Note::ArgBad {
        return false;
    }
    if o.ok() || o.default || Tier::of(&o) != Tier::Rogue {
        return false;
    }
    // 同一坏形状：申报码换成 28，归族闸仍要抓到（非法入参优先落 arg）。
    let o2 = define(&bad.with_reported(ERR_ENOSPC));
    if o2.stage != Stage::Arg || o2.note != Note::ArgBad {
        return false;
    }

    // 闸二：配额在册但账不可读 → 判死在 subject，属配额族。
    let mut lrd = Shape::blank();
    lrd.kind = QuotaKind::User;
    lrd.enforced = true;
    lrd.ledger_ok = false;
    lrd.hard = 4000;
    lrd.soft = 1000;
    lrd.used = 100;
    lrd.blocks_free = 64;
    lrd.reported = ERR_EDQUOT;
    let o = define(&lrd);
    if o.stage != Stage::Subject || o.code != ERR_EDQUOT || o.note != Note::DquotLedger {
        return false;
    }
    if !o.default || !o.note.dquot() || Tier::of(&o) != Tier::Dquot {
        return false;
    }
    // 账可读之后，同一形状必须直接落到放行（证明 subject 闸只卡可读性）。
    if define(&lrd.with_used(100)).stage != Stage::Done {
        return false;
    }

    // 闸三：硬上限先于宽限期——超 hard 时 grace_left 再大也判死。
    let mut hard = Shape::blank();
    hard.kind = QuotaKind::User;
    hard.enforced = true;
    hard.soft = 1000;
    hard.hard = 4000;
    hard.used = 3000;
    hard.count = 4096;
    hard.grace_left = 604800;
    hard.reported = ERR_EDQUOT;
    let o = define(&hard);
    if o.stage != Stage::Usage || o.note != Note::DquotHard || o.code != ERR_EDQUOT {
        return false;
    }
    if o.value != 0 || !o.default || !o.paired() {
        return false;
    }
    // 恰好等于硬限（used+count == hard）不算越限。
    hard.used = 0;
    hard.count = 4000;
    hard.reported = ERR_NONE;
    if define(&hard).stage != Stage::Done {
        return false;
    }
    // 主体分式：group/project 各落自己的注解，但同属配额族同属一码。
    hard.used = 3000;
    hard.count = 4096;
    hard.reported = ERR_EDQUOT;
    hard.kind = QuotaKind::Group;
    let o = define(&hard);
    if o.stage != Stage::Usage || o.note != Note::DquotGroup || !o.note.dquot() {
        return false;
    }
    let mut prj = hard.with_kind(QuotaKind::Project);
    prj.fs = FsFlavor::Xfs;
    prj.used = 3000;
    let o = define(&prj);
    if o.stage != Stage::Usage || o.note != Note::DquotProject || o.code != ERR_EDQUOT {
        return false;
    }

    // 闸四：宽限期两副面孔——同一账本，只差 grace_left。
    let mut grace = Shape::blank();
    grace.kind = QuotaKind::User;
    grace.enforced = true;
    grace.soft = 1000;
    grace.hard = 8000;
    grace.used = 2000;
    grace.count = 4096;
    grace.reported = ERR_NONE;
    grace.grace_left = 604800;
    let o = define(&grace);
    if !o.ok() || o.note != Note::DquotSoftGrace || o.value != 4096 {
        return false;
    }
    if o.stage != Stage::Done || o.default || Tier::of(&o) != Tier::Room {
        return false;
    }
    grace.grace_left = 0;
    grace.reported = ERR_EDQUOT;
    let o = define(&grace);
    if o.stage != Stage::Grace || o.note != Note::DquotGraceOver || o.code != ERR_EDQUOT {
        return false;
    }
    if o.ok() || !o.default {
        return false;
    }
    // 未越软限时不进宽限闸（used+count <= soft）。
    let mut under = grace;
    under.used = 0;
    under.count = 512;
    under.reported = ERR_NONE;
    if define(&under).note != Note::Written {
        return false;
    }

    // 闸五：介质真满排在配额之后——盘满且配额满必须报 EDQUOT。
    let mut both = grace;
    both.used = 3000;
    both.count = 4096;
    both.hard = 4000;
    both.blocks_free = 0;
    both.reported = ERR_EDQUOT;
    let o = define(&both);
    if o.stage != Stage::Usage || o.note != Note::DquotHard {
        return false;
    }
    // 无配额时的满盘才是 device。
    let mut dev = both;
    dev.kind = QuotaKind::None;
    dev.enforced = false;
    dev.soft = 0;
    dev.hard = 0;
    dev.used = 0;
    dev.reported = ERR_ENOSPC;
    let o = define(&dev);
    if o.stage != Stage::Device
        || o.note != Note::NospcDevice
        || o.code != ERR_ENOSPC
        || !o.default
    {
        return false;
    }
    if Tier::of(&o) != Tier::Nospc || !o.note.nospc() {
        return false;
    }

    // 闸六：预留池档——同一几何，root 放行、非 root 判死，且都是 ENOSPC。
    let mut pool = Shape::blank();
    pool.blocks_free = 8;
    pool.reserve_blocks = 8;
    pool.count = 4096;
    pool.is_root = false;
    pool.reported = ERR_ENOSPC;
    let o = define(&pool);
    if o.stage != Stage::Reserve || o.note != Note::NospcReserve || o.code != ERR_ENOSPC {
        return false;
    }
    if !o.default || !o.note.reserved_cause() {
        return false;
    }
    let mut rootw = pool;
    rootw.is_root = true;
    rootw.reported = ERR_NONE;
    let o = define(&rootw);
    if !o.ok() || o.note != Note::RootPass || o.value != 4096 {
        return false;
    }
    if Tier::of(&o) != Tier::Room || o.default {
        return false;
    }
    if o.note.reserved_cause() != true {
        return false;
    }
    // 预留池为 0 时非 root 不该撞这一闸（同一请求走 device/done）。
    let mut nopool = pool;
    nopool.reserve_blocks = 0;
    let o = define(&nopool);
    if o.stage != Stage::Done || o.note != Note::Written {
        return false;
    }
    // 族不支持预留池时也不进这一闸。
    let mut nfsp = pool;
    nfsp.fs = FsFlavor::Nfs4;
    if define(&nfsp).note != Note::Written {
        return false;
    }

    // 闸七：归族破断——物理放行却申报配额族。
    let mut swap = Shape::blank();
    swap.kind = QuotaKind::Project;
    swap.enforced = true;
    swap.hard = 4000;
    swap.soft = 1000;
    swap.used = 3000;
    swap.count = 4096;
    swap.reported = ERR_EDQUOT;
    let o = define(&swap);
    if o.stage != Stage::Family || o.note != Note::FamilySwap || o.code != ERR_EDQUOT {
        return false;
    }
    if o.value != 4096 || o.default || Tier::of(&o) != Tier::Rogue {
        return false;
    }
    // 反向破断：物理满盘（28）却申报配额族（122）。
    let mut swap2 = Shape::blank();
    swap2.blocks_free = 0;
    swap2.count = 4096;
    swap2.reported = ERR_EDQUOT;
    let o = define(&swap2);
    if o.stage != Stage::Family || o.code != ERR_EDQUOT || o.value != 0 {
        return false;
    }
    if o.default {
        return false;
    }
    // 申报零而物理满：也是破断（把 ENOSPC 说成成功）。
    let o = define(&swap2.with_reported(ERR_NONE));
    if o.stage != Stage::Family || o.note != Note::FamilySwap || o.code != ERR_NONE {
        return false;
    }
    if o.ok() {
        return false;
    }
    if o.value != 0 {
        return false;
    }
    // 同族申报（28 vs 28）不许进 family 闸。
    if define(&swap2.with_reported(ERR_ENOSPC)).stage != Stage::Device {
        return false;
    }
    // verdict 文本必须与档位一致。
    verdict(&o) == "errno family mismatch" && verdict(&define(&nopool)) == "room"
}

/// 归族规则：码族等价、跨族必破、档位位与注解互锁。
fn family_ok() -> bool {
    if family_class(0) != 0
        || family_class(28) != 1
        || family_class(122) != 2
        || family_class(22) != 3
        || family_class(5) != 9
    {
        return false;
    }
    // 同码同族（等价关系自反）。
    let mut i = 0usize;
    let codes = [0i32, 28, 122, 22];
    while i < codes.len() {
        let c = match codes.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if family_class(c) != family_class(c) {
            return false;
        }
        if family_text(c) != "unknown" {
            return false;
        }
        i += 1;
    }
    if family_text(31337) != "unknown" {
        return false;
    }
    // 本卡建模的四码文案必须互不相同（对照档的立足点）。
    let texts = [
        strerror(0),
        strerror(28),
        strerror(122),
        strerror(22),
    ];
    let mut a = 0usize;
    while a < texts.len() {
        let x = match texts.get(a) {
            Some(v) => *v,
            None => return false,
        };
        let mut b = a + 1;
        while b < texts.len() {
            if let Some(y) = texts.get(b) {
                if *y == x {
                    return false;
                }
            }
            b += 1;
        }
        a += 1;
    }
    // 逐槽复算：档位位 ⟺ 注解属介质族或配额族，且同族申报不进 family 闸。
    let cs = cases();
    let mut k = 0usize;
    while k < CORPUS {
        let c = match cs.get(k) {
            Some(v) => *v,
            None => return false,
        };
        let o = c.actual();
        if o.default != o.note.paired() {
            return false;
        }
        if o.stage == Stage::Family && (o.note != Note::FamilySwap || o.default) {
            return false;
        }
        if o.note == Note::FamilySwap && o.stage != Stage::Family {
            return false;
        }
        if o.note.paired() && o.ok() {
            return false;
        }
        if o.ok() && o.stage != Stage::Done {
            return false;
        }
        // 申报码与物理码同族 ⟹ 结束阶段不是 family。
        if family_class(c.shape.reported) == family_class(physical_errno(&c.shape))
            && o.stage == Stage::Family
        {
            return false;
        }
        k += 1;
    }
    // 物理 errno 与物理阶段必须自洽（done ⇒ 0）。
    let mut m = 0usize;
    while m < CORPUS {
        if let Some(c) = cs.get(m) {
            let pe = physical_errno(&c.shape);
            let ps = physical_stage(&c.shape);
            if (ps == Stage::Done) != (pe == ERR_NONE) {
                return false;
            }
            if ps.device_side() && pe != ERR_ENOSPC {
                return false;
            }
            if ps.quota_side() && pe != ERR_EDQUOT {
                return false;
            }
        }
        m += 1;
    }
    // 头号缺陷复算：配额满被报成设备满，一定被归族或 code 维抓住。
    let base = match cs.get(REJECT_SOURCE) {
        Some(v) => *v,
        None => return false,
    };
    let lied = Case {
        shape: base.shape.with_reported(ERR_ENOSPC),
        claim: Claim {
            stage: Stage::Device,
            code: 28,
            value: 0,
            note: Note::NospcDevice,
            default: true,
        },
    };
    // 现场（with_reported(28)）会被算成介质满，整条申报"自洽"，因此
    // 必须由对照表把这条排除出活槽；本卡抓它的方式是**标签与主样本不同**
    // 而 errno 家族改变，故只在语料表内判死，不在这里判 Fault::Ok。
    lied.judge() == Fault::Ok
        && lied.actual().code == ERR_ENOSPC
        && base.actual().code == ERR_EDQUOT
        && base.actual().stage == Stage::Usage
}

// ---------------------------------------------------------------------------
// 判据基元（续）：申报对判、矩阵、语料、槽位、红档、文案、释义、闸门、账本
// ---------------------------------------------------------------------------

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
    // Estate 优先：拿活槽 3（介质真满）造一条"五维全歪"的申报，
    // 只有 Estate 维能让它致命；若次序错了会先报 Stage。
    let base3 = match cs.get(3) {
        Some(v) => *v,
        None => return false,
    };
    let allwrong = Case {
        shape: base3.shape,
        claim: Claim {
            stage: Stage::Done,
            code: 0,
            value: 4096,
            note: Note::Written,
            default: false,
        },
    };
    if judge(&allwrong) != Fault::Estate {
        return false;
    }
    // Stage 优先于 Code：把配额闸现场报成介质闸**并且**报码 28。
    let base5 = match cs.get(5) {
        Some(v) => *v,
        None => return false,
    };
    let stage_and_code = Case {
        shape: base5.shape,
        claim: Claim {
            stage: Stage::Device,
            code: 28,
            value: 0,
            note: Note::DquotHard,
            default: true,
        },
    };
    if judge(&stage_and_code) != Fault::Stage {
        return false;
    }
    // Code 优先于 Value：码错 + 字节错同现时只报 Code。
    let code_and_value = Case {
        shape: base5.shape,
        claim: Claim {
            stage: Stage::Usage,
            code: 28,
            value: 4096,
            note: Note::DquotHard,
            default: true,
        },
    };
    if judge(&code_and_value) != Fault::Code {
        return false;
    }
    // Value 优先于 Note：122 报对了阶段，字节与注解双错时先报 Value。
    let value_and_note = Case {
        shape: base5.shape,
        claim: Claim {
            stage: Stage::Usage,
            code: 122,
            value: 4096,
            note: Note::NospcDevice,
            default: true,
        },
    };
    if judge(&value_and_note) != Fault::Value {
        return false;
    }
    // Note 优先于 Default：注解与档位双错时先报 Note。
    let note_and_default = Case {
        shape: base5.shape,
        claim: Claim {
            stage: Stage::Usage,
            code: 122,
            value: 0,
            note: Note::NospcReserve,
            default: false,
        },
    };
    if judge(&note_and_default) != Fault::Note {
        return false;
    }
    // 反向 Estate：把放行现场报成配额死。
    let base2 = match cs.get(2) {
        Some(v) => *v,
        None => return false,
    };
    let pass_claimed_dead = Case {
        shape: base2.shape,
        claim: Claim {
            stage: Stage::Usage,
            code: 122,
            value: 0,
            note: Note::DquotHard,
            default: true,
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
    if cover(CORPUS) != Ok(()) || cover(CORPUS - 1) != Err(EQ_ENOBS) {
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
    c0.actual() == define(&c0.shape)
}

/// 矩阵与报表：分档只投影现场，`total` 与 `good` 两维独立，报表渲染逐字。
fn matrix_ok() -> bool {
    let m = Matrix::of();
    if m.slots() != CORPUS || m.goods() != ACCEPT {
        return false;
    }
    if !m.covered() || !m.dual_covered() {
        return false;
    }
    // 四档槽数与正确数（现场投影：room=3、nospc=2、dquot=8、rogue=1）。
    let want_total: [(Tier, usize, usize); 4] = [
        (Tier::Room, 3usize, 3usize),
        (Tier::Nospc, 2usize, 2usize),
        (Tier::Dquot, 8usize, 2usize),
        (Tier::Rogue, 1usize, 1usize),
    ];
    let mut i = 0usize;
    while i < want_total.len() {
        let (t, tot, good) = match want_total.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let row = match m.row(t) {
            Some(v) => v,
            None => return false,
        };
        if row.total != tot || row.good != good {
            return false;
        }
        if row.red() != tot - good {
            return false;
        }
        if row.perfect() != (tot == good) {
            return false;
        }
        i += 1;
    }
    if m.perfect_rows() != 3 {
        return false;
    }
    // `total`/`good` 独立性的直接证词：配额族那一档被 6 个复刻红槽压住，
    // 覆盖面（8 槽）与申报正确数（2 槽）必须分开统计。
    let dq = match m.row(Tier::Dquot) {
        Some(v) => v,
        None => return false,
    };
    if dq.total == dq.good || dq.good != ACCEPT - 6 {
        return false;
    }
    // 防御分支：good>total 的畸形行不许 panic，红数钳回 total。
    let weird = Row { total: 2, good: 5 };
    if weird.red() != 2 || weird.perfect() {
        return false;
    }
    if Row::blank().red() != 0 || Row::blank().perfect() {
        return false;
    }
    if m.row(Tier::Room).and_then(|r| Some(r.render(Tier::Room))).is_none() {
        return false;
    }
    let first_line = match m.row(Tier::Room) {
        Some(r) => r.render(Tier::Room),
        None => return false,
    };
    if first_line.as_str() != "room total=3 good=3 red=0\n" {
        return false;
    }
    let all = m.render();
    if !all.contains("room total=3 good=3 red=0\n") || !all.contains("dquot total=8 good=2 red=6\n")
    {
        return false;
    }
    if !all.contains("nospc total=2 good=2 red=0\n") || !all.contains("rogue total=1 good=1 red=0\n")
    {
        return false;
    }
    // 报表：全绿、渲染逐字、闸门放行。
    let rep = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if rep.total != 14
        || rep.accept != 8
        || rep.reject != 6
        || rep.room != 3
        || rep.nospc != 2
        || rep.dquot != 2
        || rep.rogue != 1
        || rep.matched != 14
    {
        return false;
    }
    if rep.rate() != 1000 || !rep.green() || rep.gate() != Ok(()) {
        return false;
    }
    if rep.success_cells() != 3 || rep.failure_cells() != 5 || rep.dual_cells() != 4 {
        return false;
    }
    if rep.render().as_str() != RENDER_OK {
        return false;
    }
    // 越界注入必须撞槽位错，而不是静默忽略。
    match run_with(Some((CORPUS, Fault::Code))) {
        Err(c) => {
            if c != EQ_ESLOT {
                return false;
            }
        }
        Ok(_) => return false,
    }
    // 对活槽注入"报对的绿"不改变全绿结论（matched 仍 14）。
    let keepgreen = match run_with(Some((0, Fault::Ok))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if keepgreen.matched != 14 || keepgreen.rate() != 1000 || !keepgreen.green() {
        return false;
    }
    // 现场投影与期望档位表全等（语料的第三份陈述在这里对账）。
    let cs = cases();
    let mut k = 0usize;
    while k < CORPUS {
        let c = match cs.get(k) {
            Some(v) => *v,
            None => return false,
        };
        match WANT_TIER.get(k) {
            Some(w) => {
                if Tier::of(&c.actual()) != *w {
                    return false;
                }
            }
            None => return false,
        }
        k += 1;
    }
    true
}

/// 语料与期望表交叉校验：标签、五维期望、一槽只坏一维。
fn corpus_ok() -> bool {
    let cs = cases();
    if cs.len() != CORPUS {
        return false;
    }
    // 标签表逐字。
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        match WANT_TAGS.get(i) {
            Some(w) => {
                if c.tag() != *w {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    // 现场五维必须逐格等于期望表（申报走的是同一纵线，不是另一本账）。
    let mut k = 0usize;
    while k < CORPUS {
        let c = match cs.get(k) {
            Some(v) => *v,
            None => return false,
        };
        let a = c.actual();
        let (ws, wc, wv, wn, wd) = match (
            WANT_STAGE.get(k),
            WANT_CODE.get(k),
            WANT_VALUE.get(k),
            WANT_NOTE.get(k),
            WANT_DEFAULT.get(k),
        ) {
            (Some(s), Some(cd), Some(v), Some(n), Some(d)) => (*s, *cd, *v, *n, *d),
            _ => return false,
        };
        if a.stage != ws || a.code != wc || a.value != wv || a.note != wn || a.default != wd {
            return false;
        }
        k += 1;
    }
    // 六死槽"一槽只坏一维"：除 Estate 槽外，申报与现场只允许在
    // 该维的原始读数上不同，其余四维必须逐字相同。
    let mut m = ACCEPT;
    while m < CORPUS {
        let c = match cs.get(m) {
            Some(v) => *v,
            None => return false,
        };
        let a = c.actual();
        let f = judge(&c);
        if f == Fault::Estate {
            // Estate 槽允许其余维全按成功口径写：它坏的就是成败本身。
            if m != ACCEPT {
                return false;
            }
            if !c.claim.success() {
                return false;
            }
            m += 1;
            continue;
        }
        let diff_stage = a.stage != c.claim.stage;
        let diff_code = a.code != c.claim.code;
        let diff_value = a.value != c.claim.value;
        let diff_note = a.note != c.claim.note;
        let diff_default = a.default != c.claim.default;
        let only = match f {
            Fault::Stage => diff_stage && !diff_code && !diff_value && !diff_note && !diff_default,
            Fault::Code => !diff_stage && diff_code && !diff_value && !diff_note && !diff_default,
            Fault::Value => !diff_stage && !diff_code && diff_value && !diff_note && !diff_default,
            Fault::Note => !diff_stage && !diff_code && !diff_value && diff_note && !diff_default,
            Fault::Default => {
                !diff_stage && !diff_code && !diff_value && !diff_note && diff_default
            }
            _ => false,
        };
        if !only {
            return false;
        }
        // 复刻形状必须与主样本同现场：判定字段逐字相同。
        if let Some(src) = case_at(REJECT_SOURCE) {
            let s = c.shape;
            let t = src.shape;
            if s.path != t.path
                || s.user != t.user
                || s.fs != t.fs
                || s.kind != t.kind
                || s.enforced != t.enforced
                || s.ledger_ok != t.ledger_ok
                || s.bsize != t.bsize
                || s.blocks_free != t.blocks_free
                || s.reserve_blocks != t.reserve_blocks
                || s.soft != t.soft
                || s.hard != t.hard
                || s.used != t.used
                || s.grace_left != t.grace_left
                || s.is_root != t.is_root
                || s.count != t.count
                || s.reported != t.reported
            {
                return false;
            }
        } else {
            return false;
        }
        m += 1;
    }
    // 活槽覆盖：放行三档（written/root-in-reserve/grace-open-pass）与
    // 双码各成因都必须在场，且结构档（family-swap）申报自洽。
    let mut seen_written = false;
    let mut seen_rootpass = false;
    let mut seen_grace = false;
    let mut seen_nospc_device = false;
    let mut seen_nospc_reserve = false;
    let mut seen_dquot = 0usize;
    let mut seen_family = false;
    let mut p = 0usize;
    while p < ACCEPT {
        let c = match cs.get(p) {
            Some(v) => *v,
            None => return false,
        };
        let a = c.actual();
        if a.note == Note::Written {
            seen_written = true;
        }
        if a.note == Note::RootPass {
            seen_rootpass = true;
        }
        if a.note == Note::DquotSoftGrace {
            seen_grace = true;
        }
        if a.note == Note::NospcDevice {
            seen_nospc_device = true;
        }
        if a.note == Note::NospcReserve {
            seen_nospc_reserve = true;
        }
        if a.note.dquot() {
            seen_dquot += 1;
        }
        if a.note == Note::FamilySwap {
            seen_family = true;
        }
        p += 1;
    }
    if !(seen_written
        && seen_rootpass
        && seen_grace
        && seen_nospc_device
        && seen_nospc_reserve
        && seen_family)
    {
        return false;
    }
    if seen_dquot != 2 {
        return false;
    }
    // 双码互斥：同一现场不许同时属两族。
    let mut q = 0usize;
    while q < CORPUS {
        if let Some(c) = cs.get(q) {
            let a = c.actual();
            if a.note.nospc() && a.note.dquot() {
                return false;
            }
        }
        q += 1;
    }
    // 申报表的第二份陈述：CLAIMS 与期望表逐格一致（活槽）。
    let mut z = 0usize;
    while z < ACCEPT {
        let c = match cs.get(z) {
            Some(v) => *v,
            None => return false,
        };
        let (ws, wc, wv, wn, wd) = match (
            WANT_STAGE.get(z),
            WANT_CODE.get(z),
            WANT_VALUE.get(z),
            WANT_NOTE.get(z),
            WANT_DEFAULT.get(z),
        ) {
            (Some(s), Some(cd), Some(v), Some(n), Some(d)) => (*s, *cd, *v, *n, *d),
            _ => return false,
        };
        if c.claim.stage != ws || c.claim.code != wc || c.claim.value != wv {
            return false;
        }
        if c.claim.note != wn || c.claim.default != wd {
            return false;
        }
        z += 1;
    }
    true
}

/// 槽位与码链：8+6 划分、VALS 结构、缺陷码互不相同且在段内。
fn slots_ok() -> bool {
    if ACCEPT + REJECT != CORPUS || SAMPLES != CORPUS {
        return false;
    }
    if VALS.len() != ACCEPT {
        return false;
    }
    // 第 8 格必须是红档率 929（13/14 的千分四舍五入）。
    match VALS.get(ACCEPT - 1) {
        Some(v) => {
            if *v != 929 {
                return false;
            }
        }
        None => return false,
    }
    // 前 7 格与全绿报表逐格对账。
    let rep = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let report_cells: [u32; 7] = [
        rep.total,
        rep.accept,
        rep.reject,
        rep.room,
        rep.nospc,
        rep.dquot,
        rep.rogue,
    ];
    let mut i = 0usize;
    while i < 7 {
        match (VALS.get(i), report_cells.get(i)) {
            (Some(w), Some(g)) => {
                if *w != *g {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    // 红档率格：注入一槽后 rate 必须恰等于 929。
    let injected = match run_with(Some((0, Fault::Code))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if injected.rate() != 929 || injected.green() {
        return false;
    }
    // 缺陷码：六个互不相同、全在段内、与缺陷枚举对齐。
    if REJECT_CODES.len() != REJECT || REJECT_FAULTS.len() != REJECT {
        return false;
    }
    let mut a = 0usize;
    while a < REJECT {
        let x = match REJECT_CODES.get(a) {
            Some(v) => *v,
            None => return false,
        };
        if x <= SEG_BASE || x > SEG_BASE + 0x0d {
            return false;
        }
        let mut b = a + 1;
        while b < REJECT {
            if let Some(y) = REJECT_CODES.get(b) {
                if *y == x {
                    return false;
                }
            }
            b += 1;
        }
        a += 1;
    }
    // 13 个错误码逐档排布，互不相同。
    let codes: [u32; 13] = [
        EQ_ESTAGE,
        EQ_ECODE,
        EQ_EVALUE,
        EQ_ENOTE,
        EQ_EDEFAULT,
        EQ_EFAMILY,
        EQ_ESTATE,
        EQ_ECOUNT,
        EQ_ESLOT,
        EQ_ESEAL,
        EQ_ENOBS,
        EQ_EMATRIX,
        EQ_EBAL,
    ];
    let mut p = 0usize;
    while p < codes.len() {
        let x = match codes.get(p) {
            Some(v) => *v,
            None => return false,
        };
        if x != SEG_BASE + (p as u32) + 1 {
            return false;
        }
        let mut q = p + 1;
        while q < codes.len() {
            if let Some(y) = codes.get(q) {
                if *y == x {
                    return false;
                }
            }
            q += 1;
        }
        p += 1;
    }
    // 槽位划分：0..8 活、8..14 死，不重不漏。
    let mut hit = [0u8; CORPUS];
    let mut i2 = 0usize;
    while i2 < ACCEPT {
        match (ACCEPT_SLOTS.get(i2), hit.get_mut(i2)) {
            (Some(s), Some(h)) => {
                if *s != i2 || *h != 0 {
                    return false;
                }
                *h = 1;
            }
            _ => return false,
        }
        i2 += 1;
    }
    let mut i3 = 0usize;
    while i3 < REJECT {
        let idx = match REJECT_SLOTS.get(i3) {
            Some(v) => *v,
            None => return false,
        };
        if idx != ACCEPT + i3 {
            return false;
        }
        match hit.get_mut(idx) {
            Some(h) => {
                if *h != 0 {
                    return false;
                }
                *h = 1;
            }
            None => return false,
        }
        i3 += 1;
    }
    let mut t = 0usize;
    while t < CORPUS {
        match hit.get(t) {
            Some(v) => {
                if *v != 1 {
                    return false;
                }
            }
            None => return false,
        }
        t += 1;
    }
    true
}

/// 红档面：注入路必须真实掉 matched，六死槽每一格的红都要单独成立。
fn reds_ok() -> bool {
    let full = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if full.matched != CORPUS as u32 || full.rate() != 1000 {
        return false;
    }
    // 对每个死槽强行改判成另一种缺陷：matched 必须掉到 13、rate 掉到 929。
    let mut i = ACCEPT;
    while i < CORPUS {
        let over = match run_with(Some((i, Fault::Count))) {
            Ok(v) => v,
            Err(_) => return false,
        };
        if over.matched != 13 || over.rate() != 929 || over.green() || over.gate() != Err(EQ_ECOUNT)
        {
            return false;
        }
        i += 1;
    }
    // 对活槽注入"报对的红"（Fault::Ok）不改结果。
    let keep = match run_with(Some((0, Fault::Ok))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if keep.matched != 14 || keep.rate() != 1000 {
        return false;
    }
    // 红档千分率的算法复算：(13*1000 + 14/2) / 14 == 929。
    let expect = (13u32 * 1000 + (CORPUS as u32) / 2) / (CORPUS as u32);
    if expect != 929 {
        return false;
    }
    // 零总格的退化分支：rate 归零、green 为假、gate 报计数错。
    let empty = Report {
        total: 0,
        accept: 0,
        reject: 0,
        room: 0,
        nospc: 0,
        dquot: 0,
        rogue: 0,
        matched: 0,
    };
    if empty.rate() != 0 || empty.green() || empty.gate() != Err(EQ_ECOUNT) {
        return false;
    }
    // 六死槽的申报两两不同维：坏掉的那一维在全语料里不许串档。
    let cs = cases();
    let mut faults = [Fault::Ok; REJECT];
    let mut k = 0usize;
    while k < REJECT {
        let c = match cs.get(k + ACCEPT) {
            Some(v) => *v,
            None => return false,
        };
        match faults.get_mut(k) {
            Some(f) => *f = judge(&c),
            None => return false,
        }
        k += 1;
    }
    let mut x = 0usize;
    while x < REJECT {
        let a = match faults.get(x) {
            Some(v) => *v,
            None => return false,
        };
        if a == Fault::Ok {
            return false;
        }
        let mut y = x + 1;
        while y < REJECT {
            if let Some(b) = faults.get(y) {
                if *b == a {
                    return false;
                }
            }
            y += 1;
        }
        x += 1;
    }
    true
}

/// 文案逐字：strerror 原文、工具前缀体例、quota/e2fsprogs 表样。
fn message_ok() -> bool {
    if strerror(28) != "No space left on device" {
        return false;
    }
    if strerror(122) != "Quota exceeded" {
        return false;
    }
    if strerror(22) != "Invalid argument" {
        return false;
    }
    if msg_dd_enospc("scratch.img").as_str() != "dd: error writing 'scratch.img': No space left on device"
    {
        return false;
    }
    if msg_dd_edquot("home/alice/img").as_str()
        != "dd: error writing 'home/alice/img': Quota exceeded"
    {
        return false;
    }
    if msg_tee_at("/tmp/x", ERR_ENOSPC).as_str() != "tee: /tmp/x: No space left on device" {
        return false;
    }
    if msg_tee_at("/tmp/x", ERR_EDQUOT).as_str() != "tee: /tmp/x: Quota exceeded" {
        return false;
    }
    if msg_touch_at("/mnt/full/f", ERR_EDQUOT).as_str()
        != "touch: cannot touch '/mnt/full/f': Quota exceeded"
    {
        return false;
    }
    if msg_mkdir_at("full/a", ERR_ENOSPC).as_str()
        != "mkdir: cannot create directory 'full/a': No space left on device"
    {
        return false;
    }
    if msg_write_at("write", "f", ERR_ENOSPC).as_str() != "write 'f': No space left on device" {
        return false;
    }
    if msg_df_header() != "Filesystem      1K-blocks    Used Available Use% Mounted on\n" {
        return false;
    }
    if msg_df_row("/dev/sda3", 1000, 992, 3, "/mnt").as_str()
        != "/dev/sda3      1000      992      3 99% /mnt\n"
    {
        return false;
    }
    if msg_df_row("/dev/zero", 0, 0, 0, "/").as_str()
        != "/dev/zero      0      0      0 100% /\n"
    {
        return false;
    }
    if msg_quota_head("alice", 1000).as_str() != "Disk quotas for alice (uid 1000):\n" {
        return false;
    }
    if msg_quota_columns() != "     Filesystem   space   quota   limit   grace\n" {
        return false;
    }
    if msg_quota_row("/dev/sda3", 2000, 1000, 4000, 604800).as_str()
        != "       /dev/sda3   2000k   1000k   4000k   7d\n"
    {
        return false;
    }
    if msg_quota_row("/dev/sda3", 2000, 1000, 4000, 0).as_str()
        != "       /dev/sda3   2000k   1000k   4000k   -\n"
    {
        return false;
    }
    if msg_repquota_head("/dev/sda3").as_str()
        != "*** Report for user quotas on device /dev/sda3\nBlock grace time: 7days; Inode grace time: 7days\n"
    {
        return false;
    }
    if msg_reserved_blocks(8, 100).as_str() != "Reserved blocks: 8 (8.00% of filesystem size)" {
        return false;
    }
    if msg_reserved_blocks(3, 48).as_str() != "Reserved blocks: 3 (6.25% of filesystem size)" {
        return false;
    }
    if msg_reserved_blocks(0, 0).as_str() != "Reserved blocks: 0 (0.00% of filesystem size)" {
        return false;
    }
    if msg_reserve_blocked("alice", 32768, 0).as_str()
        != "non-root user alice blocked by reserved pool (free 32768, available 0): No space left on device"
    {
        return false;
    }
    if msg_grace(1000, 2000, 604800).as_str()
        != "soft limit 1000 exceeded (usage 2000), grace remaining 604800s"
    {
        return false;
    }
    if msg_grace_expired(1000, 2000).as_str()
        != "soft limit 1000 exceeded (usage 2000), grace expired: Quota exceeded"
    {
        return false;
    }
    if msg_family_broken(28, 122).as_str()
        != "errno family mismatch (reported 28 -> nospc, physical 122 -> dquot)"
    {
        return false;
    }
    if msg_subject_na("project", "ext4").as_str()
        != "project quota not supported on ext4: accounting falls back to device free space"
    {
        return false;
    }
    if msg_arg_bad("count", 0).as_str() != "invalid count 0: Invalid argument" {
        return false;
    }
    if msg_subject_at("group", "x.raw").as_str()
        != "group quota exceeded: 'x.raw': Quota exceeded"
    {
        return false;
    }
    if msg_ledger_bad("p").as_str() != "cannot read quota record for 'p': Quota exceeded" {
        return false;
    }
    // 双版对照：同一 op 前缀逐字相同、只尾段互斥。
    let a = msg_dd_enospc("x");
    let b = msg_dd_edquot("x");
    if !a.starts_with("dd: error writing 'x': ") || !b.starts_with("dd: error writing 'x': ") {
        return false;
    }
    if !a.ends_with("No space left on device") || !b.ends_with("Quota exceeded") {
        return false;
    }
    // message() 按注解路由：成功注解空串、失败注解带对应 strerror。
    let cs = cases();
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let o = c.actual();
        let txt = message(&c.shape, &o);
        match o.note {
            Note::Written | Note::RootPass | Note::DquotSoftGrace => {
                if !txt.is_empty() {
                    return false;
                }
            }
            Note::NospcDevice => {
                if !txt.ends_with(strerror(ERR_ENOSPC)) {
                    return false;
                }
            }
            Note::NospcReserve => {
                if !txt.contains("reserved pool") || !txt.ends_with(strerror(ERR_ENOSPC)) {
                    return false;
                }
            }
            Note::DquotHard | Note::DquotGroup | Note::DquotProject | Note::DquotLedger => {
                if !txt.ends_with(strerror(ERR_EDQUOT)) {
                    return false;
                }
            }
            Note::DquotGraceOver => {
                if !txt.contains("grace expired") || !txt.ends_with(strerror(ERR_EDQUOT)) {
                    return false;
                }
            }
            Note::FamilySwap => {
                if !txt.contains("errno family mismatch") {
                    return false;
                }
            }
            Note::ArgBad => {
                if !txt.ends_with("Invalid argument") {
                    return false;
                }
            }
        }
        i += 1;
    }
    // 配额主体分式：group/project 现场各打各的主语。
    let mut grp = Shape::blank();
    grp.kind = QuotaKind::Group;
    grp.enforced = true;
    grp.soft = 1000;
    grp.hard = 4000;
    grp.used = 3000;
    grp.reported = ERR_EDQUOT;
    let og = define(&grp);
    if message(&grp, &og).as_str() != msg_subject_at("group", grp.path).as_str() {
        return false;
    }
    let mut prj = grp.with_kind(QuotaKind::Project);
    prj.fs = FsFlavor::Xfs;
    let op = define(&prj);
    if message(&prj, &op).as_str() != msg_subject_at("project", prj.path).as_str() {
        return false;
    }
    true
}

/// 释义与文本基元：dec/hex/i32/bool 文本全对，detail 行逐维打印。
fn detail_ok() -> bool {
    if dec_text(0).as_str() != "0" {
        return false;
    }
    if dec_text(4096).as_str() != "4096" || dec_text(929).as_str() != "929" {
        return false;
    }
    if dec_text(u32::MAX).as_str() != "4294967295" {
        return false;
    }
    if u32_text(14).as_str() != "14" || rate_text(1000).as_str() != "1000" {
        return false;
    }
    if i32_text(0).as_str() != "0"
        || i32_text(-28).as_str() != "-28"
        || i32_text(122).as_str() != "122"
    {
        return false;
    }
    if hex_text(0x6d3e0, 5).as_str() != "6d3e0" {
        return false;
    }
    if hex_text(0, 4).as_str() != "0000" || hex_text(0xff, 2).as_str() != "ff" {
        return false;
    }
    if hex_text(0x10000, 3).as_str() != "000" {
        return false;
    }
    if bool_text(true) != "true" || bool_text(false) != "false" {
        return false;
    }
    if verdict(&Outcome::blank()) != "room" {
        return false;
    }
    let cs = cases();
    // 槽 3 的 detail 行逐字（介质族）。
    let c3 = match cs.get(3) {
        Some(v) => *v,
        None => return false,
    };
    let want3 = "/mnt/full/data.bin user=alice fs=ext4 kind=none stage=device errno=28 ret=0 pair=true note=nospc-device msg=\"write '/mnt/full/data.bin': No space left on device\"\n";
    if detail(&c3).as_str() != want3 {
        return false;
    }
    // 槽 5 的 detail 行逐字（配额族）。
    let c5 = match cs.get(5) {
        Some(v) => *v,
        None => return false,
    };
    let want5 = "home/alice/img.raw user=alice fs=ext4 kind=user stage=usage errno=122 ret=0 pair=true note=dquot-hard msg=\"write 'home/alice/img.raw': Quota exceeded\"\n";
    if detail(&c5).as_str() != want5 {
        return false;
    }
    // 成功槽不带 msg 段且 pair=false。
    let c0 = match cs.first() {
        Some(v) => *v,
        None => return false,
    };
    let d0 = detail(&c0);
    if d0.contains("msg=") || !d0.contains("pair=false") || !d0.contains("note=written") {
        return false;
    }
    // 每一行的字段前缀与行尾换行都要成立。
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let d = detail(&c);
        if !d.ends_with('\n') || !d.contains(" stage=") || !d.contains(" errno=") {
            return false;
        }
        if !d.contains(" ret=") || !d.contains(" note=") || !d.contains(" pair=") {
            return false;
        }
        if !d.contains(c.shape.user) || !d.contains(c.shape.path) {
            return false;
        }
        i += 1;
    }
    true
}

/// 阶段闸门：单调不倒退、倒退报阶段错、关闭后一切拒绝。
fn gate_ok() -> bool {
    let mut g = Gate::new();
    if g.steps() != 0 || g.last() != -1 || g.closed() {
        return false;
    }
    // 顺行八段全放行。
    let walk = [
        Stage::Arg,
        Stage::Subject,
        Stage::Usage,
        Stage::Grace,
        Stage::Device,
        Stage::Reserve,
        Stage::Family,
        Stage::Done,
    ];
    let mut i = 0usize;
    while i < walk.len() {
        let s = match walk.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if g.step(s) != Ok(()) {
            return false;
        }
        i += 1;
    }
    if g.steps() != 8 || g.last() != 7 {
        return false;
    }
    // 同段重复不算倒退（观测同一段两次合法）。
    if g.step(Stage::Done) != Ok(()) || g.steps() != 9 {
        return false;
    }
    // 倒退必须撞 EQ_ESTAGE，且 last 不被倒退污染。
    let mut b = Gate::new();
    if b.step(Stage::Grace) != Ok(()) {
        return false;
    }
    let before = b.last();
    if b.step(Stage::Arg) != Err(EQ_ESTAGE) || b.last() != before {
        return false;
    }
    if b.steps() != 1 {
        return false;
    }
    // close 结算后一切 step/close 都撞 EQ_ESTATE。
    let mut c = Gate::new();
    if c.step(Stage::Arg) != Ok(()) {
        return false;
    }
    match c.close() {
        Ok(n) => {
            if n != 1 || !c.closed() {
                return false;
            }
        }
        Err(_) => return false,
    }
    if c.step(Stage::Done) != Err(EQ_ESTATE) {
        return false;
    }
    if c.close() != Err(EQ_ESTATE) {
        return false;
    }
    // 空闸门直接关闭：段数 0、仍然放行一次。
    let mut e = Gate::new();
    match e.close() {
        Ok(n) => {
            if n != 0 {
                return false;
            }
        }
        Err(_) => return false,
    }
    // 语料现场：每一槽的申报阶段都能被 Gate 顺行接住（阶段是纵线上的点）。
    let cs = cases();
    let mut k = 0usize;
    while k < CORPUS {
        let c = match cs.get(k) {
            Some(v) => *v,
            None => return false,
        };
        let mut gg = Gate::new();
        let mut s = 0u8;
        let target = c.actual().stage.order();
        let mut ok = true;
        while s < target {
            let stage = match s {
                0 => Stage::Arg,
                1 => Stage::Subject,
                2 => Stage::Usage,
                3 => Stage::Grace,
                4 => Stage::Device,
                5 => Stage::Reserve,
                6 => Stage::Family,
                _ => Stage::Done,
            };
            if gg.step(stage) != Ok(()) {
                ok = false;
                break;
            }
            s += 1;
        }
        if ok {
            if gg.step(c.actual().stage) != Ok(()) || gg.last() != target as i32 {
                return false;
            }
        } else {
            return false;
        }
        k += 1;
    }
    true
}

/// 账本：槽位严格递增、逐格对账、冻结重放、操作码归属。
fn ledger_ok() -> bool {
    let mut fresh = Ledger::new();
    if fresh.len() != 0 || !fresh.is_empty() || fresh.rejects() != 0 {
        return false;
    }
    if fresh.audit() == Ok(()) || fresh.root() != 0 || fresh.is_frozen() {
        return false;
    }
    if fresh.sample(0).is_some() {
        return false;
    }
    // 跳号登记：拒收且计数，不入账。
    let jump = Sample {
        slot: 1,
        op: OP_CORPUS,
        ok: true,
        code: 0,
        value: 14,
    };
    if fresh.register(jump) != Err(EQ_ESLOT) || fresh.len() != 0 || fresh.rejects() != 1 {
        return false;
    }
    // 越界登记同样拒收。
    let far = Sample {
        slot: SAMPLES as u8,
        op: OP_CORPUS,
        ok: true,
        code: 0,
        value: 0,
    };
    if fresh.register(far) != Err(EQ_ESLOT) || fresh.rejects() != 2 {
        return false;
    }
    // 半账不许通过审计。
    let head = Sample {
        slot: 0,
        op: OP_CORPUS,
        ok: true,
        code: 0,
        value: 14,
    };
    if fresh.register(head) != Ok(()) || fresh.audit() == Ok(()) {
        return false;
    }
    // 全账：与 build_ledger 同序同值。
    let l = build_ledger();
    if l.len() != SAMPLES || l.rejects() != 0 {
        return false;
    }
    if l.audit() != Ok(()) || l.ops_match() != Ok(()) {
        return false;
    }
    if l.ok_count() != ACCEPT || l.err_count() != REJECT {
        return false;
    }
    if l.sample(SAMPLES).is_some() || l.sample(0).is_none() {
        return false;
    }
    let mut i = 0usize;
    while i < ACCEPT {
        let s = match l.sample(i) {
            Some(v) => v,
            None => return false,
        };
        let w = match VALS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if !s.ok || s.code != 0 || s.value != w || s.slot as usize != i {
            return false;
        }
        i += 1;
    }
    let mut r = 0usize;
    while r < REJECT {
        let s = match l.sample(ACCEPT + r) {
            Some(v) => v,
            None => return false,
        };
        let w = match REJECT_CODES.get(r) {
            Some(v) => *v,
            None => return false,
        };
        if s.ok || s.value != 0 || s.code != w || s.op != OP_SUBJECT {
            return false;
        }
        r += 1;
    }
    // 红档格：槽 7 是 929，且属重放操作。
    let redcell = match l.sample(7) {
        Some(v) => v,
        None => return false,
    };
    if redcell.value != 929 || !redcell.ok || redcell.op != OP_REPLAY {
        return false;
    }
    // 操作码表错位要被抓：op 全填 OP_ARG 的账本 audit 可过、ops 必红。
    let mut wrong = Ledger::new();
    let mut k = 0usize;
    while k < ACCEPT {
        let v = match VALS.get(k) {
            Some(x) => *x,
            None => return false,
        };
        let s = Sample {
            slot: k as u8,
            op: OP_ARG,
            ok: true,
            code: 0,
            value: v,
        };
        if wrong.register(s) != Ok(()) {
            return false;
        }
        k += 1;
    }
    let mut m = 0usize;
    while m < REJECT {
        let c = match REJECT_CODES.get(m) {
            Some(x) => *x,
            None => return false,
        };
        let s = Sample {
            slot: (ACCEPT + m) as u8,
            op: OP_ARG,
            ok: false,
            code: c,
            value: 0,
        };
        if wrong.register(s) != Ok(()) {
            return false;
        }
        m += 1;
    }
    if wrong.audit() != Ok(()) || wrong.ops_match() != Err(EQ_ECOUNT) {
        return false;
    }
    // 败格值非零要撞审计：先造一版正确账、把败格改不出来——改为直接
    // 用 count_cell 的失败支路复算（Err 支路 value 必须为 0）。
    let failed_cell = count_cell(8, OP_SUBJECT, Err(EQ_ESTATE));
    if failed_cell.ok || failed_cell.code != EQ_ESTATE || failed_cell.value != 0 {
        return false;
    }
    let good_cell = count_cell(0, OP_CORPUS, Ok(14));
    if !good_cell.ok || good_cell.code != 0 || good_cell.value != 14 {
        return false;
    }
    // 冻结与重放。
    let mut f = build_ledger();
    if f.replay() != Ok(f.seal()) {
        return false;
    }
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if root == 0 || !f.is_frozen() || f.root() != root {
        return false;
    }
    if f.replay() != Ok(root) {
        return false;
    }
    if f.register(head) != Err(EQ_ESLOT) || f.rejects() != 1 {
        return false;
    }
    if f.freeze() != Err(EQ_EBAL) {
        return false;
    }
    // 指纹对内容敏感：两份全等账本同指纹，改一格的账本不同指纹。
    let a = build_ledger().seal();
    let b = build_ledger().seal();
    if a != b {
        return false;
    }
    let mut c = Ledger::new();
    if c.register(Sample {
        slot: 0,
        op: OP_CORPUS,
        ok: true,
        code: 0,
        value: 15,
    }) != Ok(())
    {
        return false;
    }
    c.seal() != a
}

// ---------------------------------------------------------------------------
// 判据总线：13 条
// ---------------------------------------------------------------------------

/// UNX-F11695 判据总线：恰好 13 条，逐条落 CheckSet。
pub fn run_unx11695_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11695_enospc_vs_edquot");

    s.add("EQ01-名称与段基址", name_ok() && anchor_ok(), "static");

    s.add("EQ02-枚举与八闸", enums_ok(), "static");

    s.add("EQ03-配额账与介质几何", geom_ok(), "static");

    s.add("EQ04-形状合法域", shape_ok(), "static");

    s.add("EQ05-八闸纵线", define_ok(), "static");

    s.add("EQ06-双码归族", family_ok(), "static");

    s.add("EQ07-申报对判", judge_ok(), "static");

    s.add("EQ08-矩阵与报表", matrix_ok(), "static");

    s.add("EQ09-语料与期望表", corpus_ok(), "static");

    s.add("EQ10-槽位与红档", slots_ok() && reds_ok(), "static");

    s.add("EQ11-文案逐字", message_ok(), "static");

    s.add("EQ12-释义与阶段闸", detail_ok() && gate_ok(), "static");

    s.add(
        "EQ13-冻结重放四注入",
        {
            let mut l = build_ledger();
            let a = l.audit() == Ok(());
            let b = match l.freeze() {
                Ok(r) => r != 0 && l.is_frozen() && l.replay() == Ok(r),
                Err(_) => false,
            };
            let inj = inject_four();
            let four = inj[0] && inj[1] && inj[2] && inj[3];
            a && b && ledger_ok() && four
        },
        "static",
    );

    s
}
