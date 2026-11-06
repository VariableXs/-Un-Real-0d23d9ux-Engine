//! UNX-F11684 tee ENOSPC 半程失败判据
//!
//! `tee` 是唯一一条"同一个流写两个目的地"的命令：一份进文件、一份进标准输出。
//! 于是 ENOSPC 在它身上不再是"写挂了"这么单薄，而是**半程失败**（half write）：
//! 一个 sink 满了、另一个 sink 还在正常吃数据，两条支路的字节数从此分叉。这张卡
//! 判的就是这个分叉，以及围绕分叉的四件常被糊过去的事：
//!
//! * 文件被截断（短写到 ENOSPC 位置为止），而 stdout 早就把数据吐完了——两边都
//!   "没错"，但合起来是一个不完整的事实面；
//! * 退出码可以仍然是 0（POSIX 只要求 tee 在**全部**目标失败时才必然非零，很多
//!   实现只把"任一目标失败"折算成 1，而把"还剩一个目标活着"当成成功续写）；
//! * stdout 未必是文件——它是管道时，塞满它的不是 ENOSPC(28) 而是 EPIPE(32)，
//!   GNU tee 把 SIGPIPE 设成 SIG_IGN 所以拿得到 EPIPE 并退 1，busybox tee 不管
//!   SIGPIPE，直接被信号打死，shell 侧看到 128+13=141；
//! * 文案的归属不同：GNU 给文件操作数加单引号（`tee: 'file': No space left on
//!   device`），给 stdout 用固定名且不加引号（`tee: standard output: No space
//!   left on device`），busybox 干脆不分目的地（`tee: write error: No space left
//!   on device`）。三式混认，半程失败就会被读成全程失败或读成没失败。
//!
//! ## 闸门次序（这条纵线为什么不能倒）
//!
//! 本卡的九道闸按下面这个序走，秩从 1 起算，前一闸不产出事实，后一闸就没有输入。
//! `Gate` 只允许"前进到下一秩"，倒退与跳档都要留下指纹痕迹，封印之后一律拒动。
//!
//! 1. 参数闸（Args，秩 1）——先判命令行本身。`-a`、`-i`、`-p`、
//!    `--output-error=MODE` 决定了后面所有闸的走法：`-a` 决定打开方式是 O_APPEND
//!    还是 O_TRUNC（直接改变可用空间），`-p`/`--output-error=exit` 决定"一个 sink
//!    坏了要不要立刻整体退"。用法错在这里就退 2，一个字节都没动过，后面八闸根本
//!    不存在事实可判。
//! 2. 打开闸（Open，秩 2）——`tee file` 会把 file 截断（O_TRUNC），截掉的那些字节
//!    当场变成可用空间；`tee -a file` 不让位，从 EOF 往后写。同一条流、同一块盘，
//!    `-a` 能 ENOSPC 而裸 `tee` 不能。这一闸必须在分配之前，因为"还剩多少空间"
//!    这个数就是被打开方式改写的。
//! 3. 写序闸（Order，秩 3）——GNU tee 逐缓冲区写，每个缓冲区先写文件支路、再写
//!    stdout 支路。次序决定了半程失败的形状：stdout 后来 ENOSPC 时文件已经**完整**
//!    （只有 stdout 缺尾）；文件先 ENOSPC 时 stdout 还能续到 EOF（只有文件被截）。
//!    次序一颠倒，"谁残缺"这个结论就整个反过来，所以它必须先于分账被钉死。
//! 4. 分账闸（Split，秩 4）——把 total 字节在两个 sink 之间分成
//!    `file_written`/`stdout_written`，两条都到 EOF 才算全量，只有一条到就是半程。
//!    本卡的判据量 `value` 就是这个分叉的宽度（两支路字节差），不是"丢了几个字节"
//!    这种含混说法：文件截 32 而 stdout 全量，宽度是 32；两支路一起停在 0，宽度是
//!    0——宽度 0 却失败，正是"全程失败伪装成半程失败"的指纹。
//! 5. 归因闸（Errno，秩 5）——把失败落到具体的 errno 家族：设备满 ENOSPC(28)、
//!    配额尽 EDQUOT(122)、管道断 EPIPE(32)。三族在"写不下去"这个表象上是同一张
//!    脸，处置完全不同：EDQUOT 换个目录就能写，ENOSPC 换目录不一定，EPIPE 根本不是
//!    空间问题。`--output-error=exit` 类模式在这一闸就把整个进程带走，纵线到此为止。
//! 6. 文案闸（Msg，秩 6）——按实现口味与坏掉的支路选文案，判"报的是哪一支坏了"。
//!    它在归因之后：文案是 errno 的下游产物；它在退出之前：一条把文件操作数写成
//!    不带引号的 busybox 句式，会让人误判"坏的是 stdout"，先把文案钉住，退出码
//!    的归因才不会被人脑补带偏。
//! 7. 退出闸（Exit，秩 7）——结算 0/1/2：0 = 两支路都到 EOF；1 = 任一 sink 写失败
//!    （半程失败在这里必然露出来）；2 = 用法错，一个字节都没写。再加一条 shell 侧
//!    状态：busybox 遇管道死成 141。short write 与退出码的关系是本卡的第二层：
//!    "宽度 > 0 却退 0" 是本卡判得最死的一格谎报。
//! 8. 结算闸（Final，秩 8）——只有循环跑到 EOF 才进这一档。它落的是"文件到底留下
//!    多少个字节"这个不可逆事实（截断已写盘，回滚不可能），所以它必须在退出码之后：
//!    退出码是可重试的，残缺文件不是。
//! 9. 封印闸（Closed，秩 9）——账本冻结、指纹比对。没走完前八闸不许封印；封印之后
//!    任何 step/close 都拒。它是"这一轮判据已经出口"的唯一凭据。
//!
//! ## 关键差异（本卡深度所在）
//!
//! - 与 F11681（ENOSPC 总锚，段基址 `0x6d130`）的边界：总锚判的是**单目的地**的
//!   空间耗尽——一条写路径、一个 errno、一个结论，它的纵线是"空间账"。本卡判的是
//!   **双目的地**的分叉，空间账只是五条闸之一；本卡的主轴是"半程"这个形状：同一份
//!   输入在两处落地成两个不同的字节数。总锚允许"失败即整体失败"，本卡不允许——
//!   本卡把"一支坏一支好"单独设档（Half），并且要求这一档能给出宽度。两卡段位在
//!   全仓分配表上相差 0x30，本卡用自声明的锚常量署名对齐，不 import 对方任何符号。
//! - 与 F11682（cp 中止，段基址 `0x6d140`）的边界：cp 只有一个目的地，ENOSPC 一到
//!   就中止，留下的残件与错误码一一对照，"中止点"是它的主判据；本卡没有"中止"这个
//!   全局事件——`tee` 缺省策略是丢掉坏掉的支路、继续喂活着的支路，所以本卡的
//!   Stage 维要判的是**有没有**中止（`--output-error=exit` 才中止），而 cp 卡不必
//!   要这一维。两卡段位相差 0x20，本卡同样用自声明锚常量署名。
//! - 与 F11683（本卡前置，段基址 `0x6d150`）的边界：差 0x10，前置卡管管道链的整体
//!   退出码，本卡管链上 tee 这一节点的两支路分账。
//! - 半程 ≠ 全损，也 ≠ 成功：这是本卡最深的一格。Exit 0 + 文件截断是谎报（Estate
//!   维）；Exit 1 + 宽度 0 是全损被误记成半程（Value 维）；Exit 1 + 宽度 > 0 但文案
//!   指向错误的支路，是归因错（Note 维）。三种形态各占本卡一条死槽。
//! - stdout 不是第二个文件：它是管道时，塞满它的是读者的消失而不是磁盘的空闲，
//!   errno 家族与处置都不同。本卡把这一格显式立成活槽（`epipe-stdout`），因为把
//!   EPIPE 认成 ENOSPC 是本卡最容易骗过一个总锚卡的错。
//! - 文案引号是判据不是排版：GNU 给操作数加引号、给 stdout 用固定名，busybox 两边
//!   不分。三式的字节面必须逐字钉住，否则"哪个 sink 坏了"就无法从日志复原。
//! - 红档率 929 独占账本一格：它是 14 格里恰好中 13 格时的千分率
//!   `(13*1000 + 14/2) / 14 = 929`，是一个比率标定，不许并进 7 个统计格当作计数。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 本模块段位基址（全仓唯一，取自分配表）。
pub const SEG_BASE: u32 = 0x6d160;

/// 交叉署名：F11681「ENOSPC 总锚」的段基址。本卡**不 import** 那个模块的任何符号，
/// 只把它的段位与位距原样声明一遍，由 `anchors_agree()` 复算对齐。
pub const ANCHOR_F11681_SEG: u32 = 0x6d130;
/// F11681 与本卡的位距（0x6d160 - 0x6d130）。
pub const ANCHOR_F11681_GAP: u32 = 0x30;
/// 交叉署名：F11682「cp ENOSPC 中止」的段基址。
pub const ANCHOR_F11682_SEG: u32 = 0x6d140;
/// F11682 与本卡的位距（0x6d160 - 0x6d140）。
pub const ANCHOR_F11682_GAP: u32 = 0x20;
/// 交叉署名：F11683（本卡前置）的段基址。
pub const ANCHOR_F11683_SEG: u32 = 0x6d150;
/// F11683 与本卡的位距（0x6d160 - 0x6d150）。
pub const ANCHOR_F11683_GAP: u32 = 0x10;

/// 与 F11681 总锚在 errno 面上的署名：ENOSPC 在 Linux 就是 28，两条卡必须同数。
pub const ANCHOR_ENOSPC: u32 = 28;
/// 与 F11681/F11682 在文案面上的署名：三条卡都只能认这一串 strerror 原文。
pub const ANCHOR_ERRNO_TEXT: &str = "No space left on device";

/// 错误码 01：槽位越界/跳号——`run_with` 的注入槽超出语料、或账本登记时
/// `slot != len`（半程失败的账最容易在这里少记一格）。
pub const TE_SLOT: u32 = SEG_BASE + 0x01;
/// 错误码 02：状态违例——账本已冻结还登记、闸门已封印还 step/close，
/// 以及全绿运行里出现未命中的槽。
pub const TE_STATE: u32 = SEG_BASE + 0x02;
/// 错误码 03：身份撞格——错误码字面量重复、段位与锚位距不闭合，
/// 以及"两支路同 errno 同位置齐坏"被误记成半程（其实全损）。
pub const TE_UNIQ: u32 = SEG_BASE + 0x03;
/// 错误码 04：阶段倒退/越序——申报的阶段与本卡纵线产出的阶段不合。
pub const TE_STAGE: u32 = SEG_BASE + 0x04;
/// 错误码 05：errno 家族判错——EDQUOT 当 ENOSPC、ENOSPC 当 EPIPE 都挂这条。
pub const TE_CODE: u32 = SEG_BASE + 0x05;
/// 错误码 06：数值维不合——半程宽度（两支路字节差）算错或缺尾字节数虚报。
pub const TE_VALUE: u32 = SEG_BASE + 0x06;
/// 错误码 07：文案维不合——GNU 引号式、GNU stdout 式、busybox write error 式认错。
/// 归因闸本身判对了，但把坏掉的支路指错了地方。
pub const TE_NOTE: u32 = SEG_BASE + 0x07;
/// 错误码 08：兜底档误走——事实面本可判定却被丢进"未知实现/未知 errno"垃圾桶。
pub const TE_FAULT_DEFAULT: u32 = SEG_BASE + 0x08;
/// 错误码 09：容量维不合——文件侧可用空间（含 O_TRUNC 让位）与实写得数不闭合。
pub const TE_CAP: u32 = SEG_BASE + 0x09;
/// 错误码 10：写序不合——stdout 侧坏掉而文件侧本应完整；或跳档前进。
pub const TE_ORDER: u32 = SEG_BASE + 0x0A;
/// 错误码 11：append 语义不合——`-a` 不给 O_TRUNC 让位，却按裸 tee 的空间算账。
pub const TE_APPEND: u32 = SEG_BASE + 0x0B;
/// 错误码 12：退出码档位不合——0/1/2 三档落错，或用例未跑就报 1。
pub const TE_EXIT: u32 = SEG_BASE + 0x0C;
/// 错误码 13：短写与退出码断链——短写宽度与 exit 状态互相矛盾（截断却退 0）。
pub const TE_SHORT: u32 = SEG_BASE + 0x0D;

/// 全部 13 个错误码的枚举表（供互异性与值域复核，顺序即 0x01..0x0D）。
pub const ALL_CODES: [u32; 13] = [
    TE_SLOT,
    TE_STATE,
    TE_UNIQ,
    TE_STAGE,
    TE_CODE,
    TE_VALUE,
    TE_NOTE,
    TE_FAULT_DEFAULT,
    TE_CAP,
    TE_ORDER,
    TE_APPEND,
    TE_EXIT,
    TE_SHORT,
];

/// 错误码总数（本卡固定 13）。
pub const CODE_COUNT: usize = 13;

/// 语料槽数：8 个活形态 + 6 个死形态。
pub const CORPUS: usize = 14usize;
/// 账本登记样本数（与语料同宽，一格不落）。
pub const SAMPLES: usize = 14usize;
/// 活形态数（两支路分账的正例）。
pub const ACCEPT: usize = 8usize;
/// 死形态数（一槽只坏一维，依次挂 Estate/Stage/Code/Value/Note/Default）。
pub const REJECT: usize = 6usize;
/// 档位维数（全量 / 半程 / 全损 / 未起跑）。
pub const TIER_COUNT: usize = 4usize;
/// 纵线闸数（含封印闸）。
pub const STAGE_COUNT: u32 = 9u32;

/// 两支路的数量——`tee` 的定义就是"一份流两个目的地"。
pub const SINK_COUNT: u32 = 2u32;
/// 参与退出的码位（0/1/2）。
pub const EXIT_RANKS: u32 = 3u32;

/// Linux errno：ENOSPC = 28，设备/文件系统无空间。
pub const ENOSPC: u32 = 28;
/// Linux errno：EPIPE = 32，管道读者已消失（写端被拒）。
pub const EPIPE: u32 = 32;
/// Linux errno：EACCES = 13，打开阶段的权限拒（本卡只作为对照存在）。
pub const EACCES: u32 = 13;
/// Linux errno：EDQUOT = 122，配额耗尽——看着像 ENOSPC，判的却完全是另一件事。
pub const EDQUOT: u32 = 122;

/// GNU tee 的退出码：两支路都到 EOF。
pub const EXIT_OK: u32 = 0;
/// GNU tee 的退出码：任一 sink 写失败（含半程失败）。
pub const EXIT_FAIL: u32 = 1;
/// GNU tee 的退出码：用法/参数错，一个字节都没写。
pub const EXIT_USAGE: u32 = 2;
/// SIGPIPE 的信号号。
pub const SIGPIPE_NO: u32 = 13;
/// busybox tee 被 SIGPIPE 打死时 shell 侧看到的值（128 + 13）。
pub const SHELL_DIED: u32 = 141;

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

/// u32 → 十六进制文本，宽度钳到 0..=8，不足位补零（小写，贴 strerror/errno 习惯）。
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

/// 段位文本：`0x6d160` 形态（本卡所有段位都以此形制出现，便于跨卡比对）。
pub fn seg_text(v: u32) -> String {
    let mut s = String::new();
    s.push_str("0x");
    s.push_str(hex_text(v, 6).as_str());
    s
}

/// 把一串键值拼成 `k=v` 形式接到 out 后面（渲染共用，缺键则原样不接）。
pub fn kv(out: &mut String, key: &str, val: u32) {
    out.push_str(key);
    out.push('=');
    out.push_str(dec_text(val).as_str());
}

/// 追加一个分隔空格。
pub fn sep(out: &mut String) {
    out.push(' ');
}

/// 纵线阶段（本卡的九道闸）。秩从 1 起算；0 保留给"未起跑"的初值，
/// 只有 `Gate::new()` 才处于 0，任何真实产出的 Outcome 都必须落在 1..=9。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// 秩 1：参数闸——命令行本身。
    Args,
    /// 秩 2：打开闸——O_TRUNC 让位与否（`-a` 的分水岭）。
    Open,
    /// 秩 3：写序闸——文件支路先写、stdout 支路后写。
    Order,
    /// 秩 4：分账闸——两支路各吃到多少字节。
    Split,
    /// 秩 5：归因闸——errno 家族（ENOSPC/EDQUOT/EPIPE）。
    Errno,
    /// 秩 6：文案闸——坏掉的是哪一支，按哪套实现的句式报。
    Msg,
    /// 秩 7：退出闸——0/1/2 与 short write 的结算。
    Exit,
    /// 秩 8：结算闸——循环跑到 EOF，残缺文件已成事实。
    Final,
    /// 秩 9：封印闸——账本冻结，本轮判据出口。
    Closed,
}

impl Stage {
    /// 全部九道闸，按纵线次序。
    pub const ALL: [Stage; 9] = [
        Stage::Args,
        Stage::Open,
        Stage::Order,
        Stage::Split,
        Stage::Errno,
        Stage::Msg,
        Stage::Exit,
        Stage::Final,
        Stage::Closed,
    ];

    /// 秩（1 起算）。
    pub fn rank(&self) -> u32 {
        match self {
            Stage::Args => 1,
            Stage::Open => 2,
            Stage::Order => 3,
            Stage::Split => 4,
            Stage::Errno => 5,
            Stage::Msg => 6,
            Stage::Exit => 7,
            Stage::Final => 8,
            Stage::Closed => 9,
        }
    }

    /// 短名（渲染与判据名共用）。
    pub fn slug(&self) -> &'static str {
        match self {
            Stage::Args => "args",
            Stage::Open => "open",
            Stage::Order => "order",
            Stage::Split => "split",
            Stage::Errno => "errno",
            Stage::Msg => "msg",
            Stage::Exit => "exit",
            Stage::Final => "final",
            Stage::Closed => "closed",
        }
    }

    /// 中文名（自检明细与文档共用）。
    pub fn cn(&self) -> &'static str {
        match self {
            Stage::Args => "参数闸",
            Stage::Open => "打开闸",
            Stage::Order => "写序闸",
            Stage::Split => "分账闸",
            Stage::Errno => "归因闸",
            Stage::Msg => "文案闸",
            Stage::Exit => "退出闸",
            Stage::Final => "结算闸",
            Stage::Closed => "封印闸",
        }
    }

    /// 由秩还原阶段；秩 0 与秩 >9 都不合法（未起跑不是阶段）。
    pub fn from_rank(r: u32) -> Option<Stage> {
        match r {
            1 => Some(Stage::Args),
            2 => Some(Stage::Open),
            3 => Some(Stage::Order),
            4 => Some(Stage::Split),
            5 => Some(Stage::Errno),
            6 => Some(Stage::Msg),
            7 => Some(Stage::Exit),
            8 => Some(Stage::Final),
            9 => Some(Stage::Closed),
            _ => None,
        }
    }

    /// 下一道闸；封印闸没有下一道。
    pub fn next(&self) -> Option<Stage> {
        Stage::from_rank(self.rank().saturating_add(1))
    }

    /// 上一道闸；参数闸没有上一道——纵线不允许回退，这个方法只为
    /// `Gate` 的拒动诊断存在，判据本身绝不调用它前进。
    pub fn prev(&self) -> Option<Stage> {
        let r = self.rank();
        if r <= 1 {
            None
        } else {
            Stage::from_rank(r.saturating_sub(1))
        }
    }

    /// 是否属于 `define` 能真实产出的落点（Args/Errno/Final 三点）。
    pub fn is_landing(&self) -> bool {
        matches!(self, Stage::Args | Stage::Errno | Stage::Final)
    }
}

/// 两支路的身份。`tee` 的半程失败必须能说清坏的是哪一支。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SinkKind {
    /// 秩 1：文件操作数支路（GNU 文案里带单引号的那一个）。
    File,
    /// 秩 2：标准输出支路（可能是文件、可能是终端、也可能是管道）。
    Stdout,
    /// 秩 3：两支路同坏（全损，不是半程）。
    Both,
    /// 秩 4：都没坏。
    None,
}

