//! UNX-F11692 du 在满盘上的统计判据
//!
//! 这张卡管的是满盘现场里唯一一个**天生不该红**的命令：`du`。盘满（ENOSPC=28）
//! 打的是分配面——`write()`、`mkdir()`、块预留、日志提交；而 `du` 只做一件事：
//! 遍历目录树、`lstat()` 每个条目、把 `st_blocks` 折成块数、沿树往上累加。
//! 它一次都不分配，所以 **"满盘统计正常" 就是本卡的 J1 正判据**：数据块全耗尽
//! 的那一秒，`du` 照样把每个目录的用量报得一字不差。本卡判的不是"du 会不会被
//! 满盘打倒"（不会），而是四件互相咬合、任一顶不上就必有一假的事：
//!
//! * **读面与写面的责任切分**：du 自己从不产生 ENOSPC，唯一能让 `No space left
//!   on device` 出现在 du 的 stderr 上的通道是**输出面**——`du > report.txt`
//!   把统计文本写进满盘。此时 GNU du 打
//!   `du: standard output: No space left on device`：统计全量完成、投递在终点
//!   失败。把这一行认成"统计失败"（读面谎报）与把满盘当成 du 会失败的理由，
//!   是本卡一正一反两条最贵的死槽。
//! * **硬链接只计一次**：同 (dev, ino) 第二次出现，POSIX 口径贡献 0 字节——
//!   `du` 默认去重，`-l` 关掉去重。申报里把第二个链接再计一份 4096，就是
//!   虚增用量；把"第二链接报 0"读成"文件不见了"，就是成败维谎报。
//! * **块取整口径**：POSIX 的账本是 `st_blocks × 512`，GNU du 默认再按文件系统
//!   块大小向上取整、输出按单位（默认 1024）向上取整。1 字节的文件在 4K 块上
//!   报 4096；稀疏文件 apparent 1 MiB、`st_blocks=8` 只报 4096；
//!   `--apparent-size` 整个绕开取整。三套数（apparent / st_blocks×512 /
//!   取整后输出）各归各闸，混用即 Value 维判死。
//! * **遍历错误不是空间错误**：`du: cannot access 'x': Permission denied`
//!   （EACCES）、`du: cannot access 'x': No such file or directory`（ENOENT，
//!   竞态删除）、Input/output error（EIO）都让 du 退 1，却与 28 无涉；
//!   用法错退 2，一个条目都不 stat。errno 家族认错（Code 维）意味着处置方向
//!   整个错掉：EACCES 加权限就好，换目录救不了盘满，而 du 根本不碰盘满。
//!
//! ## 闸门次序（这条纵线为什么不能倒）
//!
//! 九道闸，秩从 1 起算；前一闸不产出事实，后一闸就没有输入。`Gate` 只允许
//! 单调前进，倒退即 `DU_STAGE`，封印（`close()`）之后一切 `step`/`close` 拒动
//! 给 `DU_STATE`。
//!
//! 1. 调用闸（Call，秩 1）——命令行本身。非法选项、缺操作数在这里判死，
//!    退 2，一个 `lstat()` 都没发生。它排最前是为了钉死：用法错永远不许
//!    被记成统计错误，更不许被记成 ENOSPC。
//! 2. 遍历闸（Walk，秩 2）——`opendir`/`readdir`。目录读不动（EACCES/EIO）
//!    在这里定性；错误条目被跳过、累加继续，这是 du 与"写入即中止"的根本
//!    不同——统计面是**尽力而为**的。
//! 3. 定位闸（Stat，秩 3）——逐条目 `lstat()`。ENOENT 竞态（列完目录条目被
//!    人删）在这一闸出 `cannot access 'x': No such file or directory`。
//!    stat 不成，后面四闸对这个条目全部作废。
//! 4. 定性闸（Type，秩 4）——文件类型分档：普通件/目录/符号链接/伪文件/
//!    设备件。`st_size` 只对普通件有意义，伪文件（procfs）的大小是谎言；
//!    符号链接默认不计。类型定错，块数算得再准也是零。
//! 5. 去重闸（Dedup，秩 5）——(dev, ino) 首次出现计块数，再出现计 0；
//!    `-l` 关掉去重。这一闸排在 Type 之后：先确定"这是个要计量的件"，
//!    才谈得上"这个 inode 计没计过"。
//! 6. 取整闸（Round，秩 6）——`st_blocks×512` → 按文件系统块大小向上取整 →
//!    按输出单位向上取整；`--apparent-size` 时换 apparent 口径。三套数在这一
//!    闸合出**申报面上那一个数**，错一步 Value 维就崩。
//! 7. 累加闸（Total，秩 7）——子树用量折进父目录（自底向上）；`-s` 只留总量。
//!    本卡语料以单条目立账：value 即该条目的贡献数。
//! 8. 投递闸（Print，秩 8）——把统计行写成输出；同时是**错误行的回执面**：
//!    遍历侧错误在这一闸打 `cannot access` 式，输出侧 ENOSPC 在这一闸打
//!    `standard output:` 式。落点在这闸的败局只有两种身份，认错即 Note 维死。
//! 9. 结算闸（Exit，秩 9）——折算退出码：0=统计全绿（满盘与否无关）；
//!    1=任一条目出错或输出投递失败；2=用法错。纵线在此闭合，随后封印。
//!
//! ## 关键差异（本卡深度所在）
//!
//! - 与 F11691（fsync 面与 ENOSPC，段基址 `0x6d390`，前置卡，位距 `0x10`）的
//!   边界：fsync 卡判**写面与同步面的时间差**——延迟分配把 ENOSPC 押到 fsync
//!   交付；本卡判**根本没有分配的那一面**：du 不写、不同步、不分配，满盘对它的
//!   统计面零影响，ENOSPC 若出现在 du 的行里，只可能是它自己的 stdout 写不下。
//!   两卡同段族、同 errno（28）、同 strerror 串，但纵线方向相反：那是"错误何时
//!   浮出"，这是"错误可否被凭空安到读面上"。
//! - 与 F11681（ENOSPC 总锚，段基址 `0x6d130`，位距 `0x270`）的边界：总锚判
//!   `write() = -1 + errno 28` 的即时配对；本卡的读面**永远不会**产出这对配对，
//!   本卡反过来用总锚的口径证明：把 `No space left on device` 安在统计动作上
//!   就是谎报——统计不分配，分配才有 28。
//! - 与 F11689（df 0 avail 呈现，段基址 `0x6d360`，位距 `0x40`）的边界：df 与
//!   du 都是只读旁观者，df 报**文件系统池子**（avail），du 报**树里占了多少**。
//!   df 的 avail 会被保留块/quota 骗，du 的块数会被 apparent 骗——本卡只管 du
//!   的三套数（apparent / st_blocks×512 / 取整输出）与去重，df 的呈现面不归
//!   本卡裁。
//! - 与 F11688（touch/mkdir 建件，段基址 `0x6d350`）的边界：建件卡判"失败之后
//!   盘上多出什么"（残留）；本卡判"统计之后盘上没多出任何事"——du 是无副作用
//!   观测，残留维在本卡恒为空集。把 du 的报错读成"盘上少了/多了什么"即越界。
//! - 去重 ≠ 丢失：硬链接第二链接贡献 0 是**正确统计**，不是"文件不见了"
//!   （Estate 维谎报的靶子）；`-l` 下的双计才是口径差异，且只动 Dedup 一闸。
//! - 红档率 929 独占账本一格：14 格恰中 13 的千分率 `(13*1000 + 14/2)/14 = 929`，
//!   是比率标定，不许并进 7 个统计格。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 本模块段位基址（全仓唯一，取自分配表，不许改）。
pub const SEG_BASE: u32 = 0x6d3a0;

/// 交叉署名：F11691「fsync 面与 ENOSPC」的段基址（本卡的直接前置卡）。
/// 本卡**不 import** 那个模块的任何符号，只把它的段位与位距原样声明一遍，
/// 由 `anchors_agree()` 复算对齐。
pub const ANCHOR_F11691_SEG: u32 = 0x6d390;
/// F11691 与本卡的位距（0x6d3a0 - 0x6d390 = 0x10）。
pub const ANCHOR_F11691_GAP: u32 = 0x10;
/// 交叉署名：F11681「ENOSPC 总锚」的段基址。
pub const ANCHOR_F11681_SEG: u32 = 0x6d130;
/// F11681 与本卡的位距（0x6d3a0 - 0x6d130 = 0x270）。
pub const ANCHOR_F11681_GAP: u32 = 0x270;
/// 交叉署名：F11689「df 0 avail 呈现」的段基址（同为只读旁观者的另一张卡）。
pub const ANCHOR_F11689_SEG: u32 = 0x6d360;
/// 本卡与 F11689 的位距（0x6d3a0 - 0x6d360 = 0x40）。
pub const ANCHOR_F11689_GAP: u32 = 0x40;

/// 与 F11681 总锚在 errno 面上的署名：ENOSPC 在 Linux 就是 28，两条卡同数。
pub const ANCHOR_ENOSPC: u32 = 28;
/// 与 F11694 文案卡在 strerror 面上的署名：本卡所有空间报错尾巴只能是这一串。
pub const ANCHOR_ERRNO_TEXT: &str = "No space left on device";

/// 错误码 01：槽位越界/跳号——`run_with` 注入槽超出语料、账本登记 `slot != len`
/// （统计面最容不下"漏一格"，登记跳号就是漏计的账上投影）。
pub const DU_SLOT: u32 = SEG_BASE + 0x01;
/// 错误码 02：状态违例——账本已冻结还登记、闸门已封印还 step/close，
/// 全绿运行里出现未命中槽；也是 Estate 维（投递失败谎报成全绿）的出口码。
pub const DU_STATE: u32 = SEG_BASE + 0x02;
/// 错误码 03：身份撞格——错误码字面量重复、段位与锚位距不闭合，
/// 以及"同一条目的两行输出"被误记成两个条目。
pub const DU_UNIQ: u32 = SEG_BASE + 0x03;
/// 错误码 04：阶段倒退/提前封印——申报落点与本卡纵线产出不合（Stage 维）：
/// 用法错被记在投递闸、统计完成被记在调用闸，都从这里出门。
pub const DU_STAGE: u32 = SEG_BASE + 0x04;
/// 错误码 05：errno 家族判错——EACCES/ENOENT 被安成 ENOSPC，或反过来
/// （Code 维）。读面没有 28，是本卡最硬的一条家族纪律。
pub const DU_CODE: u32 = SEG_BASE + 0x05;
/// 错误码 06：数值维不合——去重漏计/双计、块取整口径错、apparent 混进
/// 块账（Value 维）。三套数只有取整后的那一套能出现在申报里。
pub const DU_VALUE: u32 = SEG_BASE + 0x06;
/// 错误码 07：文案维不合——`standard output:` 式认成 `cannot access` 式，
/// 或反过来（Note 维）。两式的动词、面别、处置全部不同。
pub const DU_NOTE: u32 = SEG_BASE + 0x07;
/// 错误码 08：兜底档误走——事实面本可判定（比如伪文件计 0）却被丢进
/// "未知工具/未知条目"垃圾桶（Default 维）。
pub const DU_FAULT_DEFAULT: u32 = SEG_BASE + 0x08;
/// 错误码 09：用法与退出码档位不合——非法选项/缺操作数，退 2，
/// 一个条目都没 stat。调用闸的专属出口码。
pub const DU_EUSAGE: u32 = SEG_BASE + 0x09;
/// 错误码 10：输出面 ENOSPC——统计全量完成、`du > report` 投递到满盘失败。
/// 本卡唯一合法的 28 出处，`du: standard output:` 式的署名码。
pub const DU_WROUT: u32 = SEG_BASE + 0x0A;
/// 错误码 11：遍历侧错误——EACCES/ENOENT/EIO 家族跳格：条目没量到、
/// 累加继续、退 1，与空间无涉。
pub const DU_SKIP: u32 = SEG_BASE + 0x0B;
/// 错误码 12：去重维——(dev, ino) 二次计入或首计漏计。du 的账本身份码，
/// 语料的 link-first/link-second 两槽共同署名。
pub const DU_LINK: u32 = SEG_BASE + 0x0C;
/// 错误码 13：取整维——`st_blocks×512`、文件系统块、输出单位三段取整
/// 任一段算错。du 的口径身份码，稀疏与 apparent 两槽共同署名。
pub const DU_ROUND: u32 = SEG_BASE + 0x0D;

/// 全部 13 个错误码的枚举表（供互异性与值域复核，顺序即 0x01..0x0D）。
pub const ALL_CODES: [u32; 13] = [
    DU_SLOT,
    DU_STATE,
    DU_UNIQ,
    DU_STAGE,
    DU_CODE,
    DU_VALUE,
    DU_NOTE,
    DU_FAULT_DEFAULT,
    DU_EUSAGE,
    DU_WROUT,
    DU_SKIP,
    DU_LINK,
    DU_ROUND,
];

/// 错误码总数（本卡固定 13）。
pub const CODE_COUNT: usize = 13;

/// 语料槽数：8 个活形态 + 6 个死形态。
pub const CORPUS: usize = 14usize;
/// 账本登记样本数（与语料同宽，一格不落）。
pub const SAMPLES: usize = 14usize;
/// 活形态数（全绿统计/跳格/投递败/用法各档的正例）。
pub const ACCEPT: usize = 8usize;
/// 死形态数（一槽只坏一维，依次挂 Estate/Stage/Code/Value/Note/Default）。
pub const REJECT: usize = 6usize;
/// 档位维数（统计全绿 / 遍历跳格 / 投递败局 / 用法出局）。
pub const TIER_COUNT: usize = 4usize;
/// 纵线闸数（调用闸到结算闸）。
pub const STAGE_COUNT: u32 = 9u32;

/// 条目类型数：普通件/目录/符号链接/伪文件/设备件。
pub const OBJ_KIND_COUNT: u32 = 5u32;
/// 遍历侧结局数：无障碍/权限拒/消失/IO 错。
pub const TRAVERSE_COUNT: u32 = 4u32;
/// 参与退出的码位（0/1/2）。
pub const EXIT_RANKS: u32 = 3u32;

/// Linux errno：ENOSPC = 28，设备/文件系统无空间——只许出现在输出面。
pub const ENOSPC: u32 = 28;
/// Linux errno：EACCES = 13，权限不足——遍历闸的第一常客。
pub const EACCES: u32 = 13;
/// Linux errno：ENOENT = 2，条目消失（列目录后被删的竞态）。
pub const ENOENT: u32 = 2;
/// Linux errno：EIO = 5，底层 IO 错——坏盘读统计都会碰上，但不是 28。
pub const EIO: u32 = 5;

/// du 的退出码：统计全绿（满盘与否无关）。
pub const EXIT_OK: u32 = 0;
/// du 的退出码：任一条目出错或输出投递失败。
pub const EXIT_FAIL: u32 = 1;
/// du 的退出码：用法/参数错，一个条目都没 stat。
pub const EXIT_USAGE: u32 = 2;

/// POSIX 账本基础单位：`st_blocks` 的计量单位是 512 字节，与块大小无关。
pub const POSIX_BLOCK: u32 = 512;
/// GNU du 默认输出单位（1024 字节向上取整）。
pub const GNU_UNIT: u32 = 1024;
/// 本卡语料的典型文件系统块大小（ext4 默认 4K）。
pub const FS_BLOCK: u32 = 4096;
/// 稀疏槽的表观大小（1 MiB）：apparent 与实占的教科书写差异。
pub const SPARSE_APPARENT: u32 = 1_048_576;

/// 账本格类型 1：统计格（前 7 格）。
pub const OP_STAT: u8 = 1;
/// 账本格类型 2：红档格（第 8 格，值必须是 929）。
pub const OP_RATE: u8 = 2;
/// 账本格类型 3：缺陷码格（后 6 格，值必须为 0）。
pub const OP_DEFECT: u8 = 3;

/// FNV-1a 64 位初始偏置。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64 位素数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 红档率标定：14 格里恰中 13 格时的千分率 `(13*1000 + 14/2)/14 = 929`。
pub const RED_RATE: u32 = 929;
/// 全绿千分率。
pub const RATE_FULL: u32 = 1000;

/// FNV-1a 折叠一个字节。
pub fn fold_byte(h: u64, b: u8) -> u64 {
    let x = h ^ (b as u64);
    x.wrapping_mul(FNV_PRIME)
}

/// FNV-1a 折叠一个字（u32 的四个字节，高位先折——次序即身份）。
pub fn fold_word(h: u64, v: u32) -> u64 {
    let mut x = h;
    let mut k: u32 = 0;
    while k < 4 {
        let shift = 8 * (3 - k);
        let byte = ((v >> shift) & 0xff) as u8;
        x = fold_byte(x, byte);
        k += 1;
    }
    x
}

/// FNV-1a 折叠一个 64 位宽量（先高字后低字：账本单格指纹要整体入折）。
pub fn fold_u64(h: u64, v: u64) -> u64 {
    let hi = (v >> 32) as u32;
    let lo = (v & 0xffff_ffff) as u32;
    let x = fold_word(h, hi);
    fold_word(x, lo)
}

/// FNV-1a 折叠一段布尔（本卡把 ok 位当成独立一维参与指纹）。
pub fn fold_bool(h: u64, v: bool) -> u64 {
    fold_byte(h, if v { 0x01 } else { 0x00 })
}

/// FNV-1a 折叠一段文本（含终止标记，防止 "ab"+"c" 与 "a"+"bc" 同指纹）。
pub fn fold_str(h: u64, s: &str) -> u64 {
    let mut x = h;
    let mut i: usize = 0;
    while i < s.len() {
        match s.as_bytes().get(i) {
            Some(b) => x = fold_byte(x, *b),
            None => break,
        }
        i += 1;
    }
    fold_byte(x, 0xff)
}

/// u32 → 十进制文本，逐位展开后倒序拼接，不用递归。
pub fn dec_text(v: u32) -> String {
    let mut buf = [b'0'; 12];
    let mut i: usize = 12;
    let mut n = v;
    loop {
        if i == 0 {
            break;
        }
        i = i - 1;
        let digit = (n % 10) as u8;
        match buf.get_mut(i) {
            Some(slot) => *slot = b'0' + digit,
            None => {}
        }
        n = n / 10;
        if n == 0 {
            break;
        }
    }
    let mut out = String::new();
    let mut k = i;
    while k < 12 {
        match buf.get(k) {
            Some(b) => out.push(*b as char),
            None => {}
        }
        k += 1;
    }
    out
}

/// u32 → 十六进制文本，宽度钳到 0..=8，不足位补零（小写，贴段位习惯）。
pub fn hex_text(v: u32, width: usize) -> String {
    let w = if width > 8 { 8 } else { width };
    let digits: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::new();
    let mut k: usize = 0;
    while k < w {
        let shift: u32 = (4 * (w - 1 - k)) as u32;
        let nib = ((v >> shift) & 0x0f) as usize;
        match digits.get(nib) {
            Some(b) => out.push(*b as char),
            None => {}
        }
        k += 1;
    }
    out
}

/// u32 文本（正数无前缀，直接走 dec_text）。
pub fn u32_text(v: u32) -> String {
    dec_text(v)
}

/// i32 文本（负号 + 绝对值；MIN 的绝对值走 unsigned_abs，不溢出）。
pub fn i32_text(v: i32) -> String {
    if v < 0 {
        let mut s = String::new();
        s.push('-');
        s.push_str(dec_text(v.unsigned_abs()).as_str());
        s
    } else {
        dec_text(v as u32)
    }
}

/// 布尔文本。
pub fn bool_text(v: bool) -> String {
    let mut s = String::new();
    s.push_str(if v { "true" } else { "false" });
    s
}

/// 段位文本：`0x6d3a0` 形态（本卡所有段位都以此形制出现，便于跨卡比对）。
pub fn seg_text(v: u32) -> String {
    let mut s = String::new();
    s.push_str("0x");
    s.push_str(hex_text(v, 6).as_str());
    s
}

/// 把一串键值拼成 `k=v` 形式接到 out 后面（渲染共用）。
pub fn kv(out: &mut String, key: &str, val: u32) {
    out.push_str(key);
    out.push('=');
    out.push_str(dec_text(val).as_str());
}

/// 追加一个分隔空格。
pub fn sep(out: &mut String) {
    out.push(' ');
}

/// 向上取整到 unit 的整数倍（unit 为 0 视同 1，不除零）。
/// du 的取整闸三步里最后一步就是它：`round_up(bytes, 1024)`。
pub fn round_up(bytes: u32, unit: u32) -> u32 {
    if unit == 0 || unit == 1 {
        return bytes;
    }
    let q = bytes / unit;
    let r = bytes % unit;
    let mut v = q.saturating_mul(unit);
    if r != 0 {
        v = v.saturating_add(unit);
    }
    v
}

/// 是否 2 的幂（块大小与输出单位的合法域判据；0 不是）。
pub fn is_pow2(v: u32) -> bool {
    v != 0 && (v & (v - 1)) == 0
}

/// 块大小口径：POSIX 账本量 `st_blocks × 512`；零块的稀疏/伪文件保持 0
/// （这正是 du 报 0 的物理原因），非空条目至少占一个文件系统块。
pub fn usage_bytes(blocks512: u32, fs_block: u32) -> u32 {
    if blocks512 == 0 {
        return 0;
    }
    let raw = blocks512.saturating_mul(POSIX_BLOCK);
    if fs_block > raw {
        fs_block
    } else {
        round_up(raw, fs_block)
    }
}

/// 本卡纵线阶段（九道闸）。秩从 1 起算；0 保留给"未起跑"的初值，
/// 只有 `Gate::new()` 才处于 0，任何真实产出的 Outcome 必须落在 1..=9。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// 秩 1：调用闸——命令行本身，非法选项在此判死，退 2。
    Call,
    /// 秩 2：遍历闸——opendir/readdir，目录级 EACCES/EIO 在此定性。
    Walk,
    /// 秩 3：定位闸——逐条目 lstat()，ENOENT 竞态在此出局。
    Stat,
    /// 秩 4：定性闸——文件类型分档；st_size 只对普通件有意义。
    Type,
    /// 秩 5：去重闸——(dev, ino) 首现计量、再现计 0；-l 关闭。
    Dedup,
    /// 秩 6：取整闸——st_blocks×512 → 文件系统块 → 输出单位，三段合一。
    Round,
    /// 秩 7：累加闸——子树折进父目录，自底向上；-s 只留总量。
    Total,
    /// 秩 8：投递闸——统计行写出；遍历侧错误行与输出侧 28 都在这闸回执。
    Print,
    /// 秩 9：结算闸——退出码折算，纵线跑完的落点。
    Exit,
}

impl Stage {
    /// 全部九道闸，按纵线次序。
    pub const ALL: [Stage; 9] = [
        Stage::Call,
        Stage::Walk,
        Stage::Stat,
        Stage::Type,
        Stage::Dedup,
        Stage::Round,
        Stage::Total,
        Stage::Print,
        Stage::Exit,
    ];

    /// 秩（1 起算）。
    pub fn rank(&self) -> u32 {
        match self {
            Stage::Call => 1,
            Stage::Walk => 2,
            Stage::Stat => 3,
            Stage::Type => 4,
            Stage::Dedup => 5,
            Stage::Round => 6,
            Stage::Total => 7,
            Stage::Print => 8,
            Stage::Exit => 9,
        }
    }

    /// 短名（渲染与判据名共用）。
    pub fn slug(&self) -> &'static str {
        match self {
            Stage::Call => "call",
            Stage::Walk => "walk",
            Stage::Stat => "stat",
            Stage::Type => "type",
            Stage::Dedup => "dedup",
            Stage::Round => "round",
            Stage::Total => "total",
            Stage::Print => "print",
            Stage::Exit => "exit",
        }
    }

    /// 中文名（自检明细与文档共用）。
    pub fn cn(&self) -> &'static str {
        match self {
            Stage::Call => "调用闸",
            Stage::Walk => "遍历闸",
            Stage::Stat => "定位闸",
            Stage::Type => "定性闸",
            Stage::Dedup => "去重闸",
            Stage::Round => "取整闸",
            Stage::Total => "累加闸",
            Stage::Print => "投递闸",
            Stage::Exit => "结算闸",
        }
    }

    /// 由秩还原阶段；秩 0 与秩 >9 都不合法（未起跑不是阶段）。
    pub fn from_rank(r: u32) -> Option<Stage> {
        match r {
            1 => Some(Stage::Call),
            2 => Some(Stage::Walk),
            3 => Some(Stage::Stat),
            4 => Some(Stage::Type),
            5 => Some(Stage::Dedup),
            6 => Some(Stage::Round),
            7 => Some(Stage::Total),
            8 => Some(Stage::Print),
            9 => Some(Stage::Exit),
            _ => None,
        }
    }

    /// 下一道闸；结算闸没有下一道。
    pub fn next(&self) -> Option<Stage> {
        Stage::from_rank(self.rank().saturating_add(1))
    }

    /// 上一道闸；调用闸没有上一道——纵线不允许回退，这个方法只为
    /// `Gate` 的拒动诊断存在，判据本身绝不调用它前进。
    pub fn prev(&self) -> Option<Stage> {
        let r = self.rank();
        if r <= 1 {
            None
        } else {
            Stage::from_rank(r.saturating_sub(1))
        }
    }

    /// 是否属于 `define` 能真实产出的落点（Call/Print/Exit 三点）。
    /// 本卡事实面只会停在三处：调用即拒、错误行回执、纵线跑完结算。
    pub fn is_landing(&self) -> bool {
        matches!(self, Stage::Call | Stage::Print | Stage::Exit)
    }
}

/// 条目类型——定性闸（秩 4）的分档依据。类型错了，后面的去重与取整
/// 全都建立在错误的对象上。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjKind {
    /// 秩 1：普通文件——块账与 apparent 账的唯一合法主体。
    Plain,
    /// 秩 2：目录——自身占块（本卡语料把条目与子树总量合并立账），要下潜。
    Dir,
    /// 秩 3：符号链接——du 默认不跟随、不计其目标，链接自身也不进块账。
    Symlink,
    /// 秩 4：伪文件（procfs/sysfs）——st_size 是编的，st_blocks 恒 0，
    /// du 正确地报 0。把表观大小当用量是 Value 维死槽。
    Pseudo,
    /// 秩 5：设备/特殊件——计量在场但语义另列。
    Device,
}

impl ObjKind {
    /// 全部五格。
    pub const ALL: [ObjKind; 5] = [
        ObjKind::Plain,
        ObjKind::Dir,
        ObjKind::Symlink,
        ObjKind::Pseudo,
        ObjKind::Device,
    ];

    /// 秩。
    pub fn rank(&self) -> u32 {
        match self {
            ObjKind::Plain => 1,
            ObjKind::Dir => 2,
            ObjKind::Symlink => 3,
            ObjKind::Pseudo => 4,
            ObjKind::Device => 5,
        }
    }

    /// 短名。
    pub fn slug(&self) -> &'static str {
        match self {
            ObjKind::Plain => "plain",
            ObjKind::Dir => "dir",
            ObjKind::Symlink => "symlink",
            ObjKind::Pseudo => "pseudo",
            ObjKind::Device => "device",
        }
    }

    /// 由秩还原。
    pub fn from_rank(r: u32) -> Option<ObjKind> {
        match r {
            1 => Some(ObjKind::Plain),
            2 => Some(ObjKind::Dir),
            3 => Some(ObjKind::Symlink),
            4 => Some(ObjKind::Pseudo),
            5 => Some(ObjKind::Device),
            _ => None,
        }
    }

    /// 是否参与默认块账（symlink 不进账；pseudo 进账但恒 0）。
    pub fn counts_in_blocks(&self) -> bool {
        matches!(self, ObjKind::Plain | ObjKind::Dir | ObjKind::Pseudo | ObjKind::Device)
    }

    /// apparent 账是否对它有意义（伪文件的 st_size 是谎言）。
    pub fn apparent_legit(&self) -> bool {
        matches!(self, ObjKind::Plain)
    }
}

/// 遍历侧结局——遍历闸（秩 2）与定位闸（秩 3）的联合产出。四种结局里
/// **没有一种是 ENOSPC**：这正是本卡 J1 的正面陈述。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Traverse {
    /// 秩 1：无障碍——条目量到了，进后续闸。
    Clear,
    /// 秩 2：权限拒（EACCES）——`cannot access 'x': Permission denied`。
    Denied,
    /// 秩 3：条目消失（ENOENT）——列完目录后被删的竞态。
    Missing,
    /// 秩 4：底层 IO 错（EIO）——坏盘读统计的真身，不是 28。
    Io,
}

impl Traverse {
    /// 全部四格。
    pub const ALL: [Traverse; 4] = [Traverse::Clear, Traverse::Denied, Traverse::Missing, Traverse::Io];

    /// 秩。
    pub fn rank(&self) -> u32 {
        match self {
            Traverse::Clear => 1,
            Traverse::Denied => 2,
            Traverse::Missing => 3,
            Traverse::Io => 4,
        }
    }

    /// 短名。
    pub fn slug(&self) -> &'static str {
        match self {
            Traverse::Clear => "clear",
            Traverse::Denied => "denied",
            Traverse::Missing => "missing",
            Traverse::Io => "ioerr",
        }
    }

    /// 由秩还原。
    pub fn from_rank(r: u32) -> Option<Traverse> {
        match r {
            1 => Some(Traverse::Clear),
            2 => Some(Traverse::Denied),
            3 => Some(Traverse::Missing),
            4 => Some(Traverse::Io),
            _ => None,
        }
    }

    /// 对应的 errno（Clear 是 0）。
    pub fn errno(&self) -> u32 {
        match self {
            Traverse::Clear => 0,
            Traverse::Denied => EACCES,
            Traverse::Missing => ENOENT,
            Traverse::Io => EIO,
        }
    }

    /// 是否让 du 出局（报错误行）。Clear 之外全部出局一条。
    pub fn errored(&self) -> bool {
        !matches!(self, Traverse::Clear)
    }
}
/// 工具面（报错前缀与句式由它决定；Raw 面没有工具行）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    /// 秩 1：GNU coreutils du——`cannot access` / `standard output:` 两式的正主。
    Gnu,
    /// 秩 2：裸统计面——只有 errno 数值与 strerror 串，没有 `du:` 前缀行。
    Raw,
    /// 秩 3：认不出的实现——本卡句式对它不适用，只能进兜底。
    Unknown,
}

impl Tool {
    /// 全部三格。
    pub const ALL: [Tool; 3] = [Tool::Gnu, Tool::Raw, Tool::Unknown];

    /// 秩。
    pub fn rank(&self) -> u32 {
        match self {
            Tool::Gnu => 1,
            Tool::Raw => 2,
            Tool::Unknown => 3,
        }
    }

    /// 短名。
    pub fn slug(&self) -> &'static str {
        match self {
            Tool::Gnu => "gnu",
            Tool::Raw => "raw",
            Tool::Unknown => "unknown",
        }
    }

    /// 由秩还原。
    pub fn from_rank(r: u32) -> Option<Tool> {
        match r {
            1 => Some(Tool::Gnu),
            2 => Some(Tool::Raw),
            3 => Some(Tool::Unknown),
            _ => None,
        }
    }

    /// 该面能不能打出 `cannot access` 式（GNU 专属句式）。
    pub fn can_access_line(&self) -> bool {
        matches!(self, Tool::Gnu)
    }

    /// 该面是否有本卡可认的整行句式。
    pub fn is_legible(&self) -> bool {
        matches!(self, Tool::Gnu)
    }
}

/// 文案档位（投递闸的产出：谁、在哪个面、用哪一式报了错）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Note {
    /// 秩 1：没有任何错误行（统计全绿——含满盘下的全绿）。
    Clean,
    /// 秩 2：遍历权限拒——`du: cannot access 'data/x': Permission denied`。
    AccDenied,
    /// 秩 3：条目消失——`du: cannot access 'data/gone': No such file or directory`。
    NoSuch,
    /// 秩 4：底层 IO 错——`du: cannot access 'data/img': Input/output error`。
    IoErr,
    /// 秩 5：输出面空间拒——`du: standard output: No space left on device`。
    /// 本卡唯一合法的 28 文案，统计本身已经完成。
    StdoutErr,
    /// 秩 6：用法错——`du: unrecognized option '--bogus'`，压根没开始统计。
    Usage,
    /// 秩 7：一行都不打（裸面出局/未知实现）——沉默本身就是档位。
    Silent,
}

impl Note {
    /// 全部七格。
    pub const ALL: [Note; 7] = [
        Note::Clean,
        Note::AccDenied,
        Note::NoSuch,
        Note::IoErr,
        Note::StdoutErr,
        Note::Usage,
        Note::Silent,
    ];

    /// 秩。
    pub fn rank(&self) -> u32 {
        match self {
            Note::Clean => 1,
            Note::AccDenied => 2,
            Note::NoSuch => 3,
            Note::IoErr => 4,
            Note::StdoutErr => 5,
            Note::Usage => 6,
            Note::Silent => 7,
        }
    }

    /// 短名。
    pub fn slug(&self) -> &'static str {
        match self {
            Note::Clean => "clean",
            Note::AccDenied => "acc-denied",
            Note::NoSuch => "no-such",
            Note::IoErr => "io-error",
            Note::StdoutErr => "stdout-enospc",
            Note::Usage => "usage",
            Note::Silent => "silent",
        }
    }

    /// 由秩还原。
    pub fn from_rank(r: u32) -> Option<Note> {
        match r {
            1 => Some(Note::Clean),
            2 => Some(Note::AccDenied),
            3 => Some(Note::NoSuch),
            4 => Some(Note::IoErr),
            5 => Some(Note::StdoutErr),
            6 => Some(Note::Usage),
            7 => Some(Note::Silent),
            _ => None,
        }
    }

    /// 这一格是否报在**统计面**（cannot access 三式：权限/消失/IO 错）。
    pub fn blames_stat_side(&self) -> bool {
        matches!(self, Note::AccDenied | Note::NoSuch | Note::IoErr)
    }

    /// 这一格是否报在**输出面**（standard output 一式；唯一的 28 文案）。
    pub fn blames_out_side(&self) -> bool {
        matches!(self, Note::StdoutErr)
    }

    /// 这一格是否携带 ENOSPC 语义。三式 cannot access 全都**不**携带——
    /// 本卡的家族纪律就钉在这个方法上。
    pub fn carries_enospc(&self) -> bool {
        matches!(self, Note::StdoutErr)
    }

    /// 这一格是否算"报出了错误"。沉默不算报——兜底与裸面的出局没有行。
    pub fn reports_error(&self) -> bool {
        !matches!(self, Note::Clean | Note::Silent)
    }
}