impl SinkKind {
    /// 全部四格。
    pub const ALL: [SinkKind; 4] = [SinkKind::File, SinkKind::Stdout, SinkKind::Both, SinkKind::None];

    /// 秩。
    pub fn rank(&self) -> u32 {
        match self {
            SinkKind::File => 1,
            SinkKind::Stdout => 2,
            SinkKind::Both => 3,
            SinkKind::None => 4,
        }
    }

    /// 短名。
    pub fn slug(&self) -> &'static str {
        match self {
            SinkKind::File => "file",
            SinkKind::Stdout => "stdout",
            SinkKind::Both => "both",
            SinkKind::None => "none",
        }
    }

    /// 由秩还原。
    pub fn from_rank(r: u32) -> Option<SinkKind> {
        match r {
            1 => Some(SinkKind::File),
            2 => Some(SinkKind::Stdout),
            3 => Some(SinkKind::Both),
            4 => Some(SinkKind::None),
            _ => None,
        }
    }

    /// 坏掉的是哪一支（分账闸的产出）。
    pub fn casualty(&self) -> bool {
        matches!(self, SinkKind::File | SinkKind::Stdout | SinkKind::Both)
    }
}

/// 实现口味——文案与信号处置都由它决定。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flavor {
    /// 秩 1：GNU coreutils tee（操作数加单引号，SIGPIPE 设为 SIG_IGN）。
    Gnu,
    /// 秩 2：busybox tee（不分目的地，`write error:` 一句走天下，不管 SIGPIPE）。
    Busybox,
    /// 秩 3：别家（Toybox/BSD/…）——句式不属本卡四式，落 Foreign 文案格。
    Foreign,
    /// 秩 4：认不出来——本卡判不了，只能进兜底档。
    Unknown,
}

impl Flavor {
    /// 全部四格。
    pub const ALL: [Flavor; 4] =
        [Flavor::Gnu, Flavor::Busybox, Flavor::Foreign, Flavor::Unknown];

    /// 秩。
    pub fn rank(&self) -> u32 {
        match self {
            Flavor::Gnu => 1,
            Flavor::Busybox => 2,
            Flavor::Foreign => 3,
            Flavor::Unknown => 4,
        }
    }

    /// 短名。
    pub fn slug(&self) -> &'static str {
        match self {
            Flavor::Gnu => "gnu",
            Flavor::Busybox => "busybox",
            Flavor::Foreign => "foreign",
            Flavor::Unknown => "unknown",
        }
    }

    /// 由秩还原。
    pub fn from_rank(r: u32) -> Option<Flavor> {
        match r {
            1 => Some(Flavor::Gnu),
            2 => Some(Flavor::Busybox),
            3 => Some(Flavor::Foreign),
            4 => Some(Flavor::Unknown),
            _ => None,
        }
    }

    /// 是否屏蔽了 SIGPIPE（GNU 屏蔽，其余按未屏蔽处理——本卡的 141 判据由此而来）。
    pub fn ignores_sigpipe(&self) -> bool {
        matches!(self, Flavor::Gnu)
    }

    /// 本卡能否认出它的文案句式。
    pub fn is_legible(&self) -> bool {
        matches!(self, Flavor::Gnu | Flavor::Busybox)
    }
}

/// 文案档位（文案闸的产出，本卡四式 + 两个特殊格）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Note {
    /// 秩 1：没有写出错误（全量成功）。
    Clean,
    /// 秩 2：GNU 的文件操作数式——`tee: 'file': No space left on device`（带引号）。
    GnuFile,
    /// 秩 3：GNU 的标准输出式——`tee: standard output: No space left on device`（不带引号）。
    GnuStdout,
    /// 秩 4：busybox 式——`tee: write error: No space left on device`（不分目的地）。
    Busy,
    /// 秩 5：句式不属本卡四式（配额超限、别家实现、未知实现）。
    Foreign,
    /// 秩 6：用法错——命令行就没对，压根没有写出错误。
    Usage,
}

impl Note {
    /// 全部六格。
    pub const ALL: [Note; 6] = [
        Note::Clean,
        Note::GnuFile,
        Note::GnuStdout,
        Note::Busy,
        Note::Foreign,
        Note::Usage,
    ];

    /// 秩。
    pub fn rank(&self) -> u32 {
        match self {
            Note::Clean => 1,
            Note::GnuFile => 2,
            Note::GnuStdout => 3,
            Note::Busy => 4,
            Note::Foreign => 5,
            Note::Usage => 6,
        }
    }

    /// 短名。
    pub fn slug(&self) -> &'static str {
        match self {
            Note::Clean => "clean",
            Note::GnuFile => "gnu-file",
            Note::GnuStdout => "gnu-stdout",
            Note::Busy => "busy-write-error",
            Note::Foreign => "foreign",
            Note::Usage => "usage",
        }
    }

    /// 由秩还原。
    pub fn from_rank(r: u32) -> Option<Note> {
        match r {
            1 => Some(Note::Clean),
            2 => Some(Note::GnuFile),
            3 => Some(Note::GnuStdout),
            4 => Some(Note::Busy),
            5 => Some(Note::Foreign),
            6 => Some(Note::Usage),
            _ => None,
        }
    }

    /// 这一格是否指向文件支路（GNU 带引号的那一行）。
    pub fn blames_file(&self) -> bool {
        matches!(self, Note::GnuFile)
    }

    /// 这一格是否指向标准输出支路。
    pub fn blames_stdout(&self) -> bool {
        matches!(self, Note::GnuStdout)
    }

    /// 这一格是否根本没说出坏在哪一支（busybox 式的先天缺陷）。
    pub fn is_blind(&self) -> bool {
        matches!(self, Note::Busy)
    }

    /// 这一格是否算"报出了错误"。
    pub fn reports_error(&self) -> bool {
        !matches!(self, Note::Clean)
    }
}

/// 档位维（矩阵分档用，本卡四档）。判据的核心在于 Half 与 Both 的分野：
/// 半程失败必须有宽度，全损的宽度是 0——宽度 0 却报半程，就是本卡要抓的冒充。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// 秩 1：全量——两支路都吃到 EOF，退出码 0。
    Full,
    /// 秩 2：半程——恰好一支坏，字节分叉宽度 > 0。
    Half,
    /// 秩 3：全损——两支路一起坏，分叉宽度为 0。
    Both,
    /// 秩 4：未起跑——用法/打开失败，流没进循环。
    Usage,
}

impl Tier {
    /// 全部四档。
    pub const ALL: [Tier; 4] = [Tier::Full, Tier::Half, Tier::Both, Tier::Usage];

    /// 秩。
    pub fn rank(&self) -> u32 {
        match self {
            Tier::Full => 1,
            Tier::Half => 2,
            Tier::Both => 3,
            Tier::Usage => 4,
        }
    }

    /// 短名。
    pub fn slug(&self) -> &'static str {
        match self {
            Tier::Full => "full",
            Tier::Half => "half",
            Tier::Both => "both",
            Tier::Usage => "usage",
        }
    }

    /// 由秩还原。
    pub fn from_rank(r: u32) -> Option<Tier> {
        match r {
            1 => Some(Tier::Full),
            2 => Some(Tier::Half),
            3 => Some(Tier::Both),
            4 => Some(Tier::Usage),
            _ => None,
        }
    }

    /// 分档规则：这是本卡把"事实面"折成"一档"的唯一出口。
    /// 次序不可换——用法档看阶段（循环压根没起跑），全损档看归因码
    /// （两支路同报 TE_UNIQ：齐坏即全损，不管停在哪个水位），剩下的才是半程。
    /// 半程与全损的**宽度**关系另由 `half_ok()` 复核：本卡语料里的全损一律齐停在
    /// 同一水位，所以宽度归零；半程必然有一支到 EOF，所以宽度严格大于 0。
    pub fn of(actual: &Outcome) -> Tier {
        if actual.stage == Stage::Args {
            return Tier::Usage;
        }
        if actual.code == 0 {
            return Tier::Full;
        }
        if actual.code == TE_UNIQ {
            return Tier::Both;
        }
        Tier::Half
    }

    /// 这一档是否属于"半程失败"这一族（本卡的靶心）。
    pub fn is_half_family(&self) -> bool {
        matches!(self, Tier::Half | Tier::Both)
    }
}

/// 判定维序（judge 的六维 + Ok）。次序固定：Estate → Stage → Code → Value →
/// Note → Default → Ok。Estate 最先：成败互报（截断却退 0）是所有谎报里最贵的。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// 秩 0：申报与事实六维全合。
    Ok,
    /// 秩 1：成败互报。
    Estate,
    /// 秩 2：阶段不合。
    Stage,
    /// 秩 3：errno 归因不合。
    Code,
    /// 秩 4：分叉宽度不合。
    Value,
    /// 秩 5：文案归属不合。
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
            Fault::Estate => TE_STATE,
            Fault::Stage => TE_STAGE,
            Fault::Code => TE_CODE,
            Fault::Value => TE_VALUE,
            Fault::Note => TE_NOTE,
            Fault::Default => TE_FAULT_DEFAULT,
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
            Fault::Estate => "文件已截断却报成功（退出码谎报为 0）",
            Fault::Stage => "循环被 --output-error=exit 带走却报成跑到 EOF",
            Fault::Code => "EDQUOT 记成 ENOSPC，把配额当磁盘满",
            Fault::Value => "半程宽度虚报成全量丢失",
            Fault::Note => "GNU 带引号的文件式认成 busybox 无差别式",
            Fault::Default => "本可判定的全量成功被丢进兜底档",
        }
    }
}

/// 写出错误处置模式（GNU tee 的 `--output-error[=MODE]` 与 POSIX `-p`）。
/// 它决定本卡纵线在归因闸要不要被砍断——这是 Stage 维的全部来源。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrMode {
    /// 秩 1：缺省（等价 warn-nopipe）——管道错误只告警，空间错误告警并丢掉该支路，
    /// 另一支路续写到 EOF。半程失败正是在这一模式下发生的。
    Continue,
    /// 秩 2：warn——任何写出错误都只告警、不中止，续写。
    Warn,
    /// 秩 3：exit——任何写出错误立刻整体退出，两支路一起停在同一字节水位。
    Exit,
    /// 秩 4：exit-nopipe——非管道错误立刻退出，管道错误忽略。
    ExitNopipe,
    /// 秩 5：POSIX `-p`——诊断管道写出错误，非管道错误退出。
    PosixP,
}

impl ErrMode {
    /// 全部五格。
    pub const ALL: [ErrMode; 5] = [
        ErrMode::Continue,
        ErrMode::Warn,
        ErrMode::Exit,
        ErrMode::ExitNopipe,
        ErrMode::PosixP,
    ];

    /// 秩。
    pub fn rank(&self) -> u32 {
        match self {
            ErrMode::Continue => 1,
            ErrMode::Warn => 2,
            ErrMode::Exit => 3,
            ErrMode::ExitNopipe => 4,
            ErrMode::PosixP => 5,
        }
    }

    /// 短名（GNU 手册里的字面 MODE 名）。
    pub fn slug(&self) -> &'static str {
        match self {
            ErrMode::Continue => "default",
            ErrMode::Warn => "warn",
            ErrMode::Exit => "exit",
            ErrMode::ExitNopipe => "exit-nopipe",
            ErrMode::PosixP => "posix-p",
        }
    }

    /// 由秩还原。
    pub fn from_rank(r: u32) -> Option<ErrMode> {
        match r {
            1 => Some(ErrMode::Continue),
            2 => Some(ErrMode::Warn),
            3 => Some(ErrMode::Exit),
            4 => Some(ErrMode::ExitNopipe),
            5 => Some(ErrMode::PosixP),
            _ => None,
        }
    }

    /// 该模式会不会在第一个写出错误处整体中止。
    pub fn aborts(&self) -> bool {
        matches!(self, ErrMode::Exit | ErrMode::ExitNopipe | ErrMode::PosixP)
    }

    /// 该模式是否把管道错误豁免掉（exit-nopipe 与 `-p` 豁免，exit 不豁免）。
    pub fn excuses_pipe(&self) -> bool {
        matches!(self, ErrMode::ExitNopipe | ErrMode::PosixP)
    }
}

/// errno 家族名（Linux strerror 口径）。
pub fn errno_name(e: u32) -> &'static str {
    match e {
        0 => "success",
        ENOSPC => "No space left on device",
        EPIPE => "Broken pipe",
        EACCES => "Permission denied",
        EDQUOT => "Disk quota exceeded",
        _ => "Unknown errno",
    }
}

/// errno 是否属本卡可判家族。
pub fn errno_legible(e: u32) -> bool {
    matches!(e, 0 | ENOSPC | EPIPE | EDQUOT)
}

/// 本卡 13 个错误码的名字（诊断渲染用；越界值落 out-of-segment）。
pub fn code_name(code: u32) -> &'static str {
    if code == 0 {
        return "clean";
    }
    if code == TE_SLOT {
        "te-slot"
    } else if code == TE_STATE {
        "te-state"
    } else if code == TE_UNIQ {
        "te-uniq"
    } else if code == TE_STAGE {
        "te-stage"
    } else if code == TE_CODE {
        "te-code"
    } else if code == TE_VALUE {
        "te-value"
    } else if code == TE_NOTE {
        "te-note"
    } else if code == TE_FAULT_DEFAULT {
        "te-default"
    } else if code == TE_CAP {
        "te-cap"
    } else if code == TE_ORDER {
        "te-order"
    } else if code == TE_APPEND {
        "te-append"
    } else if code == TE_EXIT {
        "te-exit"
    } else if code == TE_SHORT {
        "te-short"
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

/// 本卡输入的全部维度——一条 `tee` 命令能被观察到的所有事实。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shape {
    /// 流宽：要写给两支路的总字节数。
    pub total: u32,
    /// 文件支路在报 errno 之前还能吃下的字节数（不含 O_TRUNC 让位）。
    pub file_free: u32,
    /// O_TRUNC 截掉旧内容所释放的字节数；`-a` 时这一笔不让位。
    pub trunc_freed: u32,
    /// stdout 支路能吃下的字节数（重定向到满盘时是剩余空间；是管道时是读者收下的量）。
    pub stdout_free: u32,
    /// 文件侧报出的 errno（0 = 不报错）。
    pub file_errno: u32,
    /// stdout 侧报出的 errno（0 = 不报错）。
    pub stdout_errno: u32,
    /// 是否 `tee -a`（O_APPEND，不截断、不让位）。
    pub append: bool,
    /// stdout 是否管道（决定 EPIPE 与 SIGPIPE 那条支线）。
    pub stdout_pipe: bool,
    /// 写出错误处置模式。
    pub mode: ErrMode,
    /// 实现口味。
    pub flavor: Flavor,
    /// 命令行本身是否错（用法档，退 2，循环未起跑）。
    pub bad_option: bool,
    /// 写序：文件支路是否排在 stdout 之前（GNU 的真实次序）。
    pub order_file_first: bool,
}

impl Shape {
    /// 一条干净的双写形态（两支路都够用）。
    pub fn clean(total: u32) -> Shape {
        Shape {
            total,
            file_free: total,
            trunc_freed: 0,
            stdout_free: total,
            file_errno: 0,
            stdout_errno: 0,
            append: false,
            stdout_pipe: true,
            mode: ErrMode::Continue,
            flavor: Flavor::Gnu,
            bad_option: false,
            order_file_first: true,
        }
    }

    /// 合法域复核：真正要判的是"报了 errno 才有短写、没报 errno 就不许短写"这条闭合。
    pub fn domain_ok(&self) -> bool {
        if self.bad_option {
            return self.file_errno == 0 && self.stdout_errno == 0 && self.trunc_freed == 0;
        }
        if self.file_free < self.total && self.file_errno == 0 {
            return false;
        }
        if self.stdout_free < self.total && self.stdout_errno == 0 {
            return false;
        }
        if self.file_room() > self.total.saturating_add(1_000_000) {
            return false;
        }
        true
    }

    /// 文件支路的实际可用空间（把 O_TRUNC 的让位算进去）。
    pub fn file_room(&self) -> u32 {
        if self.append {
            self.file_free
        } else {
            self.file_free.saturating_add(self.trunc_freed)
        }
    }

    /// 假想把 `-a` 去掉以后文件支路的可用空间——本卡用它证明"append 是唯一死因"。
    pub fn file_room_no_append(&self) -> u32 {
        self.file_free.saturating_add(self.trunc_freed)
    }

    /// stdout 支路的可用空间（`-a` 与它无关，它不是被打开的目标文件）。
    pub fn stdout_room(&self) -> u32 {
        self.stdout_free
    }

    /// 文件侧是否本来写得下（去掉 append 的让位差异之后）。
    pub fn would_fit_without_append(&self) -> bool {
        self.file_room_no_append() >= self.total
    }
}

/// 事实面（判据的应然结论）。六个维度与 Claim 一一对应。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// 纵线落点阶段。
    pub stage: Stage,
    /// errno 归因码（0 = 两支路都健康）。
    pub code: u32,
    /// 半程宽度：两支路实写字节差的绝对值。
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

    /// 成败位：错误码为 0 才是成。半程失败的错误码非 0，因此**必然**是败——
    /// 这一位是 Estate 维的锚，退出码谎报就在此处露出。
    pub fn ok(&self) -> bool {
        self.code == 0
    }

    /// 是否报出了写出错误。
    pub fn reports_error(&self) -> bool {
        self.note.reports_error()
    }

    /// 折叠五维进指纹（顺序固定：阶段→码→宽度→文案→兜底）。
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
        s.push_str("gap=");
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
    /// 申报的 errno 归因码。
    pub code: u32,
    /// 申报的分叉宽度。
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

    /// 申报折算出的退出码（只区分 0/1；2 属用法档，由 exit_code 另行给出）。
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

    /// 折叠申报六维（与 Outcome::fold 同序，供账本比对）。
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

/// 槽 1 `both-ok`：两支路都够用。64 字节流，文件剩 64，stdout（管道）收 64。
/// 这是唯一能退 0 的形态，也是全量档的基准。
const SHAPE_BOTH_OK: Shape = Shape {
    total: 64,
    file_free: 64,
    trunc_freed: 0,
    stdout_free: 64,
    file_errno: 0,
    stdout_errno: 0,
    append: false,
    stdout_pipe: true,
    mode: ErrMode::Continue,
    flavor: Flavor::Gnu,
    bad_option: false,
    order_file_first: true,
};

/// 槽 2 `file-enospc`：文件侧 32 字节就满，stdout 侧续到 64。
/// GNU 缺省策略丢掉坏支路继续喂活支路 → 半程失败，文件被截一半。
const SHAPE_FILE_ENOSPC: Shape = Shape {
    total: 64,
    file_free: 32,
    trunc_freed: 0,
    stdout_free: 64,
    file_errno: ENOSPC,
    stdout_errno: 0,
    append: false,
    stdout_pipe: true,
    mode: ErrMode::Continue,
    flavor: Flavor::Gnu,
    bad_option: false,
    order_file_first: true,
};

/// 槽 3 `stdout-enospc`：写序文件先，文件 96 全量落盘之后 stdout（重定向到满盘）
/// 停在 48。次序闸的全部意义就在这一槽：文件**完整**，缺的是尾巴上的 stdout。
const SHAPE_STDOUT_ENOSPC: Shape = Shape {
    total: 96,
    file_free: 96,
    trunc_freed: 0,
    stdout_free: 48,
    file_errno: 0,
    stdout_errno: ENOSPC,
    append: false,
    stdout_pipe: false,
    mode: ErrMode::Continue,
    flavor: Flavor::Gnu,
    bad_option: false,
    order_file_first: true,
};

/// 槽 4 `append-enospc`：盘上只剩 4096，旧文件本身占 4096。裸 `tee` 会先 O_TRUNC
/// 让位、恰好写得下；`tee -a` 不让位，4096 之后 ENOSPC。半程失败纯由打开方式造成。
const SHAPE_APPEND_ENOSPC: Shape = Shape {
    total: 8192,
    file_free: 4096,
    trunc_freed: 4096,
    stdout_free: 8192,
    file_errno: ENOSPC,
    stdout_errno: 0,
    append: true,
    stdout_pipe: true,
    mode: ErrMode::Continue,
    flavor: Flavor::Gnu,
    bad_option: false,
    order_file_first: true,
};