/// 档位维（矩阵分档用，本卡四档）。判据的核心在于 Green 与 Out 的分野：
/// 满盘统计正常（Green）是本卡 J1 的正判据；输出投递失败（Out）是同一串
/// ENOSPC 文案的**唯一合法出处**。把 Out 读成统计失败（Estate 谎报的靶子）
/// 与把 Green 读成"盘满所以 du 也会失败"，是本卡两条最贵的死槽。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// 秩 1：统计全绿——条目全部量到、账目算对；满盘与否与此档无关。
    Green,
    /// 秩 2：遍历跳格——EACCES/ENOENT/EIO 让条目缺席，累加继续、退 1。
    Skip,
    /// 秩 3：投递败局——统计完成、输出面 28；败在终点不在途中。
    Out,
    /// 秩 4：用法出局——命令行就没对，一个条目都没 stat。
    Usage,
}

impl Tier {
    /// 全部四档。
    pub const ALL: [Tier; 4] = [Tier::Green, Tier::Skip, Tier::Out, Tier::Usage];

    /// 秩。
    pub fn rank(&self) -> u32 {
        match self {
            Tier::Green => 1,
            Tier::Skip => 2,
            Tier::Out => 3,
            Tier::Usage => 4,
        }
    }

    /// 短名。
    pub fn slug(&self) -> &'static str {
        match self {
            Tier::Green => "green",
            Tier::Skip => "skip",
            Tier::Out => "out",
            Tier::Usage => "usage",
        }
    }

    /// 中文名。
    pub fn cn(&self) -> &'static str {
        match self {
            Tier::Green => "统计全绿档",
            Tier::Skip => "遍历跳格档",
            Tier::Out => "投递败局档",
            Tier::Usage => "用法出局档",
        }
    }

    /// 由秩还原。
    pub fn from_rank(r: u32) -> Option<Tier> {
        match r {
            1 => Some(Tier::Green),
            2 => Some(Tier::Skip),
            3 => Some(Tier::Out),
            4 => Some(Tier::Usage),
            _ => None,
        }
    }

    /// 分档规则：这是本卡把"事实面"折成"一档"的唯一出口。
    /// 次序不可换——先看有没有码（全绿档），再看投递闸的 28（Out 优先于
    /// 一般跳格，因为它携带本卡最敏感的 errno），再看遍历侧跳格，
    /// 调用闸出局归 Usage，其余留在 Green。
    pub fn of(actual: &Outcome) -> Tier {
        if actual.code == 0 {
            return Tier::Green;
        }
        if actual.code == DU_WROUT {
            return Tier::Out;
        }
        if actual.code == DU_SKIP {
            return Tier::Skip;
        }
        if actual.stage == Stage::Call {
            return Tier::Usage;
        }
        Tier::Green
    }

    /// 这一档是否属于"只读统计面"（Green/Skip：都发生在 du 不写一个字节
    /// 的前提下；Out 是写面事件，Usage 连面都没碰）。
    pub fn is_read_face(&self) -> bool {
        matches!(self, Tier::Green | Tier::Skip)
    }
}

/// 判定维序（judge 的六维 + Ok）。次序固定：Estate → Stage → Code → Value →
/// Note → Default → Ok。Estate 最先：本卡的头号谎报恰恰是"投递失败却报成
/// 统计成功"（`du > report` 截了尾，盘上留下半份统计却被读成全份）——
/// 成败互报比阶段错更贵，先于一切判死。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// 秩 0：申报与事实六维全合。
    Ok,
    /// 秩 1：成败互报。
    Estate,
    /// 秩 2：阶段不合。
    Stage,
    /// 秩 3：errno/归因码不合。
    Code,
    /// 秩 4：统计字节数不合。
    Value,
    /// 秩 5：文案面不合。
    Note,
    /// 秩 6：兜底档不合。
    Default,
}

impl Fault {
    /// 全部七格（含 Ok）。
    pub const ALL: [Fault; 7] = [
        Fault::Ok,
        Fault::Estate,
        Fault::Stage,
        Fault::Code,
        Fault::Value,
        Fault::Note,
        Fault::Default,
    ];

    /// 秩（Ok 是 0，其余 1..=6）。
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

    /// 由秩还原（秩 7 及以上不合法）。
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

    /// 由 WANT_NDX 的序号还原（本卡的 WANT_NDX 直接存 Fault 的秩）。
    pub fn from_ndx(ndx: u8) -> Option<Fault> {
        Fault::from_rank(ndx as u32)
    }

    /// 本缺陷维对应的错误码（judge 命中维的出口码）。
    pub fn code(&self) -> u32 {
        match self {
            Fault::Ok => 0,
            Fault::Estate => DU_STATE,
            Fault::Stage => DU_STAGE,
            Fault::Code => DU_CODE,
            Fault::Value => DU_VALUE,
            Fault::Note => DU_NOTE,
            Fault::Default => DU_FAULT_DEFAULT,
        }
    }

    /// 是否为真缺陷（Ok 不是）。
    pub fn is_fault(&self) -> bool {
        !matches!(self, Fault::Ok)
    }

    /// 本卡语义下这一维坏了意味着什么（一句话诊断，自检与判据名共用）。
    pub fn why(&self) -> &'static str {
        match self {
            Fault::Ok => "六维全合",
            Fault::Estate => "输出截尾的败局被报成统计成功",
            Fault::Stage => "用法错被记在投递闸，一个条目都没 stat 却说有行",
            Fault::Code => "EACCES 的 cannot access 被安成 ENOSPC 家族",
            Fault::Value => "硬链接第二链接被双计，虚增了 4096",
            Fault::Note => "standard output 式认成 cannot access 式",
            Fault::Default => "本可判定的伪文件计 0 被丢进兜底档",
        }
    }
}

/// errno 家族名（Linux strerror 口径，与 F11694 文案卡逐字同源）。
pub fn errno_name(e: u32) -> &'static str {
    match e {
        0 => "success",
        ENOENT => "No such file or directory",
        EACCES => "Permission denied",
        EIO => "Input/output error",
        ENOSPC => "No space left on device",
        _ => "Unknown errno",
    }
}

/// errno 是否属本卡可判家族（只读统计面只在这几种里打转；28 只在输出面）。
pub fn errno_legible(e: u32) -> bool {
    matches!(e, 0 | ENOENT | EACCES | EIO | ENOSPC)
}

/// 本卡 13 个错误码的名字（诊断渲染用；越界值落 out-of-segment）。
pub fn code_name(code: u32) -> &'static str {
    if code == 0 {
        return "clean";
    }
    if code == DU_SLOT {
        "du-slot"
    } else if code == DU_STATE {
        "du-state"
    } else if code == DU_UNIQ {
        "du-uniq"
    } else if code == DU_STAGE {
        "du-stage"
    } else if code == DU_CODE {
        "du-code"
    } else if code == DU_VALUE {
        "du-value"
    } else if code == DU_NOTE {
        "du-note"
    } else if code == DU_FAULT_DEFAULT {
        "du-default"
    } else if code == DU_EUSAGE {
        "du-eusage"
    } else if code == DU_WROUT {
        "du-wrout"
    } else if code == DU_SKIP {
        "du-skip"
    } else if code == DU_LINK {
        "du-link"
    } else if code == DU_ROUND {
        "du-round"
    } else {
        "out-of-segment"
    }
}

/// 段位与本卡基址的偏移（诊断用；越界偏移照原样算出来，不夹取）。
pub fn code_offset(code: u32) -> i32 {
    let base = SEG_BASE;
    if code >= base {
        (code - base) as i32
    } else {
        -((base - code) as i32)
    }
}

/// 本卡输入的全部维度——一次"统计一个条目"能被观察到的所有事实。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shape {
    /// 条目类型（定性闸的分档依据）。
    pub kind: ObjKind,
    /// st_size：表观字节数（普通件才是真身；伪文件是谎言）。
    pub apparent: u32,
    /// st_blocks：POSIX 账本量，单位 512 字节。
    pub blocks512: u32,
    /// 文件系统块大小（取整闸第二段；ext4 典型 4096）。
    pub fs_block: u32,
    /// 输出单位（取整闸第三段；GNU 默认 1024）。
    pub unit: u32,
    /// st_nlink：>1 即硬链接候选。
    pub nlink: u32,
    /// (dev, ino) 是否首次出现（去重闸的输入；false=再现，计 0）。
    pub first_seen: bool,
    /// --apparent-size：绕开块账，直接报表观数。
    pub apparent_size: bool,
    /// -l：关闭硬链接去重（第二链接也计满额）。
    pub count_links: bool,
    /// 遍历侧结局（EACCES/ENOENT/EIO/Clear）。
    pub traverse: Traverse,
    /// stdout 是否重定向到已满的文件系统（本卡唯一合法的 28 通道）。
    pub out_full: bool,
    /// 文件系统数据块是否耗尽（满盘实况——对读面零影响，J1 的背景板）。
    pub disk_full: bool,
    /// 工具面（文案前缀与句式来源）。
    pub tool: Tool,
    /// 命令行是否非法（未知选项/缺操作数）：调用闸判死，一条目未 stat。
    pub bad_opt: bool,
}

impl Shape {
    /// 一条干净的普通件形态（空间任意、无障碍遍历）。
    pub fn plain(blocks512: u32) -> Shape {
        Shape {
            kind: ObjKind::Plain,
            apparent: blocks512.saturating_mul(POSIX_BLOCK),
            blocks512,
            fs_block: FS_BLOCK,
            unit: GNU_UNIT,
            nlink: 1,
            first_seen: true,
            apparent_size: false,
            count_links: false,
            traverse: Traverse::Clear,
            out_full: false,
            disk_full: false,
            tool: Tool::Gnu,
            bad_opt: false,
        }
    }

    /// 块口径用量（取整闸第一、二段）：st_blocks×512 再按文件系统块取整。
    pub fn used_bytes(&self) -> u32 {
        usage_bytes(self.blocks512, self.fs_block)
    }

    /// 这一条目在默认口径下的应然申报数（去重闸与取整闸的合成功）。
    pub fn block_report(&self) -> u32 {
        let base = self.used_bytes();
        let counted = if self.nlink > 1 && !self.first_seen && !self.count_links {
            0
        } else {
            base
        };
        round_up(counted, self.unit)
    }

    /// --apparent-size 口径下的应然申报数（伪文件的表观数不予采信）。
    pub fn apparent_report(&self) -> u32 {
        if !self.kind.apparent_legit() {
            return self.block_report();
        }
        round_up(self.apparent, self.unit)
    }

    /// 应然统计数：flag 定口径，条目定资格。符号链接不进账。
    pub fn reported(&self) -> u32 {
        if !self.kind.counts_in_blocks() {
            return 0;
        }
        if self.apparent_size {
            self.apparent_report()
        } else {
            self.block_report()
        }
    }

    /// 合法域复核：真正要判的是"三套数与条目资格的闭合"——
    /// 伪文件不许有块、用法错一切归零、消失条目不许带账本量、单位必须 2 幂。
    pub fn domain_ok(&self) -> bool {
        if !is_pow2(self.fs_block) || self.fs_block < POSIX_BLOCK {
            return false;
        }
        if !is_pow2(self.unit) || self.unit < POSIX_BLOCK {
            return false;
        }
        if self.nlink == 0 {
            return false;
        }
        if self.kind == ObjKind::Pseudo && self.blocks512 != 0 {
            return false;
        }
        if self.bad_opt {
            return self.traverse == Traverse::Clear
                && !self.out_full
                && self.blocks512 == 0
                && self.apparent == 0;
        }
        if self.traverse == Traverse::Missing && (self.blocks512 != 0 || self.apparent != 0) {
            return false;
        }
        if self.apparent_size && !self.kind.apparent_legit() && self.apparent > 0 {
            return false;
        }
        true
    }
}

/// 事实面（判据的应然结论）。五个维度与 Claim 一一对应。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// 纵线落点阶段。
    pub stage: Stage,
    /// 归因码（0 = 统计全绿）。
    pub code: u32,
    /// 申报面上的统计字节数（去重与取整之后的那一个数）。
    pub value: u32,
    /// 文案档位。
    pub note: Note,
    /// 兜底档：事实面本卡判不了。
    pub default: bool,
}

impl Outcome {
    /// 构造。
    pub fn new(stage: Stage, code: u32, value: u32, note: Note, default: bool) -> Outcome {
        Outcome { stage, code, value, note, default }
    }

    /// 成败位：错误码为 0 才是成。跳格/投递败/用法三族的错误码都非 0，
    /// 因此**必然**是败——这一位是 Estate 维的锚，"投递截尾却报全绿"的谎报
    /// 就在此处露出。
    pub fn ok(&self) -> bool {
        self.code == 0
    }

    /// 是否有一个面领取了回执（Silent 档没有）。
    pub fn reported(&self) -> bool {
        self.note.reports_error()
    }

    /// 折叠五维进指纹（顺序固定：阶段→码→统计数→文案→兜底）。
    pub fn fold(&self, h: u64) -> u64 {
        let x = fold_word(h, self.stage.rank());
        let x = fold_word(x, self.code);
        let x = fold_word(x, self.value);
        let x = fold_word(x, self.note.rank());
        fold_bool(x, self.default)
    }

    /// 一行渲染。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str("stage=");
        s.push_str(self.stage.slug());
        sep(&mut s);
        s.push_str("code=");
        s.push_str(code_name(self.code));
        sep(&mut s);
        s.push_str("size=");
        s.push_str(dec_text(self.value).as_str());
        sep(&mut s);
        s.push_str("note=");
        s.push_str(self.note.slug());
        sep(&mut s);
        s.push_str("default=");
        s.push_str(bool_text(self.default).as_str());
        s
    }
}

/// 申报面（被复核的那一份结论）。比 Outcome 多一个显式成败位——
/// 现实里"报成功/报失败"就是退出码这一维独立可谎报的东西。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Claim {
    /// 申报的成败（真实语义即 exit==0）。
    pub success: bool,
    /// 申报的落点阶段。
    pub stage: Stage,
    /// 申报的归因码。
    pub code: u32,
    /// 申报的统计字节数。
    pub value: u32,
    /// 申报的文案档位。
    pub note: Note,
    /// 申报是否走兜底档。
    pub default: bool,
}

impl Claim {
    /// 构造。
    pub const fn new(
        success: bool,
        stage: Stage,
        code: u32,
        value: u32,
        note: Note,
        default: bool,
    ) -> Claim {
        Claim { success, stage, code, value, note, default }
    }

    /// 申报的成败位。
    pub fn success(&self) -> bool {
        self.success
    }

    /// 申报折算出的退出码（只区分 0/1；2 属用法档，由 claim_exit 另行给出）。
    pub fn exit_hint(&self) -> u32 {
        if self.success { EXIT_OK } else { EXIT_FAIL }
    }

    /// 由事实面照抄一份申报（活槽用；死槽在此之后只翻一维）。
    pub fn from_outcome(actual: &Outcome, success: bool) -> Claim {
        Claim::new(
            success,
            actual.stage,
            actual.code,
            actual.value,
            actual.note,
            actual.default,
        )
    }

    /// 折叠申报六维（与 Outcome::fold 同序加成败位，供账本比对）。
    pub fn fold(&self, h: u64) -> u64 {
        let x = fold_bool(h, self.success);
        let x = fold_word(x, self.stage.rank());
        let x = fold_word(x, self.code);
        let x = fold_word(x, self.value);
        let x = fold_word(x, self.note.rank());
        fold_bool(x, self.default)
    }

    /// 与事实面的六维差异掩码（bit0=Estate…bit5=Default）。死槽要求恰好一位。
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

    /// 一行渲染。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str("ok=");
        s.push_str(bool_text(self.success).as_str());
        sep(&mut s);
        s.push_str(self.stage.slug());
        sep(&mut s);
        s.push_str(code_name(self.code));
        sep(&mut s);
        s.push_str(dec_text(self.value).as_str());
        sep(&mut s);
        s.push_str(self.note.slug());
        s
    }
}

/// 一槽：输入形态 + 申报结论。
#[derive(Clone, Copy, Debug)]
pub struct Case {
    /// 输入形态。
    pub shape: Shape,
    /// 申报结论。
    pub claim: Claim,
}

impl Case {
    /// 构造。
    pub fn new(shape: Shape, claim: Claim) -> Case {
        Case { shape, claim }
    }

    /// 事实面。
    pub fn actual(&self) -> Outcome {
        define(&self.shape)
    }
}
// ---------------------------------------------------------------------------
// 语料：14 槽 = 8 活 + 6 死
// ---------------------------------------------------------------------------

/// 槽 1 `clean-dir`：满盘实况（disk_full）下统计一个目录条目，块账 2×512
/// 按 4K 块取整成 4096，全绿退 0。J1 正判据的基准形态：**盘满与读面无关**。
const SHAPE_CLEAN_DIR: Shape = Shape {
    kind: ObjKind::Dir,
    apparent: 4096,
    blocks512: 2,
    fs_block: 4096,
    unit: 1024,
    nlink: 2,
    first_seen: true,
    apparent_size: false,
    count_links: false,
    traverse: Traverse::Clear,
    out_full: false,
    disk_full: true,
    tool: Tool::Gnu,
    bad_opt: false,
};

/// 槽 2 `link-zero`：硬链接第二次出现（nlink=2、first_seen=false），
/// 默认口径贡献 0。去重闸的正例——0 不是丢失，是正确统计。
const SHAPE_LINK_ZERO: Shape = Shape {
    kind: ObjKind::Plain,
    apparent: 131072,
    blocks512: 8,
    fs_block: 4096,
    unit: 1024,
    nlink: 2,
    first_seen: false,
    apparent_size: false,
    count_links: false,
    traverse: Traverse::Clear,
    out_full: false,
    disk_full: false,
    tool: Tool::Gnu,
    bad_opt: false,
};

/// 槽 3 `sparse-4k`：稀疏文件表观 1 MiB、实占 8×512=4096；默认块账只报 4096。
/// 满盘背景同样在场——稀疏与满盘都是"账面数与表观数分家"的实况。
const SHAPE_SPARSE_4K: Shape = Shape {
    kind: ObjKind::Plain,
    apparent: SPARSE_APPARENT,
    blocks512: 8,
    fs_block: 4096,
    unit: 1024,
    nlink: 1,
    first_seen: true,
    apparent_size: false,
    count_links: false,
    traverse: Traverse::Clear,
    out_full: false,
    disk_full: true,
    tool: Tool::Gnu,
    bad_opt: false,
};

/// 槽 4 `apparent-size`：同款稀疏件开 `--apparent-size`，取整闸换轨——
/// 块账整个绕开，申报数就是表观数 1048576。口径切换只动 Round 一闸。
const SHAPE_APPARENT_SIZE: Shape = Shape {
    kind: ObjKind::Plain,
    apparent: SPARSE_APPARENT,
    blocks512: 8,
    fs_block: 4096,
    unit: 1024,
    nlink: 1,
    first_seen: true,
    apparent_size: true,
    count_links: false,
    traverse: Traverse::Clear,
    out_full: false,
    disk_full: false,
    tool: Tool::Gnu,
    bad_opt: false,
};

/// 槽 5 `proc-zero`：procfs 伪文件 st_size 巨大而 st_blocks 恒 0，
/// du 正确地报 0。把表观数当成用量是本卡 Value 维的经典死法，这里立正例。
const SHAPE_PROC_ZERO: Shape = Shape {
    kind: ObjKind::Pseudo,
    apparent: SPARSE_APPARENT,
    blocks512: 0,
    fs_block: 4096,
    unit: 1024,
    nlink: 1,
    first_seen: true,
    apparent_size: false,
    count_links: false,
    traverse: Traverse::Clear,
    out_full: false,
    disk_full: false,
    tool: Tool::Gnu,
    bad_opt: false,
};

/// 槽 6 `denied-skip`：遍历闸权限拒（EACCES），条目缺席、累加继续、退 1。
/// 满盘背景在场，但错误与空间无涉——Code 维死槽（把 13 安成 28）的对照组。
const SHAPE_DENIED_SKIP: Shape = Shape {
    kind: ObjKind::Plain,
    apparent: 0,
    blocks512: 0,
    fs_block: 4096,
    unit: 1024,
    nlink: 1,
    first_seen: true,
    apparent_size: false,
    count_links: false,
    traverse: Traverse::Denied,
    out_full: false,
    disk_full: true,
    tool: Tool::Gnu,
    bad_opt: false,
};

/// 槽 7 `stdout-enospc`：本卡的镜像靶心。统计**全量完成**（4096 已入账），
/// `du > report` 的输出面撞上满盘——`du: standard output: No space left on
/// device`，退 1。28 只许活在这一闸，且只打在投递面上。
const SHAPE_STDOUT_ENOSPC: Shape = Shape {
    kind: ObjKind::Plain,
    apparent: 1024,
    blocks512: 8,
    fs_block: 4096,
    unit: 1024,
    nlink: 1,
    first_seen: true,
    apparent_size: false,
    count_links: false,
    traverse: Traverse::Clear,
    out_full: true,
    disk_full: true,
    tool: Tool::Gnu,
    bad_opt: false,
};

/// 槽 8 `usage-badopt`：非法选项，调用闸判死，退 2，一个条目都没 stat。
/// 用法错与统计错互斥是本卡的第一道墙。
const SHAPE_USAGE_BADOPT: Shape = Shape {
    kind: ObjKind::Plain,
    apparent: 0,
    blocks512: 0,
    fs_block: 4096,
    unit: 1024,
    nlink: 1,
    first_seen: true,
    apparent_size: false,
    count_links: false,
    traverse: Traverse::Clear,
    out_full: false,
    disk_full: true,
    tool: Tool::Gnu,
    bad_opt: true,
};

/// 全部 14 槽的输入形态。后 6 槽的事实面**整槽复用**前面的活形态——
/// 一槽只坏一维，坏的是申报而不是事实，这是语料纪律的硬要求。
pub const SHAPES: [Shape; CORPUS] = [
    SHAPE_CLEAN_DIR,
    SHAPE_LINK_ZERO,
    SHAPE_SPARSE_4K,
    SHAPE_APPARENT_SIZE,
    SHAPE_PROC_ZERO,
    SHAPE_DENIED_SKIP,
    SHAPE_STDOUT_ENOSPC,
    SHAPE_USAGE_BADOPT,
    SHAPE_STDOUT_ENOSPC,
    SHAPE_USAGE_BADOPT,
    SHAPE_DENIED_SKIP,
    SHAPE_LINK_ZERO,
    SHAPE_STDOUT_ENOSPC,
    SHAPE_PROC_ZERO,
];

/// 14 槽的申报结论（活槽与事实面逐维相等；死槽只翻一维）。
pub const CLAIMS: [Claim; CORPUS] = [
    // 槽 1：满盘下目录统计全绿，4096 入账。
    Claim::new(true, Stage::Exit, 0, 4096, Note::Clean, false),
    // 槽 2：硬链接第二链接计 0，仍是全绿统计。
    Claim::new(true, Stage::Exit, 0, 0, Note::Clean, false),
    // 槽 3：稀疏件块账 4096（表观 1 MiB 不进默认账）。
    Claim::new(true, Stage::Exit, 0, 4096, Note::Clean, false),
    // 槽 4：apparent 口径 1048576。
    Claim::new(true, Stage::Exit, 0, 1_048_576, Note::Clean, false),
    // 槽 5：伪文件 0 块，报 0，全绿。
    Claim::new(true, Stage::Exit, 0, 0, Note::Clean, false),
    // 槽 6：权限拒跳格，落投递闸，cannot access 式，退 1。
    Claim::new(false, Stage::Print, DU_SKIP, 0, Note::AccDenied, false),
    // 槽 7：统计完成、投递失败，standard output 式，退 1。
    Claim::new(false, Stage::Print, DU_WROUT, 4096, Note::StdoutErr, false),
    // 槽 8：用法错落调用闸，退 2。
    Claim::new(false, Stage::Call, DU_EUSAGE, 0, Note::Usage, false),
    // 死 1 estate-swap：投递失败的败局，申报却报统计成功（谎报全绿）。
    Claim::new(true, Stage::Print, DU_WROUT, 4096, Note::StdoutErr, false),
    // 死 2 stage-swap：用法错没 stat 过任何东西，申报却记在投递闸。
    Claim::new(false, Stage::Print, DU_EUSAGE, 0, Note::Usage, false),
    // 死 3 code-swap：EACCES 的跳格被安成 ENOSPC 家族。
    Claim::new(false, Stage::Print, DU_WROUT, 0, Note::AccDenied, false),
    // 死 4 value-double：硬链接第二链接被双计（实 0 报 4096）。
    Claim::new(true, Stage::Exit, 0, 4096, Note::Clean, false),
    // 死 5 note-swap：standard output 式认成 cannot access 式。
    Claim::new(false, Stage::Print, DU_WROUT, 4096, Note::AccDenied, false),
    // 死 6 default-bucket：本可判定的伪文件计 0 被丢进兜底档。
    Claim::new(true, Stage::Exit, 0, 0, Note::Clean, true),
];

/// 14 槽的标签（后 6 槽一槽只坏一维，依次挂六维）。
pub const WANT_TAGS: [&str; CORPUS] = [
    "clean-dir",
    "link-zero",
    "sparse-4k",
    "apparent-size",
    "proc-zero",
    "denied-skip",
    "stdout-enospc",
    "usage-badopt",
    "estate-swap",
    "stage-swap",
    "code-swap",
    "value-double",
    "note-swap",
    "default-bucket",
];

/// 期望表的第二形态：落点阶段（与 CLAIMS 交叉校验，任一处笔误都要露出来）。
pub const WANT_STAGE: [Stage; CORPUS] = [
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
    Stage::Print,
    Stage::Print,
    Stage::Call,
    Stage::Print,
    Stage::Print,
    Stage::Print,
    Stage::Exit,
    Stage::Print,
    Stage::Exit,
];

/// 期望表的第二形态：归因码。
pub const WANT_CODE: [u32; CORPUS] = [
    0,
    0,
    0,
    0,
    0,
    DU_SKIP,
    DU_WROUT,
    DU_EUSAGE,
    DU_WROUT,
    DU_EUSAGE,
    DU_WROUT,
    0,
    DU_WROUT,
    0,
];

/// 期望表的第三形态：期望命中的判定维（0 = Ok，1..=6 = Fault 秩）。
pub const WANT_NDX: [u8; CORPUS] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 4, 5, 6];

/// 期望表的第二形态：文案档位。
pub const WANT_NOTE: [Note; CORPUS] = [
    Note::Clean,
    Note::Clean,
    Note::Clean,
    Note::Clean,
    Note::Clean,
    Note::AccDenied,
    Note::StdoutErr,
    Note::Usage,
    Note::StdoutErr,
    Note::Usage,
    Note::AccDenied,
    Note::Clean,
    Note::AccDenied,
    Note::Clean,
];

/// 期望表的第二形态：兜底档。
pub const WANT_DEFAULT: [bool; CORPUS] = [
    false, false, false, false, false, false, false, false, false, false, false, false, false,
    true,
];

/// 期望表的第二形态：申报成败位（等价于 exit==0）。
pub const WANT_OK: [bool; CORPUS] = [
    true, true, true, true, true, false, false, false, true, false, false, true, false, true,
];

/// 期望表的第二形态：申报的统计字节数。
pub const WANT_VALUE: [u32; CORPUS] = [
    4096, 0, 4096, 1_048_576, 0, 0, 4096, 0, 4096, 0, 0, 4096, 4096, 0,
];

/// 事实面第二形态（与 `define` 独立列出，用来同时复核 define 与 CLAIMS）：落点阶段。
pub const ACT_STAGE: [Stage; CORPUS] = [
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
    Stage::Print,
    Stage::Print,
    Stage::Call,
    Stage::Print,
    Stage::Call,
    Stage::Print,
    Stage::Exit,
    Stage::Print,
    Stage::Exit,
];

/// 事实面第二形态：归因码。
pub const ACT_CODE: [u32; CORPUS] = [
    0,
    0,
    0,
    0,
    0,
    DU_SKIP,
    DU_WROUT,
    DU_EUSAGE,
    DU_WROUT,
    DU_EUSAGE,
    DU_SKIP,
    0,
    DU_WROUT,
    0,
];

/// 事实面第二形态：统计字节数。
pub const ACT_VALUE: [u32; CORPUS] = [
    4096, 0, 4096, 1_048_576, 0, 0, 4096, 0, 4096, 0, 0, 0, 4096, 0,
];

/// 事实面第二形态：文案档位。
pub const ACT_NOTE: [Note; CORPUS] = [
    Note::Clean,
    Note::Clean,
    Note::Clean,
    Note::Clean,
    Note::Clean,
    Note::AccDenied,
    Note::StdoutErr,
    Note::Usage,
    Note::StdoutErr,
    Note::Usage,
    Note::AccDenied,
    Note::Clean,
    Note::StdoutErr,
    Note::Clean,
];

/// 事实面第二形态：兜底档（14 槽全部可判，没有一格该走垃圾桶）。
pub const ACT_DEFAULT: [bool; CORPUS] = [
    false, false, false, false, false, false, false, false, false, false, false, false, false,
    false,
];

/// 事实面第二形态：成败位。
pub const ACT_OK: [bool; CORPUS] = [
    true, true, true, true, true, false, false, false, false, false, false, true, false, true,
];

/// 事实面第二形态：去重与取整之前的块口径用量（`st_blocks×512` 按 fs_block
/// 取整；零块恒 0）。它是三套数里的"账本数"，用来复核 reported 的第一段。
pub const ACT_USED: [u32; CORPUS] = [
    4096, 4096, 4096, 4096, 0, 0, 4096, 0, 4096, 0, 0, 4096, 4096, 0,
];

/// 事实面第二形态：档位归属（分档规则的独立复写，任一处漂移都要露出）。
pub const ACT_TIER: [Tier; CORPUS] = [
    Tier::Green,
    Tier::Green,
    Tier::Green,
    Tier::Green,
    Tier::Green,
    Tier::Skip,
    Tier::Out,
    Tier::Usage,
    Tier::Out,
    Tier::Usage,
    Tier::Skip,
    Tier::Green,
    Tier::Out,
    Tier::Green,
];

/// 死槽 → 它所复用的活槽（语料纪律：后 6 槽的事实面整槽照抄前 8 槽的某一行）。
pub const DEAD_FROM: [usize; REJECT] = [6usize, 7usize, 5usize, 1usize, 6usize, 4usize];

// ---------------------------------------------------------------------------
// 闸门纵线
// ---------------------------------------------------------------------------

/// 读面免疫判定（J1 的核心陈述）：满盘只打分配面，du 不分配——
/// 因此 disk_full 且没有自撞输出面、遍历无障碍、命令行合法时，统计必然全绿。
pub fn read_face_immune(sh: &Shape) -> bool {
    sh.disk_full && !sh.out_full && sh.traverse == Traverse::Clear && !sh.bad_opt
}

/// 遍历侧是否出局（跳格）：错误行落投递闸，条目不进账。
pub fn walk_denied(sh: &Shape) -> bool {
    !sh.bad_opt && sh.traverse.errored()
}

/// 输出侧是否撞墙（本卡唯一合法的 28 通道）：统计已全量完成才轮得到它。
pub fn out_wall(sh: &Shape) -> bool {
    !sh.bad_opt && sh.traverse == Traverse::Clear && sh.out_full
}

/// 闸门纵线：从形态推出应然结论。次序即九道闸的次序，前一闸不产出事实后一闸无输入。
pub fn define(sh: &Shape) -> Outcome {
    // 闸 1 调用：非法选项/缺操作数，一个条目都没 stat，落调用闸。
    if sh.bad_opt {
        return Outcome::new(Stage::Call, DU_EUSAGE, 0, Note::Usage, sh.tool == Tool::Unknown);
    }
    // 闸 2/3 遍历与定位：EACCES/ENOENT/EIO 跳格——错误行在投递闸回执，
    // 条目贡献 0，累加继续。三式的家族码都是 DU_SKIP， errno 只在文案面分家。
    if walk_denied(sh) {
        let note = if sh.tool.is_legible() {
            match sh.traverse {
                Traverse::Denied => Note::AccDenied,
                Traverse::Missing => Note::NoSuch,
                Traverse::Io => Note::IoErr,
                Traverse::Clear => Note::Clean,
            }
        } else {
            Note::Silent
        };
        return Outcome {
            stage: Stage::Print,
            code: DU_SKIP,
            value: 0,
            note,
            default: sh.tool == Tool::Unknown,
        };
    }
    // 闸 8 投递：统计全量完成（value=reported 照常入账），输出面撞满盘——
    // 本卡唯一合法的 28。落点必须在投递闸，不许前移到定位/取整/累加任何一闸。
    if out_wall(sh) {
        let note = if sh.tool.is_legible() { Note::StdoutErr } else { Note::Silent };
        return Outcome {
            stage: Stage::Print,
            code: DU_WROUT,
            value: sh.reported(),
            note,
            default: sh.tool == Tool::Unknown,
        };
    }
    // 闸 4~7 定性/去重/取整/累加：三套数在这一段合出申报数；闸 9 结算落 Exit。
    Outcome {
        stage: Stage::Exit,
        code: 0,
        value: sh.reported(),
        note: Note::Clean,
        default: sh.tool == Tool::Unknown,
    }
}

/// 退出码结算（闸 9）：2 = 用法错；1 = 遍历跳格或投递失败；0 = 统计全绿。
/// **满盘本身不改变任何退出码**——读面免疫的账面上证明。
pub fn exit_code(sh: &Shape) -> u32 {
    if sh.bad_opt {
        return EXIT_USAGE;
    }
    let a = define(sh);
    if a.code == 0 {
        return EXIT_OK;
    }
    EXIT_FAIL
}

/// 这一条目实际进账的字节数（= define 的 value 在同口径下的独立复算：
/// 跳格与用法是 0，投递败照样全量入账，全绿即 reported）。
pub fn counted_bytes(sh: &Shape) -> u32 {
    if sh.bad_opt || sh.traverse.errored() {
        return 0;
    }
    sh.reported()
}

/// 一槽的申报退出码（供 Estate 维与渲染共用）。
pub fn claim_exit(c: &Claim) -> u32 {
    if c.default {
        return EXIT_USAGE;
    }
    c.exit_hint()
}