/// 槽 5 `epipe-stdout`：stdout 是管道，读者 256 字节后退场 → EPIPE(32)，
/// 不是 ENOSPC。GNU 屏蔽 SIGPIPE，所以拿得到 EPIPE 并照旧把文件写完（512 全量）。
const SHAPE_EPIPE_STDOUT: Shape = Shape {
    total: 512,
    file_free: 512,
    trunc_freed: 0,
    stdout_free: 256,
    file_errno: 0,
    stdout_errno: EPIPE,
    append: false,
    stdout_pipe: true,
    mode: ErrMode::Continue,
    flavor: Flavor::Gnu,
    bad_option: false,
    order_file_first: true,
};

/// 槽 6 `both-enospc`：两支路一个字节都写不进（盘满 + 重定向目标也满），
/// 且模式是 exit → 纵线在归因闸就被砍断。宽度 0 的失败不是半程，是全损。
const SHAPE_BOTH_ENOSPC: Shape = Shape {
    total: 64,
    file_free: 0,
    trunc_freed: 0,
    stdout_free: 0,
    file_errno: ENOSPC,
    stdout_errno: ENOSPC,
    append: false,
    stdout_pipe: false,
    mode: ErrMode::Exit,
    flavor: Flavor::Busybox,
    bad_option: false,
    order_file_first: true,
};

/// 槽 7 `edquot-file`：文件侧 512 之后报 EDQUOT(122)。表象与 ENOSPC 一模一样，
/// 但它是配额，换个目录就能写——归因闸判错，后面全错。
const SHAPE_EDQUOT_FILE: Shape = Shape {
    total: 1024,
    file_free: 512,
    trunc_freed: 0,
    stdout_free: 1024,
    file_errno: EDQUOT,
    stdout_errno: 0,
    append: false,
    stdout_pipe: true,
    mode: ErrMode::Continue,
    flavor: Flavor::Gnu,
    bad_option: false,
    order_file_first: true,
};

/// 槽 8 `usage-badopt`：命令行就错了（`tee -z`），退 2，循环压根没起跑。
/// 这一槽存在的意义：它让"两支路都写了 0 字节"这一事实有两种完全不同的解读。
const SHAPE_USAGE_BAD: Shape = Shape {
    total: 2048,
    file_free: 0,
    trunc_freed: 0,
    stdout_free: 0,
    file_errno: 0,
    stdout_errno: 0,
    append: false,
    stdout_pipe: false,
    mode: ErrMode::Warn,
    flavor: Flavor::Foreign,
    bad_option: true,
    order_file_first: true,
};

/// 全部 14 槽的输入形态。后 6 槽的事实面**整槽复用**前面的活形态——
/// 一槽只坏一维，坏的是申报而不是事实，这是语料纪律的硬要求。
pub const SHAPES: [Shape; CORPUS] = [
    SHAPE_BOTH_OK,
    SHAPE_FILE_ENOSPC,
    SHAPE_STDOUT_ENOSPC,
    SHAPE_APPEND_ENOSPC,
    SHAPE_EPIPE_STDOUT,
    SHAPE_BOTH_ENOSPC,
    SHAPE_EDQUOT_FILE,
    SHAPE_USAGE_BAD,
    SHAPE_FILE_ENOSPC,
    SHAPE_BOTH_ENOSPC,
    SHAPE_EDQUOT_FILE,
    SHAPE_STDOUT_ENOSPC,
    SHAPE_FILE_ENOSPC,
    SHAPE_BOTH_OK,
];

/// 14 槽的申报结论（活槽与事实面逐维相等；死槽只翻一维）。
pub const CLAIMS: [Claim; CORPUS] = [
    // 槽 1：全量成功，六维照实。
    Claim::new(true, Stage::Final, 0, 0, Note::Clean, false),
    // 槽 2：文件截半、stdout 全量，宽度 32，GNU 带引号式。
    Claim::new(false, Stage::Final, TE_CAP, 32, Note::GnuFile, false),
    // 槽 3：文件全量、stdout 缺尾，宽度 48，GNU stdout 式。
    Claim::new(false, Stage::Final, TE_ORDER, 48, Note::GnuStdout, false),
    // 槽 4：-a 未让位造成的半程，宽度 4096。
    Claim::new(false, Stage::Final, TE_APPEND, 4096, Note::GnuFile, false),
    // 槽 5：管道断，宽度 256，文件侧仍全量。
    Claim::new(false, Stage::Final, TE_SHORT, 256, Note::GnuStdout, false),
    // 槽 6：两支路齐坏 + 模式 exit 中止，宽度 0，busybox 式。
    Claim::new(false, Stage::Errno, TE_UNIQ, 0, Note::Busy, false),
    // 槽 7：配额（非盘满），宽度 512，文案落 Foreign。
    Claim::new(false, Stage::Final, TE_CODE, 512, Note::Foreign, false),
    // 槽 8：用法错，落参数闸，宽度 0。
    Claim::new(false, Stage::Args, TE_EXIT, 0, Note::Usage, false),
    // 死 1 estate-swap：事实是半程失败，申报却报成功（退出码谎成 0）。
    Claim::new(true, Stage::Final, TE_CAP, 32, Note::GnuFile, false),
    // 死 2 stage-skip：循环在归因闸就被带走，申报却说跑到了 EOF。
    Claim::new(false, Stage::Final, TE_UNIQ, 0, Note::Busy, false),
    // 死 3 code-swap：配额耗尽记成设备满。
    Claim::new(false, Stage::Final, TE_CAP, 512, Note::Foreign, false),
    // 死 4 value-off：半程 48 报成全量 96 都没落地。
    Claim::new(false, Stage::Final, TE_ORDER, 96, Note::GnuStdout, false),
    // 死 5 note-swap：GNU 带引号的文件式认成 busybox 无差别式。
    Claim::new(false, Stage::Final, TE_CAP, 32, Note::Busy, false),
    // 死 6 default-bucket：本可判定的全量成功被丢进兜底档。
    Claim::new(true, Stage::Final, 0, 0, Note::Clean, true),
];

/// 14 槽的标签（后 6 槽一槽只坏一维，依次挂六维）。
pub const WANT_TAGS: [&str; CORPUS] = [
    "both-ok",
    "file-enospc",
    "stdout-enospc",
    "append-enospc",
    "epipe-stdout",
    "both-enospc",
    "edquot-file",
    "usage-badopt",
    "estate-swap",
    "stage-skip",
    "code-swap",
    "value-off",
    "note-swap",
    "default-bucket",
];

/// 期望表的第二形态：落点阶段（与 CLAIMS 交叉校验，任一处笔误都要露出来）。
pub const WANT_STAGE: [Stage; CORPUS] = [
    Stage::Final,
    Stage::Final,
    Stage::Final,
    Stage::Final,
    Stage::Final,
    Stage::Errno,
    Stage::Final,
    Stage::Args,
    Stage::Final,
    Stage::Final,
    Stage::Final,
    Stage::Final,
    Stage::Final,
    Stage::Final,
];

/// 期望表的第二形态：归因码。
pub const WANT_CODE: [u32; CORPUS] = [
    0,
    TE_CAP,
    TE_ORDER,
    TE_APPEND,
    TE_SHORT,
    TE_UNIQ,
    TE_CODE,
    TE_EXIT,
    TE_CAP,
    TE_UNIQ,
    TE_CAP,
    TE_ORDER,
    TE_CAP,
    0,
];

/// 期望表的第三形态：期望命中的判定维（0 = Ok，1..=6 = Fault 秩）。
pub const WANT_NDX: [u8; CORPUS] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 4, 5, 6];

/// 期望表的第二形态：文案档位。
pub const WANT_NOTE: [Note; CORPUS] = [
    Note::Clean,
    Note::GnuFile,
    Note::GnuStdout,
    Note::GnuFile,
    Note::GnuStdout,
    Note::Busy,
    Note::Foreign,
    Note::Usage,
    Note::GnuFile,
    Note::Busy,
    Note::Foreign,
    Note::GnuStdout,
    Note::Busy,
    Note::Clean,
];

/// 期望表的第二形态：兜底档。
pub const WANT_DEFAULT: [bool; CORPUS] = [
    false, false, false, false, false, false, false, false, false, false, false, false, false,
    true,
];

/// 期望表的第二形态：申报成败位（等价于 exit==0）。
pub const WANT_OK: [bool; CORPUS] = [
    true, false, false, false, false, false, false, false, true, false, false, false, false, true,
];

/// 期望表的第二形态：申报的分叉宽度。
pub const WANT_VALUE: [u32; CORPUS] = [
    0, 32, 48, 4096, 256, 0, 512, 0, 32, 0, 512, 96, 32, 0,
];

/// 事实面第二形态（与 `define` 独立列出，用来同时复核 define 与 CLAIMS）：落点阶段。
pub const ACT_STAGE: [Stage; CORPUS] = [
    Stage::Final,
    Stage::Final,
    Stage::Final,
    Stage::Final,
    Stage::Final,
    Stage::Errno,
    Stage::Final,
    Stage::Args,
    Stage::Final,
    Stage::Errno,
    Stage::Final,
    Stage::Final,
    Stage::Final,
    Stage::Final,
];

/// 事实面第二形态：归因码。
pub const ACT_CODE: [u32; CORPUS] = [
    0,
    TE_CAP,
    TE_ORDER,
    TE_APPEND,
    TE_SHORT,
    TE_UNIQ,
    TE_CODE,
    TE_EXIT,
    TE_CAP,
    TE_UNIQ,
    TE_CODE,
    TE_ORDER,
    TE_CAP,
    0,
];

/// 事实面第二形态：分叉宽度。
pub const ACT_VALUE: [u32; CORPUS] = [
    0, 32, 48, 4096, 256, 0, 512, 0, 32, 0, 512, 48, 32, 0,
];

/// 事实面第二形态：文案档位。
pub const ACT_NOTE: [Note; CORPUS] = [
    Note::Clean,
    Note::GnuFile,
    Note::GnuStdout,
    Note::GnuFile,
    Note::GnuStdout,
    Note::Busy,
    Note::Foreign,
    Note::Usage,
    Note::GnuFile,
    Note::Busy,
    Note::Foreign,
    Note::GnuStdout,
    Note::GnuFile,
    Note::Clean,
];

/// 事实面第二形态：兜底档（14 槽全部可判，没有一格该走垃圾桶）。
pub const ACT_DEFAULT: [bool; CORPUS] = [
    false, false, false, false, false, false, false, false, false, false, false, false, false,
    false,
];

/// 事实面第二形态：成败位。
pub const ACT_OK: [bool; CORPUS] = [
    true, false, false, false, false, false, false, false, false, false, false, false, false, true,
];

/// 事实面第二形态：文件支路实写字节。
pub const ACT_FILE_WR: [u32; CORPUS] = [
    64, 32, 96, 4096, 512, 0, 512, 0, 32, 0, 512, 96, 32, 64,
];

/// 事实面第二形态：stdout 支路实写字节。
pub const ACT_STDOUT_WR: [u32; CORPUS] = [
    64, 64, 48, 8192, 256, 0, 1024, 0, 64, 0, 1024, 48, 64, 64,
];

/// 死槽 → 它所复用的活槽（语料纪律：后 6 槽的事实面整槽照抄前 8 槽的某一行）。
pub const DEAD_FROM: [usize; REJECT] = [1usize, 5usize, 6usize, 2usize, 1usize, 0usize];

// ---------------------------------------------------------------------------
// 双支路分账与闸门纵线
// ---------------------------------------------------------------------------

/// stdout 侧是否报的是管道错（EPIPE 且 stdout 真是管道）。
pub fn is_pipe_error(sh: &Shape) -> bool {
    sh.stdout_pipe && sh.stdout_errno == EPIPE
}

/// 该形态下 tee 会不会在第一个写出错误处整体中止。
pub fn aborts_at_errno(sh: &Shape) -> bool {
    if sh.bad_option {
        return false;
    }
    let any_short = sh.file_room() < sh.total || sh.stdout_room() < sh.total;
    if !any_short {
        return false;
    }
    match sh.mode {
        ErrMode::Continue | ErrMode::Warn => false,
        ErrMode::Exit => true,
        ErrMode::ExitNopipe | ErrMode::PosixP => !is_pipe_error(sh),
    }
}

/// 分账闸：两支路各实写多少字节。
///
/// 缺省（续写）模式下两条支路各自独立跑到自己的上限；中止模式下整个进程停在
/// **第一个**撞上 errno 的缓冲区水位，因此两条支路一起停在同一水平——宽度归零。
/// 用法错则一条都没起跑，两支路都是 0。
pub fn split_pair(sh: &Shape) -> (u32, u32) {
    if sh.bad_option {
        return (0, 0);
    }
    let f_room = sh.file_room();
    let o_room = sh.stdout_room();
    let f_raw = if f_room >= sh.total { sh.total } else { f_room };
    let o_raw = if o_room >= sh.total { sh.total } else { o_room };
    if aborts_at_errno(sh) {
        let stop = if f_raw <= o_raw { f_raw } else { o_raw };
        return (stop, stop);
    }
    (f_raw, o_raw)
}

/// 归因闸之后坏掉的是哪一支。
pub fn casualty_of(sh: &Shape) -> SinkKind {
    if sh.bad_option {
        return SinkKind::None;
    }
    let (fw, ow) = split_pair(sh);
    let f_bad = fw < sh.total;
    let o_bad = ow < sh.total;
    match (f_bad, o_bad) {
        (true, true) => SinkKind::Both,
        (true, false) => SinkKind::File,
        (false, true) => SinkKind::Stdout,
        (false, false) => SinkKind::None,
    }
}

/// 闸门纵线：从形态推出应然结论。次序即九道闸的次序，前一闸不产出事实后一闸无输入。
pub fn define(sh: &Shape) -> Outcome {
    // 闸 1 参数：用法错直接落 Args，两支路都没起跑。
    if sh.bad_option {
        return Outcome::new(Stage::Args, TE_EXIT, 0, Note::Usage, false);
    }
    // 闸 2 打开 + 闸 3 写序 + 闸 4 分账：全部折进 split_pair。
    let (fw, ow) = split_pair(sh);
    let gap = fw.saturating_sub(ow).saturating_add(ow.saturating_sub(fw));
    let cas = casualty_of(sh);
    // 闸 5 归因：把失败落到具体 errno 家族。
    let code: u32 = match cas {
        SinkKind::None => 0,
        SinkKind::Both => TE_UNIQ,
        SinkKind::File => {
            if sh.file_errno == EDQUOT {
                TE_CODE
            } else if sh.append && sh.trunc_freed > 0 && sh.would_fit_without_append() {
                TE_APPEND
            } else if sh.file_errno == ENOSPC {
                TE_CAP
            } else {
                TE_FAULT_DEFAULT
            }
        }
        SinkKind::Stdout => {
            if is_pipe_error(sh) {
                TE_SHORT
            } else if sh.stdout_errno == ENOSPC {
                TE_ORDER
            } else {
                TE_FAULT_DEFAULT
            }
        }
    };
    // 闸 6 文案：按实现口味与坏掉的支路选句式。
    let note: Note = if cas == SinkKind::None {
        Note::Clean
    } else if sh.flavor == Flavor::Busybox {
        Note::Busy
    } else if sh.flavor == Flavor::Foreign || sh.flavor == Flavor::Unknown {
        Note::Foreign
    } else if cas == SinkKind::File && sh.file_errno == EDQUOT {
        Note::Foreign
    } else if cas == SinkKind::File {
        Note::GnuFile
    } else {
        Note::GnuStdout
    };
    // 闸 7/8 退出与结算：中止的形态停在归因闸，续写/健康的形态跑到 EOF。
    let stage: Stage = if aborts_at_errno(sh) { Stage::Errno } else { Stage::Final };
    // 兜底档：实现认不出，或 errno 家族不属本卡四族。
    let default = sh.flavor == Flavor::Unknown
        || (cas == SinkKind::File && !errno_legible(sh.file_errno))
        || (cas == SinkKind::Stdout && !errno_legible(sh.stdout_errno))
        || (cas == SinkKind::Both && (!errno_legible(sh.file_errno) || !errno_legible(sh.stdout_errno)));
    Outcome { stage, code, value: gap, note, default }
}

/// 退出码结算（闸 7）：2 = 用法错；1 = 任一 sink 写失败；0 = 两支路全量。
pub fn exit_code(sh: &Shape) -> u32 {
    if sh.bad_option {
        return EXIT_USAGE;
    }
    let (fw, ow) = split_pair(sh);
    if fw < sh.total || ow < sh.total {
        EXIT_FAIL
    } else {
        EXIT_OK
    }
}

/// shell 侧看到的值：GNU 屏蔽 SIGPIPE 所以退 1；busybox 不管 SIGPIPE，
/// 管道断时被信号打死，shell 记 128+13=141，且**一行错误都不打**。
pub fn shell_status(sh: &Shape) -> u32 {
    let base = exit_code(sh);
    if !sh.flavor.ignores_sigpipe() && is_pipe_error(sh) {
        SHELL_DIED
    } else {
        base
    }
}

/// 半程宽度与退出码的闭合关系：截断却退 0 是谎报，本卡把它单独判死。
pub fn short_write_lies(sh: &Shape) -> bool {
    let (fw, ow) = split_pair(sh);
    let short = fw < sh.total || ow < sh.total;
    short && exit_code(sh) == EXIT_OK
}

/// 一槽的申报退出码（供 Estate 维与渲染共用）。
pub fn claim_exit(c: &Claim) -> u32 {
    if c.default {
        return EXIT_USAGE;
    }
    c.exit_hint()
}

/// 活槽的成败位（前 8 槽）。
pub const LIVE_OK: [bool; ACCEPT] = [true, false, false, false, false, false, false, false];

// ---------------------------------------------------------------------------
// 文案：逐字复刻真实报错
// ---------------------------------------------------------------------------

/// 文件操作数在本卡语料里的字面名（GNU 会给它套单引号）。
pub const OPERAND: &str = "file";

/// GNU coreutils 式，坏的是带引号的文件操作数：
/// `tee: 'file': No space left on device`
pub fn msg_gnu_file(op: &str) -> String {
    let mut s = String::new();
    s.push_str("tee: '");
    s.push_str(op);
    s.push_str("': ");
    s.push_str(ANCHOR_ERRNO_TEXT);
    s
}

/// GNU 式，坏的是标准输出（固定名，**不带**引号）：
/// `tee: standard output: No space left on device`
pub fn msg_gnu_stdout(op: &str) -> String {
    let mut s = String::new();
    s.push_str("tee: ");
    s.push_str(op);
    s.push_str(": ");
    s.push_str(ANCHOR_ERRNO_TEXT);
    s
}

/// GNU 式，stdout 是管道且读者退场：`tee: standard output: Broken pipe`
pub fn msg_gnu_broken_pipe() -> String {
    let mut s = String::new();
    s.push_str("tee: standard output: Broken pipe");
    s
}

/// busybox 式，不分目的地：`tee: write error: No space left on device`
pub fn msg_busy_write_error() -> String {
    let mut s = String::new();
    s.push_str("tee: write error: ");
    s.push_str(ANCHOR_ERRNO_TEXT);
    s
}

/// GNU 式，配额耗尽（不是设备满）：`tee: 'file': Disk quota exceeded`
pub fn msg_gnu_quota(op: &str) -> String {
    let mut s = String::new();
    s.push_str("tee: '");
    s.push_str(op);
    s.push_str("': ");
    s.push_str(errno_name(EDQUOT));
    s
}

/// 用法错：`tee: invalid option -- 'z'`
pub fn msg_usage_opt(c: char) -> String {
    let mut s = String::new();
    s.push_str("tee: invalid option -- '");
    s.push(c);
    s.push('\'');
    s
}

/// 用法错的提示行：`Try 'tee --help' for more information.`
pub fn msg_usage_hint() -> String {
    let mut s = String::new();
    s.push_str("Try 'tee --help' for more information.");
    s
}

/// 别家实现式（Toybox/BSD 口径）：`tee: stdout: No space left on device`
pub fn msg_foreign_stdout() -> String {
    let mut s = String::new();
    s.push_str("tee: stdout: ");
    s.push_str(ANCHOR_ERRNO_TEXT);
    s
}

/// 卡面原文形态（不带参数时的默认文本，判据名与测试都盯这两串）。
pub fn msg_gnu_file_default() -> String {
    msg_gnu_file(OPERAND)
}

/// 卡面原文形态：GNU stdout。
pub fn msg_gnu_stdout_default() -> String {
    msg_gnu_stdout("standard output")
}

/// 按文案档位给出该档的规范整行（Clean 档是空串——没错误就没行）。
pub fn msg_note(n: Note) -> String {
    match n {
        Note::Clean => String::new(),
        Note::GnuFile => msg_gnu_file_default(),
        Note::GnuStdout => msg_gnu_stdout_default(),
        Note::Busy => msg_busy_write_error(),
        Note::Foreign => msg_gnu_quota(OPERAND),
        Note::Usage => msg_usage_opt('z'),
    }
}

/// 按形态给出这一槽实际会打出的那一行（比 msg_note 更贴事实：它区分 EPIPE 与 ENOSPC）。
pub fn msg_for(sh: &Shape) -> String {
    if sh.bad_option {
        return msg_usage_opt('z');
    }
    let cas = casualty_of(sh);
    if cas == SinkKind::None {
        return String::new();
    }
    match sh.flavor {
        Flavor::Gnu => match cas {
            SinkKind::File => {
                if sh.file_errno == EDQUOT {
                    msg_gnu_quota(OPERAND)
                } else {
                    msg_gnu_file(OPERAND)
                }
            }
            SinkKind::Stdout => {
                if is_pipe_error(sh) {
                    msg_gnu_broken_pipe()
                } else {
                    msg_gnu_stdout("standard output")
                }
            }
            SinkKind::Both => msg_gnu_file(OPERAND),
            SinkKind::None => String::new(),
        },
        Flavor::Busybox => {
            if is_pipe_error(sh) {
                String::new()
            } else {
                msg_busy_write_error()
            }
        }
        Flavor::Foreign | Flavor::Unknown => match cas {
            SinkKind::File => msg_gnu_file(OPERAND),
            _ => msg_foreign_stdout(),
        },
    }
}

/// 文案是否含 GNU 特有的单引号包围（busybox 式没有，这是三式的最小可分特征）。
pub fn has_quoted_operand(s: &str) -> bool {
    s.contains("'")
}

/// 文案是否报的是空间类错误（含 ENOSPC 与 EDQUOT 两种尾巴）。
pub fn is_space_family(s: &str) -> bool {
    s.contains(ANCHOR_ERRNO_TEXT) || s.contains(errno_name(EDQUOT))
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
    TE_STATE,
    TE_STAGE,
    TE_CODE,
    TE_VALUE,
    TE_NOTE,
    TE_FAULT_DEFAULT,
];

/// 账本的 8 个正数格：前 7 格是统计数，第 8 格是红档率 929。
///
/// 七个统计数逐个字面取，含义写死在 `stat_meaning()` 里，账本与期望不许各自漂移。
pub const VALS: [u32; ACCEPT] = [
    SINK_COUNT,
    STAGE_COUNT,
    CODE_COUNT as u32,
    EXIT_RANKS,
    CORPUS as u32,
    4,
    6,
    RED_RATE,
];

/// 统计格的字面期望（第二形态，交叉校验 VALS 的笔误）。
pub const VALS_CHECK: [u32; ACCEPT] = [2, 9, 13, 3, 14, 4, 6, 929];

/// 第 8 格之外的统计格数。
pub const STAT_CELLS: usize = 7usize;

/// 统计格语义（本卡的身份数，一格一句话）。
pub fn stat_meaning(i: usize) -> &'static str {
    match i {
        0 => "两支路：一份流两个目的地",
        1 => "九道闸：含封印闸的纵线宽度",
        2 => "十三个错误码：SEG_BASE+0x01..0x0D",
        3 => "三个退出码位：0/1/2",
        4 => "十四个语料槽：8 活 + 6 死",
        5 => "四个档位：全量/半程/全损/未起跑",
        6 => "六个缺陷维：一槽只坏一维",
        7 => "红档率 929：14 格中 13 的千分率",
        _ => "越界",
    }
}

/// 判定次序固定：Estate → Stage → Code → Value → Note → Default → Ok。
///
/// Estate 的定义是 `actual.ok() != claim.success()`——成败互报优先于阶段与错误码，
/// 因为"文件已截断却退 0"是本卡唯一能把失败读成成功的谎法，别的维错都只影响归因。
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

/// 判定注入（`run_with` 的内部出口）：越界槽返回 Err(TE_SLOT)，被点中的槽用注入维，
/// 其余槽照常判。
fn fault_at(c: &Case, i: usize, over: Option<(usize, Fault)>) -> Result<Fault, u32> {
    if i >= CORPUS {
        return Err(TE_SLOT);
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
                Row::of(Tier::Full),
                Row::of(Tier::Half),
                Row::of(Tier::Both),
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

/// 全语料运行的报告。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Report {
    /// 语料槽数。
    pub total: usize,
    /// 活槽数。
    pub accept: usize,
    /// 死槽数。
    pub reject: usize,
    /// 全量档槽数。
    pub full: usize,
    /// 半程档槽数。
    pub half: usize,
    /// 全损档槽数。
    pub both: usize,
    /// 未起跑档槽数。
    pub usage: usize,
    /// 命中期望结论的槽数。
    pub matched: usize,
}

impl Report {
    /// 全绿运行的逐字渲染（先按手算占位，探针观测后按字节钉死；越界一红即变串）。
    pub const RENDER_OK: &'static str =
        "tee2s total=14 accept=8 reject=6 full=2 half=9 both=2 usage=1 matched=14 rate=1000 verdict=green";

    /// 千分率：`(matched*1000 + total/2) / total`。14 格中 13 → 929（红档率标定）。
    pub fn rate(&self) -> u32 {
        if self.total == 0 {
            return 0;
        }
        let num = (self.matched as u32).saturating_mul(1000).saturating_add((self.total / 2) as u32);
        num / (self.total as u32)
    }

    /// 四档求和。
    pub fn tier_sum(&self) -> usize {
        self.full
            .saturating_add(self.half)
            .saturating_add(self.both)
            .saturating_add(self.usage)
    }

    /// 是否全绿。
    pub fn green(&self) -> bool {
        self.gate().is_ok()
    }

    /// 是否恰中 13/14（红档率标定态）——一红即 929，这一格把 929 与渲染路绑死。
    pub fn is_red_calibrated(&self) -> bool {
        self.total == CORPUS && self.matched + 1 == CORPUS && self.rate() == RED_RATE
    }

    /// 出口闸门：逐项校验语料宽度、活死配比、四档闭合、命中率、千分率。
    pub fn gate(&self) -> Result<(), u32> {
        if self.total != CORPUS {
            return Err(TE_SLOT);
        }
        if self.accept.saturating_add(self.reject) != self.total {
            return Err(TE_UNIQ);
        }
        if self.tier_sum() != self.total {
            return Err(TE_VALUE);
        }
        if self.matched != self.total {
            return Err(TE_STATE);
        }
        if self.rate() != RATE_FULL {
            return Err(TE_EXIT);
        }
        Ok(())
    }

    /// 逐字渲染（RENDER_OK 钉的就是这一串）。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str("tee2s");
        sep(&mut s);
        kv(&mut s, "total", self.total as u32);
        sep(&mut s);
        kv(&mut s, "accept", self.accept as u32);
        sep(&mut s);
        kv(&mut s, "reject", self.reject as u32);
        sep(&mut s);
        kv(&mut s, "full", self.full as u32);
        sep(&mut s);
        kv(&mut s, "half", self.half as u32);
        sep(&mut s);
        kv(&mut s, "both", self.both as u32);
        sep(&mut s);
        kv(&mut s, "usage", self.usage as u32);
        sep(&mut s);
        kv(&mut s, "matched", self.matched as u32);
        sep(&mut s);
        kv(&mut s, "rate", self.rate());
        sep(&mut s);
        s.push_str("verdict=");
        s.push_str(if self.green() { "green" } else { "red" });
        s
    }
}

/// 全绿运行的逐字渲染（先占位，探针观测后按字节钉死）。
pub const RENDER_OK: &str = Report::RENDER_OK;

/// 跑一遍全语料。
pub fn run() -> Result<Report, u32> {
    run_with(None)
}

/// 带注入的运行：`over` 点中某槽时强行改判其结论，槽号越界 → `TE_SLOT`。
pub fn run_with(over: Option<(usize, Fault)>) -> Result<Report, u32> {
    if let Some((slot, _)) = over {
        if slot >= CORPUS {
            return Err(TE_SLOT);
        }
    }
    let mut total: usize = 0;
    let mut matched: usize = 0;
    let mut full: usize = 0;
    let mut half: usize = 0;
    let mut both: usize = 0;
    let mut usage: usize = 0;
    let mut i: usize = 0;
    while i < CORPUS {
        let c = match case_at(i) {
            Some(v) => v,
            None => return Err(TE_SLOT),
        };
        let got = match fault_at(&c, i, over) {
            Ok(f) => f,
            Err(e) => return Err(e),
        };
        let tier = Tier::of(&define(&c.shape));
        total = total.saturating_add(1);
        if got == want_fault(i) {
            matched = matched.saturating_add(1);
        }
        match tier {
            Tier::Full => full = full.saturating_add(1),
            Tier::Half => half = half.saturating_add(1),
            Tier::Both => both = both.saturating_add(1),
            Tier::Usage => usage = usage.saturating_add(1),
        }
        i += 1;
    }
    Ok(Report {
        total,
        accept: ACCEPT,
        reject: REJECT,
        full,
        half,
        both,
        usage,
        matched,
    })
}

/// 14 槽是否全部命中期望结论（表路自检的核心谓词）。
pub fn all_claims_land() -> bool {
    match run() {
        Ok(r) => r.matched == r.total && r.total == CORPUS,
        Err(_) => false,
    }
}

/// 逐槽结论表（诊断用）。
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
// 闸门（阶段纵线，单调不倒退）
// ---------------------------------------------------------------------------

/// 阶段闸门。规则：只允许前进到**下一道**闸；倒退 → `TE_STAGE`，跳档 → `TE_ORDER`，
/// 未走完纵线就封印 → `TE_STAGE`，封印之后一切动作 → `TE_STATE`。
/// 被拒的动作同样改指纹——这是"拒动也要留痕"的硬要求。
#[derive(Clone, Copy, Debug)]
pub struct Gate {
    rank: u32,
    closed: bool,
    fp: u64,
    moves: u32,
    rejected: u32,
}

impl Gate {
    /// 新建：秩 0（未起跑）。
    pub fn new() -> Gate {
        Gate { rank: 0, closed: false, fp: FNV_OFF, moves: 0, rejected: 0 }
    }

    /// 当前秩。
    pub fn rank(&self) -> u32 {
        self.rank
    }

    /// 当前阶段（秩 0 没有阶段）。
    pub fn stage(&self) -> Option<Stage> {
        Stage::from_rank(self.rank)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// 尝试次数（含被拒的）。
    pub fn moves(&self) -> u32 {
        self.moves
    }

    /// 被拒次数。
    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    /// 是否已封印。
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// 前进一道。只有 `rank + 1` 这一格被接受。
    pub fn step(&mut self, to: Stage) -> Result<(), u32> {
        self.moves = self.moves.saturating_add(1);
        self.fp = fold_word(self.fp, to.rank());
        if self.closed {
            self.rejected = self.rejected.saturating_add(1);
            return Err(TE_STATE);
        }
        let want = self.rank.saturating_add(1);
        let r = to.rank();
        if r == want {
            self.rank = r;
            self.fp = fold_name(self.fp, b"advance");
            return Ok(());
        }
        self.rejected = self.rejected.saturating_add(1);
        self.fp = fold_name(self.fp, b"reject");
        if r < self.rank {
            return Err(TE_STAGE);
        }
        Err(TE_ORDER)
    }

    /// 封印：必须已经站到最后一道闸（Closed，秩 9）。
    pub fn close(&mut self) -> Result<(), u32> {
        self.moves = self.moves.saturating_add(1);
        self.fp = fold_name(self.fp, b"close");
        if self.closed {
            self.rejected = self.rejected.saturating_add(1);
            return Err(TE_STATE);
        }
        if self.rank != Stage::Closed.rank() {
            self.rejected = self.rejected.saturating_add(1);
            return Err(TE_STAGE);
        }
        self.closed = true;
        Ok(())
    }

    /// 走完整条纵线并封印（正例通道）。
    pub fn walk_and_seal(&mut self) -> Result<(), u32> {
        let mut k: usize = 0;
        while k < Stage::ALL.len() {
            let st = match Stage::ALL.get(k) {
                Some(v) => *v,
                None => return Err(TE_SLOT),
            };
            match self.step(st) {
                Ok(()) => {}
                Err(e) => return Err(e),
            }
            k += 1;
        }
        self.close()
    }
}

/// 折叠一段静态文本的名字（`b"..."` 形态，账本与闸门共用）。
fn fold_name(h: u64, bytes: &[u8]) -> u64 {
    let mut x = h;
    let mut i: usize = 0;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            x = fold_byte(x, *b);
        }
        i += 1;
    }
    fold_byte(x, b'|')
}

// ---------------------------------------------------------------------------
// 账本
// ---------------------------------------------------------------------------

/// 一格账。`op` 决定这一格是统计格、红档格还是缺陷码格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    /// 槽位（0..14，必须严格等于登记时的账本长度）。
    pub slot: u8,
    /// 格类型：OP_STAT / OP_RATE / OP_DEFECT。
    pub op: u8,
    /// 成败位——与 `code == 0` 必须互推，否则就是被篡改的一格。
    pub ok: bool,
    /// 错误码（统计格与红档格恒 0；缺陷格挂 REJECT_CODES）。
    pub code: u32,
    /// 数值（统计格挂 VALS，红档格挂 929，缺陷格必须为 0）。
    pub value: u32,
}

/// 空白格（构造定容数组用）。
const ZERO_SAMPLE: Sample = Sample { slot: 0, op: 0, ok: true, code: 0, value: 0 };

impl Sample {
    /// 统计格。
    pub fn stat(slot: usize, value: u32) -> Sample {
        Sample { slot: slot as u8, op: OP_STAT, ok: true, code: 0, value }
    }

    /// 红档格（值恒 929）。
    pub fn rate(slot: usize) -> Sample {
        Sample { slot: slot as u8, op: OP_RATE, ok: true, code: 0, value: RED_RATE }
    }

    /// 缺陷码格：值必须为 0，成败位必须为败。
    pub fn defect(slot: usize, code: u32) -> Sample {
        Sample { slot: slot as u8, op: OP_DEFECT, ok: false, code, value: 0 }
    }

    /// 折叠一格进指纹（五维全覆盖，次序固定）。
    pub fn fold(&self, h: u64) -> u64 {
        let x = fold_byte(h, self.slot);
        let x = fold_byte(x, self.op);
        let x = fold_bool(x, self.ok);
        let x = fold_word(x, self.code);
        fold_word(x, self.value)
    }

    /// 本格自洽性：op 合法且 ok 与 code 互推。
    pub fn self_consistent(&self) -> bool {
        if self.op < OP_STAT || self.op > OP_DEFECT {
            return false;
        }
        if self.ok != (self.code == 0) {
            return false;
        }
        match self.op {
            OP_STAT => self.ok && self.code == 0 && self.value > 0,
            OP_RATE => self.ok && self.code == 0 && self.value == RED_RATE,
            OP_DEFECT => !self.ok && self.code != 0 && self.value == 0,
            _ => false,
        }
    }

    /// 渲染一格。
    pub fn render(&self) -> String {
        let mut s = String::new();
        kv(&mut s, "slot", self.slot as u32);
        sep(&mut s);
        kv(&mut s, "op", self.op as u32);
        sep(&mut s);
        s.push_str("ok=");
        s.push_str(bool_text(self.ok).as_str());
        sep(&mut s);
        s.push_str("code=");
        s.push_str(code_name(self.code));
        sep(&mut s);
        kv(&mut s, "value", self.value);
        s
    }
}

/// 14 格账本。登记要求 `slot == len`（跳号/越界一律拒入账并计 `rejects`），
/// 篡改（自相矛盾的一格）拒入账并归 `TE_UNIQ`；封印与冻结各自留指纹。
#[derive(Clone, Copy, Debug)]
pub struct Ledger {
    items: [Sample; SAMPLES],
    len: usize,
    sealed: bool,
    frozen: bool,
    root: u64,
    rejects: u32,
}

impl Ledger {
    /// 空账。
    pub fn new() -> Ledger {
        Ledger {
            items: [ZERO_SAMPLE; SAMPLES],
            len: 0,
            sealed: false,
            frozen: false,
            root: 0,
            rejects: 0,
        }
    }

    /// 已入账格数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 被拒登记次数（越界、跳号、篡改、冻结后动作全计入，显式不静默）。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 是否已封印。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 是否已冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 根指纹（未封印为 0）。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 取一格。
    pub fn sample(&self, i: usize) -> Option<Sample> {
        match self.items.get(i) {
            Some(s) => {
                if i < self.len {
                    Some(*s)
                } else {
                    None
                }
            }
            None => None,
        }
    }