/// 活槽的成败位（前 8 槽）。
pub const LIVE_OK: [bool; ACCEPT] = [true, true, true, true, true, false, false, false];
// ---------------------------------------------------------------------------
// 文案：逐字复刻真实报错
// ---------------------------------------------------------------------------

/// 被权限拒的条目在本卡语料里的字面名（GNU 会给它套单引号）。
pub const OPERAND_DENIED: &str = "data/x";
/// 竞态消失的条目字面名。
pub const OPERAND_GONE: &str = "data/gone";
/// IO 错的条目字面名。
pub const OPERAND_IO: &str = "data/img";
/// 投递闸输出统计的目的地字面名（`du > report.txt` 的 report.txt）。
pub const OPERAND_LOG: &str = "data/app.log";

/// GNU du 遍历侧权限拒式：
/// `du: cannot access 'data/x': Permission denied`
pub fn msg_du_denied(op: &str) -> String {
    let mut s = String::new();
    s.push_str("du: cannot access '");
    s.push_str(op);
    s.push_str("': ");
    s.push_str(errno_name(EACCES));
    s
}

/// GNU du 条目消失式：
/// `du: cannot access 'data/gone': No such file or directory`
pub fn msg_du_missing(op: &str) -> String {
    let mut s = String::new();
    s.push_str("du: cannot access '");
    s.push_str(op);
    s.push_str("': ");
    s.push_str(errno_name(ENOENT));
    s
}

/// GNU du 底层 IO 错式：
/// `du: cannot access 'data/img': Input/output error`
pub fn msg_du_ioerr(op: &str) -> String {
    let mut s = String::new();
    s.push_str("du: cannot access '");
    s.push_str(op);
    s.push_str("': ");
    s.push_str(errno_name(EIO));
    s
}

/// GNU du 输出面空间拒式（本卡唯一合法的 28 行）：
/// `du: standard output: No space left on device`
pub fn msg_du_stdout_enospc() -> String {
    let mut s = String::new();
    s.push_str("du: standard output: ");
    s.push_str(ANCHOR_ERRNO_TEXT);
    s
}

/// 用法错：`du: unrecognized option '--bogus'`
pub fn msg_du_usage_opt(o: &str) -> String {
    let mut s = String::new();
    s.push_str("du: unrecognized option '");
    s.push_str(o);
    s.push('\'');
    s
}

/// 用法错的提示行：`Try 'du --help' for more information.`
pub fn msg_du_hint() -> String {
    let mut s = String::new();
    s.push_str("Try 'du --help' for more information.");
    s
}

/// 裸统计面：`stat: Permission denied`（无 `du:` 前缀、无引号包围——
/// 句式归属的最小可分特征）。
pub fn msg_raw_stat() -> String {
    let mut s = String::new();
    s.push_str("stat: ");
    s.push_str(errno_name(EACCES));
    s
}

/// 卡面原文形态（默认操作数下的权限拒式，判据名与测试都盯这一串）。
pub fn msg_du_denied_default() -> String {
    msg_du_denied(OPERAND_DENIED)
}

/// 卡面原文形态：条目消失式。
pub fn msg_du_missing_default() -> String {
    msg_du_missing(OPERAND_GONE)
}

/// 卡面原文形态：IO 错式。
pub fn msg_du_ioerr_default() -> String {
    msg_du_ioerr(OPERAND_IO)
}

/// 按文案档位给出该档的规范整行（Clean/Silent 档是空串——没行就是没行）。
pub fn msg_note(n: Note) -> String {
    match n {
        Note::Clean => String::new(),
        Note::Silent => String::new(),
        Note::AccDenied => msg_du_denied_default(),
        Note::NoSuch => msg_du_missing_default(),
        Note::IoErr => msg_du_ioerr_default(),
        Note::StdoutErr => msg_du_stdout_enospc(),
        Note::Usage => msg_du_usage_opt("--bogus"),
    }
}

/// 按形态给出这一槽实际会打出的那一行（比 msg_note 更贴事实：它区分工具面）。
pub fn msg_for(sh: &Shape) -> String {
    if sh.bad_opt {
        return msg_du_usage_opt("--bogus");
    }
    let a = define(sh);
    match a.note {
        Note::Clean | Note::Silent => String::new(),
        Note::Usage => msg_du_usage_opt("--bogus"),
        Note::StdoutErr => msg_du_stdout_enospc(),
        Note::AccDenied => match sh.tool {
            Tool::Raw => msg_raw_stat(),
            _ => msg_du_denied(OPERAND_DENIED),
        },
        Note::NoSuch => msg_du_missing(OPERAND_GONE),
        Note::IoErr => msg_du_ioerr(OPERAND_IO),
    }
}

/// 文案是否含 GNU 特有的单引号包围（裸统计面没有）。
pub fn has_quoted_operand(s: &str) -> bool {
    s.contains("'")
}

/// 文案是否报在**输出面**（standard output 一式——唯一携 28 的行）。
pub fn is_stdout_line(s: &str) -> bool {
    s.contains("standard output:")
}

/// 文案是否报在**统计面**（cannot access 三式：权限/消失/IO 错）。
pub fn is_access_line(s: &str) -> bool {
    s.contains("cannot access")
}

/// 文案是否是空间类报错。本卡的家族纪律：**cannot access 三式永远不是**，
/// 只有 standard output 式才是。
pub fn is_space_line(s: &str) -> bool {
    s.contains(ANCHOR_ERRNO_TEXT)
}

// ---------------------------------------------------------------------------
// 判定
// ---------------------------------------------------------------------------

/// 六个缺陷维（不含 Ok），次序即 judge 的判定次序。
pub const REJECT_FAULTS: [Fault; REJECT] = [
    Fault::Estate,
    Fault::Stage,
    Fault::Code,
    Fault::Value,
    Fault::Note,
    Fault::Default,
];

/// 六个缺陷维对应的错误码（互不相同，与 REJECT_FAULTS 一一对位）。
pub const REJECT_CODES: [u32; REJECT] = [
    DU_STATE,
    DU_STAGE,
    DU_CODE,
    DU_VALUE,
    DU_NOTE,
    DU_FAULT_DEFAULT,
];

/// 账本的 8 个正数格：前 7 格是统计数，第 8 格是红档率 929。
///
/// 七个统计数逐个字面取，含义写死在 `stat_meaning()` 里，账本与期望不许各自漂移。
pub const VALS: [u32; ACCEPT] = [
    OBJ_KIND_COUNT,
    STAGE_COUNT,
    CODE_COUNT as u32,
    EXIT_RANKS,
    CORPUS as u32,
    TIER_COUNT as u32,
    REJECT as u32,
    RED_RATE,
];

/// 统计格的字面期望（第二形态，交叉校验 VALS 的笔误）。
pub const VALS_CHECK: [u32; ACCEPT] = [5, 9, 13, 3, 14, 4, 6, 929];

/// 第 8 格之外的统计格数。
pub const STAT_CELLS: usize = 7usize;

/// 统计格语义（本卡的身份数，一格一句话）。
pub fn stat_meaning(i: usize) -> &'static str {
    match i {
        0 => "五种条目类型：plain/dir/symlink/pseudo/device",
        1 => "九道闸：调用到结算的纵线宽度",
        2 => "十三个错误码：SEG_BASE+0x01..0x0D",
        3 => "三个退出码位：0/1/2",
        4 => "十四个语料槽：8 活 + 6 死",
        5 => "四个档位：全绿/跳格/投递/用法",
        6 => "六个缺陷维：一槽只坏一维",
        7 => "红档率 929：14 格中 13 的千分率",
        _ => "越界",
    }
}

/// 判定次序固定：Estate → Stage → Code → Value → Note → Default → Ok。
///
/// Estate 的定义是 `actual.ok() != claim.success()`——成败互报优先于阶段与错误码，
/// 因为"投递截尾却报统计成功"是本卡唯一能把失败读成成功的谎法，别的维错只影响归因。
pub fn judge(c: &Case) -> Fault {
    let actual = define(&c.shape);
    if actual.ok() != c.claim.success() {
        return Fault::Estate;
    }
    if actual.stage != c.claim.stage {
        return Fault::Stage;
    }
    if actual.code != c.claim.code {
        return Fault::Code;
    }
    if actual.value != c.claim.value {
        return Fault::Value;
    }
    if actual.note != c.claim.note {
        return Fault::Note;
    }
    if actual.default != c.claim.default {
        return Fault::Default;
    }
    Fault::Ok
}

/// 第 i 槽的形态。
pub fn shape_at(i: usize) -> Option<Shape> {
    match SHAPES.get(i) {
        Some(s) => Some(*s),
        None => None,
    }
}

/// 第 i 槽的申报（原表）。
pub fn claim_at(i: usize) -> Option<Claim> {
    match CLAIMS.get(i) {
        Some(c) => Some(*c),
        None => None,
    }
}

/// 第 i 槽由期望表（第二形态）重建的申报——与 `claim_at` 必须逐维相等。
pub fn claim_of(i: usize) -> Option<Claim> {
    let stage = match WANT_STAGE.get(i) {
        Some(v) => *v,
        None => return None,
    };
    let code = match WANT_CODE.get(i) {
        Some(v) => *v,
        None => return None,
    };
    let value = match WANT_VALUE.get(i) {
        Some(v) => *v,
        None => return None,
    };
    let note = match WANT_NOTE.get(i) {
        Some(v) => *v,
        None => return None,
    };
    let default = match WANT_DEFAULT.get(i) {
        Some(v) => *v,
        None => return None,
    };
    let success = match WANT_OK.get(i) {
        Some(v) => *v,
        None => return None,
    };
    Some(Claim::new(success, stage, code, value, note, default))
}

/// 第 i 槽的标签。
pub fn tag_at(i: usize) -> &'static str {
    match WANT_TAGS.get(i) {
        Some(s) => s,
        None => "out-of-range",
    }
}

/// 第 i 槽期望命中的判定维。
pub fn want_fault(i: usize) -> Fault {
    match WANT_NDX.get(i) {
        Some(v) => match Fault::from_ndx(*v) {
            Some(f) => f,
            None => Fault::Default,
        },
        None => Fault::Default,
    }
}

/// 组装第 i 槽。
pub fn case_at(i: usize) -> Option<Case> {
    let shape = match shape_at(i) {
        Some(v) => v,
        None => return None,
    };
    let claim = match claim_at(i) {
        Some(v) => v,
        None => return None,
    };
    Some(Case::new(shape, claim))
}

/// 全语料槽数。
pub fn slot_count() -> usize {
    CORPUS
}

/// 判定注入（`run_with` 的内部出口）：越界槽返回 Err(DU_SLOT)，被点中的槽用注入维，
/// 其余槽照常判。
fn fault_at(c: &Case, i: usize, over: Option<(usize, Fault)>) -> Result<Fault, u32> {
    if i >= CORPUS {
        return Err(DU_SLOT);
    }
    let got = judge(c);
    match over {
        Some((slot, f)) => {
            if slot == i {
                Ok(f)
            } else {
                Ok(got)
            }
        }
        None => Ok(got),
    }
}

// ---------------------------------------------------------------------------
// 矩阵与报告
// ---------------------------------------------------------------------------

/// 单档计数行。`good` 与 `total` 独立记账：一行可以"槽位有但没命中"，
/// 也可以压根没有槽位——空行永远不算完美（缺档本身就是缺陷，不是满分）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    /// 所属档位。
    pub tier: Tier,
    /// 该档的槽数。
    pub total: u32,
    /// 该档命中期望结论的槽数。
    pub good: u32,
}

impl Row {
    /// 建一行。
    pub fn of(tier: Tier) -> Row {
        Row { tier, total: 0, good: 0 }
    }

    /// 是否满命中。**空行一律判不完美**——本卡的四档必须都有事实，缺档就是缺陷。
    pub fn perfect(&self) -> bool {
        self.total > 0 && self.total == self.good
    }

    /// 是否空行。
    pub fn empty(&self) -> bool {
        self.total == 0
    }

    /// 命中缺口。
    pub fn miss(&self) -> u32 {
        self.total.saturating_sub(self.good)
    }

    /// 渲染一行。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str(self.tier.slug());
        s.push(':');
        s.push_str(dec_text(self.good).as_str());
        s.push('/');
        s.push_str(dec_text(self.total).as_str());
        s
    }
}

/// 四档矩阵。`total` 是投槽数、`synced` 是登记数、`aligned()` 是四档行求和——
/// 三者本应相等，任何一格漂移都要被 `covered()` 抓到（所以它们是独立累计的）。
#[derive(Clone, Copy, Debug)]
pub struct Matrix {
    /// 四档行。
    pub rows: [Row; TIER_COUNT],
    /// 投槽总数。
    pub total: u32,
    /// 登记总数（与 total 同路累计，但走的是另一条计数）。
    pub synced: u32,
    /// 命中总数。
    pub matched: u32,
}

impl Matrix {
    /// 空矩阵：四档就位、计数全 0。
    pub fn new() -> Matrix {
        Matrix {
            rows: [
                Row::of(Tier::Green),
                Row::of(Tier::Skip),
                Row::of(Tier::Out),
                Row::of(Tier::Usage),
            ],
            total: 0,
            synced: 0,
            matched: 0,
        }
    }

    /// 落一槽事实。
    pub fn push_actual(&mut self, tier: Tier, hit: bool) {
        self.total = self.total.saturating_add(1);
        self.synced = self.synced.saturating_add(1);
        if hit {
            self.matched = self.matched.saturating_add(1);
        }
        let mut k: usize = 0;
        while k < TIER_COUNT {
            if let Some(r) = self.rows.get_mut(k) {
                if r.tier == tier {
                    r.total = r.total.saturating_add(1);
                    if hit {
                        r.good = r.good.saturating_add(1);
                    }
                }
            }
            k += 1;
        }
    }

    /// 只加投槽数、不进任何档——用来造 `total != aligned()` 这种账目不平的形态。
    pub fn desync(&mut self) {
        self.total = self.total.saturating_add(1);
    }

    /// 四档求和（独立于 total 的第二条口径）。
    pub fn aligned(&self) -> u32 {
        let mut sum: u32 = 0;
        let mut k: usize = 0;
        while k < TIER_COUNT {
            if let Some(r) = self.rows.get(k) {
                sum = sum.saturating_add(r.total);
            }
            k += 1;
        }
        sum
    }

    /// 两条口径是否闭合。
    pub fn covered(&self) -> bool {
        self.total == self.aligned() && self.aligned() == self.synced
    }

    /// 满命中的非空行数。
    pub fn perfect_rows(&self) -> u32 {
        let mut n: u32 = 0;
        let mut k: usize = 0;
        while k < TIER_COUNT {
            if let Some(r) = self.rows.get(k) {
                if r.perfect() {
                    n = n.saturating_add(1);
                }
            }
            k += 1;
        }
        n
    }

    /// 非空行数。
    pub fn occupied(&self) -> u32 {
        let mut n: u32 = 0;
        let mut k: usize = 0;
        while k < TIER_COUNT {
            if let Some(r) = self.rows.get(k) {
                if !r.empty() {
                    n = n.saturating_add(1);
                }
            }
            k += 1;
        }
        n
    }

    /// 取某档的行。
    pub fn row(&self, tier: Tier) -> Option<Row> {
        let mut k: usize = 0;
        while k < TIER_COUNT {
            if let Some(r) = self.rows.get(k) {
                if r.tier == tier {
                    return Some(*r);
                }
            }
            k += 1;
        }
        None
    }

    /// 该档的槽数（缺行按 0）。
    pub fn count_of(&self, tier: Tier) -> u32 {
        match self.row(tier) {
            Some(r) => r.total,
            None => 0,
        }
    }

    /// 由全语料建矩阵：命中定义为"判出的结论等于该槽期望结论"。
    pub fn from_cases() -> Matrix {
        let mut m = Matrix::new();
        let mut i: usize = 0;
        while i < CORPUS {
            let c = match case_at(i) {
                Some(v) => v,
                None => break,
            };
            let tier = Tier::of(&define(&c.shape));
            let hit = judge(&c) == want_fault(i);
            m.push_actual(tier, hit);
            i += 1;
        }
        m
    }

    /// 渲染四档。
    pub fn render(&self) -> String {
        let mut s = String::new();
        let mut k: usize = 0;
        while k < TIER_COUNT {
            if let Some(r) = self.rows.get(k) {
                if k > 0 {
                    sep(&mut s);
                }
                s.push_str(r.render().as_str());
            }
            k += 1;
        }
        sep(&mut s);
        kv(&mut s, "aligned", self.aligned());
        sep(&mut s);
        kv(&mut s, "total", self.total);
        s
    }
}
// ---------------------------------------------------------------------------
// 交叉署名复算
// ---------------------------------------------------------------------------

/// 三条相邻卡的段位闭合复核：`ANCHOR_x_SEG + GAP == SEG_BASE`（或反向差）。
/// 位距是本卡与邻卡边界的物理证据：任何一次改段基址忘了改位距，这里就红。
pub fn anchors_agree() -> bool {
    if ANCHOR_F11691_SEG.checked_add(ANCHOR_F11691_GAP) != Some(SEG_BASE) {
        return false;
    }
    if ANCHOR_F11681_SEG.checked_add(ANCHOR_F11681_GAP) != Some(SEG_BASE) {
        return false;
    }
    if SEG_BASE.checked_sub(ANCHOR_F11689_SEG) != Some(ANCHOR_F11689_GAP) {
        return false;
    }
    if ANCHOR_ENOSPC != ENOSPC {
        return false;
    }
    if ANCHOR_ERRNO_TEXT != errno_name(ENOSPC) {
        return false;
    }
    true
}