    /// 登记一格。返回 Err 时**不入账**，只加 rejects。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(TE_STATE);
        }
        if (s.slot as usize) != self.len {
            self.rejects = self.rejects.saturating_add(1);
            return Err(TE_SLOT);
        }
        if !s.self_consistent() {
            self.rejects = self.rejects.saturating_add(1);
            return Err(TE_UNIQ);
        }
        if self.len >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(TE_SLOT);
        }
        match self.items.get_mut(self.len) {
            Some(slot) => *slot = s,
            None => {
                self.rejects = self.rejects.saturating_add(1);
                return Err(TE_SLOT);
            }
        }
        self.len = self.len.saturating_add(1);
        Ok(())
    }

    /// 成功格数。
    pub fn ok_count(&self) -> usize {
        let mut n: usize = 0;
        let mut i: usize = 0;
        while i < self.len {
            if let Some(s) = self.sample(i) {
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

    /// 按格类型计数。
    pub fn count_op(&self, op: u8) -> usize {
        let mut n: usize = 0;
        let mut i: usize = 0;
        while i < self.len {
            if let Some(s) = self.sample(i) {
                if s.op == op {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 逐格复核：前 7 格对 VALS、第 8 格对 929、后 6 格对 REJECT_CODES 且值必为 0。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES {
            return Err(TE_SLOT);
        }
        let mut i: usize = 0;
        while i < ACCEPT {
            let s = match self.sample(i) {
                Some(v) => v,
                None => return Err(TE_SLOT),
            };
            let want_v = match VALS.get(i) {
                Some(v) => *v,
                None => return Err(TE_SLOT),
            };
            if s.slot as usize != i {
                return Err(TE_UNIQ);
            }
            if i < STAT_CELLS {
                if s.op != OP_STAT || s.code != 0 || !s.ok || s.value != want_v {
                    return Err(TE_VALUE);
                }
            } else if s.op != OP_RATE || s.code != 0 || s.value != RED_RATE {
                return Err(TE_VALUE);
            }
            i += 1;
        }
        let mut j: usize = 0;
        while j < REJECT {
            let idx = ACCEPT.saturating_add(j);
            let s = match self.sample(idx) {
                Some(v) => v,
                None => return Err(TE_SLOT),
            };
            let want_c = match REJECT_CODES.get(j) {
                Some(v) => *v,
                None => return Err(TE_SLOT),
            };
            if s.op != OP_DEFECT || s.slot as usize != idx || s.code != want_c {
                return Err(TE_UNIQ);
            }
            if s.value != 0 || s.ok {
                return Err(TE_VALUE);
            }
            j += 1;
        }
        Ok(())
    }

    /// 折叠全账（不写入状态，纯函数，replay 用它独立复算）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i: usize = 0;
        while i < self.len {
            if let Some(s) = self.sample(i) {
                h = s.fold(h);
            }
            i += 1;
        }
        fold_str(h, "tee-halfwrite")
    }

    /// 封印：折叠全账并落根指纹。
    pub fn seal(&mut self) -> u64 {
        self.root = self.fingerprint();
        self.sealed = true;
        self.root
    }

    /// 冻结：封印之后不再接受任何登记。
    pub fn freeze(&mut self) -> u64 {
        let r = self.seal();
        self.frozen = true;
        r
    }

    /// 重放：独立重建一遍账再比指纹。
    pub fn replay(&self) -> Result<u64, u32> {
        if !self.sealed {
            return Err(TE_STATE);
        }
        let mut rebuilt = Ledger::new();
        let mut i: usize = 0;
        while i < self.len {
            if let Some(s) = self.sample(i) {
                if rebuilt.register(s).is_err() {
                    return Err(TE_UNIQ);
                }
            }
            i += 1;
        }
        if rebuilt.len != self.len {
            return Err(TE_SLOT);
        }
        if rebuilt.fingerprint() != self.root {
            return Err(TE_UNIQ);
        }
        Ok(self.root)
    }

    /// 渲染整账（诊断用）。
    pub fn render(&self) -> String {
        let mut s = String::new();
        kv(&mut s, "len", self.len as u32);
        sep(&mut s);
        kv(&mut s, "ok", self.ok_count() as u32);
        sep(&mut s);
        kv(&mut s, "err", self.err_count() as u32);
        sep(&mut s);
        kv(&mut s, "rejects", self.rejects);
        sep(&mut s);
        s.push_str("root=");
        s.push_str(hex_text(self.root as u32, 8).as_str());
        s
    }
}

/// 造 14 格账：7 统计格 + 1 红档格（929）+ 6 缺陷码格。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut i: usize = 0;
    while i < STAT_CELLS {
        let v = match VALS.get(i) {
            Some(x) => *x,
            None => 0,
        };
        match l.register(Sample::stat(i, v)) {
            Ok(()) => {}
            Err(_) => {}
        }
        i += 1;
    }
    match l.register(Sample::rate(ACCEPT - 1)) {
        Ok(()) => {}
        Err(_) => {}
    }
    let mut j: usize = 0;
    while j < REJECT {
        let c = match REJECT_CODES.get(j) {
            Some(x) => *x,
            None => 0,
        };
        let idx = ACCEPT.saturating_add(j);
        match l.register(Sample::defect(idx, c)) {
            Ok(()) => {}
            Err(_) => {}
        }
        j += 1;
    }
    l
}

/// 造一格错值的账（只给自检与测试用：第 3 统计格塞进 999，audit 必须抓到）。
pub fn broken_value_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut i: usize = 0;
    while i < STAT_CELLS {
        let v = match VALS.get(i) {
            Some(x) => *x,
            None => 0,
        };
        let patched = if i == 3 { 999 } else { v };
        match l.register(Sample::stat(i, patched)) {
            Ok(()) => {}
            Err(_) => {}
        }
        i += 1;
    }
    match l.register(Sample::rate(ACCEPT - 1)) {
        Ok(()) => {}
        Err(_) => {}
    }
    let mut j: usize = 0;
    while j < REJECT {
        let c = match REJECT_CODES.get(j) {
            Some(x) => *x,
            None => 0,
        };
        let idx = ACCEPT.saturating_add(j);
        match l.register(Sample::defect(idx, c)) {
            Ok(()) => {}
            Err(_) => {}
        }
        j += 1;
    }
    l
}

/// 造一格身份错的账：缺陷码被换成不在 REJECT_CODES 里的 TE_CAP。
pub fn broken_code_ledger() -> Ledger {
    let mut l = build_ledger();
    let mut j: usize = 0;
    while j < REJECT {
        if j == 2 {
            let idx = ACCEPT.saturating_add(j);
            match l.items.get_mut(idx) {
                Some(s) => s.code = TE_CAP,
                None => {}
            }
        }
        j += 1;
    }
    l
}

/// 造一格被篡改的账（defect 格值非 0，audit 必须以 TE_VALUE 判死）。
pub fn broken_defect_ledger() -> Ledger {
    let mut l = build_ledger();
    match l.items.get_mut(ACCEPT) {
        Some(s) => s.value = 7,
        None => {}
    }
    l
}

/// 四路注入自检：表路 / 判路 / 渲染路 / 账本路，各自独立，四格全真才算过。
pub fn inject_four() -> [bool; 4] {
    // 路 1 表路：14 槽申报逐格命中期望结论。
    let table = all_claims_land() && slot_count() == CORPUS;

    // 路 2 判路：把一格的结论注入成期望之外的维，命中数必须正好掉 1，
    // 千分率必须正好落在红档标定 929 上。
    let judged = match run_with(Some((1usize, Fault::Default))) {
        Ok(r) => r.matched + 1 == r.total && r.rate() == RED_RATE && !r.green(),
        Err(_) => false,
    };

    // 路 3 渲染路：全绿渲染必须逐字等于 RENDER_OK，且出口闸放行。
    let rendered = match run() {
        Ok(r) => r.render().as_str() == RENDER_OK && r.gate().is_ok(),
        Err(_) => false,
    };

    // 路 4 账本路：建账→审计→冻结→重放→乱序拒登→篡改拒登，一环都不能松。
    let mut ledger_route = true;
    let mut l = build_ledger();
    if l.audit() != Ok(()) {
        ledger_route = false;
    }
    let root = l.freeze();
    if root == 0 || !l.is_frozen() {
        ledger_route = false;
    }
    if l.replay() != Ok(root) {
        ledger_route = false;
    }
    if l.register(Sample::stat(SAMPLES, 1)).is_ok() {
        ledger_route = false;
    }
    if l.rejects() == 0 {
        ledger_route = false;
    }

    // 顺带复核闸门：走完整条纵线后倒退必须被拒且留下指纹变化。
    let mut g = Gate::new();
    let mut spine_ok = true;
    match g.walk_and_seal() {
        Ok(()) => {}
        Err(_) => spine_ok = false,
    }
    let before = g.fingerprint();
    if g.step(Stage::Args).is_ok() {
        spine_ok = false;
    }
    if g.fingerprint() == before {
        spine_ok = false;
    }
    [table, judged, rendered, ledger_route && spine_ok]
}

/// 四路的短名（自检明细与测试共用）。
pub const INJECT_ROUTES: [&str; 4] = ["table", "judge", "render", "ledger"];

/// 对照形态（**不入语料**）：busybox tee 的 stdout 是管道且读者退场。
/// 它存在的唯一目的是钉住两条与本卡边界相关的对照事实：busybox 被 SIGPIPE 打死
/// 时一行错误都不打，shell 侧看到 141；GNU 同样情形打出 `Broken pipe` 并退 1。
pub const SHAPE_BUSY_PIPE: Shape = Shape {
    total: 4096,
    file_free: 4096,
    trunc_freed: 0,
    stdout_free: 512,
    file_errno: 0,
    stdout_errno: EPIPE,
    append: false,
    stdout_pipe: true,
    mode: ErrMode::Continue,
    flavor: Flavor::Busybox,
    bad_option: false,
    order_file_first: true,
};

/// 期望退出码表（第二形态；与 `exit_code` 独立列出，交叉校验）。
pub const WANT_EXIT: [u32; CORPUS] = [
    0, 1, 1, 1, 1, 1, 1, 2, 1, 1, 1, 1, 1, 0,
];

/// 期望档位表（第二形态；与 `Tier::of` 独立列出，交叉校验）。
pub const WANT_TIER: [Tier; CORPUS] = [
    Tier::Full,
    Tier::Half,
    Tier::Half,
    Tier::Half,
    Tier::Half,
    Tier::Both,
    Tier::Half,
    Tier::Usage,
    Tier::Half,
    Tier::Both,
    Tier::Half,
    Tier::Half,
    Tier::Half,
    Tier::Full,
];

/// 期望归因码的语义名（第二形态；与 `code_name` 独立列出）。
pub const WANT_CODE_NAME: [&str; CORPUS] = [
    "clean",
    "te-cap",
    "te-order",
    "te-append",
    "te-short",
    "te-uniq",
    "te-code",
    "te-exit",
    "te-cap",
    "te-uniq",
    "te-code",
    "te-order",
    "te-cap",
    "clean",
];

/// 三张相邻卡的锚位是否闭合（本卡自声明位距，不复算对方段位就等于没署名）。
pub fn anchors_agree() -> bool {
    if SEG_BASE.saturating_sub(ANCHOR_F11681_GAP) != ANCHOR_F11681_SEG {
        return false;
    }
    if SEG_BASE.saturating_sub(ANCHOR_F11682_GAP) != ANCHOR_F11682_SEG {
        return false;
    }
    if SEG_BASE.saturating_sub(ANCHOR_F11683_GAP) != ANCHOR_F11683_SEG {
        return false;
    }
    if ANCHOR_F11681_SEG >= ANCHOR_F11682_SEG {
        return false;
    }
    if ANCHOR_F11682_SEG >= ANCHOR_F11683_SEG {
        return false;
    }
    if ANCHOR_F11683_SEG >= SEG_BASE {
        return false;
    }
    if ENOSPC != ANCHOR_ENOSPC {
        return false;
    }
    if ANCHOR_ERRNO_TEXT.len() == 0 {
        return false;
    }
    true
}

/// 判据 01：模块身份、段位、锚位与规模常量。
fn name_ok() -> bool {
    if SEG_BASE != 0x6d160 {
        return false;
    }
    if !anchors_agree() {
        return false;
    }
    if CODE_COUNT != 13 || ALL_CODES.len() != 13 {
        return false;
    }
    if CORPUS != 14 || SAMPLES != 14 || ACCEPT != 8 || REJECT != 6 || TIER_COUNT != 4 {
        return false;
    }
    if STAGE_COUNT != 9 || VALS.len() != ACCEPT || REJECT_CODES.len() != REJECT {
        return false;
    }
    if SHAPES.len() != CORPUS || CLAIMS.len() != CORPUS || WANT_TAGS.len() != CORPUS {
        return false;
    }
    let mut i: usize = 0;
    while i < CODE_COUNT {
        let c = match ALL_CODES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let want = SEG_BASE.saturating_add((i as u32) + 1);
        if c != want {
            return false;
        }
        if code_offset(c) != (i as i32) + 1 {
            return false;
        }
        let mut j = i + 1;
        while j < CODE_COUNT {
            if let Some(x) = ALL_CODES.get(j) {
                if *x == c {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    if code_offset(SEG_BASE) != 0 {
        return false;
    }
    if code_offset(SEG_BASE.saturating_sub(1)) != -1 {
        return false;
    }
    if code_name(TE_SLOT) != "te-slot" || code_name(0) != "clean" {
        return false;
    }
    if code_name(SEG_BASE.saturating_add(0x3f)) != "out-of-segment" {
        return false;
    }
    true
}

/// 一段秩序列是否从 `start` 起连续递增且无重复。
fn ranks_contiguous(ranks: &[u32], start: u32) -> bool {
    let mut k: usize = 0;
    while k < ranks.len() {
        match ranks.get(k) {
            Some(v) => {
                if *v != start.saturating_add(k as u32) {
                    return false;
                }
            }
            None => return false,
        }
        k += 1;
    }
    true
}

/// 若干短名是否全部非空且互不相同。
fn slugs_unique(slugs: &[&str]) -> bool {
    let mut i: usize = 0;
    while i < slugs.len() {
        match slugs.get(i) {
            Some(s) => {
                if s.len() == 0 {
                    return false;
                }
            }
            None => return false,
        }
        let mut j = i + 1;
        while j < slugs.len() {
            if let Some(y) = slugs.get(j) {
                if let Some(x) = slugs.get(i) {
                    if x == y {
                        return false;
                    }
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 判据 02：六枚枚举的秩、slug、from_rank 往返与对位。
fn enums_ok() -> bool {
    if Stage::ALL.len() != STAGE_COUNT as usize {
        return false;
    }
    let mut ranks: [u32; 9] = [0; 9];
    let mut slugs: [&str; 9] = ["a", "a", "a", "a", "a", "a", "a", "a", "a"];
    let mut k: usize = 0;
    while k < Stage::ALL.len() {
        let st = match Stage::ALL.get(k) {
            Some(v) => *v,
            None => return false,
        };
        match ranks.get_mut(k) {
            Some(s) => *s = st.rank(),
            None => return false,
        }
        match slugs.get_mut(k) {
            Some(s) => *s = st.slug(),
            None => return false,
        }
        if Stage::from_rank(st.rank()) != Some(st) {
            return false;
        }
        if st.prev().is_some() && Stage::from_rank(st.rank().saturating_sub(1)) != st.prev() {
            return false;
        }
        k += 1;
    }
    if !ranks_contiguous(&ranks, 1) || !slugs_unique(&slugs) {
        return false;
    }
    if Stage::from_rank(0).is_some() || Stage::from_rank(10).is_some() {
        return false;
    }
    if Stage::Closed.next().is_some() || Stage::Args.prev().is_some() {
        return false;
    }
    if Stage::Exit.next() != Some(Stage::Final) {
        return false;
    }
    let mut n_land: u32 = 0;
    let mut m: usize = 0;
    while m < Stage::ALL.len() {
        if let Some(st) = Stage::ALL.get(m) {
            if st.is_landing() {
                n_land = n_land.saturating_add(1);
            }
        }
        m += 1;
    }
    if n_land != 3 {
        return false;
    }

    if SinkKind::ALL.len() != 4 {
        return false;
    }
    let mut sink_ranks: [u32; 4] = [0; 4];
    let mut sink_slugs: [&str; 4] = ["a", "a", "a", "a"];
    let mut p: usize = 0;
    while p < SinkKind::ALL.len() {
        let sk = match SinkKind::ALL.get(p) {
            Some(v) => *v,
            None => return false,
        };
        match sink_ranks.get_mut(p) {
            Some(s) => *s = sk.rank(),
            None => return false,
        }
        match sink_slugs.get_mut(p) {
            Some(s) => *s = sk.slug(),
            None => return false,
        }
        if SinkKind::from_rank(sk.rank()) != Some(sk) {
            return false;
        }
        if sk == SinkKind::None && sk.casualty() {
            return false;
        }
        if sk != SinkKind::None && !sk.casualty() {
            return false;
        }
        p += 1;
    }
    if !ranks_contiguous(&sink_ranks, 1) || !slugs_unique(&sink_slugs) {
        return false;
    }

    if Flavor::ALL.len() != 4 || ErrMode::ALL.len() != 5 || Note::ALL.len() != 6 {
        return false;
    }
    let mut f_ranks: [u32; 4] = [0; 4];
    let mut f_slugs: [&str; 4] = ["a", "a", "a", "a"];
    let mut q: usize = 0;
    while q < Flavor::ALL.len() {
        let fv = match Flavor::ALL.get(q) {
            Some(v) => *v,
            None => return false,
        };
        match f_ranks.get_mut(q) {
            Some(s) => *s = fv.rank(),
            None => return false,
        }
        match f_slugs.get_mut(q) {
            Some(s) => *s = fv.slug(),
            None => return false,
        }
        if Flavor::from_rank(fv.rank()) != Some(fv) {
            return false;
        }
        q += 1;
    }
    if !ranks_contiguous(&f_ranks, 1) || !slugs_unique(&f_slugs) {
        return false;
    }
    if !Flavor::Gnu.ignores_sigpipe() || Flavor::Busybox.ignores_sigpipe() {
        return false;
    }
    if !Flavor::Gnu.is_legible() || Flavor::Unknown.is_legible() {
        return false;
    }

    let mut n_ranks: [u32; 6] = [0; 6];
    let mut n_slugs: [&str; 6] = ["a", "a", "a", "a", "a", "a"];
    let mut r: usize = 0;
    while r < Note::ALL.len() {
        let nv = match Note::ALL.get(r) {
            Some(v) => *v,
            None => return false,
        };
        match n_ranks.get_mut(r) {
            Some(s) => *s = nv.rank(),
            None => return false,
        }
        match n_slugs.get_mut(r) {
            Some(s) => *s = nv.slug(),
            None => return false,
        }
        if Note::from_rank(nv.rank()) != Some(nv) {
            return false;
        }
        r += 1;
    }
    if !ranks_contiguous(&n_ranks, 1) || !slugs_unique(&n_slugs) {
        return false;
    }
    if !Note::GnuFile.blames_file() || !Note::GnuStdout.blames_stdout() {
        return false;
    }    if Note::GnuStdout.blames_file() || !Note::Busy.is_blind() || Note::Clean.reports_error() {
        return false;
    }

    let mut e_ranks: [u32; 5] = [0; 5];
    let mut e_slugs: [&str; 5] = ["a", "a", "a", "a", "a"];
    let mut t: usize = 0;
    while t < ErrMode::ALL.len() {
        let ev = match ErrMode::ALL.get(t) {
            Some(v) => *v,
            None => return false,
        };
        match e_ranks.get_mut(t) {
            Some(s) => *s = ev.rank(),
            None => return false,
        }
        match e_slugs.get_mut(t) {
            Some(s) => *s = ev.slug(),
            None => return false,
        }
        if ErrMode::from_rank(ev.rank()) != Some(ev) {
            return false;
        }
        t += 1;
    }
    if !ranks_contiguous(&e_ranks, 1) || !slugs_unique(&e_slugs) {
        return false;
    }
    if ErrMode::Continue.aborts() || !ErrMode::Exit.aborts() {
        return false;
    }
    if !ErrMode::ExitNopipe.excuses_pipe() || ErrMode::Exit.excuses_pipe() {
        return false;
    }

    if Tier::ALL.len() != TIER_COUNT || Fault::ALL.len() != 7 {
        return false;
    }
    let mut tier_ranks: [u32; 4] = [0; 4];
    let mut tier_slugs: [&str; 4] = ["a", "a", "a", "a"];
    let mut u: usize = 0;
    while u < Tier::ALL.len() {
        let tv = match Tier::ALL.get(u) {
            Some(v) => *v,
            None => return false,
        };
        match tier_ranks.get_mut(u) {
            Some(s) => *s = tv.rank(),
            None => return false,
        }
        match tier_slugs.get_mut(u) {
            Some(s) => *s = tv.slug(),
            None => return false,
        }
        if Tier::from_rank(tv.rank()) != Some(tv) {
            return false;
        }
        u += 1;
    }
    if !ranks_contiguous(&tier_ranks, 1) || !slugs_unique(&tier_slugs) {
        return false;
    }
    if Tier::Usage.is_half_family() || !Tier::Half.is_half_family() {
        return false;
    }

    let mut fa_ranks: [u32; 7] = [0; 7];
    let mut fa_slugs: [&str; 7] = ["a", "a", "a", "a", "a", "a", "a"];
    let mut v: usize = 0;
    while v < Fault::ALL.len() {
        let fv2 = match Fault::ALL.get(v) {
            Some(x) => *x,
            None => return false,
        };
        match fa_ranks.get_mut(v) {
            Some(s) => *s = fv2.rank(),
            None => return false,
        }
        match fa_slugs.get_mut(v) {
            Some(s) => *s = fv2.slug(),
            None => return false,
        }
        if Fault::from_rank(fv2.rank()) != Some(fv2) {
            return false;
        }
        if Fault::from_ndx(fv2.rank() as u8) != Some(fv2) {
            return false;
        }
        v += 1;
    }
    if !ranks_contiguous(&fa_ranks, 0) || !slugs_unique(&fa_slugs) {
        return false;
    }
    if Fault::Ok.is_fault() || Fault::Default.is_fault() == false {
        return false;
    }
    if Fault::from_rank(7).is_some() {
        return false;
    }
    if Fault::Ok.code() != 0 {
        return false;
    }
    let mut w: usize = 0;
    while w < REJECT_FAULTS.len() {
        let f = match REJECT_FAULTS.get(w) {
            Some(x) => *x,
            None => return false,
        };
        let c = match REJECT_CODES.get(w) {
            Some(x) => *x,
            None => return false,
        };
        if !f.is_fault() || f.code() != c {
            return false;
        }
        if f.rank() != (w as u32) + 1 {
            return false;
        }
        if f.why().len() == 0 {
            return false;
        }
        w += 1;
    }
    true
}

/// 判据 03：文件系统几何——容量、O_TRUNC 让位、errno 家族的数值闭合。
fn geom_ok() -> bool {
    if ENOSPC != 28 || EPIPE != 32 || EDQUOT != 122 || EACCES != 13 {
        return false;
    }
    if ENOSPC == EPIPE || ENOSPC == EDQUOT || EPIPE == EDQUOT {
        return false;
    }
    if errno_name(ENOSPC) != ANCHOR_ERRNO_TEXT {
        return false;
    }
    if errno_name(0) != "success" || errno_name(999) != "Unknown errno" {
        return false;
    }
    if !errno_legible(ENOSPC) || !errno_legible(EPIPE) || !errno_legible(EDQUOT) {
        return false;
    }
    if errno_legible(EACCES) || errno_legible(999) {
        return false;
    }
    if EXIT_OK != 0 || EXIT_FAIL != 1 || EXIT_USAGE != 2 || SHELL_DIED != 141 {
        return false;
    }
    if SIGPIPE_NO != 13 || SHELL_DIED != 128 + SIGPIPE_NO {
        return false;
    }
    // 打开闸的算术：-a 不给让位，裸 tee 给。
    let app = match shape_at(3usize) {
        Some(v) => v,
        None => return false,
    };
    if !app.append || app.trunc_freed != 4096 || app.file_free != 4096 {
        return false;
    }
    if app.file_room() != 4096 || app.file_room_no_append() != 8192 {
        return false;
    }
    if app.total != 8192 || !app.would_fit_without_append() {
        return false;
    }
    if app.file_room() >= app.total {
        return false;
    }
    // 只有这一槽带让位额，其余槽的旧文件都是零长度。
    let mut freed: usize = 0;
    let mut i: usize = 0;
    while i < CORPUS {
        let sh = match shape_at(i) {
            Some(v) => v,
            None => return false,
        };
        if sh.trunc_freed > 0 {
            freed = freed.saturating_add(1);
            if !sh.append {
                return false;
            }
        }
        if sh.append && sh.trunc_freed == 0 {
            return false;
        }
        if sh.file_room() < sh.file_free {
            return false;
        }
        if sh.file_room() > sh.file_free.saturating_add(sh.trunc_freed) {
            return false;
        }
        i += 1;
    }
    if freed != 1 {
        return false;
    }
    // 支路数的身份：两支路是 tee 的定义，不是可调参数。
    if SINK_COUNT != 2 || EXIT_RANKS != 3 {
        return false;
    }
    true
}

/// 判据 04：形态合法域与语料身份（死槽整槽复用活槽的事实面）。
fn shape_ok() -> bool {
    let mut i: usize = 0;
    let mut bad_opt: usize = 0;
    while i < CORPUS {
        let sh = match shape_at(i) {
            Some(v) => v,
            None => return false,
        };
        if !sh.domain_ok() {
            return false;
        }
        if sh.bad_option {
            bad_opt = bad_opt.saturating_add(1);
            if define(&sh).stage != Stage::Args {
                return false;
            }
        } else if define(&sh).stage == Stage::Args {
            return false;
        }
        if !sh.order_file_first {
            return false;
        }
        if sh.total == 0 {
            return false;
        }
        if ErrMode::from_rank(sh.mode.rank()) != Some(sh.mode) {
            return false;
        }
        if Flavor::from_rank(sh.flavor.rank()) != Some(sh.flavor) {
            return false;
        }
        i += 1;
    }
    if bad_opt != 1 {
        return false;
    }
    if !SHAPE_BUSY_PIPE.domain_ok() {
        return false;
    }
    // 后 6 槽的事实面必须整槽照抄某一活槽——一槽只坏一维，坏的是申报不是事实。
    let mut j: usize = 0;
    while j < REJECT {
        let idx = ACCEPT.saturating_add(j);
        let src = match DEAD_FROM.get(j) {
            Some(v) => *v,
            None => return false,
        };
        if src >= ACCEPT {
            return false;
        }
        let dead = match shape_at(idx) {
            Some(v) => v,
            None => return false,
        };
        let live = match shape_at(src) {
            Some(v) => v,
            None => return false,
        };
        if dead != live {
            return false;
        }
        if define(&dead) != define(&live) {
            return false;
        }
        j += 1;
    }
    if WANT_TAGS.len() != CORPUS {
        return false;
    }
    if slugs_unique(&WANT_TAGS) == false {
        return false;
    }
    true
}

/// 判据 05：九闸纵线的产出必须与事实面第二形态逐维相等。
fn define_ok() -> bool {
    let mut i: usize = 0;
    while i < CORPUS {
        let sh = match shape_at(i) {
            Some(v) => v,
            None => return false,
        };
        let o = define(&sh);
        let st = match ACT_STAGE.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let cd = match ACT_CODE.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let vl = match ACT_VALUE.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let nt = match ACT_NOTE.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let df = match ACT_DEFAULT.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let okf = match ACT_OK.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if o.stage != st || o.code != cd || o.value != vl || o.note != nt {
            return false;
        }
        if o.default != df || o.ok() != okf {
            return false;
        }
        if !o.stage.is_landing() {
            return false;
        }
        if o.render().len() == 0 {
            return false;
        }
        let name = match WANT_CODE_NAME.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if code_name(o.code) != name {
            return false;
        }
        i += 1;
    }
    // 中止模式停在归因闸，续写模式跑到结算闸——Stage 维的全部来源。
    let exit_mode = match shape_at(5usize) {
        Some(v) => v,
        None => return false,
    };
    if !aborts_at_errno(&exit_mode) || define(&exit_mode).stage != Stage::Errno {
        return false;
    }
    let cont = match shape_at(1usize) {
        Some(v) => v,
        None => return false,
    };
    if aborts_at_errno(&cont) || define(&cont).stage != Stage::Final {
        return false;
    }
    if aborts_at_errno(&SHAPE_BOTH_OK) {
        return false;
    }
    true
}

/// 判据 06：两支路分账——写入了多少、谁活着、宽度是多少。
fn half_ok() -> bool {
    let mut half_n: usize = 0;
    let mut i: usize = 0;
    while i < CORPUS {
        let sh = match shape_at(i) {
            Some(v) => v,
            None => return false,
        };
        let (fw, ow) = split_pair(&sh);
        let want_f = match ACT_FILE_WR.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let want_o = match ACT_STDOUT_WR.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if fw != want_f || ow != want_o {
            return false;
        }
        if fw > sh.total || ow > sh.total {
            return false;
        }
        let gap = fw.saturating_sub(ow).saturating_add(ow.saturating_sub(fw));
        if gap != define(&sh).value {
            return false;
        }
        let cas = casualty_of(&sh);
        match cas {
            SinkKind::None => {
                if sh.bad_option {
                    // 用法档：循环没起跑，两支路都停在 0，这不是"写全了"。
                    if fw != 0 || ow != 0 {
                        return false;
                    }
                } else if fw != sh.total || ow != sh.total {
                    return false;
                }
            }
            SinkKind::File => {
                if fw >= sh.total || ow != sh.total {
                    return false;
                }
                half_n = half_n.saturating_add(1);
            }
            SinkKind::Stdout => {
                if ow >= sh.total || fw != sh.total {
                    return false;
                }
                half_n = half_n.saturating_add(1);
            }
            SinkKind::Both => {
                if sh.bad_option {
                    return false;
                }
                if !(fw < sh.total) || !(ow < sh.total) {
                    return false;
                }
                // 本卡语料里的全损都来自"两支路齐停"（中止模式）：宽度必须归零，
                // 且必须真的被中止过。齐坏却没中止的形态不在本卡语料内。
                if gap != 0 || !aborts_at_errno(&sh) {
                    return false;
                }
                if define(&sh).code != TE_UNIQ {
                    return false;
                }
                if Tier::of(&define(&sh)) != Tier::Both {
                    return false;
                }
            }
        }
        // 半程档的每一格都必须有正宽度；宽度 0 的失败归全损。
        let tier = Tier::of(&define(&sh));
        if tier == Tier::Half && gap == 0 {
            return false;
        }
        if tier == Tier::Both && (gap != 0 || define(&sh).ok()) {
            return false;
        }
        i += 1;
    }
    if half_n != 9 {
        return false;
    }
    // 反事实：把缺省的"丢掉坏支路续写"换成 `--output-error=exit`，整个进程停在
    // 第一个撞上 errno 的缓冲区水位（48），两支路一起残缺、宽度归零，半程形态消失、
    // 档位落回全损。这一格证明 Stage 维与 Value 维是被模式**同时**牵动的。
    let mut swapped = match shape_at(2usize) {
        Some(v) => v,
        None => return false,
    };
    swapped.mode = ErrMode::Exit;
    let (a, b) = split_pair(&swapped);
    if a != b || a >= swapped.total {
        return false;
    }
    if casualty_of(&swapped) != SinkKind::Both {
        return false;
    }
    if Tier::of(&define(&swapped)) != Tier::Both {
        return false;
    }
    true
}

/// 判据 07：双写次序事实 + 闸门单调。前半是 tee 的写序，后半是 Gate。
fn order_ok() -> bool {
    let l1 = match shape_at(1usize) {
        Some(v) => v,
        None => return false,
    };
    let (f1, o1) = split_pair(&l1);
    if f1 >= l1.total || o1 != l1.total {
        return false;
    }
    let l2 = match shape_at(2usize) {
        Some(v) => v,
        None => return false,
    };
    let (f2, o2) = split_pair(&l2);
    if f2 != l2.total || o2 >= l2.total {
        return false;
    }
    if casualty_of(&l1) != SinkKind::File || casualty_of(&l2) != SinkKind::Stdout {
        return false;
    }
    let l4 = match shape_at(4usize) {
        Some(v) => v,
        None => return false,
    };
    if !is_pipe_error(&l4) {
        return false;
    }
    if casualty_of(&l4) != SinkKind::Stdout {
        return false;
    }
    let (f4, _) = split_pair(&l4);
    if f4 != l4.total {
        return false;
    }
    true
}

/// 判据 07 的后半：Gate 只许前进到下一秩，拒动留痕，封印后一律拒。
fn gate_ok() -> bool {
    let mut g = Gate::new();
    if g.rank() != 0 || g.is_closed() || g.fingerprint() != FNV_OFF {
        return false;
    }
    if g.step(Stage::Open).is_ok() {
        return false;
    }
    if g.fingerprint() == FNV_OFF {
        return false;
    }
    let after_skip = g.fingerprint();
    if g.step(Stage::Args).is_err() {
        return false;
    }
    if g.rank() != 1 {
        return false;
    }
    if g.fingerprint() == after_skip {
        return false;
    }
    if g.step(Stage::Args).is_ok() {
        return false;
    }
    if g.step(Stage::Open).is_err() {
        return false;
    }
    let mut k: usize = 2;
    while k < Stage::ALL.len() {
        let st = match Stage::ALL.get(k) {
            Some(v) => *v,
            None => return false,
        };
        if g.step(st).is_err() {
            return false;
        }
        k += 1;
    }
    if g.rank() != Stage::Closed.rank() {
        return false;
    }
    if g.close().is_err() || !g.is_closed() {
        return false;
    }
    let sealed = g.fingerprint();
    if g.step(Stage::Args).is_ok() {
        return false;
    }
    if g.close().is_ok() {
        return false;
    }
    if g.fingerprint() == sealed {
        return false;
    }
    if g.moves() < 12 || g.rejected() < 4 {
        return false;
    }
    // 倒退必须改指纹：两个不同的拒动序列不能给出同一个指纹。
    let mut h = Gate::new();
    let mut m: usize = 0;
    while m < 5 {
        let st = match Stage::ALL.get(m) {
            Some(v) => *v,
            None => return false,
        };
        if h.step(st).is_err() {
            return false;
        }
        m += 1;
    }
    let base = h.fingerprint();
    if h.step(Stage::Args).is_ok() {
        return false;
    }
    if h.fingerprint() == base {
        return false;
    }
    let back_fp = h.fingerprint();
    if h.rank() != Stage::Errno.rank() {
        return false;
    }
    if h.step(Stage::Open).is_ok() {
        return false;
    }
    if h.fingerprint() == back_fp {
        return false;
    }
    if h.close().is_ok() {
        return false;
    }
    let fresh = Gate::new();
    if fresh.fingerprint() != FNV_OFF {
        return false;
    }
    if fresh.rank() != 0 {
        return false;
    }
    if back_fp == h.fingerprint() {
        return false;
    }
    true
}

/// 判据 08：文案四式逐字。GNU 的引号、GNU stdout 的固定名、busybox 的无差别句式、
/// 配额式与 EPIPE 式各占一格，且形状特征（有无单引号、有无 `write error`）互斥。
fn message_ok() -> bool {
    if msg_gnu_file_default().as_str() != "tee: 'file': No space left on device" {
        return false;
    }
    if msg_gnu_stdout_default().as_str() != "tee: standard output: No space left on device" {
        return false;
    }
    if msg_busy_write_error().as_str() != "tee: write error: No space left on device" {
        return false;
    }
    if msg_gnu_broken_pipe().as_str() != "tee: standard output: Broken pipe" {
        return false;
    }
    if msg_gnu_quota(OPERAND).as_str() != "tee: 'file': Disk quota exceeded" {
        return false;
    }
    if msg_usage_opt('z').as_str() != "tee: invalid option -- 'z'" {
        return false;
    }
    if msg_usage_hint().as_str() != "Try 'tee --help' for more information." {
        return false;
    }
    if msg_foreign_stdout().as_str() != "tee: stdout: No space left on device" {
        return false;
    }
    if msg_note(Note::Clean).len() != 0 {
        return false;
    }
    // 形状特征：只有 GNU 文件式与配额式带单引号；只有 busybox 式带 write error。
    if !has_quoted_operand(msg_gnu_file_default().as_str()) {
        return false;
    }
    if !has_quoted_operand(msg_gnu_quota(OPERAND).as_str()) {
        return false;
    }
    if has_quoted_operand(msg_gnu_stdout_default().as_str()) {
        return false;
    }
    if has_quoted_operand(msg_busy_write_error().as_str()) {
        return false;
    }
    if msg_busy_write_error().as_str().contains("write error") == false {
        return false;
    }
    if msg_busy_write_error().as_str().contains("standard output") {
        return false;
    }
    if msg_gnu_file("file").as_str() == msg_gnu_file("out.txt").as_str() {
        return false;
    }
    // 空间族归属。
    if !is_space_family(msg_busy_write_error().as_str()) {
        return false;
    }
    if !is_space_family(msg_gnu_quota(OPERAND).as_str()) {
        return false;
    }
    if is_space_family(msg_gnu_broken_pipe().as_str()) {
        return false;
    }
    if !msg_gnu_file_default().as_str().contains(ANCHOR_ERRNO_TEXT) {
        return false;
    }
    // 逐槽：形状文案与档位规范文本一致，唯一的合法例外是 GNU 的 EPIPE 式。
    let mut i: usize = 0;
    while i < CORPUS {
        let sh = match shape_at(i) {
            Some(v) => v,
            None => return false,
        };
        let o = define(&sh);
        let shape_text = msg_for(&sh);
        let canon = msg_note(o.note);
        let special = sh.flavor == Flavor::Gnu
            && casualty_of(&sh) == SinkKind::Stdout
            && is_pipe_error(&sh);
        if special {
            if shape_text.as_str() != "tee: standard output: Broken pipe" {
                return false;
            }
            if canon.as_str() != "tee: standard output: No space left on device" {
                return false;
            }
        } else if shape_text.as_str() != canon.as_str() {
            return false;
        }
        if o.note.blames_file() && !has_quoted_operand(shape_text.as_str()) {
            return false;
        }
        if o.note.is_blind() && shape_text.as_str().contains(OPERAND) {
            return false;
        }
        i += 1;
    }
    // 对照形态：busybox 遇管道死成信号，一行错误都不打。
    if msg_for(&SHAPE_BUSY_PIPE).len() != 0 {
        return false;
    }
    if shell_status(&SHAPE_BUSY_PIPE) != SHELL_DIED {
        return false;
    }
    if exit_code(&SHAPE_BUSY_PIPE) != EXIT_FAIL {
        return false;
    }
    if exit_code(&l4_gnu()) != EXIT_FAIL || msg_for(&l4_gnu()).as_str() != "tee: standard output: Broken pipe" {
        return false;
    }
    true
}

/// 取槽 5 的 EPIPE 形态（判据内部复用，避免重复拼表）。
fn l4_gnu() -> Shape {
    match shape_at(4usize) {
        Some(v) => v,
        None => SHAPE_BOTH_OK,
    }
}

/// 判据 09：退出码三档与 short write 的关系。
fn exit_ok() -> bool {
    let mut zero: usize = 0;
    let mut one: usize = 0;
    let mut two: usize = 0;
    let mut i: usize = 0;
    while i < CORPUS {
        let sh = match shape_at(i) {
            Some(v) => v,
            None => return false,
        };
        let got = exit_code(&sh);
        let want = match WANT_EXIT.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if got != want {
            return false;
        }
        match got {
            0 => zero = zero.saturating_add(1),
            1 => one = one.saturating_add(1),
            2 => two = two.saturating_add(1),
            _ => return false,
        }
        // 事实面不许自相矛盾：退出码为 0 的槽两支路必须都全量。
        if got == EXIT_OK {
            let (fw, ow) = split_pair(&sh);
            if fw != sh.total || ow != sh.total {
                return false;
            }
            if define(&sh).code != 0 {
                return false;
            }
            if define(&sh).value != 0 {
                return false;
            }
        }
        if got == EXIT_USAGE && !sh.bad_option {
            return false;
        }
        if sh.bad_option && got != EXIT_USAGE {
            return false;
        }
        // 半程失败（宽度 > 0 的一支坏）永远退 1，绝不许退 0。
        let o = define(&sh);
        if o.value > 0 && o.code != 0 && got != EXIT_FAIL {
            return false;
        }
        if short_write_lies(&sh) {
            return false;
        }
        if shell_status(&sh) > SHELL_DIED {
            return false;
        }
        if !sh.flavor.ignores_sigpipe() && is_pipe_error(&sh) && shell_status(&sh) != SHELL_DIED {
            return false;
        }
        i += 1;
    }
    if zero != 2 || one != 11 || two != 1 {
        return false;
    }
    if zero.saturating_add(one).saturating_add(two) != CORPUS {
        return false;
    }
    // 谎报侧：申报把半程失败报成成功时，claim 折算出的 exit 与事实 exit 必须打架。
    let mut liars: usize = 0;
    let mut j: usize = 0;
    while j < CORPUS {
        let c = match case_at(j) {
            Some(v) => v,
            None => return false,
        };
        let sh = match shape_at(j) {
            Some(v) => v,
            None => return false,
        };
        if c.claim.success() && exit_code(&sh) != EXIT_OK {
            liars = liars.saturating_add(1);
        }
        if !c.claim.success() && exit_code(&sh) == EXIT_OK && !c.claim.default {
            return false;
        }
        if claim_exit(&c.claim) == 2 && !c.claim.default && define(&sh).stage != Stage::Args {
            return false;
        }
        j += 1;
    }
    if liars != 1 {
        return false;
    }
    true
}

/// 判据 10：矩阵分档——四档计数、空行不完美、两条口径闭合。
fn matrix_ok() -> bool {
    let fresh = Matrix::new();
    if fresh.perfect_rows() != 0 {
        return false;
    }
    if fresh.occupied() != 0 {
        return false;
    }
    if fresh.covered() == false {
        return false;
    }
    let empty_row = match fresh.rows.get(1usize) {
        Some(v) => *v,
        None => return false,
    };
    if empty_row.perfect() || !empty_row.empty() {
        return false;
    }
    let mut m = Matrix::new();
    m.push_actual(Tier::Full, true);
    m.push_actual(Tier::Full, false);
    if m.covered() == false {
        return false;
    }
    if m.perfect_rows() != 0 {
        return false;
    }
    if m.aligned() != 2 || m.total != 2 || m.synced != 2 {
        return false;
    }
    m.desync();
    if m.covered() {
        return false;
    }
    if m.total != 3 || m.aligned() != 2 {
        return false;
    }
    let full = match m.row(Tier::Full) {
        Some(v) => v,
        None => return false,
    };
    if full.total != 2 || full.good != 1 || full.miss() != 1 {
        return false;
    }
    if m.render().len() == 0 {
        return false;
    }

    let cm = Matrix::from_cases();
    if cm.total != CORPUS as u32 || cm.aligned() != CORPUS as u32 {
        return false;
    }
    if cm.synced != CORPUS as u32 || !cm.covered() {
        return false;
    }
    if cm.count_of(Tier::Full) != 2 {
        return false;
    }
    if cm.count_of(Tier::Half) != 9 {
        return false;
    }
    if cm.count_of(Tier::Both) != 2 {
        return false;
    }
    if cm.count_of(Tier::Usage) != 1 {
        return false;
    }
    if cm.occupied() != TIER_COUNT as u32 {
        return false;
    }
    if cm.matched != CORPUS as u32 || cm.perfect_rows() != TIER_COUNT as u32 {
        return false;
    }
    let mut k: usize = 0;
    while k < CORPUS {
        let sh = match shape_at(k) {
            Some(v) => v,
            None => return false,
        };
        let want_t = match WANT_TIER.get(k) {
            Some(v) => *v,
            None => return false,
        };
        if Tier::of(&define(&sh)) != want_t {
            return false;
        }
        k += 1;
    }
    if Tier::of(&Outcome::new(Stage::Args, TE_EXIT, 0, Note::Usage, false)) != Tier::Usage {
        return false;
    }
    if Tier::of(&Outcome::new(Stage::Final, 0, 0, Note::Clean, false)) != Tier::Full {
        return false;
    }
    true
}

/// 判据 11：全绿运行、出口闸与红档率标定。
fn report_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if r.total != CORPUS || r.accept != ACCEPT || r.reject != REJECT {
        return false;
    }
    if r.full != 2 || r.half != 9 || r.both != 2 || r.usage != 1 {
        return false;
    }
    if r.tier_sum() != CORPUS || r.matched != CORPUS {
        return false;
    }
    if r.rate() != RATE_FULL || !r.green() || r.gate() != Ok(()) {
        return false;
    }
    if r.render().as_str() != Report::RENDER_OK {
        return false;
    }
    if RENDER_OK.as_bytes().len() != Report::RENDER_OK.as_bytes().len() {
        return false;
    }
    if Report::RENDER_OK.contains("rate=1000") == false {
        return false;
    }
    if Report::RENDER_OK.contains("verdict=green") == false {
        return false;
    }
    if r.is_red_calibrated() {
        return false;
    }
    // 越界槽必须直接判死，不许悄悄当没看见。
    if run_with(Some((CORPUS, Fault::Value))) != Err(TE_SLOT) {
        return false;
    }
    if run_with(Some((99usize, Fault::Ok))) != Err(TE_SLOT) {
        return false;
    }
    // 六个缺陷维各注入一格活槽：命中数恰好掉 1，千分率恰好落在 929。
    let mut w: usize = 0;
    while w < REJECT {
        let f = match REJECT_FAULTS.get(w) {
            Some(v) => *v,
            None => return false,
        };
        match run_with(Some((0usize, f))) {
            Ok(x) => {
                if x.matched + 1 != x.total || x.rate() != RED_RATE || x.green() {
                    return false;
                }
                if !x.is_red_calibrated() {
                    return false;
                }
                if x.render().as_str() == RENDER_OK {
                    return false;
                }
                if x.gate() != Err(TE_STATE) {
                    return false;
                }
            }
            Err(_) => return false,
        }
        w += 1;
    }
    // 把死槽的期望维注入回去，结论一个都不变——注入不是万能改判器。
    match run_with(Some((8usize, Fault::Estate))) {
        Ok(x) => {
            if x.matched != CORPUS || x.rate() != RATE_FULL {
                return false;
            }
        }
        Err(_) => return false,
    }
    if RED_RATE != (13u32 * 1000u32 + 7u32) / 14u32 {
        return false;
    }
    if (13u32 * 1000u32 + (CORPUS as u32) / 2u32) / (CORPUS as u32) != RED_RATE {
        return false;
    }
    if all_claims_land() == false {
        return false;
    }
    let vs = verdicts();
    if vs.len() != CORPUS {
        return false;
    }
    let mut v_all = true;
    let mut i: usize = 0;
    while i < CORPUS {
        let c = match case_at(i) {
            Some(x) => x,
            None => return false,
        };
        match vs.get(i) {
            Some(f) => {
                if *f != judge(&c) {
                    v_all = false;
                }
            }
            None => v_all = false,
        }
        i += 1;
    }
    if !v_all {
        return false;
    }
    true
}

/// 判据 12：语料纪律——三形态互校（申报原表 / 期望表 / 判定序）与活死配比。
fn corpus_ok() -> bool {
    let mut i: usize = 0;
    while i < CORPUS {
        let raw = match claim_at(i) {
            Some(v) => v,
            None => return false,
        };
        let rebuilt = match claim_of(i) {
            Some(v) => v,
            None => return false,
        };
        if raw != rebuilt {
            return false;
        }
        let want_success = match WANT_OK.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let want_stage = match WANT_STAGE.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let want_code = match WANT_CODE.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let want_value = match WANT_VALUE.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let want_note = match WANT_NOTE.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let want_default = match WANT_DEFAULT.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if rebuilt.success != want_success
            || rebuilt.stage != want_stage
            || rebuilt.code != want_code
            || rebuilt.value != want_value
            || rebuilt.note != want_note
            || rebuilt.default != want_default
        {
            return false;
        }
        let sh = match shape_at(i) {
            Some(v) => v,
            None => return false,
        };
        let act = define(&sh);
        if i < ACCEPT {
            let m = raw.diff_mask(&act);
            if m != 0 {
                return false;
            }
            if judge(&Case::new(sh, raw)) != Fault::Ok {
                return false;
            }
        } else {
            let m = raw.diff_mask(&act);
            if m.count_ones() != 1 {
                return false;
            }
            let expect_bit: u32 = match want_fault(i) {
                Fault::Estate => 0x01,
                Fault::Stage => 0x02,
                Fault::Code => 0x04,
                Fault::Value => 0x08,
                Fault::Note => 0x10,
                Fault::Default => 0x20,
                Fault::Ok => 0x00,
            };
            if m != expect_bit {
                return false;
            }
            if judge(&Case::new(sh, raw)) != want_fault(i) {
                return false;
            }
        }
        i += 1;
    }
    if WANT_NDX.len() != CORPUS || WANT_STAGE.len() != CORPUS {
        return false;
    }
    if WANT_CODE.len() != CORPUS || WANT_VALUE.len() != CORPUS {
        return false;
    }
    if WANT_NOTE.len() != CORPUS || WANT_DEFAULT.len() != CORPUS {
        return false;
    }
    let mut live_ok: usize = 0;
    let mut dead_ok: usize = 0;
    let mut j: usize = 0;
    while j < CORPUS {
        match WANT_NDX.get(j) {
            Some(v) => {
                if *v == 0 {
                    live_ok = live_ok.saturating_add(1);
                } else {
                    dead_ok = dead_ok.saturating_add(1);
                }
            }
            None => return false,
        }
        j += 1;
    }
    if live_ok != ACCEPT || dead_ok != REJECT {
        return false;
    }
    // 活槽必须覆盖四个档位（每条闸门的正例都在场）。
    let mut seen: u32 = 0;
    let mut k: usize = 0;
    while k < ACCEPT {
        let sh = match shape_at(k) {
            Some(v) => v,
            None => return false,
        };
        let t = Tier::of(&define(&sh));
        seen |= 1u32 << t.rank();
        k += 1;
    }
    if seen & 0b0001_1110 != 0b0001_1110 {
        return false;
    }
    // 八活槽的归因码互不相同：一格一判，不许两格共用一个结论。
    let mut codes: [u32; ACCEPT] = [0; ACCEPT];
    let mut p: usize = 0;
    while p < ACCEPT {
        let sh = match shape_at(p) {
            Some(v) => v,
            None => return false,
        };
        match codes.get_mut(p) {
            Some(s) => *s = define(&sh).code,
            None => return false,
        }
        p += 1;
    }
    if codes.len() != ACCEPT {
        return false;
    }
    let mut a: usize = 0;
    while a < ACCEPT {
        let mut b = a + 1;
        while b < ACCEPT {
            if let (Some(x), Some(y)) = (codes.get(a), codes.get(b)) {
                if x == y {
                    return false;
                }
            }
            b += 1;
        }
        a += 1;
    }
    true
}

/// 判据 12 的后半：六格红档一维一槽、错误码不重复、槽位连续。
fn slots_ok() -> bool {
    let mut seen: u32 = 0;
    let mut i: usize = 0;
    while i < REJECT {
        let ndx = match WANT_NDX.get(ACCEPT + i) {
            Some(v) => *v,
            None => return false,
        };
        if ndx as usize != i + 1 {
            return false;
        }
        let f = match Fault::from_ndx(ndx) {
            Some(v) => v,
            None => return false,
        };
        if f.code() == 0 {
            return false;
        }
        seen |= 1u32 << f.rank();
        i += 1;
    }
    if seen != 0b111_1110 {
        return false;
    }
    let mut j: usize = 0;
    while j < REJECT_CODES.len() {
        let c = match REJECT_CODES.get(j) {
            Some(v) => *v,
            None => return false,
        };
        if code_offset(c) < 1 || code_offset(c) > 13 {
            return false;
        }
        let mut k = j + 1;
        while k < REJECT_CODES.len() {
            if let Some(x) = REJECT_CODES.get(k) {
                if *x == c {
                    return false;
                }
            }
            k += 1;
        }
        if c == 0 {
            return false;
        }
        j += 1;
    }
    // 缺陷槽的标签必须逐字挂上它坏掉的那一维。
    let dims: [&str; 6] = ["estate", "stage", "code", "value", "note", "default"];
    let mut d: usize = 0;
    while d < REJECT {
        let idx = ACCEPT + d;
        let tag = tag_at(idx);
        match dims.get(d) {
            Some(w) => {
                if tag.contains(w) == false {
                    return false;
                }
            }
            None => return false,
        }
        if tag_at(idx) == tag_at(d) {
            return false;
        }
        d += 1;
    }
    if tag_at(CORPUS + 1) != "out-of-range" {
        return false;
    }
    true
}

/// 判据 12 的账本半：14 格审计、乱序拒登、篡改拒登、冻结重放。
fn ledger_ok() -> bool {
    let mut l = build_ledger();
    if l.len() != SAMPLES || l.is_empty() {
        return false;
    }
    if l.audit().is_ok() == false {
        return false;
    }
    if l.ok_count() != ACCEPT || l.err_count() != REJECT {
        return false;
    }
    if l.count_op(OP_STAT) != STAT_CELLS || l.count_op(OP_RATE) != 1 {
        return false;
    }
    if l.count_op(OP_DEFECT) != REJECT {
        return false;
    }
    if l.rejects() != 0 {
        return false;
    }
    let mut i: usize = 0;
    while i < STAT_CELLS {
        let s = match l.sample(i) {
            Some(v) => v,
            None => return false,
        };
        let want_v = match VALS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if s.value != want_v || s.op != OP_STAT {
            return false;
        }
        if stat_meaning(i) == "越界" {
            return false;
        }
        i += 1;
    }
    if stat_meaning(7) == "越界" {
        return false;
    }
    if stat_meaning(99) != "越界" {
        return false;
    }
    let rate_cell = match l.sample(ACCEPT - 1) {
        Some(v) => v,
        None => return false,
    };
    if rate_cell.value != RED_RATE || rate_cell.op != OP_RATE {
        return false;
    }
    let mut j: usize = 0;
    while j < REJECT {
        let idx = ACCEPT + j;
        let s = match l.sample(idx) {
            Some(v) => v,
            None => return false,
        };
        let want_c = match REJECT_CODES.get(j) {
            Some(v) => *v,
            None => return false,
        };
        if s.code != want_c || s.value != 0 || s.ok {
            return false;
        }
        if !s.self_consistent() {
            return false;
        }
        j += 1;
    }
    // 越界/跳号：slot 必须严格等于当前长度。
    let before_len = l.len();
    let before_fp = l.fingerprint();
    if l.register(Sample::stat(9usize, 1)) != Err(TE_SLOT) {
        return false;
    }
    if l.len() != before_len || l.rejects() != 1 {
        return false;
    }
    if l.register(Sample::stat(0usize, 2)) != Err(TE_SLOT) {
        return false;
    }
    if l.fingerprint() != before_fp {
        return false;
    }
    // 篡改：ok 位与 code 互推不上（自相矛盾的一格）按身份错判死。
    let tamp = Sample { slot: SAMPLES as u8, op: OP_DEFECT, ok: true, code: TE_STAGE, value: 0 };
    if tamp.self_consistent() {
        return false;
    }
    if l.register(tamp) != Err(TE_UNIQ) {
        return false;
    }
    if l.rejects() != 3 {
        return false;
    }
    if l.render().len() == 0 {
        return false;
    }
    let root = l.freeze();
    if root == 0 || !l.is_frozen() || !l.is_sealed() {
        return false;
    }
    if l.replay() != Ok(root) {
        return false;
    }
    if l.register(Sample::rate(SAMPLES)) != Err(TE_STATE) {
        return false;
    }
    // 未封印的账不许重放。
    let fresh = Ledger::new();
    if fresh.replay() != Err(TE_STATE) {
        return false;
    }
    if !fresh.is_empty() || fresh.len() != 0 {
        return false;
    }
    if fresh.audit() != Err(TE_SLOT) {
        return false;
    }
    // 统计格值错 → TE_VALUE；缺陷码错 → TE_UNIQ；缺陷格值非 0 → TE_VALUE。
    if broken_value_ledger().audit() != Err(TE_VALUE) {
        return false;
    }
    if broken_code_ledger().audit() != Err(TE_UNIQ) {
        return false;
    }
    if broken_defect_ledger().audit() != Err(TE_VALUE) {
        return false;
    }
    let mut k: usize = 0;
    while k < ACCEPT {
        let a = match VALS.get(k) {
            Some(v) => *v,
            None => return false,
        };
        let b = match VALS_CHECK.get(k) {
            Some(v) => *v,
            None => return false,
        };
        if a != b {
            return false;
        }
        k += 1;
    }
    if VALS.len() != ACCEPT {
        return false;
    }
    let last_check = match VALS_CHECK.get(ACCEPT - 1) {
        Some(v) => *v,
        None => return false,
    };
    if last_check != RED_RATE {
        return false;
    }
    true
}

// ---------------------------------------------------------------------------
// 出口：13 条判据
// ---------------------------------------------------------------------------

/// 本卡自检出口。恰好 13 条判据，一条一闸，名字逐字钉死。
pub fn run_unx11684_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11684_tee_enospc_halfwrite");
    s.add("TE01-名称与段基址", name_ok(), "static");
    s.add("TE02-枚举与纵线", enums_ok(), "static");
    s.add("TE03-文件系统几何闸", geom_ok(), "static");
    s.add("TE04-形态与合法域", shape_ok(), "static");
    s.add("TE05-九闸纵线判定", define_ok(), "static");
    s.add("TE06-半程分账", half_ok(), "static");
    s.add("TE07-双写次序与闸门", order_ok() && gate_ok(), "static");
    s.add("TE08-文案四式逐字", message_ok(), "static");
    s.add("TE09-退出码与短写", exit_ok(), "static");
    s.add("TE10-矩阵分档与渲染", matrix_ok(), "static");
    s.add("TE11-全绿运行与红档率", report_ok(), "static");
    s.add(
        "TE12-语料账本双审计",
        corpus_ok() && slots_ok() && ledger_ok(),
        "static",
    );
    let mut l = build_ledger();
    let audited = l.audit() == Ok(());
    let root = l.freeze();
    let frozen = l.is_frozen();
    let replayed = l.replay() == Ok(root);
    let inj = inject_four();
    let mut four = true;
    let mut q: usize = 0;
    while q < inj.len() {
        if let Some(x) = inj.get(q) {
            if !x {
                four = false;
            }
        }
        q += 1;
    }
    s.add(
        "TE13-冻结重放四注入",
        audited && root != 0 && frozen && replayed && four,
        "static",
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_is_assigned_and_anchors_close() {
        assert_eq!(SEG_BASE, 0x6d160);
        assert!(anchors_agree());
        assert_eq!(SEG_BASE - ANCHOR_F11681_GAP, ANCHOR_F11681_SEG);
        assert_eq!(SEG_BASE - ANCHOR_F11682_GAP, ANCHOR_F11682_SEG);
        assert_eq!(SEG_BASE - ANCHOR_F11683_GAP, ANCHOR_F11683_SEG);
        assert_eq!(ENOSPC, ANCHOR_ENOSPC);
        assert_eq!(ANCHOR_ERRNO_TEXT, "No space left on device");
    }

    #[test]
    fn thirteen_codes_are_contiguous_and_distinct() {
        assert_eq!(ALL_CODES.len(), 13);
        assert_eq!(TE_SLOT, SEG_BASE + 1);
        assert_eq!(TE_SHORT, SEG_BASE + 13);
        assert_eq!(code_offset(TE_SLOT), 1);
        assert_eq!(code_offset(TE_SHORT), 13);
        assert_eq!(code_offset(SEG_BASE), 0);
        assert_eq!(code_offset(SEG_BASE - 1), -1);
        assert!(name_ok());
    }

    #[test]
    fn gate_ranks_are_one_based_and_zero_is_not_a_stage() {
        assert_eq!(Stage::Args.rank(), 1);
        assert_eq!(Stage::Closed.rank(), 9);
        assert!(Stage::from_rank(0).is_none());
        assert!(Stage::from_rank(10).is_none());
        assert!(Stage::from_rank(1) == Some(Stage::Args));
        assert!(enums_ok());
    }

    #[test]
    fn gate_only_advances_to_the_next_rank() {
        let mut g = Gate::new();
        assert_eq!(g.rank(), 0);
        assert_eq!(g.step(Stage::Open), Err(TE_ORDER));
        assert_eq!(g.rank(), 0);
        assert!(g.step(Stage::Args).is_ok());
        assert_eq!(g.rank(), 1);
        assert_eq!(g.step(Stage::Args), Err(TE_ORDER));
    }

    #[test]
    fn rejected_moves_change_the_fingerprint() {
        let mut g = Gate::new();
        let before = g.fingerprint();
        assert_eq!(g.step(Stage::Final), Err(TE_ORDER));
        assert_ne!(g.fingerprint(), before);
        assert_eq!(g.rank(), 0);
        assert_eq!(g.rejected(), 1);
        assert_eq!(g.moves(), 1);
    }

    #[test]
    fn backward_step_is_a_stage_fault() {
        let mut g = Gate::new();
        assert!(g.walk_and_seal().is_ok());
        let mut h = Gate::new();
        let mut k = 0usize;
        while k < 5 {
            if let Some(st) = Stage::ALL.get(k) {
                assert!(h.step(*st).is_ok());
            }
            k += 1;
        }
        assert_eq!(h.rank(), Stage::Errno.rank());
        let fp = h.fingerprint();
        assert_eq!(h.step(Stage::Args), Err(TE_STAGE));
        assert_ne!(h.fingerprint(), fp);
        assert_eq!(h.rank(), Stage::Errno.rank());
    }

    #[test]
    fn close_before_the_spine_end_is_refused() {
        let mut g = Gate::new();
        assert!(g.step(Stage::Args).is_ok());
        assert_eq!(g.close(), Err(TE_STAGE));
        assert!(!g.is_closed());
    }

    #[test]
    fn after_close_everything_is_a_state_fault() {
        let mut g = Gate::new();
        assert!(g.walk_and_seal().is_ok());
        let fp = g.fingerprint();
        assert_eq!(g.step(Stage::Closed), Err(TE_STATE));
        assert_eq!(g.close(), Err(TE_STATE));
        assert_ne!(g.fingerprint(), fp);
        assert!(gate_ok());
    }

    #[test]
    fn clean_double_write_lands_at_final_with_zero_gap() {
        let o = define(&SHAPE_BOTH_OK);
        assert_eq!(o.stage, Stage::Final);
        assert_eq!(o.code, 0);
        assert_eq!(o.value, 0);
        assert_eq!(o.note, Note::Clean);
        assert!(!o.default);
        assert!(o.ok());
        assert_eq!(Tier::of(&o), Tier::Full);
        assert_eq!(exit_code(&SHAPE_BOTH_OK), EXIT_OK);
    }

    #[test]
    fn file_enospc_is_a_half_write_with_a_surviving_stdout() {
        let (fw, ow) = split_pair(&SHAPE_FILE_ENOSPC);
        assert_eq!((fw, ow), (32, 64));
        let o = define(&SHAPE_FILE_ENOSPC);
        assert_eq!(o.code, TE_CAP);
        assert_eq!(o.value, 32);
        assert_eq!(o.note, Note::GnuFile);
        assert!(!o.ok());
        assert_eq!(exit_code(&SHAPE_FILE_ENOSPC), EXIT_FAIL);
        assert_eq!(Tier::of(&o), Tier::Half);
    }

    #[test]
    fn ordering_keeps_the_file_whole_when_stdout_fails_later() {
        let (fw, ow) = split_pair(&SHAPE_STDOUT_ENOSPC);
        assert_eq!((fw, ow), (96, 48));
        assert_eq!(casualty_of(&SHAPE_STDOUT_ENOSPC), SinkKind::Stdout);
        let o = define(&SHAPE_STDOUT_ENOSPC);
        assert_eq!(o.code, TE_ORDER);
        assert_eq!(o.value, 48);
        assert_eq!(o.note, Note::GnuStdout);
    }

    #[test]
    fn append_is_the_only_reason_the_file_ran_out() {
        let s = SHAPE_APPEND_ENOSPC;
        assert_eq!(s.file_room(), 4096);
        assert_eq!(s.file_room_no_append(), 8192);
        assert!(s.would_fit_without_append());
        let o = define(&s);
        assert_eq!(o.code, TE_APPEND);
        assert_eq!(o.value, 4096);
        let mut no_append = s;
        no_append.append = false;
        let o2 = define(&no_append);
        assert_eq!(o2.code, 0);
        assert!(o2.ok());
    }

    #[test]
    fn pipe_failure_is_not_a_space_failure() {
        assert!(is_pipe_error(&SHAPE_EPIPE_STDOUT));
        assert_eq!(SHAPE_EPIPE_STDOUT.stdout_errno, EPIPE);
        assert_ne!(SHAPE_EPIPE_STDOUT.stdout_errno, ENOSPC);
        let o = define(&SHAPE_EPIPE_STDOUT);
        assert_eq!(o.code, TE_SHORT);
        assert_eq!(o.value, 256);
        assert_eq!(o.note, Note::GnuStdout);
        assert_eq!(msg_for(&SHAPE_EPIPE_STDOUT).as_str(), "tee: standard output: Broken pipe");
        assert_eq!(exit_code(&SHAPE_EPIPE_STDOUT), EXIT_FAIL);
        assert_eq!(shell_status(&SHAPE_EPIPE_STDOUT), EXIT_FAIL);
    }

    #[test]
    fn busybox_pipe_death_says_nothing_and_exits_141() {
        assert_eq!(exit_code(&SHAPE_BUSY_PIPE), EXIT_FAIL);
        assert_eq!(shell_status(&SHAPE_BUSY_PIPE), SHELL_DIED);
        assert_eq!(msg_for(&SHAPE_BUSY_PIPE).len(), 0);
        assert_eq!(SHELL_DIED, 128 + SIGPIPE_NO);
        assert!(!Flavor::Busybox.ignores_sigpipe());
        assert!(Flavor::Gnu.ignores_sigpipe());
    }

    #[test]
    fn both_sinks_dead_is_not_a_half_write() {
        let o = define(&SHAPE_BOTH_ENOSPC);
        assert_eq!(o.code, TE_UNIQ);
        assert_eq!(o.value, 0);
        assert_eq!(o.stage, Stage::Errno);
        assert_eq!(Tier::of(&o), Tier::Both);
        assert!(aborts_at_errno(&SHAPE_BOTH_ENOSPC));
        assert_eq!(msg_for(&SHAPE_BOTH_ENOSPC).as_str(), "tee: write error: No space left on device");
    }

    #[test]
    fn quota_is_not_device_full() {
        let o = define(&SHAPE_EDQUOT_FILE);
        assert_eq!(o.code, TE_CODE);
        assert_eq!(o.note, Note::Foreign);
        assert_eq!(o.value, 512);
        assert!(!o.default);
        assert_eq!(
            msg_for(&SHAPE_EDQUOT_FILE).as_str(),
            "tee: 'file': Disk quota exceeded"
        );
        assert!(!is_space_family(msg_gnu_broken_pipe().as_str()));
        assert!(is_space_family(msg_gnu_quota(OPERAND).as_str()));
    }

    #[test]
    fn usage_error_writes_nothing_and_exits_two() {
        let o = define(&SHAPE_USAGE_BAD);
        assert_eq!(o.stage, Stage::Args);
        assert_eq!(o.code, TE_EXIT);
        assert_eq!(o.value, 0);
        assert_eq!(o.note, Note::Usage);
        assert_eq!(Tier::of(&o), Tier::Usage);
        assert_eq!(split_pair(&SHAPE_USAGE_BAD), (0, 0));
        assert_eq!(exit_code(&SHAPE_USAGE_BAD), EXIT_USAGE);
        assert_eq!(casualty_of(&SHAPE_USAGE_BAD), SinkKind::None);
    }

    #[test]
    fn gnu_quotes_the_file_operand_and_leaves_stdout_bare() {
        assert_eq!(msg_gnu_file_default().as_str(), "tee: 'file': No space left on device");
        assert_eq!(
            msg_gnu_stdout_default().as_str(),
            "tee: standard output: No space left on device"
        );
        assert!(has_quoted_operand(msg_gnu_file_default().as_str()));
        assert!(!has_quoted_operand(msg_gnu_stdout_default().as_str()));
        assert!(message_ok());
    }

    #[test]
    fn busybox_text_does_not_blame_a_sink() {
        let t = msg_busy_write_error();
        assert_eq!(t.as_str(), "tee: write error: No space left on device");
        assert!(t.as_str().contains("write error"));
        assert!(!t.as_str().contains("standard output"));
        assert!(!has_quoted_operand(t.as_str()));
        assert!(Note::Busy.is_blind());
    }

    #[test]
    fn usage_text_is_verbatim() {
        assert_eq!(msg_usage_opt('z').as_str(), "tee: invalid option -- 'z'");
        assert_eq!(
            msg_usage_hint().as_str(),
            "Try 'tee --help' for more information."
        );
    }

    #[test]
    fn estate_lie_is_caught_before_everything_else() {
        let c = match case_at(8usize) {
            Some(v) => v,
            None => panic!("slot 8 missing"),
        };
        assert_eq!(judge(&c), Fault::Estate);
        assert!(c.claim.success());
        assert!(!define(&c.shape).ok());
        assert_eq!(exit_code(&c.shape), EXIT_FAIL);
        assert_eq!(claim_exit(&c.claim), EXIT_OK);
        assert!(!short_write_lies(&c.shape));
    }

    #[test]
    fn a_case_wrong_in_two_dims_reports_the_earlier_dim() {
        let sh = match shape_at(8usize) {
            Some(v) => v,
            None => panic!("slot 8 missing"),
        };
        let bad = Claim::new(true, Stage::Args, TE_CAP, 32, Note::Busy, true);
        assert_eq!(judge(&Case::new(sh, bad)), Fault::Estate);
        let ok_claim = Claim::from_outcome(&define(&sh), false);
        assert_eq!(judge(&Case::new(sh, ok_claim)), Fault::Ok);
    }

    #[test]
    fn stage_skip_is_caught() {
        let c = match case_at(9usize) {
            Some(v) => v,
            None => panic!("slot 9 missing"),
        };
        assert_eq!(judge(&c), Fault::Stage);
        assert_eq!(define(&c.shape).stage, Stage::Errno);
        assert_eq!(c.claim.stage, Stage::Final);
    }

    #[test]
    fn code_swap_is_caught() {
        let c = match case_at(10usize) {
            Some(v) => v,
            None => panic!("slot 10 missing"),
        };
        assert_eq!(judge(&c), Fault::Code);
        assert_eq!(c.claim.code, TE_CAP);
        assert_eq!(define(&c.shape).code, TE_CODE);
    }

    #[test]
    fn value_off_is_caught() {
        let c = match case_at(11usize) {
            Some(v) => v,
            None => panic!("slot 11 missing"),
        };
        assert_eq!(judge(&c), Fault::Value);
        assert_eq!(c.claim.value, 96);
        assert_eq!(define(&c.shape).value, 48);
    }

    #[test]
    fn note_swap_is_caught() {
        let c = match case_at(12usize) {
            Some(v) => v,
            None => panic!("slot 12 missing"),
        };
        assert_eq!(judge(&c), Fault::Note);
        assert_eq!(c.claim.note, Note::Busy);
        assert_eq!(define(&c.shape).note, Note::GnuFile);
    }

    #[test]
    fn default_bucket_is_caught() {
        let c = match case_at(13usize) {
            Some(v) => v,
            None => panic!("slot 13 missing"),
        };
        assert_eq!(judge(&c), Fault::Default);
        assert!(c.claim.default);
        assert!(!define(&c.shape).default);
    }

    #[test]
    fn live_slots_all_land_and_dead_slots_break_one_dim() {
        assert!(all_claims_land());
        assert_eq!(slot_count(), CORPUS);
        let mut i = 0usize;
        while i < CORPUS {
            let c = match case_at(i) {
                Some(v) => v,
                None => panic!("case missing"),
            };
            assert_eq!(judge(&c), want_fault(i));
            let m = c.claim.diff_mask(&define(&c.shape));
            if i < ACCEPT {
                assert_eq!(m, 0);
            } else {
                assert_eq!(m.count_ones(), 1);
            }
            i += 1;
        }
        assert!(corpus_ok());
    }

    #[test]
    fn dead_slots_reuse_a_live_fact_face_verbatim() {
        let mut j = 0usize;
        while j < REJECT {
            let idx = ACCEPT + j;
            let src = match DEAD_FROM.get(j) {
                Some(v) => *v,
                None => panic!("dead map short"),
            };
            assert_eq!(shape_at(idx), shape_at(src));
            assert_ne!(tag_at(idx), tag_at(src));
            j += 1;
        }
        assert!(shape_ok());
    }

    #[test]
    fn run_is_green_and_render_is_pinned() {
        let r = match run() {
            Ok(v) => v,
            Err(_) => panic!("run failed"),
        };
        assert!(r.green());
        assert!(r.gate().is_ok());
        assert_eq!(r.rate(), RATE_FULL);
        assert_eq!(r.total, CORPUS);
        assert_eq!(r.full, 2);
        assert_eq!(r.half, 9);
        assert_eq!(r.both, 2);
        assert_eq!(r.usage, 1);
        assert_eq!(r.render().as_str(), RENDER_OK);
        assert!(report_ok());
    }

    #[test]
    fn out_of_range_slot_is_a_slot_fault() {
        assert_eq!(run_with(Some((CORPUS, Fault::Value))), Err(TE_SLOT));
        assert_eq!(run_with(Some((40usize, Fault::Ok))), Err(TE_SLOT));
        assert!(run_with(None).is_ok());
    }

    #[test]
    fn one_red_slot_rates_exactly_929() {
        let r = match run_with(Some((0usize, Fault::Note))) {
            Ok(v) => v,
            Err(_) => panic!("injection failed"),
        };
        assert_eq!(r.matched, 13);
        assert_eq!(r.total, 14);
        assert_eq!(r.rate(), RED_RATE);
        assert_eq!(RED_RATE, (13 * 1000 + 7) / 14);
        assert!(!r.green());
        assert_eq!(r.gate(), Err(TE_STATE));
        assert!(r.is_red_calibrated());
        assert_ne!(r.render().as_str(), RENDER_OK);
    }

    #[test]
    fn injecting_a_dead_slots_own_dim_changes_nothing() {
        let r = match run_with(Some((11usize, Fault::Value))) {
            Ok(v) => v,
            Err(_) => panic!("injection failed"),
        };
        assert_eq!(r.matched, CORPUS);
        assert_eq!(r.rate(), RATE_FULL);
    }

    #[test]
    fn matrix_counts_tiers_and_refuses_empty_perfection() {
        let fresh = Matrix::new();
        assert_eq!(fresh.perfect_rows(), 0);
        assert_eq!(fresh.occupied(), 0);
        assert!(fresh.covered());
        let cm = Matrix::from_cases();
        assert_eq!(cm.total, CORPUS as u32);
        assert_eq!(cm.aligned(), cm.total);
        assert_eq!(cm.perfect_rows(), TIER_COUNT as u32);
        let mut d = Matrix::from_cases();
        d.desync();
        assert!(!d.covered());
        assert_eq!(d.total, d.aligned() + 1);
        assert!(matrix_ok());
    }

    #[test]
    fn ledger_seventh_stats_one_rate_six_defects() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.count_op(OP_STAT), 7);
        assert_eq!(l.count_op(OP_RATE), 1);
        assert_eq!(l.count_op(OP_DEFECT), REJECT);
        assert_eq!(l.ok_count(), ACCEPT);
        assert_eq!(l.err_count(), REJECT);
        assert!(l.audit().is_ok());
        assert!(ledger_ok());
    }

    #[test]
    fn ledger_red_cell_is_the_rate_not_a_count() {
        assert_eq!(VALS.len(), ACCEPT);
        let eighth = match VALS.get(7usize) {
            Some(v) => *v,
            None => panic!("vals short"),
        };
        assert_eq!(eighth, 929);
        let l = build_ledger();
        let cell = match l.sample(7usize) {
            Some(v) => v,
            None => panic!("cell missing"),
        };
        assert_eq!(cell.value, RED_RATE);
        assert_eq!(cell.op, OP_RATE);
        assert_eq!(stat_meaning(7usize), "红档率 929：14 格中 13 的千分率");
    }

    #[test]
    fn out_of_order_registration_is_refused_and_counted() {
        let mut l = build_ledger();
        let before = l.rejects();
        assert_eq!(l.register(Sample::stat(3usize, 3)), Err(TE_SLOT));
        assert_eq!(l.rejects(), before + 1);
        assert_eq!(l.len(), SAMPLES);
        let tamp = Sample { slot: SAMPLES as u8, op: OP_STAT, ok: true, code: TE_VALUE, value: 5 };
        assert!(!tamp.self_consistent());
        assert_eq!(l.register(tamp), Err(TE_UNIQ));
    }

    #[test]
    fn frozen_ledger_refuses_writes_and_replays_its_root() {
        let mut l = build_ledger();
        let root = l.freeze();
        assert_ne!(root, 0);
        assert!(l.is_frozen());
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.register(Sample::rate(9usize)), Err(TE_STATE));
        let fresh = Ledger::new();
        assert_eq!(fresh.replay(), Err(TE_STATE));
        assert!(fresh.is_empty());
    }

    #[test]
    fn audit_catches_stat_defect_and_identity_drift() {
        assert_eq!(broken_value_ledger().audit(), Err(TE_VALUE));
        assert_eq!(broken_code_ledger().audit(), Err(TE_UNIQ));
        assert_eq!(broken_defect_ledger().audit(), Err(TE_VALUE));
    }

    #[test]
    fn inject_four_routes_are_all_caught() {
        let inj = inject_four();
        assert_eq!(inj.len(), 4);
        let mut all = true;
        let mut k = 0usize;
        while k < inj.len() {
            if let Some(x) = inj.get(k) {
                assert!(*x);
                if !x {
                    all = false;
                }
            }
            k += 1;
        }
        assert!(all);
        assert_eq!(INJECT_ROUTES.len(), 4);
    }

    #[test]
    fn text_helpers_render_without_recursion_or_panic() {
        assert_eq!(dec_text(0).as_str(), "0");
        assert_eq!(dec_text(929).as_str(), "929");
        assert_eq!(dec_text(4294967295).as_str(), "4294967295");
        assert_eq!(hex_text(0x6d160, 5).as_str(), "6d160");
        assert_eq!(hex_text(255, 2).as_str(), "ff");
        assert_eq!(seg_text(SEG_BASE).as_str(), "0x06d160");
        assert_eq!(u32_text(14).as_str(), "14");
        assert_eq!(i32_text(-13).as_str(), "-13");
        assert_eq!(i32_text(0).as_str(), "0");
        assert_eq!(bool_text(true).as_str(), "true");
        assert_eq!(bool_text(false).as_str(), "false");
    }

    #[test]
    fn abort_mode_collapses_both_sinks_to_one_watermark() {
        let mut s = SHAPE_STDOUT_ENOSPC;
        s.mode = ErrMode::Exit;
        let (a, b) = split_pair(&s);
        assert_eq!(a, b);
        assert_eq!(a, 48);
        assert_eq!(define(&s).value, 0);
        assert_eq!(define(&s).stage, Stage::Errno);
        let mut q = SHAPE_STDOUT_ENOSPC;
        q.mode = ErrMode::ExitNopipe;
        // exit-nopipe 只豁免**管道**错误：这一格的 stdout 是重定向到满盘的普通文件，
        // 报的是 ENOSPC，所以照样中止。
        assert!(aborts_at_errno(&q));
        assert!(!is_pipe_error(&q));
        let mut w = SHAPE_EPIPE_STDOUT;
        w.mode = ErrMode::ExitNopipe;
        assert!(!aborts_at_errno(&w));
        let mut x = SHAPE_EPIPE_STDOUT;
        x.mode = ErrMode::Exit;
        assert!(aborts_at_errno(&x));
    }

    #[test]
    fn exit_code_tally_is_two_zero_eleven_one_and_one_two() {
        let mut z = 0usize;
        let mut o = 0usize;
        let mut t = 0usize;
        let mut i = 0usize;
        while i < CORPUS {
            let sh = match shape_at(i) {
                Some(v) => v,
                None => panic!("shape missing"),
            };
            match exit_code(&sh) {
                0 => z += 1,
                1 => o += 1,
                _ => t += 1,
            }
            i += 1;
        }
        assert_eq!((z, o, t), (2, 11, 1));
        assert!(exit_ok());
    }

    #[test]
    fn ten_lies_about_the_gap_are_all_value_faults() {
        let sh = SHAPE_FILE_ENOSPC;
        let truth = define(&sh);
        let mut k = 1u32;
        while k <= 10 {
            let wrong = truth.value + k;
            let c = Claim::from_outcome(&truth, false);
            assert_eq!(judge(&Case::new(sh, c)), Fault::Ok);
            let lied = Claim::new(false, truth.stage, truth.code, wrong, truth.note, truth.default);
            if wrong == truth.value {
                assert_eq!(judge(&Case::new(sh, lied)), Fault::Ok);
            } else {
                assert_eq!(judge(&Case::new(sh, lied)), Fault::Value);
            }
            k += 1;
        }
    }

    #[test]
    fn tier_rule_keeps_usage_ahead_of_everything() {
        let u = Outcome::new(Stage::Args, TE_UNIQ, 4096, Note::Busy, false);
        assert_eq!(Tier::of(&u), Tier::Usage);
        let b = Outcome::new(Stage::Errno, TE_UNIQ, 0, Note::Busy, false);
        assert_eq!(Tier::of(&b), Tier::Both);
        let h = Outcome::new(Stage::Final, TE_CAP, 32, Note::GnuFile, false);
        assert_eq!(Tier::of(&h), Tier::Half);
        let f = Outcome::new(Stage::Final, 0, 0, Note::Clean, false);
        assert_eq!(Tier::of(&f), Tier::Full);
    }

    #[test]
    fn fold_order_matters_for_the_fingerprint() {
        let a = fold_str(FNV_OFF, "ab");
        let b = fold_str(a, "c");
        let c = fold_str(FNV_OFF, "a");
        let d = fold_str(c, "bc");
        assert_ne!(b, d);
        let e = fold_str(FNV_OFF, "");
        assert_ne!(e, FNV_OFF);
        assert_eq!(fold_word(FNV_OFF, 0) != FNV_OFF, true);
    }

    #[test]
    fn checks_are_thirteen_and_all_green() {
        let cs = run_unx11684_checks();
        assert_eq!(cs.len(), 13);
        assert!(cs.all_passed());
        assert_eq!(cs.failed_count(), 0);
        let last = match cs.get(12usize) {
            Some(v) => v,
            None => panic!("check 13 missing"),
        };
        assert_eq!(last.name, "TE13-冻结重放四注入");
        let first = match cs.get(0usize) {
            Some(v) => v,
            None => panic!("check 1 missing"),
        };
        assert_eq!(first.name, "TE01-名称与段基址");
    }
}