// ---------------------------------------------------------------------------
// 报告
// ---------------------------------------------------------------------------

/// 全绿渲染的逐字锚：报告的 `render()` 与它必须一字不差。
/// 四档计数（green=7 skip=2 out=3 usage=2）是本卡语料的物理分布，
/// 改任何一槽的形态都会把这一串撞碎——这正是它被钉住的意义。
pub const RENDER_OK: &str = "du-report green=7 skip=2 out=3 usage=2 matched=14/14 rate=1000 green=true";

/// 全语料判定后的汇总。`total`/`accept`/`reject` 与四档计数各自独立累计，
/// `matched` 只统计"判出的维等于该槽期望维"的槽——红档率 929 就是
/// matched=13、total=14 时的千分率，因此第 8 账格与本报告共用同一条算式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Report {
    /// 判定总槽数。
    pub total: u32,
    /// 活槽（申报诚实）数。
    pub accept: u32,
    /// 死槽（一槽只坏一维）数。
    pub reject: u32,
    /// 统计全绿档槽数。
    pub green_count: u32,
    /// 遍历跳格档槽数。
    pub skip: u32,
    /// 投递败局档槽数。
    pub out: u32,
    /// 用法出局档槽数。
    pub usage: u32,
    /// 命中期望判定维的槽数。
    pub matched: u32,
}

impl Report {
    /// 由矩阵与两侧计数组装。四档计数直接取矩阵行，避免第二套口径。
    pub fn of(m: &Matrix, accept: u32, reject: u32) -> Report {
        Report {
            total: m.total,
            accept,
            reject,
            green_count: m.count_of(Tier::Green),
            skip: m.count_of(Tier::Skip),
            out: m.count_of(Tier::Out),
            usage: m.count_of(Tier::Usage),
            matched: m.matched,
        }
    }

    /// 千分率：`(matched*1000 + total/2) / total`。total 为 0 时判 0（空账不是满分）。
    /// 13/14 → `(13000+7)/14 = 929`，与账本第 8 格同源。
    pub fn rate(&self) -> u32 {
        if self.total == 0 {
            return 0;
        }
        let num = self.matched.saturating_mul(1000).saturating_add(self.total / 2);
        num / self.total
    }

    /// 全绿：满槽、满命中、四档皆非空。红档注入下 matched=13，这里必须 false——
    /// "恰中 13" 是本卡的标定样本，不是满分。
    pub fn green(&self) -> bool {
        if self.total != CORPUS as u32 {
            return false;
        }
        if self.accept as usize != ACCEPT || self.reject as usize != REJECT {
            return false;
        }
        if self.matched != self.total {
            return false;
        }
        self.green_count > 0 && self.skip > 0 && self.out > 0 && self.usage > 0
    }

    /// 结构闸：只查口径闭合，不查命中率（红档报告的口径同样必须闭合，
    /// 否则账本第 8 格的 929 无处可取）。
    pub fn gate(&self) -> Result<(), u32> {
        if self.total != CORPUS as u32 {
            return Err(DU_UNIQ);
        }
        if self.accept as usize != ACCEPT || self.reject as usize != REJECT {
            return Err(DU_STATE);
        }
        let rows_sum = self
            .green_count
            .saturating_add(self.skip)
            .saturating_add(self.out)
            .saturating_add(self.usage);
        if rows_sum != self.total {
            return Err(DU_STAGE);
        }
        if self.green_count == 0 || self.skip == 0 || self.out == 0 || self.usage == 0 {
            return Err(DU_VALUE);
        }
        Ok(())
    }

    /// 全绿渲染。逐字钉在 `RENDER_OK` 上。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str("du-report ");
        kv(&mut s, "green", self.green_count);
        sep(&mut s);
        kv(&mut s, "skip", self.skip);
        sep(&mut s);
        kv(&mut s, "out", self.out);
        sep(&mut s);
        kv(&mut s, "usage", self.usage);
        sep(&mut s);
        s.push_str("matched=");
        s.push_str(dec_text(self.matched).as_str());
        s.push('/');
        s.push_str(dec_text(self.total).as_str());
        sep(&mut s);
        kv(&mut s, "rate", self.rate());
        sep(&mut s);
        s.push_str("green=");
        s.push_str(bool_text(self.green()).as_str());
        s
    }
}

/// 全语料判定（无注入）。
pub fn run() -> Result<Report, u32> {
    run_with(None)
}

/// 带注入的全语料判定：`over = Some((槽号, 判定维))` 把该槽的判出维强行改写，
/// 用来制造"恰中 13"的红档标定样本；注入槽越界 → `Err(DU_SLOT)`。
pub fn run_with(over: Option<(usize, Fault)>) -> Result<Report, u32> {
    if let Some((slot, _)) = over {
        if slot >= CORPUS {
            return Err(DU_SLOT);
        }
    }
    let mut m = Matrix::new();
    let mut accept: u32 = 0;
    let mut reject: u32 = 0;
    let mut i: usize = 0;
    while i < CORPUS {
        let c = match case_at(i) {
            Some(v) => v,
            None => return Err(DU_UNIQ),
        };
        if i < ACCEPT {
            // 活槽口径：形态合法且申报与事实六维全合，才计入 accept。
            if c.shape.domain_ok() && judge(&c) == Fault::Ok {
                accept = accept.saturating_add(1);
            }
        } else {
            // 死槽口径：差异掩码恰好一位才计入 reject——一槽只坏一维是语料纪律。
            let bits = c.claim.diff_mask(&define(&c.shape)).count_ones();
            if bits == 1 {
                reject = reject.saturating_add(1);
            }
        }
        let tier = Tier::of(&define(&c.shape));
        let got = match fault_at(&c, i, over) {
            Ok(f) => f,
            Err(e) => return Err(e),
        };
        let hit = got == want_fault(i);
        m.push_actual(tier, hit);
        i += 1;
    }
    let report = Report::of(&m, accept, reject);
    match report.gate() {
        Ok(()) => Ok(report),
        Err(code) => Err(code),
    }
}

/// 全语料的判定向量（自检与账本共用的一条出口）。
pub fn verdicts() -> [Fault; CORPUS] {
    let mut out = [Fault::Default; CORPUS];
    let mut i: usize = 0;
    while i < CORPUS {
        if let Some(c) = case_at(i) {
            if let Some(slot) = out.get_mut(i) {
                *slot = judge(&c);
            }
        }
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// 阶段闸门
// ---------------------------------------------------------------------------

/// 九闸纵线的运行态。只许前进、不许倒退：`step` 的秩低于上一秩 → `DU_STAGE`
/// （申报者拿着文案倒推闸位——比如把没 stat 过的用法错说成落在投递闸，
/// 正是运行版的阶段谎报）；`close()` 之后再 `step`/`close` → `DU_STATE`。
#[derive(Clone, Copy, Debug)]
pub struct Gate {
    /// 上一段秩（-1 表示未走）。
    last: i32,
    /// 已走段数。
    steps: u32,
    /// 违例次数（显性记录，不许静默吞）。
    falls: u32,
    /// 是否已封印。
    closed: bool,
}

impl Gate {
    /// 新闸门：未起跑（秩 0 只属于这里，不属于任何 Outcome）。
    pub const fn new() -> Gate {
        Gate {
            last: -1,
            steps: 0,
            falls: 0,
            closed: false,
        }
    }

    /// 走一道闸。倒退 → DU_STAGE；封印后 → DU_STATE。
    pub fn step(&mut self, s: Stage) -> Result<(), u32> {
        if self.closed {
            self.falls = self.falls.saturating_add(1);
            return Err(DU_STATE);
        }
        let o = s.rank() as i32;
        if o < self.last {
            self.falls = self.falls.saturating_add(1);
            return Err(DU_STAGE);
        }
        self.last = o;
        self.steps = self.steps.saturating_add(1);
        Ok(())
    }

    /// 按纵线把九道闸各走一遍并封印。任何一步失败都原样带回错误码。
    pub fn walk_all(&mut self) -> Result<u32, u32> {
        let mut i: usize = 0;
        while i < Stage::ALL.len() {
            let s = match Stage::ALL.get(i) {
                Some(v) => *v,
                None => return Err(DU_UNIQ),
            };
            self.step(s)?;
            i += 1;
        }
        self.close()
    }

    /// 封印。九闸未走满就封印 = 提前封印 → DU_STAGE；重复封印 → DU_STATE。
    pub fn close(&mut self) -> Result<u32, u32> {
        if self.closed {
            self.falls = self.falls.saturating_add(1);
            return Err(DU_STATE);
        }
        if self.steps != STAGE_COUNT {
            self.falls = self.falls.saturating_add(1);
            return Err(DU_STAGE);
        }
        self.closed = true;
        Ok(self.steps)
    }

    /// 上一段秩。
    pub const fn last_rank(&self) -> i32 {
        self.last
    }

    /// 已走段数。
    pub const fn steps(&self) -> u32 {
        self.steps
    }

    /// 违例计数。
    pub const fn falls(&self) -> u32 {
        self.falls
    }

    /// 是否已封印。
    pub const fn closed(&self) -> bool {
        self.closed
    }

    /// 闸门状态指纹（防止"封印后再改状态"逃过重放）。
    pub fn seal(&self) -> u64 {
        let h = fold_word(FNV_OFF, (self.last + 1) as u32);
        let h = fold_word(h, self.steps);
        let h = fold_word(h, self.falls);
        fold_bool(h, self.closed)
    }
}
// ---------------------------------------------------------------------------
// 账本
// ---------------------------------------------------------------------------

/// 账本格：一格 = 一次"统计"的可复核投影。槽位、操作、成败、码、值五维
/// 全部入指纹——账本自己也是被判的对象（登记跳号就是统计漏计的账上形态）。
#[derive(Clone, Copy, Debug)]
pub struct Sample {
    /// 槽位（登记时必须严格等于当前长度）。
    pub slot: u8,
    /// 操作码（OP_STAT/OP_RATE/OP_DEFECT 三族）。
    pub op: u8,
    /// 是否成功格。
    pub ok: bool,
    /// 错误码（成功格恒为 0）。
    pub code: u32,
    /// 观测值（失败格恒为 0）。
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

    /// 统计格。
    pub const fn stat(slot: u8, value: u32) -> Sample {
        Sample { slot, op: OP_STAT, ok: true, code: 0, value }
    }

    /// 红档格（值必须是 929，槽位固定第 8 格）。
    pub const fn rate(slot: u8, value: u32) -> Sample {
        Sample { slot, op: OP_RATE, ok: true, code: 0, value }
    }

    /// 缺陷码格（值恒 0，败格）。
    pub const fn defect(slot: u8, code: u32) -> Sample {
        Sample { slot, op: OP_DEFECT, ok: false, code, value: 0 }
    }

    /// 一格指纹。
    pub fn seal(&self) -> u64 {
        let h = fold_word(FNV_OFF, self.slot as u32);
        let h = fold_word(h, self.op as u32);
        let h = fold_bool(h, self.ok);
        let h = fold_word(h, self.code);
        fold_word(h, self.value)
    }
}

/// 账本：14 格，7 格统计 + 1 格红档率 + 6 格缺陷码；冻结后拒登记。
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
            return Err(DU_SLOT);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = s;
        }
        self.len = self.len.saturating_add(1);
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
        match self.rows.get(i) {
            Some(s) => Some(*s),
            None => None,
        }
    }

    /// 成功格数。
    pub fn ok_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                if s.ok {
                    n = n.saturating_add(1);
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

    /// 逐格对账：槽位连续、格型对位（前 7 统计、第 8 红档、后 6 缺陷）、
    /// 成格值等于 `VALS`、败格码等于 `REJECT_CODES`、败格值必归零。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != ACCEPT || self.err_count() != REJECT {
            return Err(DU_STATE);
        }
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(DU_STATE),
            };
            if (s.slot as usize) != i {
                return Err(DU_SLOT);
            }
            if i < STAT_CELLS {
                if s.op != OP_STAT {
                    return Err(DU_STATE);
                }
            } else if i == STAT_CELLS {
                if s.op != OP_RATE {
                    return Err(DU_STATE);
                }
            } else if s.op != OP_DEFECT {
                return Err(DU_STATE);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(DU_VALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(DU_VALUE);
                        }
                    }
                    None => return Err(DU_STATE),
                }
                vi = vi.saturating_add(1);
            } else {
                if s.value != 0 {
                    return Err(DU_VALUE);
                }
                match REJECT_CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(DU_VALUE);
                        }
                    }
                    None => return Err(DU_STATE),
                }
                ci = ci.saturating_add(1);
            }
            i += 1;
        }
        if vi != ACCEPT || ci != REJECT {
            return Err(DU_STATE);
        }
        Ok(())
    }

    /// 全账指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                h = fold_u64(h, s.seal());
            }
            i += 1;
        }
        fold_word(h, self.rejects)
    }

    /// 冻结。未走满 14 格 = 提前封印 → DU_STAGE；已冻结再冻结 → DU_STATE。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(DU_STATE);
        }
        if self.len != SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(DU_STAGE);
        }
        self.audit()?;
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
            Err(DU_UNIQ)
        }
    }

    /// 篡改留痕：把某格观测值改掉，重放必须报指纹不符。
    pub fn tamper(&mut self, i: usize, value: u32) -> Result<(), u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(DU_STATE);
        }
        if i >= self.len {
            self.rejects = self.rejects.saturating_add(1);
            return Err(DU_SLOT);
        }
        if let Some(dst) = self.rows.get_mut(i) {
            dst.value = value;
        }
        Ok(())
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

/// 统计结果 → 样本。
fn stat_cell(slot: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op: if slot as usize == STAT_CELLS { OP_RATE } else { OP_STAT }, ok: true, code: 0, value: v },
        Err(c) => Sample { slot, op: OP_DEFECT, ok: false, code: c, value: 0 },
    }
}

/// 建账本：7 格统计 + 1 格红档率（929）+ 6 格缺陷码。
///
/// 八个正数格全部**现场重算**（不许直接抄 `VALS`）：条目类型数、纵线宽度、
/// 错误码数、退出码位、语料总数、档位占位、死槽数——最后红档格取
/// "恰中 13" 注入运行的千分率。`audit` 再拿 `VALS` 逐格对回来，
/// 两条独立算路必须在 929 这一格会师。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let green = run();
    let red = run_with(Some((6, Fault::Estate)));

    let _ = l.register(stat_cell(0, Ok(OBJ_KIND_COUNT)));
    let _ = l.register(stat_cell(1, Ok(STAGE_COUNT)));
    let _ = l.register(stat_cell(2, Ok(ALL_CODES.len() as u32)));
    let _ = l.register(stat_cell(3, Ok(EXIT_RANKS)));
    let c4 = match &green {
        Ok(r) => Ok(r.total),
        Err(e) => Err(*e),
    };
    let _ = l.register(stat_cell(4, c4));
    let c5 = match &green {
        Ok(_) => Ok(TIER_COUNT as u32),
        Err(e) => Err(*e),
    };
    let _ = l.register(stat_cell(5, c5));
    let c6 = match &green {
        Ok(r) => Ok(r.reject),
        Err(e) => Err(*e),
    };
    let _ = l.register(stat_cell(6, c6));
    let c7 = match &red {
        Ok(r) => Ok(r.rate()),
        Err(e) => Err(*e),
    };
    let _ = l.register(stat_cell(7, c7));

    let mut i: usize = 0;
    while i < REJECT {
        let c = match case_at(ACCEPT + i) {
            Some(v) => v,
            None => break,
        };
        let f = judge(&c);
        let _ = l.register(Sample::defect((ACCEPT + i) as u8, f.code()));
        i += 1;
    }
    l
}
// ---------------------------------------------------------------------------
// 四路注入自检
// ---------------------------------------------------------------------------

/// 四路注入：表路 / 判路 / 渲染路 / 账本路，各自独立自检。
///
/// 任何一路静默退化都会在这里变成 false：
/// 1. 表路——段基址、错误码链、枚举、取整口径与形状合法域；
/// 2. 判路——九闸纵线、读面免疫、去重分野、申报对判；
/// 3. 渲染路——矩阵覆盖、语料与死槽、槽位红档、文案逐字、退出码闭合；
/// 4. 账本路——阶段闸门、逐格对账、冻结重放。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = name_ok() && enums_ok() && round_ok() && shape_ok();
    out[1] = define_ok() && ro_ok() && dedup_ok() && judge_ok();
    out[2] = matrix_ok() && corpus_ok() && slots_ok() && reds_ok() && message_ok() && exit_ok();
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
// 判据基元：13 组私有谓词，每组对应一个错误码维
// ---------------------------------------------------------------------------

/// 秩序列是否严格从 start 起连续递增（各枚举的 rank 纪律共用）。
fn ranks_contiguous(ranks: &[u32], start: u32) -> bool {
    let mut k: usize = 0;
    while k < ranks.len() {
        let want = match ranks.get(k) {
            Some(v) => *v,
            None => return false,
        };
        if want != start.saturating_add(k as u32) {
            return false;
        }
        k += 1;
    }
    true
}

/// 短名是否互异（slug 撞名会让渲染与判据失去唯一寻址）。
fn slugs_unique(slugs: &[&str]) -> bool {
    let mut i: usize = 0;
    while i < slugs.len() {
        let a = match slugs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let mut j = i + 1;
        while j < slugs.len() {
            if let Some(b) = slugs.get(j) {
                if a == *b {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 01 名称与段基址：常量链、尺寸、errno 真值、退出码档、交叉署名闭合。
fn name_ok() -> bool {
    if SEG_BASE != 0x6d3a0 {
        return false;
    }
    if DU_SLOT != SEG_BASE + 0x01
        || DU_STATE != SEG_BASE + 0x02
        || DU_UNIQ != SEG_BASE + 0x03
        || DU_STAGE != SEG_BASE + 0x04
        || DU_CODE != SEG_BASE + 0x05
        || DU_VALUE != SEG_BASE + 0x06
        || DU_NOTE != SEG_BASE + 0x07
        || DU_FAULT_DEFAULT != SEG_BASE + 0x08
        || DU_EUSAGE != SEG_BASE + 0x09
        || DU_WROUT != SEG_BASE + 0x0A
        || DU_SKIP != SEG_BASE + 0x0B
        || DU_LINK != SEG_BASE + 0x0C
        || DU_ROUND != SEG_BASE + 0x0D
    {
        return false;
    }
    if ALL_CODES.len() != 13 {
        return false;
    }
    let mut i: usize = 0;
    while i < ALL_CODES.len() {
        let c = match ALL_CODES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if c <= SEG_BASE || c > SEG_BASE + 0x0d {
            return false;
        }
        let mut j = i + 1;
        while j < ALL_CODES.len() {
            if let Some(d) = ALL_CODES.get(j) {
                if c == *d {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    if CORPUS != 14 || SAMPLES != 14 || ACCEPT != 8 || REJECT != 6 || ACCEPT + REJECT != CORPUS {
        return false;
    }
    if TIER_COUNT != 4 || STAGE_COUNT != 9 || STAT_CELLS != 7 {
        return false;
    }
    if RED_RATE != 929 || RATE_FULL != 1000 {
        return false;
    }
    // 交叉署名：ENOSPC 在 Linux 就是 28，strerror 原文与前置卡同串。
    if ENOSPC != 28 || ANCHOR_ENOSPC != ENOSPC {
        return false;
    }
    if ANCHOR_ERRNO_TEXT != errno_name(ENOSPC) {
        return false;
    }
    if EACCES != 13 || ENOENT != 2 || EIO != 5 {
        return false;
    }
    if EXIT_OK != 0 || EXIT_FAIL != 1 || EXIT_USAGE != 2 || EXIT_RANKS != 3 {
        return false;
    }
    if POSIX_BLOCK != 512 || GNU_UNIT != 1024 || FS_BLOCK != 4096 {
        return false;
    }
    if OBJ_KIND_COUNT != 5 || TRAVERSE_COUNT != 4 {
        return false;
    }
    if !anchors_agree() {
        return false;
    }
    if OP_STAT == OP_RATE || OP_RATE == OP_DEFECT || OP_STAT == OP_DEFECT {
        return false;
    }
    true
}

/// 02 枚举与九闸：各枚举的秩、往返、slug 唯一性与语义方法。
fn enums_ok() -> bool {
    if Stage::ALL.len() != STAGE_COUNT as usize {
        return false;
    }
    let mut ranks: [u32; 9] = [0; 9];
    let mut slugs: [&str; 9] = ["a"; 9];
    let mut i: usize = 0;
    while i < 9 {
        let s = match Stage::ALL.get(i) {
            Some(v) => *v,
            None => return false,
        };
        ranks[i] = s.rank();
        slugs[i] = s.slug();
        if Stage::from_rank(s.rank()) != Some(s) {
            return false;
        }
        if s.prev().is_some() != (s.rank() > 1) {
            return false;
        }
        if s.next() != Stage::from_rank(s.rank().saturating_add(1)) {
            return false;
        }
        i += 1;
    }
    if !ranks_contiguous(&ranks, 1) || !slugs_unique(&slugs) {
        return false;
    }
    if Stage::Call.prev().is_some() || Stage::Exit.next().is_some() {
        return false;
    }
    if !Stage::Call.is_landing() || !Stage::Print.is_landing() || !Stage::Exit.is_landing() {
        return false;
    }
    if Stage::Walk.is_landing() || Stage::Dedup.is_landing() || Stage::Round.is_landing() {
        return false;
    }
    if ObjKind::ALL.len() != OBJ_KIND_COUNT as usize {
        return false;
    }
    let mut k: usize = 0;
    while k < ObjKind::ALL.len() {
        let o = match ObjKind::ALL.get(k) {
            Some(v) => *v,
            None => return false,
        };
        if ObjKind::from_rank(o.rank()) != Some(o) {
            return false;
        }
        if o.counts_in_blocks() && o == ObjKind::Symlink {
            return false;
        }
        if o.apparent_legit() && o != ObjKind::Plain {
            return false;
        }
        k += 1;
    }
    if Traverse::ALL.len() != TRAVERSE_COUNT as usize {
        return false;
    }
    if Traverse::Clear.errored() || !Traverse::Denied.errored() {
        return false;
    }
    if Traverse::Denied.errno() != EACCES
        || Traverse::Missing.errno() != ENOENT
        || Traverse::Io.errno() != EIO
        || Traverse::Clear.errno() != 0
    {
        return false;
    }
    for t in Traverse::ALL.iter() {
        if Traverse::from_rank(t.rank()) != Some(*t) {
            return false;
        }
        if t.errored() && *t == Traverse::Clear {
            return false;
        }
        if !errno_legible(t.errno()) {
            return false;
        }
    }
    if Tool::ALL.len() != 3 || !Tool::Gnu.can_access_line() || Tool::Raw.can_access_line() {
        return false;
    }
    let mut m: usize = 0;
    while m < Tool::ALL.len() {
        let t = match Tool::ALL.get(m) {
            Some(v) => *v,
            None => return false,
        };
        if Tool::from_rank(t.rank()) != Some(t) {
            return false;
        }
        m += 1;
    }
    if Note::ALL.len() != 7 {
        return false;
    }
    for n in Note::ALL.iter() {
        if Note::from_rank(n.rank()) != Some(*n) {
            return false;
        }
        if n.blames_stat_side() && n.blames_out_side() {
            return false;
        }
        if n.carries_enospc() != (*n == Note::StdoutErr) {
            return false;
        }
        if n.reports_error() == (*n == Note::Clean || *n == Note::Silent) {
            return false;
        }
    }
    if Note::AccDenied.blames_stat_side() == false
        || Note::NoSuch.blames_stat_side() == false
        || Note::IoErr.blames_stat_side() == false
    {
        return false;
    }
    if Tier::ALL.len() != TIER_COUNT {
        return false;
    }
    for t in Tier::ALL.iter() {
        if Tier::from_rank(t.rank()) != Some(*t) {
            return false;
        }
        if t.is_read_face() == (*t == Tier::Out || *t == Tier::Usage) {
            return false;
        }
    }
    if Fault::ALL.len() != 7 {
        return false;
    }
    let mut fr: [u32; 7] = [0; 7];
    let mut fs: [&str; 7] = ["a"; 7];
    let mut q: usize = 0;
    while q < 7 {
        let f = match Fault::ALL.get(q) {
            Some(v) => *v,
            None => return false,
        };
        fr[q] = f.rank();
        fs[q] = f.slug();
        if Fault::from_rank(f.rank()) != Some(f) {
            return false;
        }
        if Fault::from_ndx(f.rank() as u8) != Some(f) {
            return false;
        }
        if f.is_fault() == (f == Fault::Ok) {
            return false;
        }
        if f.why().is_empty() {
            return false;
        }
        q += 1;
    }
    if !ranks_contiguous(&fr, 0) || !slugs_unique(&fs) {
        return false;
    }
    if Fault::Ok.code() != 0 {
        return false;
    }
    let mut z: usize = 0;
    while z < REJECT {
        let f = match REJECT_FAULTS.get(z) {
            Some(v) => *v,
            None => return false,
        };
        let c = match REJECT_CODES.get(z) {
            Some(v) => *v,
            None => return false,
        };
        if f.code() != c {
            return false;
        }
        z += 1;
    }
    true
}

/// 取整域恒等式：三段取整（st_blocks×512 → fs_block → unit）的算路复核。
/// 断言方向：`if 违约条件 { return false; }`，末行是成立条件。
fn round_ok() -> bool {
    if round_up(0, 1024) != 0 || round_up(1, 1024) != 1024 || round_up(1024, 1024) != 1024 {
        return false;
    }
    if round_up(1025, 1024) != 2048 || round_up(4096, 1024) != 4096 {
        return false;
    }
    if round_up(7, 0) != 7 || round_up(7, 1) != 7 {
        return false;
    }
    if usage_bytes(0, 4096) != 0 {
        return false;
    }
    if usage_bytes(1, 4096) != 4096 {
        return false;
    }
    if usage_bytes(8, 4096) != 4096 || usage_bytes(9, 4096) != 8192 {
        return false;
    }
    if usage_bytes(3, 512) != 1536 || usage_bytes(1024, 512) != 524288 {
        return false;
    }
    if !is_pow2(512) || !is_pow2(1024) || !is_pow2(4096) {
        return false;
    }
    if is_pow2(0) || is_pow2(768) || is_pow2(1000) {
        return false;
    }
    // 语料的账本数（ACT_USED）必须与 shape.used_bytes 逐槽一致。
    let mut i: usize = 0;
    while i < CORPUS {
        let sh = match shape_at(i) {
            Some(v) => v,
            None => return false,
        };
        let want = match ACT_USED.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if sh.used_bytes() != want {
            return false;
        }
        i += 1;
    }
    // 表观与块口径只在 apparent_size 时才允许换轨：槽 3/4 是同一稀疏件的两套数。
    let blk = match shape_at(2) {
        Some(v) => v,
        None => return false,
    };
    let app = match shape_at(3) {
        Some(v) => v,
        None => return false,
    };
    if blk.apparent != app.apparent || blk.blocks512 != app.blocks512 {
        return false;
    }
    if blk.reported() != 4096 || app.reported() != SPARSE_APPARENT {
        return false;
    }
    true
}

/// 形状合法域：14 槽全部落在合法域内；三套数的闭合恒等式。
fn shape_ok() -> bool {
    let mut i: usize = 0;
    while i < CORPUS {
        let sh = match shape_at(i) {
            Some(v) => v,
            None => return false,
        };
        if !sh.domain_ok() {
            return false;
        }
        if !sh.apparent_size && sh.reported() != round_up(sh.block_report(), sh.unit) {
            return false;
        }
        if sh.reported() > sh.apparent.saturating_add(FS_BLOCK)
            && !sh.apparent_size
            && sh.blocks512 != 0
        {
            return false;
        }
        i += 1;
    }
    // 违例形状必须被 domain_ok 拒掉（负样本四连：伪文件带块 / 单位非 2 幂 /
    // nlink 归零 / 用法错还带着账本量）。
    let bad_pseudo = Shape { blocks512: 8, ..SHAPE_PROC_ZERO };
    if bad_pseudo.domain_ok() {
        return false;
    }
    let bad_unit = Shape { unit: 700, ..SHAPE_CLEAN_DIR };
    if bad_unit.domain_ok() {
        return false;
    }
    let bad_zero = Shape { nlink: 0, ..SHAPE_CLEAN_DIR };
    if bad_zero.domain_ok() {
        return false;
    }
    let bad_usage = Shape { blocks512: 8, ..SHAPE_USAGE_BADOPT };
    if bad_usage.domain_ok() {
        return false;
    }
    true
}
/// 05 九闸纵线：`define` 与事实面表（ACT_*）逐槽逐维相等；落点只在三点；
/// 判定可复算（同形状两次产出逐位同指纹）。
fn define_ok() -> bool {
    let mut i: usize = 0;
    while i < CORPUS {
        let sh = match shape_at(i) {
            Some(v) => v,
            None => return false,
        };
        let a = define(&sh);
        let wst = match ACT_STAGE.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let wco = match ACT_CODE.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let wva = match ACT_VALUE.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let wno = match ACT_NOTE.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let wde = match ACT_DEFAULT.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let wok = match ACT_OK.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if a.stage != wst || a.code != wco || a.value != wva || a.note != wno || a.default != wde {
            return false;
        }
        if a.ok() != wok {
            return false;
        }
        if !a.stage.is_landing() {
            return false;
        }
        if a.fold(FNV_OFF) != define(&sh).fold(FNV_OFF) {
            return false;
        }
        i += 1;
    }
    // counted_bytes 与 define 的 value 在全部 14 槽上必须同数（两条独立算路）。
    let mut k: usize = 0;
    while k < CORPUS {
        let sh = match shape_at(k) {
            Some(v) => v,
            None => return false,
        };
        if counted_bytes(&sh) != define(&sh).value {
            return false;
        }
        k += 1;
    }
    true
}

/// 06 读面免疫（J1 的灵魂）：满盘对统计面零影响；28 只许活在投递闸。
/// 违例方向逐条写死：任何 out_full=false 的形状判出 DU_WROUT、或满盘绿槽
/// 判出非零码、或 cannot access 式里混进空间串，都判死。
fn ro_ok() -> bool {
    let mut i: usize = 0;
    while i < CORPUS {
        let sh = match shape_at(i) {
            Some(v) => v,
            None => return false,
        };
        let a = define(&sh);
        if a.code == DU_WROUT && !sh.out_full {
            return false;
        }
        if a.code == DU_WROUT && a.stage != Stage::Print {
            return false;
        }
        if read_face_immune(&sh) {
            if a.code != 0 || exit_code(&sh) != EXIT_OK {
                return false;
            }
            if a.note != Note::Clean {
                return false;
            }
        }
        if sh.disk_full && a.code != 0 && a.code != DU_WROUT && a.code != DU_EUSAGE {
            // 满盘现场里除投递与用法两族，不许有任何"盘满所以失败"的伪归因：
            // 遍历跳格族存在，但其码必须是 DU_SKIP 而不是任何空间码。
            if a.code == DU_ROUND || a.code == DU_LINK {
                return false;
            }
        }
        if a.note.blames_stat_side() && a.code == DU_WROUT {
            return false;
        }
        if a.note.blames_out_side() && a.code != DU_WROUT {
            return false;
        }
        i += 1;
    }
    // 文案家族纪律：三式 cannot access 全都不携带 28；standard output 式携带。
    if is_space_line(msg_du_denied(OPERAND_DENIED).as_str()) {
        return false;
    }
    if is_space_line(msg_du_missing(OPERAND_GONE).as_str()) {
        return false;
    }
    if is_space_line(msg_du_ioerr(OPERAND_IO).as_str()) {
        return false;
    }
    if !is_space_line(msg_du_stdout_enospc().as_str()) || !is_stdout_line(msg_du_stdout_enospc().as_str()) {
        return false;
    }
    if is_access_line(msg_du_stdout_enospc().as_str()) {
        return false;
    }
    // 三套 errno 全在本卡家族内且互不撞 28。
    if Traverse::Denied.errno() == ENOSPC || Traverse::Missing.errno() == ENOSPC {
        return false;
    }
    if !errno_legible(EACCES) || !errno_legible(ENOENT) || !errno_legible(EIO) {
        return false;
    }
    true
}

/// 07 去重分野：nlink>1 的二次出现计 0；`-l` 关闭去重后照计；
/// 符号链接不进账；伪文件恒 0。同一 inode 翻转 first_seen 必须精确换数。
fn dedup_ok() -> bool {
    let zero = match shape_at(1) {
        Some(v) => v,
        None => return false,
    };
    if zero.reported() != 0 {
        return false;
    }
    let mut flipped = zero;
    flipped.first_seen = true;
    if flipped.reported() != 4096 {
        return false;
    }
    let mut with_l = zero;
    with_l.count_links = true;
    if with_l.reported() != 4096 {
        return false;
    }
    if define(&with_l).code != 0 {
        return false;
    }
    let mut link1 = match shape_at(0) {
        Some(v) => v,
        None => return false,
    };
    link1.first_seen = false;
    if link1.reported() != 0 {
        return false;
    }
    let sym = Shape {
        kind: ObjKind::Symlink,
        apparent: 4096,
        blocks512: 8,
        fs_block: 4096,
        unit: 1024,
        nlink: 1,
        first_seen: true,
        apparent_size: false,
        count_links: false,
        traverse: Traverse::Clear,
        out_full: false,
        disk_full: false,
        tool: Tool::Gnu,
        bad_opt: false,
    };
    if sym.reported() != 0 {
        return false;
    }
    let pseudo = match shape_at(4) {
        Some(v) => v,
        None => return false,
    };
    if pseudo.reported() != 0 || pseudo.used_bytes() != 0 {
        return false;
    }
    // nlink==1 时 first_seen 不参与（单链接件两次出现是不同 inode 的义务形态）。
    let mut plain = match shape_at(2) {
        Some(v) => v,
        None => return false,
    };
    plain.first_seen = false;
    if plain.reported() != 4096 {
        return false;
    }
    true
}

/// 08 申报对判：14 槽判出维等于期望；死槽差异掩码恰一位；活槽六维全合；
/// Estate 优先序——成败谎报压过一切阶段/错误码谎报。
fn judge_ok() -> bool {
    let mut i: usize = 0;
    while i < CORPUS {
        let c = match case_at(i) {
            Some(v) => v,
            None => return false,
        };
        if judge(&c) != want_fault(i) {
            return false;
        }
        if i >= ACCEPT {
            let bits = c.claim.diff_mask(&define(&c.shape)).count_ones();
            if bits != 1 {
                return false;
            }
            let expect_bit = 1u32 << (i - ACCEPT);
            if c.claim.diff_mask(&define(&c.shape)) != expect_bit {
                return false;
            }
        } else if c.claim.diff_mask(&define(&c.shape)) != 0 {
            return false;
        }
        i += 1;
    }
    // Estate 优先：一个既翻成败又翻阶段的双维谎报，判出的必须是 Estate。
    let sh = match shape_at(6) {
        Some(v) => v,
        None => return false,
    };
    let dbl = Case::new(sh, Claim::new(true, Stage::Exit, DU_WROUT, 4096, Note::StdoutErr, false));
    if judge(&dbl) != Fault::Estate {
        return false;
    }
    // 次序复核：成败相合、阶段先坏时判 Stage；后坏时依次判出 Code/Value/Note/Default。
    let c0 = match case_at(0) {
        Some(v) => v,
        None => return false,
    };
    let mut order_claims: [(Claim, Fault); 5] = [
        (Claim::new(true, Stage::Print, 0, 4096, Note::Clean, false), Fault::Stage),
        (Claim::new(true, Stage::Exit, DU_SKIP, 4096, Note::Clean, false), Fault::Code),
        (Claim::new(true, Stage::Exit, 0, 8192, Note::Clean, false), Fault::Value),
        (Claim::new(true, Stage::Exit, 0, 4096, Note::AccDenied, false), Fault::Note),
        (Claim::new(true, Stage::Exit, 0, 4096, Note::Clean, true), Fault::Default),
    ];
    let mut k: usize = 0;
    while k < order_claims.len() {
        let (cl, want) = match order_claims.get(k) {
            Some(v) => v.clone(),
            None => return false,
        };
        if judge(&Case::new(c0.shape, cl)) != want {
            return false;
        }
        k += 1;
    }
    true
}

/// 04 矩阵四档：覆盖闭合、行数、分档计数与 ACT_TIER 一致、desync 破坏闭合。
fn matrix_ok() -> bool {
    let m = Matrix::from_cases();
    if !m.covered() || m.aligned() != CORPUS as u32 || m.total != CORPUS as u32 {
        return false;
    }
    if m.occupied() != TIER_COUNT as u32 || m.perfect_rows() != TIER_COUNT as u32 {
        return false;
    }
    if m.count_of(Tier::Green) != 7 || m.count_of(Tier::Skip) != 2 {
        return false;
    }
    if m.count_of(Tier::Out) != 3 || m.count_of(Tier::Usage) != 2 {
        return false;
    }
    if m.matched != CORPUS as u32 {
        return false;
    }
    let mut i: usize = 0;
    while i < CORPUS {
        let sh = match shape_at(i) {
            Some(v) => v,
            None => return false,
        };
        let want = match ACT_TIER.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if Tier::of(&define(&sh)) != want {
            return false;
        }
        i += 1;
    }
    let mut broken = Matrix::from_cases();
    broken.desync();
    if broken.covered() {
        return false;
    }
    // 空行不完美：缺档本身就是缺陷，不是满分。
    let mut partial = Matrix::new();
    partial.push_actual(Tier::Green, true);
    match partial.row(Tier::Skip) {
        Some(r) => {
            if r.perfect() || !r.empty() {
                return false;
            }
        }
        None => return false,
    }
    if partial.perfect_rows() != 1 {
        return false;
    }
    true
}

/// 09 语料与死槽：CLAIMS 与期望表交叉相等；标签互异且后六槽按维序命名；
/// 死槽整槽复用活形态。
fn corpus_ok() -> bool {
    let mut i: usize = 0;
    while i < CORPUS {
        let c = match claim_at(i) {
            Some(v) => v,
            None => return false,
        };
        let r = match claim_of(i) {
            Some(v) => v,
            None => return false,
        };
        if c != r {
            return false;
        }
        i += 1;
    }
    if !slugs_unique(&WANT_TAGS) {
        return false;
    }
    let mut k: usize = 0;
    while k < REJECT {
        let t = match WANT_TAGS.get(ACCEPT + k) {
            Some(v) => *v,
            None => return false,
        };
        let f = match REJECT_FAULTS.get(k) {
            Some(v) => *v,
            None => return false,
        };
        if !t.starts_with(f.slug()) {
            return false;
        }
        let src = match DEAD_FROM.get(k) {
            Some(v) => *v,
            None => return false,
        };
        if src >= ACCEPT {
            return false;
        }
        let dead_shape = match shape_at(ACCEPT + k) {
            Some(v) => v,
            None => return false,
        };
        let live_shape = match shape_at(src) {
            Some(v) => v,
            None => return false,
        };
        if dead_shape != live_shape {
            return false;
        }
        k += 1;
    }
    true
}
/// 10a 槽位纪律：WANT_NDX 前八全 0、后六恰为 1..=6；LIVE_OK 与两侧表同序。
fn slots_ok() -> bool {
    let mut i: usize = 0;
    while i < ACCEPT {
        match WANT_NDX.get(i) {
            Some(0) => {}
            _ => return false,
        }
        let lo = match LIVE_OK.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let wo = match WANT_OK.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let ao = match ACT_OK.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if lo != wo || lo != ao {
            return false;
        }
        i += 1;
    }
    while i < CORPUS {
        let want_ndx = (i - ACCEPT + 1) as u8;
        match WANT_NDX.get(i) {
            Some(v) => {
                if *v != want_ndx {
                    return false;
                }
            }
            None => return false,
        }
        if Fault::from_ndx(want_ndx).map(|f| f.code()) != REJECT_CODES.get(i - ACCEPT).copied() {
            return false;
        }
        i += 1;
    }
    // 死槽的期望成败位必须与事实面相反当且仅当它是 estate 槽；其余维同位。
    let mut k: usize = 1;
    while k < REJECT {
        let wo = match WANT_OK.get(ACCEPT + k) {
            Some(v) => *v,
            None => return false,
        };
        let ao = match ACT_OK.get(ACCEPT + k) {
            Some(v) => *v,
            None => return false,
        };
        if wo != ao {
            return false;
        }
        k += 1;
    }
    let wo0 = match WANT_OK.get(ACCEPT) {
        Some(v) => *v,
        None => return false,
    };
    let ao0 = match ACT_OK.get(ACCEPT) {
        Some(v) => *v,
        None => return false,
    };
    wo0 != ao0
}

/// 10b 红档标定：929 的算式、VALS 双表一致、注入恰中 13、渲染逐字钉死。
fn reds_ok() -> bool {
    if RED_RATE != (13 * 1000 + 14 / 2) / 14 {
        return false;
    }
    let mut i: usize = 0;
    while i < ACCEPT {
        let a = match VALS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let b = match VALS_CHECK.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if a != b {
            return false;
        }
        i += 1;
    }
    match VALS.get(7) {
        Some(v) => {
            if *v != RED_RATE {
                return false;
            }
        }
        None => return false,
    }
    let green = match run() {
        Ok(r) => r,
        Err(_) => return false,
    };
    if green.rate() != RATE_FULL || !green.green() {
        return false;
    }
    if green.render().as_str() != RENDER_OK {
        return false;
    }
    let red = match run_with(Some((6, Fault::Estate))) {
        Ok(r) => r,
        Err(_) => return false,
    };
    if red.matched != 13 || red.rate() != RED_RATE || red.green() {
        return false;
    }
    if red.total != green.total {
        return false;
    }
    match run_with(Some((CORPUS, Fault::Code))) {
        Err(e) => e == DU_SLOT,
        Ok(_) => false,
    }
}

/// 11 文案逐字：五式整行逐字符复核 + 分类函数与档位语义闭合。
fn message_ok() -> bool {
    if msg_du_denied_default().as_str() != "du: cannot access 'data/x': Permission denied" {
        return false;
    }
    if msg_du_missing_default().as_str() != "du: cannot access 'data/gone': No such file or directory" {
        return false;
    }
    if msg_du_ioerr_default().as_str() != "du: cannot access 'data/img': Input/output error" {
        return false;
    }
    if msg_du_stdout_enospc().as_str() != "du: standard output: No space left on device" {
        return false;
    }
    if msg_du_usage_opt("--bogus").as_str() != "du: unrecognized option '--bogus'" {
        return false;
    }
    if msg_du_hint().as_str() != "Try 'du --help' for more information." {
        return false;
    }
    if msg_raw_stat().as_str() != "stat: Permission denied" {
        return false;
    }
    if msg_note(Note::Clean).len() != 0 || msg_note(Note::Silent).len() != 0 {
        return false;
    }
    if msg_note(Note::AccDenied) != msg_du_denied_default() {
        return false;
    }
    if msg_note(Note::StdoutErr) != msg_du_stdout_enospc() {
        return false;
    }
    if has_quoted_operand(msg_du_denied_default().as_str()) == false
        || has_quoted_operand(msg_raw_stat().as_str())
    {
        return false;
    }
    if has_quoted_operand(msg_du_stdout_enospc().as_str()) {
        return false;
    }
    if !is_access_line(msg_du_missing_default().as_str())
        || !is_access_line(msg_du_ioerr_default().as_str())
        || is_access_line(msg_du_usage_opt("--bogus").as_str())
    {
        return false;
    }
    // 语料逐槽的 msg_for 与文案档位一致：绿槽无行、跳槽有行。
    let mut i: usize = 0;
    while i < CORPUS {
        let sh = match shape_at(i) {
            Some(v) => v,
            None => return false,
        };
        let a = define(&sh);
        let line = msg_for(&sh);
        match a.note {
            Note::Clean | Note::Silent => {
                if line.len() != 0 {
                    return false;
                }
            }
            Note::StdoutErr => {
                if !is_stdout_line(line.as_str()) {
                    return false;
                }
            }
            Note::AccDenied | Note::NoSuch | Note::IoErr => {
                if sh.tool == Tool::Raw {
                    if is_access_line(line.as_str()) {
                        return false;
                    }
                } else if !is_access_line(line.as_str()) {
                    return false;
                }
            }
            Note::Usage => {
                if !line.contains("unrecognized option") {
                    return false;
                }
            }
        }
        i += 1;
    }
    true
}

/// 12a 退出码闭合：槽位 → 退出码的满表 + 申报侧 claim_exit 的档位。
fn exit_ok() -> bool {
    let mut i: usize = 0;
    while i < CORPUS {
        let sh = match shape_at(i) {
            Some(v) => v,
            None => return false,
        };
        let e = exit_code(&sh);
        if e > EXIT_USAGE {
            return false;
        }
        if sh.bad_opt != (e == EXIT_USAGE) {
            return false;
        }
        if (define(&sh).code == 0) != (e == EXIT_OK) {
            return false;
        }
        i += 1;
    }
    // 满盘的绿槽必须退 0（J1 的退出码形态）：槽 0/2 与槽 5 全在满盘现场。
    let g0 = match shape_at(0) {
        Some(v) => v,
        None => return false,
    };
    let g2 = match shape_at(2) {
        Some(v) => v,
        None => return false,
    };
    let d5 = match shape_at(5) {
        Some(v) => v,
        None => return false,
    };
    if !(g0.disk_full && g2.disk_full && d5.disk_full) {
        return false;
    }
    if exit_code(&g0) != EXIT_OK || exit_code(&g2) != EXIT_OK {
        return false;
    }
    if exit_code(&d5) != EXIT_FAIL {
        return false;
    }
    // claim_exit 三档：兜底→2，成→0，败→1。
    let mut k: usize = 0;
    while k < CORPUS {
        let c = match claim_at(k) {
            Some(v) => v,
            None => return false,
        };
        let e = claim_exit(&c);
        if c.default {
            if e != EXIT_USAGE {
                return false;
            }
        } else if e != c.exit_hint() {
            return false;
        }
        k += 1;
    }
    true
}

/// 12b 阶段闸门：单调不倒退；倒退 → DU_STAGE；封印后 step/close → DU_STATE；
/// 提前封印 → DU_STAGE；指纹非零且确定。
fn gate_ok() -> bool {
    let mut g = Gate::new();
    if g.steps() != 0 || g.closed() {
        return false;
    }
    if g.walk_all() != Ok(STAGE_COUNT) {
        return false;
    }
    if !g.closed() || g.falls() != 0 {
        return false;
    }
    let s1 = g.seal();
    if s1 == 0 || s1 != g.seal() {
        return false;
    }
    if g.step(Stage::Exit) != Err(DU_STATE) || g.close() != Err(DU_STATE) {
        return false;
    }
    let mut h = Gate::new();
    if h.close() != Err(DU_STAGE) {
        return false;
    }
    if h.step(Stage::Call) != Ok(()) || h.step(Stage::Print) != Ok(()) {
        return false;
    }
    if h.step(Stage::Stat) != Err(DU_STAGE) {
        return false;
    }
    if h.falls() != 1 || h.last_rank() != Stage::Print.rank() as i32 {
        return false;
    }
    let mut p = Gate::new();
    if p.step(Stage::Total) != Ok(()) || p.close() != Err(DU_STAGE) {
        return false;
    }
    true
}

/// 13 账本：建账逐格对平；槽序纪律；冻结拒登；重放同指纹；篡改留痕。
fn ledger_ok() -> bool {
    let mut l = build_ledger();
    if l.len() != SAMPLES || l.is_empty() || l.rejects() != 0 {
        return false;
    }
    if l.ok_count() != ACCEPT || l.err_count() != REJECT {
        return false;
    }
    if l.audit() != Ok(()) {
        return false;
    }
    let c0 = match l.sample(0) {
        Some(v) => v,
        None => return false,
    };
    if c0.value != 5 || c0.op != OP_STAT {
        return false;
    }
    let c7 = match l.sample(7) {
        Some(v) => v,
        None => return false,
    };
    if c7.value != RED_RATE || c7.op != OP_RATE {
        return false;
    }
    let c13 = match l.sample(13) {
        Some(v) => v,
        None => return false,
    };
    if c13.code != DU_FAULT_DEFAULT || c13.value != 0 || c13.ok {
        return false;
    }
    if l.sample(SAMPLES).is_some() {
        return false;
    }
    // 跳号登记必须被拒且不入账。
    if l.register(Sample::stat(13, 7)) != Err(DU_SLOT) || l.len() != SAMPLES {
        return false;
    }
    if l.rejects() != 1 {
        return false;
    }
    let root = match l.freeze() {
        Ok(r) => r,
        Err(_) => return false,
    };
    if root == 0 || !l.is_frozen() {
        return false;
    }
    if l.freeze() != Err(DU_STATE) || l.register(Sample::stat(13, 1)) != Err(DU_SLOT) {
        return false;
    }
    if l.replay() != Ok(root) {
        return false;
    }
    if l.root() != root {
        return false;
    }
    // 未走满就冻结 → DU_STAGE；篡改一格后重放必须指纹不符。
    let mut short = Ledger::new();
    if short.register(Sample::stat(0, 5)) != Ok(()) || short.freeze() != Err(DU_STAGE) {
        return false;
    }
    let mut t = build_ledger();
    if t.tamper(7, 928) != Ok(()) {
        return false;
    }
    match t.freeze() {
        Ok(_) => return false,
        Err(e) => {
            if e != DU_VALUE {
                return false;
            }
        }
    }
    // 冻结前篡改则重放必须红。
    let mut t2 = build_ledger();
    if t2.audit() != Ok(()) {
        return false;
    }
    if t2.tamper(2, 999) != Ok(()) {
        return false;
    }
    let fresh = build_ledger();
    if t2.seal() == fresh.seal() {
        return false;
    }
    true
}

// ---------------------------------------------------------------------------
// 自检出口：13 项，逐项对应一个错误码维
// ---------------------------------------------------------------------------

/// 本卡自检：13 项，逐项对应一个错误码维。
///
/// 断言方向：每一项都是"判据本身成立"，因此任何一维退化都变红；`add` 只走
/// `add(name, bool, "static")`，不塞动态明细，域聚合渲染保持零分配。
pub fn run_unx11692_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11692_du_readonly_stat");

    s.add("DU01-名称与段基址", name_ok(), "static");

    s.add("DU02-枚举与九闸", enums_ok(), "static");

    s.add("DU03-取整域恒等式", round_ok(), "static");

    s.add("DU04-形状合法域", shape_ok(), "static");

    s.add("DU05-九闸纵线", define_ok(), "static");

    s.add("DU06-读面免疫", ro_ok(), "static");

    s.add("DU07-去重分野", dedup_ok(), "static");

    s.add("DU08-申报对判", judge_ok(), "static");

    s.add("DU09-矩阵四档", matrix_ok(), "static");

    s.add("DU10-语料与死槽", corpus_ok() && slots_ok(), "static");

    s.add("DU11-槽位与红档", reds_ok(), "static");

    s.add("DU12-文案与退出码", message_ok() && exit_ok() && gate_ok(), "static");

    s.add(
        "DU13-冻结重放四注入",
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
// __C10__
