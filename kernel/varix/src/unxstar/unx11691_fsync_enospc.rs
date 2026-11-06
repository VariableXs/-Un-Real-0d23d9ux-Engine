//! UNX-F11691 fsync 面与 ENOSPC 判据
//!
//! 这张卡管的是延迟分配（delayed allocation）时代最反直觉的一条事实：
//! `write()` 返回的字节数**不代表数据落盘**。XFS/ext4 都先把数据挂进页缓存，
//! 块分配推迟到回写；盘真满了，`write()` 当场不一定报错——ENOSPC 被推迟到
//! `fsync()`/`fdatasync()`/`close()`/下一次 `write()` 里的某一个出口。于是
//! "应用写成功、空间其实不够" 变成一个**时间差问题**，本卡判的就是这个时间差
//! 上的四件事：
//!
//! * 写面与同步面的**责任切分**：O_DIRECT 或块预留足够的文件系统上，ENOSPC
//!   在 `write()` 当场返回（写面判死）；延迟分配下，写面全绿、错误在同步面
//!   浮出（同步面判死）。两面的文案、退出码、处置完全不同，混认即判错。
//! * 错误态的**一次性消费**：Linux 的页缓存回写错误状态在第一次被报告后清除，
//!   第二次 `fsync()` 可能返回成功——数据从来没落盘，但没人再报错了。这是
//!   "错误丢失" 的实况形态，本卡把它单列一档（Stranded），不许并进成功。
//! * `sync_file_range()` 与不带同步面的写法：`sync_file_range` 只下指令不等
//!   回执，ENOSPC 根本不在它的返回面上；靠 close/重开文件去"撞"错误是另一条
//!   实况（close 后重新打开读回短尾）。两种"没人接错误"的形态共享 Stranded
//!   档。
//! * 文案归属：GNU dd 的 `conv=fsync` 失败行是
//!   `dd: fsync failed for 'x': No space left on device`，`conv=fdatasync`
//!   的失败行是 `dd: fdatasync failed for 'x': ...`；普通写出失败行是
//!   `dd: error writing 'x': ...`；`cp` 面是 `cp: error writing 'x': ...`。
//!   三式各归各闸，把同步面的失败认成写面的失败，就是丢掉了"延迟分配"这个
//!   本卡最重要的事实。
//!
//! ## 闸门次序（这条纵线为什么不能倒）
//!
//! 本卡的九道闸按下面这个序走，秩从 1 起算，前一闸不产出事实，后一闸就没有
//! 输入。`Gate` 只允许"前进到下一秩"，倒退与跳档都要留下指纹痕迹，封印之后
//! 一律拒动。
//!
//! 1. 调用闸（Call，秩 1）——write/fsync 这层调用本身是否成立：坏文件描述符、
//!    非法参数、命令行用法错，都在这闸判死，退 2 或 EBADF，一个字节都没进缓存。
//! 2. 缓存闸（Cache，秩 2）——write 的字节进页缓存。这一闸产出的事实是
//!    "写面返回了成功"，它**不等于**空间够；它是整条延迟分配纵线的起点。
//! 3. 配给闸（Alloc，秩 3）——文件系统按块预留/延迟分配决定这块写面要不要
//!    当场给 ENOSPC：O_DIRECT、ext4 早期分配、预留充足时写面会在这里判死；
//!    XFS 式延迟分配放行到下一闸。
//! 4. 落盘闸（Flush，秩 4）——脏页回写真正消耗设备空间，短尾（申报字节与
//!    实落字节之差）在这一闸形成，回写错误挂进地址空间的错误态。
//! 5. 同步闸（Sync，秩 5）——fsync/fdatasync 等待回写完成并领取回执；
//!    本卡的靶心闸：延迟分配把 ENOSPC 押送到这里才交付。
//! 6. 回执闸（Report，秩 6）——同步面向调用方报告 errno。它与同步闸分开，
//!    因为"等待完成"与"报告错误"是两件事：fdatasync 只等数据不等元数据、
//!    sync_file_range 连等都不等，回执面各自不同。
//! 7. 消费闸（Consume，秩 7）——错误态被领取一次即清除。这一闸是
//!    "第二次 fsync 返回成功" 的成因，也是本卡与恢复面卡（F11696）的边界：
//!    本卡只管"错误被吞了吗"，不管"清理空间后重试"。
//! 8. 结算闸（Exit，秩 8）——工具面折算退出码：0 = 两面全绿；1 = 任一面报了
//!    写出/同步错误；2 = 用法错。短尾与退出码的闭合关系在这一闸结算。
//! 9. 封印闸（Closed，秩 9）——账本冻结、指纹比对。没走完前八闸不许封印；
//!    封印之后任何 step/close 都拒。它是"这一轮判据已经出口"的唯一凭据。
//!
//! ## 关键差异（本卡深度所在）
//!
//! - 与 F11681（ENOSPC 总锚，段基址 `0x6d130`）的边界：总锚判的是**写面即时**
//!   的 `write() = -1 + errno ENOSPC`——错误与调用同帧出现。本卡判的是它的
//!   镜像面：写面全绿、错误在另一帧浮出。总锚不许"成功写入却空间不足"的形态
//!   存在，本卡恰恰以该形态为主判据（Deferred 档）。两卡段位差 0x260，本卡用
//!   自声明锚常量署名对齐，不 import 对方任何符号。
//! - 与 F11684（tee 半程失败，段基址 `0x6d160`）的边界：半程卡判**空间维的
//!   分叉**（两个目的地字节数分叉），本卡判**时间维的分叉**（同一目的地，
//!   写面与同步面分叉）。半程卡的 value 是两支路宽度，本卡的 value 是短尾
//!   字节数（申报 - 实落）。两卡段位差 0x230。
//! - 与 F11689（df 呈现，段基址 `0x6d360`）的边界：df 卡判"avail 为 0 的
//!   呈现面"，是旁观者的读数；本卡判"写进去的人什么时候被告知"，是当事人的
//!   回执。df 不改变错误态，fsync 消费错误态——这是两卡纵线的根本不同。
//! - 与 F11696（释放后重试恢复，本批恢复面锚）的边界：本卡的 Consumed 档只判
//!   "错误被吞"这一事实本身；释放空间→重试成功→数据完整 的正判据归 F11696。
//!   本卡不许把 Consumed 认成恢复，也不许把恢复记回本卡段位。
//! - 延迟 ≠ 丢失：Deferred 档（错误在同步面报出来）与 Stranded 档（错误没人
//!   报）必须分档。把 Deferred 认成写面失败（Stage 维谎报）与把 Stranded 认成
//!   成功（Estate 维谎报）是本卡两条最贵的死槽。
//! - 红档率 929 独占账本一格：它是 14 格里恰好中 13 格时的千分率
//!   `(13*1000 + 14/2) / 14 = 929`，是一个比率标定，不许并进 7 个统计格。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 本模块段位基址（全仓唯一，取自分配表）。
pub const SEG_BASE: u32 = 0x6d390;

/// 交叉署名：F11681「ENOSPC 总锚」的段基址。本卡**不 import** 那个模块的任何
/// 符号，只把它的段位与位距原样声明一遍，由 `anchors_agree()` 复算对齐。
pub const ANCHOR_F11681_SEG: u32 = 0x6d130;
/// F11681 与本卡的位距（0x6d390 - 0x6d130 = 0x260）。
pub const ANCHOR_F11681_GAP: u32 = 0x260;
/// 交叉署名：F11684「tee 半程失败」的段基址。
pub const ANCHOR_F11684_SEG: u32 = 0x6d160;
/// F11684 与本卡的位距（0x6d390 - 0x6d160 = 0x230）。
pub const ANCHOR_F11684_GAP: u32 = 0x230;
/// 交叉署名：F11689「df 0 avail 呈现」的段基址。
pub const ANCHOR_F11689_SEG: u32 = 0x6d360;
/// F11689 与本卡的位距（0x6d390 - 0x6d360 = 0x30）。
pub const ANCHOR_F11689_GAP: u32 = 0x30;

/// 与 F11681 总锚在 errno 面上的署名：ENOSPC 在 Linux 就是 28，两条卡同数。
pub const ANCHOR_ENOSPC: u32 = 28;
/// 与 F11694 文案卡在 strerror 面上的署名：本卡所有报错尾巴只能是这一串。
pub const ANCHOR_ERRNO_TEXT: &str = "No space left on device";

/// 错误码 01：槽位越界/跳号——`run_with` 的注入槽超出语料、或账本登记时
/// `slot != len`（错误延迟最容易在账上少记一帧）。
pub const FS_SLOT: u32 = SEG_BASE + 0x01;
/// 错误码 02：状态违例——账本已冻结还登记、闸门已封印还 step/close，
/// 全绿运行里出现未命中槽；也是 Estate 维（写成功谎报/失败谎报成功）的出口码。
pub const FS_STATE: u32 = SEG_BASE + 0x02;
/// 错误码 03：身份撞格——错误码字面量重复、段位与锚位距不闭合，
/// 以及"写面与同步面同时报同一笔错误"被误记成两笔独立损失。
pub const FS_UNIQ: u32 = SEG_BASE + 0x03;
/// 错误码 04：阶段倒退/提前封印——申报的阶段与本卡纵线产出的阶段不合（Stage 维）。
pub const FS_STAGE: u32 = SEG_BASE + 0x04;
/// 错误码 05：errno 家族判错——EDQUOT（配额尽）在同步面被认成 ENOSPC（Code 维）。
pub const FS_CODE: u32 = SEG_BASE + 0x05;
/// 错误码 06：数值维不合——短尾字节数（申报写成功减实落）算错或虚报（Value 维）。
pub const FS_VALUE: u32 = SEG_BASE + 0x06;
/// 错误码 07：文案维不合——`fsync failed for` / `fdatasync failed for` /
/// `error writing` 三式认错归属（Note 维）。
pub const FS_NOTE: u32 = SEG_BASE + 0x07;
/// 错误码 08：兜底档误走——事实面本可判定却被丢进"未知工具/未知错误态"垃圾桶
/// （Default 维）。
pub const FS_FAULT_DEFAULT: u32 = SEG_BASE + 0x08;
/// 错误码 09：延迟分配面——写面全绿、ENOSPC 押送到同步面交付。本卡靶心档的署名码。
pub const FS_DEFER: u32 = SEG_BASE + 0x09;
/// 错误码 10：错误态消费——回写错误已被前一次报告领取并清除，本次回执面静默。
/// "第二次 fsync 成功而数据从未落盘"由这一码钉死。
pub const FS_CLEAR: u32 = SEG_BASE + 0x0A;
/// 错误码 11：同步面/写面切分错档——O_DIRECT 使 ENOSPC 在写面即时返回；
/// 另用于闸门跳档（秩不合纵线）。
pub const FS_KIND: u32 = SEG_BASE + 0x0B;
/// 错误码 12：用法与退出码档位不合——坏描述符/非法参数，一字节未进缓存即退 2。
pub const FS_EXIT: u32 = SEG_BASE + 0x0C;
/// 错误码 13：错误被吞——没有任何同步面领取回执（sync_file_range 不等、
/// 或压根不 sync），ENOSPC 悬置无人报。
pub const FS_SWAL: u32 = SEG_BASE + 0x0D;

/// 全部 13 个错误码的枚举表（供互异性与值域复核，顺序即 0x01..0x0D）。
pub const ALL_CODES: [u32; 13] = [
    FS_SLOT,
    FS_STATE,
    FS_UNIQ,
    FS_STAGE,
    FS_CODE,
    FS_VALUE,
    FS_NOTE,
    FS_FAULT_DEFAULT,
    FS_DEFER,
    FS_CLEAR,
    FS_KIND,
    FS_EXIT,
    FS_SWAL,
];

/// 错误码总数（本卡固定 13）。
pub const CODE_COUNT: usize = 13;

/// 语料槽数：8 个活形态 + 6 个死形态。
pub const CORPUS: usize = 14usize;
/// 账本登记样本数（与语料同宽，一格不落）。
pub const SAMPLES: usize = 14usize;
/// 活形态数（延迟/即时/悬置各档的正例）。
pub const ACCEPT: usize = 8usize;
/// 死形态数（一槽只坏一维，依次挂 Estate/Stage/Code/Value/Note/Default）。
pub const REJECT: usize = 6usize;
/// 档位维数（全绿 / 延迟报出 / 写面即时 / 错误悬置）。
pub const TIER_COUNT: usize = 4usize;
/// 纵线闸数（含封印闸）。
pub const STAGE_COUNT: u32 = 9u32;

/// 同步面种类数：fsync / fdatasync / sync_file_range / 不 sync 靠 close。
pub const SYNC_FACE_COUNT: u32 = 4u32;
/// 参与退出的码位（0/1/2）。
pub const EXIT_RANKS: u32 = 3u32;

/// Linux errno：ENOSPC = 28，设备/文件系统无空间。
pub const ENOSPC: u32 = 28;
/// Linux errno：EDQUOT = 122，配额耗尽——同步面上与 ENOSPC 同一张脸。
pub const EDQUOT: u32 = 122;
/// Linux errno：EBADF = 9，坏文件描述符——调用闸的即时拒。
pub const EBADF: u32 = 9;

/// dd/cp 的退出码：两面全绿。
pub const EXIT_OK: u32 = 0;
/// dd/cp 的退出码：任一面报了写出或同步错误（含延迟报出）。
pub const EXIT_FAIL: u32 = 1;
/// dd/cp 的退出码：用法/参数错，一字节未进缓存。
pub const EXIT_USAGE: u32 = 2;

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

/// u32 → 十六进制文本，宽度钳到 0..=8，不足位补零（小写，贴 errno 习惯）。
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

/// 段位文本：`0x6d390` 形态（本卡所有段位都以此形制出现，便于跨卡比对）。
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
    /// 秩 1：调用闸——fd/参数本身，坏 fd 与用法错在此判死。
    Call,
    /// 秩 2：缓存闸——write 的字节进页缓存，写面返回成功。
    Cache,
    /// 秩 3：配给闸——块预留/延迟分配决定错误在写面还是同步面交付。
    Alloc,
    /// 秩 4：落盘闸——回写真正消耗空间，短尾在此形成。
    Flush,
    /// 秩 5：同步闸——fsync/fdatasync 等待回写；延迟错误的押送终点。
    Sync,
    /// 秩 6：回执闸——同步面向调用方报 errno；Deferred 档的落点。
    Report,
    /// 秩 7：消费闸——错误态领取一次即清除；Stranded 档的成因。
    Consume,
    /// 秩 8：结算闸——工具折算退出码；纵线跑完的落点。
    Exit,
    /// 秩 9：封印闸——账本冻结，本轮判据出口。
    Closed,
}

impl Stage {
    /// 全部九道闸，按纵线次序。
    pub const ALL: [Stage; 9] = [
        Stage::Call,
        Stage::Cache,
        Stage::Alloc,
        Stage::Flush,
        Stage::Sync,
        Stage::Report,
        Stage::Consume,
        Stage::Exit,
        Stage::Closed,
    ];

    /// 秩（1 起算）。
    pub fn rank(&self) -> u32 {
        match self {
            Stage::Call => 1,
            Stage::Cache => 2,
            Stage::Alloc => 3,
            Stage::Flush => 4,
            Stage::Sync => 5,
            Stage::Report => 6,
            Stage::Consume => 7,
            Stage::Exit => 8,
            Stage::Closed => 9,
        }
    }

    /// 短名（渲染与判据名共用）。
    pub fn slug(&self) -> &'static str {
        match self {
            Stage::Call => "call",
            Stage::Cache => "cache",
            Stage::Alloc => "alloc",
            Stage::Flush => "flush",
            Stage::Sync => "sync",
            Stage::Report => "report",
            Stage::Consume => "consume",
            Stage::Exit => "exit",
            Stage::Closed => "closed",
        }
    }

    /// 中文名（自检明细与文档共用）。
    pub fn cn(&self) -> &'static str {
        match self {
            Stage::Call => "调用闸",
            Stage::Cache => "缓存闸",
            Stage::Alloc => "配给闸",
            Stage::Flush => "落盘闸",
            Stage::Sync => "同步闸",
            Stage::Report => "回执闸",
            Stage::Consume => "消费闸",
            Stage::Exit => "结算闸",
            Stage::Closed => "封印闸",
        }
    }

    /// 由秩还原阶段；秩 0 与秩 >9 都不合法（未起跑不是阶段）。
    pub fn from_rank(r: u32) -> Option<Stage> {
        match r {
            1 => Some(Stage::Call),
            2 => Some(Stage::Cache),
            3 => Some(Stage::Alloc),
            4 => Some(Stage::Flush),
            5 => Some(Stage::Sync),
            6 => Some(Stage::Report),
            7 => Some(Stage::Consume),
            8 => Some(Stage::Exit),
            9 => Some(Stage::Closed),
            _ => None,
        }
    }

    /// 下一道闸；封印闸没有下一道。
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

    /// 是否属于 `define` 能真实产出的落点（Call/Report/Exit 三点）。
    /// 本卡事实面只会停在三处：调用即拒、同步面报出、纵线跑完结算。
    pub fn is_landing(&self) -> bool {
        matches!(self, Stage::Call | Stage::Report | Stage::Exit)
    }
}

/// 同步面种类——本卡纵线第 5/6 闸的走法由它决定。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncKind {
    /// 秩 1：`fsync()`——等数据与必要的元数据，错误在返回面上交付。
    Fsync,
    /// 秩 2：`fdatasync()`——只等数据，元数据按需；错误面与 fsync 相同，
    /// 但文案与 dd 的 conv 档不同，归案必须分开。
    Fdatasync,
    /// 秩 3：`sync_file_range()`——只下刷写指令，不等回执；ENOSPC 不在
    /// 它的返回面上，错误必然悬置到别的口。
    SyncRange,
    /// 秩 4：不 sync——靠 close/重开/下次 write 去撞错误，或根本不撞。
    None,
}

impl SyncKind {
    /// 全部四格。
    pub const ALL: [SyncKind; 4] = [
        SyncKind::Fsync,
        SyncKind::Fdatasync,
        SyncKind::SyncRange,
        SyncKind::None,
    ];

    /// 秩。
    pub fn rank(&self) -> u32 {
        match self {
            SyncKind::Fsync => 1,
            SyncKind::Fdatasync => 2,
            SyncKind::SyncRange => 3,
            SyncKind::None => 4,
        }
    }

    /// 短名。
    pub fn slug(&self) -> &'static str {
        match self {
            SyncKind::Fsync => "fsync",
            SyncKind::Fdatasync => "fdatasync",
            SyncKind::SyncRange => "sync-range",
            SyncKind::None => "no-sync",
        }
    }

    /// 中文名。
    pub fn cn(&self) -> &'static str {
        match self {
            SyncKind::Fsync => "全同步面",
            SyncKind::Fdatasync => "数据同步面",
            SyncKind::SyncRange => "区段刷写面",
            SyncKind::None => "无同步面",
        }
    }

    /// 由秩还原。
    pub fn from_rank(r: u32) -> Option<SyncKind> {
        match r {
            1 => Some(SyncKind::Fsync),
            2 => Some(SyncKind::Fdatasync),
            3 => Some(SyncKind::SyncRange),
            4 => Some(SyncKind::None),
            _ => None,
        }
    }

    /// 该面会不会**等待**回写完成并把错误带回调用方。
    /// fsync/fdatasync 等；sync_file_range 不等回执；不 sync 更不等。
    pub fn awaits(&self) -> bool {
        matches!(self, SyncKind::Fsync | SyncKind::Fdatasync)
    }

    /// 该面是否属于"本卡能逐字认出文案"的面（无同步面根本没有行）。
    pub fn has_line(&self) -> bool {
        matches!(self, SyncKind::Fsync | SyncKind::Fdatasync | SyncKind::SyncRange)
    }
}

/// 回写错误态（页缓存地址空间的 writeback error state）。本卡的第二台机器，
/// Consumed 一格是"错误丢失"实况的全部来源：领取一次即清除。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrState {
    /// 秩 1：干净——没有挂起的回写错误。
    Idle,
    /// 秩 2：挂账——回写失败已记录，等待第一个领取者。
    Pending,
    /// 秩 3：已消费——此前已被某次报告领取并清除；再问就是成功。
    Consumed,
}

impl ErrState {
    /// 全部三格。
    pub const ALL: [ErrState; 3] = [ErrState::Idle, ErrState::Pending, ErrState::Consumed];

    /// 秩。
    pub fn rank(&self) -> u32 {
        match self {
            ErrState::Idle => 1,
            ErrState::Pending => 2,
            ErrState::Consumed => 3,
        }
    }

    /// 短名。
    pub fn slug(&self) -> &'static str {
        match self {
            ErrState::Idle => "idle",
            ErrState::Pending => "pending",
            ErrState::Consumed => "consumed",
        }
    }

    /// 由秩还原。
    pub fn from_rank(r: u32) -> Option<ErrState> {
        match r {
            1 => Some(ErrState::Idle),
            2 => Some(ErrState::Pending),
            3 => Some(ErrState::Consumed),
            _ => None,
        }
    }

    /// 是否有可领取的错误。已消费的一格**不可再领**——这是消费闸的全部判据。
    pub fn claimable(&self) -> bool {
        matches!(self, ErrState::Pending)
    }
}

/// 工具面（报错前缀与句式由它决定；Raw 面没有前缀行）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    /// 秩 1：GNU dd——conv=fsync/fdatasync 两式专属；写面用 error writing。
    Dd,
    /// 秩 2：GNU cp——只有写面式 `cp: error writing`，没有同步面式。
    Cp,
    /// 秩 3：裸 syscall 面——只有 errno 数值与 strerror 串，没有工具行。
    Raw,
    /// 秩 4：认不出的实现——本卡句式对它不适用，只能进兜底。
    Unknown,
}

impl Tool {
    /// 全部四格。
    pub const ALL: [Tool; 4] = [Tool::Dd, Tool::Cp, Tool::Raw, Tool::Unknown];

    /// 秩。
    pub fn rank(&self) -> u32 {
        match self {
            Tool::Dd => 1,
            Tool::Cp => 2,
            Tool::Raw => 3,
            Tool::Unknown => 4,
        }
    }

    /// 短名。
    pub fn slug(&self) -> &'static str {
        match self {
            Tool::Dd => "dd",
            Tool::Cp => "cp",
            Tool::Raw => "raw",
            Tool::Unknown => "unknown",
        }
    }

    /// 由秩还原。
    pub fn from_rank(r: u32) -> Option<Tool> {
        match r {
            1 => Some(Tool::Dd),
            2 => Some(Tool::Cp),
            3 => Some(Tool::Raw),
            4 => Some(Tool::Unknown),
            _ => None,
        }
    }

    /// 该面能不能打出同步式文案（只有 dd 有 conv=fsync/fdatasync 的检查）。
    pub fn can_sync_line(&self) -> bool {
        matches!(self, Tool::Dd)
    }

    /// 该面是否有本卡可认的整行句式。
    pub fn is_legible(&self) -> bool {
        matches!(self, Tool::Dd | Tool::Cp)
    }
}
/// 文案档位（回执闸的产出：谁、在哪个面、用哪一式报了错）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Note {
    /// 秩 1：没有任何错误行（两面全绿）。
    Clean,
    /// 秩 2：GNU dd 同步式——`dd: fsync failed for 'f': No space left on device`。
    DdFsync,
    /// 秩 3：GNU dd 数据同步式——`dd: fdatasync failed for 'f': ...`。
    DdFdatasync,
    /// 秩 4：写面式——`dd: error writing 'f': ...` / `cp: error writing 'f': ...`。
    WriteErr,
    /// 秩 5：同步面报的是配额——`dd: fsync failed for 'f': Disk quota exceeded`。
    Quota,
    /// 秩 6：一行都不打（错误悬置/已消费）——沉默本身就是档位。
    Silent,
    /// 秩 7：用法错——命令行就没对，压根没进缓存。
    Usage,
}

impl Note {
    /// 全部七格。
    pub const ALL: [Note; 7] = [
        Note::Clean,
        Note::DdFsync,
        Note::DdFdatasync,
        Note::WriteErr,
        Note::Quota,
        Note::Silent,
        Note::Usage,
    ];

    /// 秩。
    pub fn rank(&self) -> u32 {
        match self {
            Note::Clean => 1,
            Note::DdFsync => 2,
            Note::DdFdatasync => 3,
            Note::WriteErr => 4,
            Note::Quota => 5,
            Note::Silent => 6,
            Note::Usage => 7,
        }
    }

    /// 短名。
    pub fn slug(&self) -> &'static str {
        match self {
            Note::Clean => "clean",
            Note::DdFsync => "dd-fsync",
            Note::DdFdatasync => "dd-fdatasync",
            Note::WriteErr => "write-error",
            Note::Quota => "quota",
            Note::Silent => "silent",
            Note::Usage => "usage",
        }
    }

    /// 由秩还原。
    pub fn from_rank(r: u32) -> Option<Note> {
        match r {
            1 => Some(Note::Clean),
            2 => Some(Note::DdFsync),
            3 => Some(Note::DdFdatasync),
            4 => Some(Note::WriteErr),
            5 => Some(Note::Quota),
            6 => Some(Note::Silent),
            7 => Some(Note::Usage),
            _ => None,
        }
    }

    /// 这一格是否报在**同步面**（fsync/fdatasync 两式 + 配额回执）。
    pub fn blames_sync_face(&self) -> bool {
        matches!(self, Note::DdFsync | Note::DdFdatasync | Note::Quota)
    }

    /// 这一格是否报在**写面**（error writing 一式）。
    pub fn blames_write_face(&self) -> bool {
        matches!(self, Note::WriteErr)
    }

    /// 这一格是否算"报出了错误"。沉默不算报——这是 Stranded 档的文案指纹。
    pub fn reports_error(&self) -> bool {
        !matches!(self, Note::Clean | Note::Silent)
    }
}

/// 档位维（矩阵分档用，本卡四档）。判据的核心在于 Deferred 与 Stranded 的分野：
/// 延迟报出的错误必须**有人报**，悬置的错误一个字节都没被领取——两档的处置
/// 完全不同（前者可依据报错止损，后者必须改写法），混档即判错。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// 秩 1：全绿——写面与同步面都没errno，数据完整落盘。
    Clean,
    /// 秩 2：延迟报出——写面全绿、ENOSPC 在同步面浮出（本卡靶心档）。
    Deferred,
    /// 秩 3：写面即时——O_DIRECT 即时拒，或调用闸就判死（总锚卡的地盘）。
    Direct,
    /// 秩 4：错误悬置——短尾存在但没有任何面领取回执。
    Stranded,
}

impl Tier {
    /// 全部四档。
    pub const ALL: [Tier; 4] = [Tier::Clean, Tier::Deferred, Tier::Direct, Tier::Stranded];

    /// 秩。
    pub fn rank(&self) -> u32 {
        match self {
            Tier::Clean => 1,
            Tier::Deferred => 2,
            Tier::Direct => 3,
            Tier::Stranded => 4,
        }
    }

    /// 短名。
    pub fn slug(&self) -> &'static str {
        match self {
            Tier::Clean => "clean",
            Tier::Deferred => "deferred",
            Tier::Direct => "direct",
            Tier::Stranded => "stranded",
        }
    }

    /// 中文名。
    pub fn cn(&self) -> &'static str {
        match self {
            Tier::Clean => "全绿档",
            Tier::Deferred => "延迟报出档",
            Tier::Direct => "写面即时档",
            Tier::Stranded => "错误悬置档",
        }
    }

    /// 由秩还原。
    pub fn from_rank(r: u32) -> Option<Tier> {
        match r {
            1 => Some(Tier::Clean),
            2 => Some(Tier::Deferred),
            3 => Some(Tier::Direct),
            4 => Some(Tier::Stranded),
            _ => None,
        }
    }

    /// 分档规则：这是本卡把"事实面"折成"一档"的唯一出口。
    /// 次序不可换——先看有没有码（全绿档），再看是不是悬置（无人领取优先，
    /// 因为悬置形态也可能落在 Report 面之外的任意阶段），再看调用闸即时拒，
    /// 剩下的非零码都是同步面报出。
    pub fn of(actual: &Outcome) -> Tier {
        if actual.code == 0 {
            return Tier::Clean;
        }
        if actual.code == FS_SWAL || actual.code == FS_CLEAR {
            return Tier::Stranded;
        }
        if actual.stage == Stage::Call {
            return Tier::Direct;
        }
        Tier::Deferred
    }

    /// 这一档是否属于"错误的时间差"这一族（本卡的靶心：延迟与悬置）。
    pub fn is_timing_family(&self) -> bool {
        matches!(self, Tier::Deferred | Tier::Stranded)
    }
}

/// 判定维序（judge 的六维 + Ok）。次序固定：Estate → Stage → Code → Value →
/// Note → Default → Ok。Estate 最先：本卡的头号谎报恰恰是"写面成功 = 落盘成功"
/// ——成败互报（数据没落盘却报全绿）比阶段错更贵，先于一切判死。
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
    /// 秩 4：短尾字节数不合。
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
            Fault::Estate => FS_STATE,
            Fault::Stage => FS_STAGE,
            Fault::Code => FS_CODE,
            Fault::Value => FS_VALUE,
            Fault::Note => FS_NOTE,
            Fault::Default => FS_FAULT_DEFAULT,
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
            Fault::Estate => "数据从未落盘却报成同步成功",
            Fault::Stage => "O_DIRECT 写面即时拒却申报成同步面报出",
            Fault::Code => "EDQUOT 在同步面被认成 ENOSPC",
            Fault::Value => "短尾虚报：申报写成功的字节全被当成落盘",
            Fault::Note => "dd 同步式 fsync failed for 认成写面式 error writing",
            Fault::Default => "本可判定的全绿落盘被丢进兜底档",
        }
    }
}

/// errno 家族名（Linux strerror 口径，与 F11694 文案卡逐字同源）。
pub fn errno_name(e: u32) -> &'static str {
    match e {
        0 => "success",
        ENOSPC => "No space left on device",
        EDQUOT => "Disk quota exceeded",
        EBADF => "Bad file descriptor",
        _ => "Unknown errno",
    }
}

/// errno 是否属本卡可判家族（延迟分配只在这几种回执里打转）。
pub fn errno_legible(e: u32) -> bool {
    matches!(e, 0 | ENOSPC | EDQUOT | EBADF)
}

/// 本卡 13 个错误码的名字（诊断渲染用；越界值落 out-of-segment）。
pub fn code_name(code: u32) -> &'static str {
    if code == 0 {
        return "clean";
    }
    if code == FS_SLOT {
        "fs-slot"
    } else if code == FS_STATE {
        "fs-state"
    } else if code == FS_UNIQ {
        "fs-uniq"
    } else if code == FS_STAGE {
        "fs-stage"
    } else if code == FS_CODE {
        "fs-code"
    } else if code == FS_VALUE {
        "fs-value"
    } else if code == FS_NOTE {
        "fs-note"
    } else if code == FS_FAULT_DEFAULT {
        "fs-default"
    } else if code == FS_DEFER {
        "fs-defer"
    } else if code == FS_CLEAR {
        "fs-clear"
    } else if code == FS_KIND {
        "fs-kind"
    } else if code == FS_EXIT {
        "fs-exit"
    } else if code == FS_SWAL {
        "fs-swal"
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

/// 本卡输入的全部维度——一次"写 + 同步"能被观察到的所有事实。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shape {
    /// write() 申报写成功的总字节数。
    pub write_bytes: u32,
    /// 设备/配额实际能让这个文件增长到的字节数（含已有长度归零后的口径）。
    pub room: u32,
    /// 是否 O_DIRECT：真值意味着配给闸即时判死，错误不进延迟通道。
    pub direct: bool,
    /// 同步面种类。
    pub kind: SyncKind,
    /// 进入同步面**之前**的回写错误态（Consumed 一格专判"第二次报告"实况）。
    pub prior: ErrState,
    /// 同步面失败报的是 EDQUOT 而非 ENOSPC（配额延迟与空间延迟同形不同罪）。
    pub quota: bool,
    /// 工具面（文案前缀与句式来源）。
    pub tool: Tool,
    /// 调用是否非法（坏 fd / 非法参数）：调用闸判死，一字节未进缓存。
    pub bad_call: bool,
}

impl Shape {
    /// 一条干净的同步形态（空间够、无挂账错误）。
    pub fn clean(total: u32, kind: SyncKind) -> Shape {
        Shape {
            write_bytes: total,
            room: total,
            direct: false,
            kind,
            prior: ErrState::Idle,
            quota: false,
            tool: Tool::Dd,
            bad_call: false,
        }
    }

    /// 短尾：申报写成功与实落字节的差（saturating，不留减法溢出空间）。
    pub fn shortfall(&self) -> u32 {
        self.write_bytes.saturating_sub(self.room)
    }

    /// 实落字节（min(write_bytes, room)）。
    pub fn landed(&self) -> u32 {
        if self.room >= self.write_bytes {
            self.write_bytes
        } else {
            self.room
        }
    }

    /// 假想把 O_DIRECT 去掉以后，错误会落在哪个面——本卡用它证明
    /// "即时/延迟两面的分野完全由 direct 位决定"。
    pub fn would_defer_without_direct(&self) -> bool {
        self.direct && self.shortfall() > 0 && self.kind.awaits()
    }

    /// 合法域复核：真正要判的是"错误态与短尾的闭合"——
    /// prior 非 Idle 的前提是确实存在过短尾；bad_call 则一切归零。
    pub fn domain_ok(&self) -> bool {
        if self.bad_call {
            return self.write_bytes == 0 && self.prior == ErrState::Idle && !self.quota;
        }
        if self.prior != ErrState::Idle && self.shortfall() == 0 {
            return false;
        }
        if self.quota && self.shortfall() == 0 {
            return false;
        }
        if self.room > self.write_bytes.saturating_add(1_000_000) {
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
    /// 归因码（0 = 两面全绿）。
    pub code: u32,
    /// 短尾字节数：申报写成功 - 实落。
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

    /// 成败位：错误码为 0 才是成。延迟报出/悬置/即时三族的错误码都非 0，
    /// 因此**必然**是败——这一位是 Estate 维的锚，"写成功 = 落盘成功"的谎报
    /// 就在此处露出。
    pub fn ok(&self) -> bool {
        self.code == 0
    }

    /// 是否有一个面领取了回执（Silent 档没有）。
    pub fn reported(&self) -> bool {
        self.note.reports_error()
    }

    /// 折叠五维进指纹（顺序固定：阶段→码→短尾→文案→兜底）。
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
        s.push_str("tail=");
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
/// 现实里"报成功/报失败"就是工具退出码这一维独立可谎报的东西。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Claim {
    /// 申报的成败（真实语义即 exit==0）。
    pub success: bool,
    /// 申报的落点阶段。
    pub stage: Stage,
    /// 申报的归因码。
    pub code: u32,
    /// 申报的短尾字节数。
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

/// 槽 1 `clean-fsync`：写 4096、空间 4096、fsync 全绿。
/// 唯一能退 0 的基准形态，全绿档的正例。
const SHAPE_CLEAN_FSYNC: Shape = Shape {
    write_bytes: 4096,
    room: 4096,
    direct: false,
    kind: SyncKind::Fsync,
    prior: ErrState::Idle,
    quota: false,
    tool: Tool::Dd,
    bad_call: false,
};

/// 槽 2 `clean-fdatasync`：写 8192、空间 8192、fdatasync 全绿。
/// 与槽 1 同档不同面——两式的文案在失败时才分叉，成功时都必须沉默。
const SHAPE_CLEAN_FDATASYNC: Shape = Shape {
    write_bytes: 8192,
    room: 8192,
    direct: false,
    kind: SyncKind::Fdatasync,
    prior: ErrState::Idle,
    quota: false,
    tool: Tool::Dd,
    bad_call: false,
};

/// 槽 3 `deferred-fsync`：本卡靶心。延迟分配放行 write，回写撞满盘，
/// fsync 领到 ENOSPC；短尾 4096（申报 8192 - 实落 4096）。
/// dd 报 `dd: fsync failed for 'out': No space left on device`，退 1。
const SHAPE_DEFERRED_FSYNC: Shape = Shape {
    write_bytes: 8192,
    room: 4096,
    direct: false,
    kind: SyncKind::Fsync,
    prior: ErrState::Idle,
    quota: false,
    tool: Tool::Dd,
    bad_call: false,
};

/// 槽 4 `deferred-fdatasync`：同款延迟，同步面换成 fdatasync；
/// 短尾 8192（申报 12288 - 实落 4096），文案必须落到 fdatasync failed for。
const SHAPE_DEFERRED_FDATASYNC: Shape = Shape {
    write_bytes: 12288,
    room: 4096,
    direct: false,
    kind: SyncKind::Fdatasync,
    prior: ErrState::Idle,
    quota: false,
    tool: Tool::Dd,
    bad_call: false,
};

/// 槽 5 `direct-enospc`：O_DIRECT 让配给闸即时判死——错误回到写面，
/// 这是 F11681 总锚的地盘；本卡用它划界：落点必须在调用闸，文案是
/// `dd: error writing`，同步面无从谈起。
const SHAPE_DIRECT_ENOSPC: Shape = Shape {
    write_bytes: 8192,
    room: 4096,
    direct: true,
    kind: SyncKind::Fsync,
    prior: ErrState::Idle,
    quota: false,
    tool: Tool::Dd,
    bad_call: false,
};

/// 槽 6 `edquot-sync`：延迟分配同样押送 EDQUOT——同步面报的是配额不是盘满。
/// 换个目录就能写，换目录救不了盘满；归因错则处置全错。
const SHAPE_EDQUOT_SYNC: Shape = Shape {
    write_bytes: 8192,
    room: 4096,
    direct: false,
    kind: SyncKind::Fsync,
    prior: ErrState::Idle,
    quota: true,
    tool: Tool::Dd,
    bad_call: false,
};

/// 槽 7 `syncrange-swallow`：sync_file_range 只下指令不等回执，dd 甚至不调它
/// 的失败面——短尾 4096 存在，但**没有任何一行**被打印。悬置档正例一。
const SHAPE_SYNCRANGE_SWALLOW: Shape = Shape {
    write_bytes: 8192,
    room: 4096,
    direct: false,
    kind: SyncKind::SyncRange,
    prior: ErrState::Idle,
    quota: false,
    tool: Tool::Raw,
    bad_call: false,
};

/// 槽 8 `consumed-silent`：错误态已被上一次报告消费，本次 fsync 返回成功，
/// 数据却从未落盘。悬置档正例二——"错误丢失"的最隐蔽形态。
const SHAPE_CONSUMED_SILENT: Shape = Shape {
    write_bytes: 8192,
    room: 4096,
    direct: false,
    kind: SyncKind::Fsync,
    prior: ErrState::Consumed,
    quota: false,
    tool: Tool::Dd,
    bad_call: false,
};

/// 全部 14 槽的输入形态。后 6 槽的事实面**整槽复用**前面的活形态——
/// 一槽只坏一维，坏的是申报而不是事实，这是语料纪律的硬要求。
pub const SHAPES: [Shape; CORPUS] = [
    SHAPE_CLEAN_FSYNC,
    SHAPE_CLEAN_FDATASYNC,
    SHAPE_DEFERRED_FSYNC,
    SHAPE_DEFERRED_FDATASYNC,
    SHAPE_DIRECT_ENOSPC,
    SHAPE_EDQUOT_SYNC,
    SHAPE_SYNCRANGE_SWALLOW,
    SHAPE_CONSUMED_SILENT,
    SHAPE_DEFERRED_FSYNC,
    SHAPE_DIRECT_ENOSPC,
    SHAPE_EDQUOT_SYNC,
    SHAPE_DEFERRED_FDATASYNC,
    SHAPE_DEFERRED_FSYNC,
    SHAPE_CLEAN_FSYNC,
];

/// 14 槽的申报结论（活槽与事实面逐维相等；死槽只翻一维）。
pub const CLAIMS: [Claim; CORPUS] = [
    // 槽 1：fsync 全绿，六维照实。
    Claim::new(true, Stage::Exit, 0, 0, Note::Clean, false),
    // 槽 2：fdatasync 全绿。
    Claim::new(true, Stage::Exit, 0, 0, Note::Clean, false),
    // 槽 3：延迟报出，落回执闸，短尾 4096，dd 同步式。
    Claim::new(false, Stage::Report, FS_DEFER, 4096, Note::DdFsync, false),
    // 槽 4：延迟报出，短尾 8192，fdatasync 式。
    Claim::new(false, Stage::Report, FS_DEFER, 8192, Note::DdFdatasync, false),
    // 槽 5：写面即时拒，落调用闸，短尾 4096，写面式。
    Claim::new(false, Stage::Call, FS_KIND, 4096, Note::WriteErr, false),
    // 槽 6：配额延迟交付，归因码 FS_CODE，文案落 Quota。
    Claim::new(false, Stage::Report, FS_CODE, 4096, Note::Quota, false),
    // 槽 7：sync_file_range 无人领取，短尾 4096，沉默。
    Claim::new(false, Stage::Exit, FS_SWAL, 4096, Note::Silent, false),
    // 槽 8：错误态已消费，本次回执静默，短尾 4096。
    Claim::new(false, Stage::Exit, FS_CLEAR, 4096, Note::Silent, false),
    // 死 1 estate-swap：事实是延迟报出的败局，申报却报同步成功（谎报全绿）。
    Claim::new(true, Stage::Report, FS_DEFER, 4096, Note::DdFsync, false),
    // 死 2 stage-skip：写面即时拒，申报却说错误在同步面浮出。
    Claim::new(false, Stage::Report, FS_KIND, 4096, Note::WriteErr, false),
    // 死 3 code-swap：配额耗尽记成空间不足。
    Claim::new(false, Stage::Report, FS_DEFER, 4096, Note::Quota, false),
    // 死 4 value-off：短尾 8192 虚报成 4096。
    Claim::new(false, Stage::Report, FS_DEFER, 4096, Note::DdFdatasync, false),
    // 死 5 note-swap：dd 同步式认成写面式。
    Claim::new(false, Stage::Report, FS_DEFER, 4096, Note::WriteErr, false),
    // 死 6 default-bucket：本可判定的全绿落盘被丢进兜底档。
    Claim::new(true, Stage::Exit, 0, 0, Note::Clean, true),
];

/// 14 槽的标签（后 6 槽一槽只坏一维，依次挂六维）。
pub const WANT_TAGS: [&str; CORPUS] = [
    "clean-fsync",
    "clean-fdatasync",
    "deferred-fsync",
    "deferred-fdatasync",
    "direct-enospc",
    "edquot-sync",
    "syncrange-swallow",
    "consumed-silent",
    "estate-swap",
    "stage-skip",
    "code-swap",
    "value-off",
    "note-swap",
    "default-bucket",
];

/// 期望表的第二形态：落点阶段（与 CLAIMS 交叉校验，任一处笔误都要露出来）。
pub const WANT_STAGE: [Stage; CORPUS] = [
    Stage::Exit,
    Stage::Exit,
    Stage::Report,
    Stage::Report,
    Stage::Call,
    Stage::Report,
    Stage::Exit,
    Stage::Exit,
    Stage::Report,
    Stage::Report,
    Stage::Report,
    Stage::Report,
    Stage::Report,
    Stage::Exit,
];

/// 期望表的第二形态：归因码。
pub const WANT_CODE: [u32; CORPUS] = [
    0,
    0,
    FS_DEFER,
    FS_DEFER,
    FS_KIND,
    FS_CODE,
    FS_SWAL,
    FS_CLEAR,
    FS_DEFER,
    FS_KIND,
    FS_DEFER,
    FS_DEFER,
    FS_DEFER,
    0,
];

/// 期望表的第三形态：期望命中的判定维（0 = Ok，1..=6 = Fault 秩）。
pub const WANT_NDX: [u8; CORPUS] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 4, 5, 6];

/// 期望表的第二形态：文案档位。
pub const WANT_NOTE: [Note; CORPUS] = [
    Note::Clean,
    Note::Clean,
    Note::DdFsync,
    Note::DdFdatasync,
    Note::WriteErr,
    Note::Quota,
    Note::Silent,
    Note::Silent,
    Note::DdFsync,
    Note::WriteErr,
    Note::Quota,
    Note::DdFdatasync,
    Note::WriteErr,
    Note::Clean,
];

/// 期望表的第二形态：兜底档。
pub const WANT_DEFAULT: [bool; CORPUS] = [
    false, false, false, false, false, false, false, false, false, false, false, false, false,
    true,
];

/// 期望表的第二形态：申报成败位（等价于 exit==0）。
pub const WANT_OK: [bool; CORPUS] = [
    true, true, false, false, false, false, false, false, true, false, false, false, false, true,
];

/// 期望表的第二形态：申报的短尾字节数。
pub const WANT_VALUE: [u32; CORPUS] = [
    0, 0, 4096, 8192, 4096, 4096, 4096, 4096, 4096, 4096, 4096, 4096, 4096, 0,
];

/// 事实面第二形态（与 `define` 独立列出，用来同时复核 define 与 CLAIMS）：落点阶段。
pub const ACT_STAGE: [Stage; CORPUS] = [
    Stage::Exit,
    Stage::Exit,
    Stage::Report,
    Stage::Report,
    Stage::Call,
    Stage::Report,
    Stage::Exit,
    Stage::Exit,
    Stage::Report,
    Stage::Call,
    Stage::Report,
    Stage::Report,
    Stage::Report,
    Stage::Exit,
];

/// 事实面第二形态：归因码。
pub const ACT_CODE: [u32; CORPUS] = [
    0,
    0,
    FS_DEFER,
    FS_DEFER,
    FS_KIND,
    FS_CODE,
    FS_SWAL,
    FS_CLEAR,
    FS_DEFER,
    FS_KIND,
    FS_CODE,
    FS_DEFER,
    FS_DEFER,
    0,
];

/// 事实面第二形态：短尾字节数。
pub const ACT_VALUE: [u32; CORPUS] = [
    0, 0, 4096, 8192, 4096, 4096, 4096, 4096, 4096, 4096, 4096, 8192, 4096, 0,
];

/// 事实面第二形态：文案档位。
pub const ACT_NOTE: [Note; CORPUS] = [
    Note::Clean,
    Note::Clean,
    Note::DdFsync,
    Note::DdFdatasync,
    Note::WriteErr,
    Note::Quota,
    Note::Silent,
    Note::Silent,
    Note::DdFsync,
    Note::WriteErr,
    Note::Quota,
    Note::DdFdatasync,
    Note::DdFsync,
    Note::Clean,
];

/// 事实面第二形态：兜底档（14 槽全部可判，没有一格该走垃圾桶）。
pub const ACT_DEFAULT: [bool; CORPUS] = [
    false, false, false, false, false, false, false, false, false, false, false, false, false,
    false,
];

/// 事实面第二形态：成败位。
pub const ACT_OK: [bool; CORPUS] = [
    true, true, false, false, false, false, false, false, false, false, false, false, false, true,
];

/// 事实面第二形态：实落字节（min(申报, 空间)；调用闸拒则 0）。
pub const ACT_LANDED: [u32; CORPUS] = [
    4096, 8192, 4096, 4096, 4096, 4096, 4096, 4096, 4096, 4096, 4096, 4096, 4096, 4096,
];

/// 死槽 → 它所复用的活槽（语料纪律：后 6 槽的事实面整槽照抄前 8 槽的某一行）。
pub const DEAD_FROM: [usize; REJECT] = [2usize, 4usize, 5usize, 3usize, 2usize, 0usize];
// ---------------------------------------------------------------------------
// 闸门纵线
// ---------------------------------------------------------------------------

/// 配给闸是否会把错误挡在写面：O_DIRECT 是唯一让 ENOSPC 即时返回的标准形态。
pub fn rejects_at_write_face(sh: &Shape) -> bool {
    !sh.bad_call && sh.direct && sh.shortfall() > 0
}

/// 延迟通道是否成立：非即时、空间不够、且错误态还没被消费。
pub fn defers_to_sync_face(sh: &Shape) -> bool {
    !sh.bad_call && !rejects_at_write_face(sh) && sh.shortfall() > 0
        && sh.prior == ErrState::Idle
}

/// 回执闸是否有人领取：只有"等待型"同步面（fsync/fdatasync）才有回执。
pub fn has_receipt(sh: &Shape) -> bool {
    sh.kind.awaits()
}

/// 错误悬置判定：短尾存在、错误本应存在，但没有任何面领取——
/// 要么同步面不等待（sync_file_range），要么错误态已被上一次消费。
pub fn error_stranded(sh: &Shape) -> bool {
    if sh.bad_call || rejects_at_write_face(sh) {
        return false;
    }
    if sh.shortfall() == 0 && !sh.quota {
        return false;
    }
    sh.prior == ErrState::Consumed || !has_receipt(sh)
}

/// 闸门纵线：从形态推出应然结论。次序即九道闸的次序，前一闸不产出事实后一闸无输入。
pub fn define(sh: &Shape) -> Outcome {
    // 闸 1 调用：坏 fd / 非法参数，一字节未进缓存，落调用闸。
    if sh.bad_call {
        return Outcome::new(Stage::Call, FS_EXIT, 0, Note::Usage, sh.tool == Tool::Unknown);
    }
    // 闸 2/3 缓存与配给：即时拒的形态停在调用闸（错误与调用同帧）。
    if rejects_at_write_face(sh) {
        let note = match sh.tool {
            Tool::Dd | Tool::Cp => Note::WriteErr,
            _ => Note::Silent,
        };
        let default = sh.tool == Tool::Unknown || !errno_legible(if sh.quota { EDQUOT } else { ENOSPC });
        return Outcome {
            stage: Stage::Call,
            code: FS_KIND,
            value: sh.shortfall(),
            note,
            default,
        };
    }
    // 闸 4~7：短尾、押送、回执、消费。
    if error_stranded(sh) {
        let code = if sh.prior == ErrState::Consumed { FS_CLEAR } else { FS_SWAL };
        return Outcome {
            stage: Stage::Exit,
            code,
            value: sh.shortfall(),
            note: Note::Silent,
            default: sh.tool == Tool::Unknown,
        };
    }
    if defers_to_sync_face(sh) && has_receipt(sh) {
        // 同步面领取回执：延迟报出。配额与盘满在文案面上分家。
        let code = if sh.quota { FS_CODE } else { FS_DEFER };
        let note = if sh.quota {
            Note::Quota
        } else {
            match sh.kind {
                SyncKind::Fsync => Note::DdFsync,
                SyncKind::Fdatasync => Note::DdFdatasync,
                _ => Note::Silent,
            }
        };
        let default = !sh.tool.is_legible() && sh.tool != Tool::Raw;
        return Outcome {
            stage: Stage::Report,
            code,
            value: sh.shortfall(),
            note,
            default,
        };
    }
    // 闸 8：两面全绿，纵线跑完，结算落 Exit。
    Outcome {
        stage: Stage::Exit,
        code: 0,
        value: 0,
        note: Note::Clean,
        default: sh.tool == Tool::Unknown,
    }
}

/// 退出码结算（闸 8）：2 = 用法错；1 = 任一面报出错误；0 = 全绿或错误悬置
/// （悬置时工具**自认为**成功——这正是 Stranded 档最贵的一格：退 0 不等于落盘）。
pub fn exit_code(sh: &Shape) -> u32 {
    if sh.bad_call {
        return EXIT_USAGE;
    }
    let a = define(sh);
    if a.code == 0 || a.code == FS_SWAL || a.code == FS_CLEAR {
        return EXIT_OK;
    }
    EXIT_FAIL
}

/// 落盘字节（不可逆事实）：调用闸拒 → 0；否则 min(申报, 空间)。
pub fn landed_bytes(sh: &Shape) -> u32 {
    if sh.bad_call {
        return 0;
    }
    sh.landed()
}

/// 短尾与退出码的闭合关系：短尾存在却退 0（写面谎报落盘）——本卡判得最死的一格。
pub fn tail_lies(sh: &Shape) -> bool {
    sh.shortfall() > 0 && exit_code(sh) == EXIT_OK
}

/// 一槽的申报退出码（供 Estate 维与渲染共用）。
pub fn claim_exit(c: &Claim) -> u32 {
    if c.default {
        return EXIT_USAGE;
    }
    c.exit_hint()
}

/// 活槽的成败位（前 8 槽）。
pub const LIVE_OK: [bool; ACCEPT] = [true, true, false, false, false, false, false, false];

// ---------------------------------------------------------------------------
// 文案：逐字复刻真实报错
// ---------------------------------------------------------------------------

/// 输出文件在本卡语料里的字面名（GNU 会给它套单引号）。
pub const OPERAND: &str = "out";

/// GNU dd 的 conv=fsync 同步面失败式：
/// `dd: fsync failed for 'out': No space left on device`
pub fn msg_dd_fsync(op: &str) -> String {
    let mut s = String::new();
    s.push_str("dd: fsync failed for '");
    s.push_str(op);
    s.push_str("': ");
    s.push_str(ANCHOR_ERRNO_TEXT);
    s
}

/// GNU dd 的 conv=fdatasync 同步面失败式：
/// `dd: fdatasync failed for 'out': No space left on device`
pub fn msg_dd_fdatasync(op: &str) -> String {
    let mut s = String::new();
    s.push_str("dd: fdatasync failed for '");
    s.push_str(op);
    s.push_str("': ");
    s.push_str(ANCHOR_ERRNO_TEXT);
    s
}

/// 写面失败式（dd 口径）：`dd: error writing 'out': No space left on device`
pub fn msg_dd_write(op: &str) -> String {
    let mut s = String::new();
    s.push_str("dd: error writing '");
    s.push_str(op);
    s.push_str("': ");
    s.push_str(ANCHOR_ERRNO_TEXT);
    s
}

/// 写面失败式（cp 口径）：`cp: error writing 'out': No space left on device`
pub fn msg_cp_write(op: &str) -> String {
    let mut s = String::new();
    s.push_str("cp: error writing '");
    s.push_str(op);
    s.push_str("': ");
    s.push_str(ANCHOR_ERRNO_TEXT);
    s
}

/// 同步面报配额：`dd: fsync failed for 'out': Disk quota exceeded`
pub fn msg_dd_quota(op: &str) -> String {
    let mut s = String::new();
    s.push_str("dd: fsync failed for '");
    s.push_str(op);
    s.push_str("': ");
    s.push_str(errno_name(EDQUOT));
    s
}

/// 裸 syscall 面：`write: No space left on device`（无引号、无操作数名）。
pub fn msg_raw_write() -> String {
    let mut s = String::new();
    s.push_str("write: ");
    s.push_str(ANCHOR_ERRNO_TEXT);
    s
}

/// 用法错：`dd: invalid option -- 'z'`
pub fn msg_usage_opt(c: char) -> String {
    let mut s = String::new();
    s.push_str("dd: invalid option -- '");
    s.push(c);
    s.push('\'');
    s
}

/// 用法错的提示行：`Try 'dd --help' for more information.`
pub fn msg_usage_hint() -> String {
    let mut s = String::new();
    s.push_str("Try 'dd --help' for more information.");
    s
}

/// 卡面原文形态（默认操作数下的同步式，判据名与测试都盯这一串）。
pub fn msg_dd_fsync_default() -> String {
    msg_dd_fsync(OPERAND)
}

/// 卡面原文形态：fdatasync 式。
pub fn msg_dd_fdatasync_default() -> String {
    msg_dd_fdatasync(OPERAND)
}

/// 按文案档位给出该档的规范整行（Clean/Silent 档是空串——没行就是没行）。
pub fn msg_note(n: Note) -> String {
    match n {
        Note::Clean => String::new(),
        Note::Silent => String::new(),
        Note::DdFsync => msg_dd_fsync_default(),
        Note::DdFdatasync => msg_dd_fdatasync_default(),
        Note::WriteErr => msg_dd_write(OPERAND),
        Note::Quota => msg_dd_quota(OPERAND),
        Note::Usage => msg_usage_opt('z'),
    }
}

/// 按形态给出这一槽实际会打出的那一行（比 msg_note 更贴事实：它区分工具面）。
pub fn msg_for(sh: &Shape) -> String {
    if sh.bad_call {
        return msg_usage_opt('z');
    }
    let a = define(sh);
    match a.note {
        Note::Clean | Note::Silent => String::new(),
        Note::Usage => msg_usage_opt('z'),
        Note::Quota => msg_dd_quota(OPERAND),
        Note::DdFsync => msg_dd_fsync(OPERAND),
        Note::DdFdatasync => msg_dd_fdatasync(OPERAND),
        Note::WriteErr => match sh.tool {
            Tool::Cp => msg_cp_write(OPERAND),
            Tool::Raw => msg_raw_write(),
            _ => msg_dd_write(OPERAND),
        },
    }
}

/// 文案是否含 GNU 特有的单引号包围（裸 syscall 面没有，这是句式的最小可分特征）。
pub fn has_quoted_operand(s: &str) -> bool {
    s.contains("'")
}

/// 文案是否报在同步面（fsync/fdatasync failed for 两式的共同前缀）。
pub fn is_sync_face_line(s: &str) -> bool {
    s.contains("fsync failed for") || s.contains("fdatasync failed for")
}

/// 文案是否报在写面（error writing 一式）。
pub fn is_write_face_line(s: &str) -> bool {
    s.contains("error writing")
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
    FS_STATE,
    FS_STAGE,
    FS_CODE,
    FS_VALUE,
    FS_NOTE,
    FS_FAULT_DEFAULT,
];

/// 账本的 8 个正数格：前 7 格是统计数，第 8 格是红档率 929。
///
/// 七个统计数逐个字面取，含义写死在 `stat_meaning()` 里，账本与期望不许各自漂移。
pub const VALS: [u32; ACCEPT] = [
    SYNC_FACE_COUNT,
    STAGE_COUNT,
    CODE_COUNT as u32,
    EXIT_RANKS,
    CORPUS as u32,
    TIER_COUNT as u32,
    DEFECT_CELLS as u32,
    RED_RATE,
];

/// 统计格的字面期望（第二形态，交叉校验 VALS 的笔误）。
pub const VALS_CHECK: [u32; ACCEPT] = [4, 9, 13, 3, 14, 4, 6, 929];

/// 第 8 格之外的统计格数。
pub const STAT_CELLS: usize = 7usize;

/// 缺陷码格数：账本后 6 格，一格挂一维（Estate/Stage/Code/Value/Note/Default）。
/// 与 `REJECT` 同宽但语义独立——语料的死槽数与账本的缺陷格数若哪天分叉，
/// 这一码会把分叉显性化，而不是让 `build_ledger` 悄悄按另一个数铺格。
pub const DEFECT_CELLS: usize = 6usize;

/// 统计格语义（本卡的身份数，一格一句话）。
pub fn stat_meaning(i: usize) -> &'static str {
    match i {
        0 => "四个同步面：fsync/fdatasync/sync_range/无",
        1 => "九道闸：含封印闸的纵线宽度",
        2 => "十三个错误码：SEG_BASE+0x01..0x0D",
        3 => "三个退出码位：0/1/2",
        4 => "十四个语料槽：8 活 + 6 死",
        5 => "四个档位：全绿/延迟/即时/悬置",
        6 => "六个缺陷维：一槽只坏一维",
        7 => "红档率 929：14 格中 13 的千分率",
        _ => "越界",
    }
}

/// 判定次序固定：Estate → Stage → Code → Value → Note → Default → Ok。
///
/// Estate 的定义是 `actual.ok() != claim.success()`——成败互报优先于阶段与错误码，
/// 因为"数据没落盘却报全绿"是本卡唯一能把失败读成成功的谎法，别的维错只影响归因。
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

/// 判定注入（`run_with` 的内部出口）：越界槽返回 Err(FS_SLOT)，被点中的槽用注入维，
/// 其余槽照常判。
fn fault_at(c: &Case, i: usize, over: Option<(usize, Fault)>) -> Result<Fault, u32> {
    if i >= CORPUS {
        return Err(FS_SLOT);
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
                Row::of(Tier::Clean),
                Row::of(Tier::Deferred),
                Row::of(Tier::Direct),
                Row::of(Tier::Stranded),
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
    /// 全绿档槽数。
    pub clean: usize,
    /// 延迟报出档槽数。
    pub deferred: usize,
    /// 写面即时档槽数。
    pub direct: usize,
    /// 错误悬置档槽数。
    pub stranded: usize,
    /// 命中期望结论的槽数。
    pub matched: usize,
}

impl Report {
    /// 全绿运行的逐字渲染。四档分布 clean=2 deferred=8 direct=2 stranded=2
    /// 由语料表逐槽数出，rate=1000 由全命中推出——一字节之差即判渲染路红。
    pub const RENDER_OK: &'static str = "fs1s total=14 accept=8 reject=6 clean=3 deferred=7 direct=2 stranded=2 matched=14 rate=1000 verdict=green";

    /// 千分率：`(matched*1000 + total/2) / total`。14 格中 13 → 929（红档率标定）。
    pub fn rate(&self) -> u32 {
        if self.total == 0 {
            return 0;
        }
        let num =
            (self.matched as u32).saturating_mul(1000).saturating_add((self.total / 2) as u32);
        num / (self.total as u32)
    }

    /// 四档求和。
    pub fn tier_sum(&self) -> usize {
        self.clean
            .saturating_add(self.deferred)
            .saturating_add(self.direct)
            .saturating_add(self.stranded)
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
            return Err(FS_SLOT);
        }
        if self.accept.saturating_add(self.reject) != self.total {
            return Err(FS_UNIQ);
        }
        if self.tier_sum() != self.total {
            return Err(FS_VALUE);
        }
        if self.matched != self.total {
            return Err(FS_STATE);
        }
        if self.rate() != RATE_FULL {
            return Err(FS_EXIT);
        }
        Ok(())
    }

    /// 逐字渲染（RENDER_OK 钉的就是这一串）。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str("fs1s");
        sep(&mut s);
        kv(&mut s, "total", self.total as u32);
        sep(&mut s);
        kv(&mut s, "accept", self.accept as u32);
        sep(&mut s);
        kv(&mut s, "reject", self.reject as u32);
        sep(&mut s);
        kv(&mut s, "clean", self.clean as u32);
        sep(&mut s);
        kv(&mut s, "deferred", self.deferred as u32);
        sep(&mut s);
        kv(&mut s, "direct", self.direct as u32);
        sep(&mut s);
        kv(&mut s, "stranded", self.stranded as u32);
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

/// 全绿运行的逐字渲染（模块级别名，判据 11 直接钉这一串）。
pub const RENDER_OK: &str = Report::RENDER_OK;

/// 跑一遍全语料。
pub fn run() -> Result<Report, u32> {
    run_with(None)
}

/// 带注入的运行：`over` 点中某槽时强行改判其结论，槽号越界 → `FS_SLOT`。
pub fn run_with(over: Option<(usize, Fault)>) -> Result<Report, u32> {
    if let Some((slot, _)) = over {
        if slot >= CORPUS {
            return Err(FS_SLOT);
        }
    }
    let mut total: usize = 0;
    let mut matched: usize = 0;
    let mut clean: usize = 0;
    let mut deferred: usize = 0;
    let mut direct: usize = 0;
    let mut stranded: usize = 0;
    let mut i: usize = 0;
    while i < CORPUS {
        let c = match case_at(i) {
            Some(v) => v,
            None => return Err(FS_SLOT),
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
            Tier::Clean => clean = clean.saturating_add(1),
            Tier::Deferred => deferred = deferred.saturating_add(1),
            Tier::Direct => direct = direct.saturating_add(1),
            Tier::Stranded => stranded = stranded.saturating_add(1),
        }
        i += 1;
    }
    Ok(Report {
        total,
        accept: ACCEPT,
        reject: REJECT,
        clean,
        deferred,
        direct,
        stranded,
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

/// 阶段闸门。规则：只允许前进到**下一道**闸；倒退 → `FS_STAGE`，跳档 → `FS_KIND`
/// （面序跳档与同步面/写面切分错档同罪），未走完纵线就封印 → `FS_STAGE`，
/// 封印之后一切动作 → `FS_STATE`。被拒的动作同样改指纹——"拒动也要留痕"。
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
            return Err(FS_STATE);
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
            return Err(FS_STAGE);
        }
        Err(FS_KIND)
    }

    /// 封印：必须已经站到最后一道闸（Closed，秩 9）。
    pub fn close(&mut self) -> Result<(), u32> {
        self.moves = self.moves.saturating_add(1);
        self.fp = fold_name(self.fp, b"close");
        if self.closed {
            self.rejected = self.rejected.saturating_add(1);
            return Err(FS_STATE);
        }
        if self.rank != Stage::Closed.rank() {
            self.rejected = self.rejected.saturating_add(1);
            return Err(FS_STAGE);
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
                None => return Err(FS_SLOT),
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
/// 篡改（自相矛盾的一格）拒入账并归 `FS_UNIQ`；封印与冻结各自留指纹。
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
            return Err(FS_STATE);
        }
        if (s.slot as usize) != self.len {
            self.rejects = self.rejects.saturating_add(1);
            return Err(FS_SLOT);
        }
        if !s.self_consistent() {
            self.rejects = self.rejects.saturating_add(1);
            return Err(FS_UNIQ);
        }
        if self.len >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(FS_SLOT);
        }
        match self.items.get_mut(self.len) {
            Some(slot) => *slot = s,
            None => {
                self.rejects = self.rejects.saturating_add(1);
                return Err(FS_SLOT);
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
            return Err(FS_SLOT);
        }
        let mut i: usize = 0;
        while i < ACCEPT {
            let s = match self.sample(i) {
                Some(v) => v,
                None => return Err(FS_SLOT),
            };
            let want_v = match VALS.get(i) {
                Some(v) => *v,
                None => return Err(FS_SLOT),
            };
            if s.slot as usize != i {
                return Err(FS_UNIQ);
            }
            if i < STAT_CELLS {
                if s.op != OP_STAT || s.code != 0 || !s.ok || s.value != want_v {
                    return Err(FS_VALUE);
                }
            } else if s.op != OP_RATE || s.code != 0 || s.value != RED_RATE {
                return Err(FS_VALUE);
            }
            i += 1;
        }
        let mut j: usize = 0;
        while j < REJECT {
            let idx = ACCEPT.saturating_add(j);
            let s = match self.sample(idx) {
                Some(v) => v,
                None => return Err(FS_SLOT),
            };
            let want_c = match REJECT_CODES.get(j) {
                Some(v) => *v,
                None => return Err(FS_SLOT),
            };
            if s.op != OP_DEFECT || s.slot as usize != idx || s.code != want_c {
                return Err(FS_UNIQ);
            }
            if s.value != 0 || s.ok {
                return Err(FS_VALUE);
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
        fold_str(h, "fsync-delayed-alloc")
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
            return Err(FS_STATE);
        }
        let mut rebuilt = Ledger::new();
        let mut i: usize = 0;
        while i < self.len {
            if let Some(s) = self.sample(i) {
                if rebuilt.register(s).is_err() {
                    return Err(FS_UNIQ);
                }
            }
            i += 1;
        }
        if rebuilt.len != self.len {
            return Err(FS_SLOT);
        }
        if rebuilt.fingerprint() != self.root {
            return Err(FS_UNIQ);
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

/// 造一格错值的账（只给自检与测试用：第 4 统计格塞进 999，audit 必须抓到）。
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

/// 造一格身份错的账：缺陷码被换成不在 REJECT_CODES 里的 FS_DEFER。
pub fn broken_code_ledger() -> Ledger {
    let mut l = build_ledger();
    let mut j: usize = 0;
    while j < REJECT {
        if j == 2 {
            let idx = ACCEPT.saturating_add(j);
            match l.items.get_mut(idx) {
                Some(s) => s.code = FS_DEFER,
                None => {}
            }
        }
        j += 1;
    }
    l
}

/// 造一格被篡改的账（defect 格值非 0，audit 必须以 FS_VALUE 判死）。
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
    if g.step(Stage::Call).is_ok() {
        spine_ok = false;
    }
    if g.fingerprint() == before {
        spine_ok = false;
    }
    [table, judged, rendered, ledger_route && spine_ok]
}

/// 四路的短名（自检明细与测试共用）。
pub const INJECT_ROUTES: [&str; 4] = ["table", "judge", "render", "ledger"];

/// 对照形态（**不入语料**）：O_DIRECT 上 cp 的即时拒——它钉住本卡与总锚卡的
/// 边界：同样的字节几何，direct 位置一翻，落点从 Report 面跳回 Call 面，
/// 文案从 `fsync failed for` 跳回 `error writing`，归因卡从本卡跳回 F11681。
pub const SHAPE_DIRECT_CP: Shape = Shape {
    write_bytes: 16384,
    room: 4096,
    direct: true,
    kind: SyncKind::None,
    prior: ErrState::Idle,
    quota: false,
    tool: Tool::Cp,
    bad_call: false,
};

/// 期望退出码表（第二形态；与 `exit_code` 独立列出，交叉校验）。
pub const WANT_EXIT: [u32; CORPUS] = [
    0, 0, 1, 1, 1, 1, 0, 0, 1, 1, 1, 1, 1, 0,
];

/// 期望档位表（第二形态；与 `Tier::of` 独立列出，交叉校验）。
pub const WANT_TIER: [Tier; CORPUS] = [
    Tier::Clean,
    Tier::Clean,
    Tier::Deferred,
    Tier::Deferred,
    Tier::Direct,
    Tier::Deferred,
    Tier::Stranded,
    Tier::Stranded,
    Tier::Deferred,
    Tier::Direct,
    Tier::Deferred,
    Tier::Deferred,
    Tier::Deferred,
    Tier::Clean,
];

/// 期望归因码的语义名（第二形态；与 `code_name` 独立列出）。
pub const WANT_CODE_NAME: [&str; CORPUS] = [
    "clean",
    "clean",
    "fs-defer",
    "fs-defer",
    "fs-kind",
    "fs-code",
    "fs-swal",
    "fs-clear",
    "fs-defer",
    "fs-kind",
    "fs-code",
    "fs-defer",
    "fs-defer",
    "clean",
];

/// 相邻卡的锚位是否闭合（本卡自声明位距，不复算对方段位就等于没署名）。
pub fn anchors_agree() -> bool {
    if SEG_BASE.saturating_sub(ANCHOR_F11681_GAP) != ANCHOR_F11681_SEG {
        return false;
    }
    if SEG_BASE.saturating_sub(ANCHOR_F11684_GAP) != ANCHOR_F11684_SEG {
        return false;
    }
    if SEG_BASE.saturating_sub(ANCHOR_F11689_GAP) != ANCHOR_F11689_SEG {
        return false;
    }
    if ANCHOR_F11681_SEG >= ANCHOR_F11684_SEG {
        return false;
    }
    if ANCHOR_F11684_SEG >= ANCHOR_F11689_SEG {
        return false;
    }
    if ANCHOR_F11689_SEG >= SEG_BASE {
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
// ---------------------------------------------------------------------------
// 判据谓词（十三道，私有；断言方向：cond 为违约条件，末行为成立条件）
// ---------------------------------------------------------------------------

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

/// 判据 01：模块身份、段位、锚位与规模常量。
fn name_ok() -> bool {
    if SEG_BASE != 0x6d390 {
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
    if code_name(FS_SLOT) != "fs-slot" || code_name(0) != "clean" {
        return false;
    }
    if code_name(SEG_BASE.saturating_add(0x3f)) != "out-of-segment" {
        return false;
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
    if Stage::Closed.next().is_some() || Stage::Call.prev().is_some() {
        return false;
    }
    if Stage::Report.next() != Some(Stage::Consume) {
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

    if SyncKind::ALL.len() != SYNC_FACE_COUNT as usize {
        return false;
    }
    let mut sk_ranks: [u32; 4] = [0; 4];
    let mut sk_slugs: [&str; 4] = ["a", "a", "a", "a"];
    let mut p: usize = 0;
    while p < SyncKind::ALL.len() {
        let sk = match SyncKind::ALL.get(p) {
            Some(v) => *v,
            None => return false,
        };
        match sk_ranks.get_mut(p) {
            Some(s) => *s = sk.rank(),
            None => return false,
        }
        match sk_slugs.get_mut(p) {
            Some(s) => *s = sk.slug(),
            None => return false,
        }
        if SyncKind::from_rank(sk.rank()) != Some(sk) {
            return false;
        }
        p += 1;
    }
    if !ranks_contiguous(&sk_ranks, 1) || !slugs_unique(&sk_slugs) {
        return false;
    }
    if SyncKind::from_rank(0).is_some() || SyncKind::from_rank(5).is_some() {
        return false;
    }
    // 等待面恰好两格（fsync/fdatasync），非等待面恰好两格——这是押送分岔的根。
    let mut awaits_n: u32 = 0;
    let mut q: usize = 0;
    while q < SyncKind::ALL.len() {
        if let Some(sk) = SyncKind::ALL.get(q) {
            if sk.awaits() {
                awaits_n = awaits_n.saturating_add(1);
            }
        }
        q += 1;
    }
    if awaits_n != 2 {
        return false;
    }

    if ErrState::ALL.len() != 3 {
        return false;
    }
    let mut es_ranks: [u32; 3] = [0; 3];
    let mut es_slugs: [&str; 3] = ["a", "a", "a"];
    let mut e: usize = 0;
    while e < ErrState::ALL.len() {
        let es = match ErrState::ALL.get(e) {
            Some(v) => *v,
            None => return false,
        };
        match es_ranks.get_mut(e) {
            Some(s) => *s = es.rank(),
            None => return false,
        }
        match es_slugs.get_mut(e) {
            Some(s) => *s = es.slug(),
            None => return false,
        }
        if ErrState::from_rank(es.rank()) != Some(es) {
            return false;
        }
        e += 1;
    }
    if !ranks_contiguous(&es_ranks, 1) || !slugs_unique(&es_slugs) {
        return false;
    }
    // 可领取的错误态恰好一格（Pending）：Consumed 不可再领是本卡消费闸的全部判据。
    let mut claimable_n: u32 = 0;
    let mut r: usize = 0;
    while r < ErrState::ALL.len() {
        if let Some(es) = ErrState::ALL.get(r) {
            if es.claimable() {
                claimable_n = claimable_n.saturating_add(1);
            }
        }
        r += 1;
    }
    if claimable_n != 1 {
        return false;
    }

    if Tool::ALL.len() != 4 {
        return false;
    }
    let mut tool_ranks: [u32; 4] = [0; 4];
    let mut tool_slugs: [&str; 4] = ["a", "a", "a", "a"];
    let mut t: usize = 0;
    while t < Tool::ALL.len() {
        let tl = match Tool::ALL.get(t) {
            Some(v) => *v,
            None => return false,
        };
        match tool_ranks.get_mut(t) {
            Some(s) => *s = tl.rank(),
            None => return false,
        }
        match tool_slugs.get_mut(t) {
            Some(s) => *s = tl.slug(),
            None => return false,
        }
        if Tool::from_rank(tl.rank()) != Some(tl) {
            return false;
        }
        t += 1;
    }
    if !ranks_contiguous(&tool_ranks, 1) || !slugs_unique(&tool_slugs) {
        return false;
    }
    if !Tool::Dd.can_sync_line() || Tool::Cp.can_sync_line() {
        return false;
    }

    if Note::ALL.len() != 7 {
        return false;
    }
    let mut note_ranks: [u32; 7] = [0; 7];
    let mut note_slugs: [&str; 7] = ["a", "a", "a", "a", "a", "a", "a"];
    let mut n: usize = 0;
    while n < Note::ALL.len() {
        let nt = match Note::ALL.get(n) {
            Some(v) => *v,
            None => return false,
        };
        match note_ranks.get_mut(n) {
            Some(s) => *s = nt.rank(),
            None => return false,
        }
        match note_slugs.get_mut(n) {
            Some(s) => *s = nt.slug(),
            None => return false,
        }
        if Note::from_rank(nt.rank()) != Some(nt) {
            return false;
        }
        n += 1;
    }
    if !ranks_contiguous(&note_ranks, 1) || !slugs_unique(&note_slugs) {
        return false;
    }
    // Silent 不算报出错误；同步面式三格、写面式一格——面归属的枚举闭合。
    if Note::Silent.reports_error() || !Note::DdFsync.reports_error() {
        return false;
    }
    let mut sync_notes: u32 = 0;
    let mut w: usize = 0;
    while w < Note::ALL.len() {
        if let Some(nt) = Note::ALL.get(w) {
            if nt.blames_sync_face() {
                sync_notes = sync_notes.saturating_add(1);
            }
        }
        w += 1;
    }
    if sync_notes != 3 {
        return false;
    }
    if !Note::WriteErr.blames_write_face() || Note::Clean.blames_write_face() {
        return false;
    }

    if Tier::ALL.len() != TIER_COUNT {
        return false;
    }
    let mut tier_ranks: [u32; 4] = [0; 4];
    let mut tier_slugs: [&str; 4] = ["a", "a", "a", "a"];
    let mut y: usize = 0;
    while y < Tier::ALL.len() {
        let tr = match Tier::ALL.get(y) {
            Some(v) => *v,
            None => return false,
        };
        match tier_ranks.get_mut(y) {
            Some(s) => *s = tr.rank(),
            None => return false,
        }
        match tier_slugs.get_mut(y) {
            Some(s) => *s = tr.slug(),
            None => return false,
        }
        if Tier::from_rank(tr.rank()) != Some(tr) {
            return false;
        }
        y += 1;
    }
    if !ranks_contiguous(&tier_ranks, 1) || !slugs_unique(&tier_slugs) {
        return false;
    }
    let mut timing_n: u32 = 0;
    let mut z: usize = 0;
    while z < Tier::ALL.len() {
        if let Some(tr) = Tier::ALL.get(z) {
            if tr.is_timing_family() {
                timing_n = timing_n.saturating_add(1);
            }
        }
        z += 1;
    }
    if timing_n != 2 {
        return false;
    }

    if Fault::ALL.len() != 7 {
        return false;
    }
    if Fault::Ok.code() != 0 || !Fault::Estate.is_fault() {
        return false;
    }
    let mut f_ranks: [u32; 7] = [0; 7];
    let mut f: usize = 0;
    while f < Fault::ALL.len() {
        if let Some(x) = Fault::ALL.get(f) {
            match f_ranks.get_mut(f) {
                Some(s) => *s = x.rank(),
                None => return false,
            }
            if Fault::from_rank(x.rank()) != Some(*x) {
                return false;
            }
        }
        f += 1;
    }
    if !ranks_contiguous(&f_ranks, 0) {
        return false;
    }
    if Fault::from_rank(7).is_some() {
        return false;
    }
    true
}

/// 判据 03：延迟分配几何——短尾、实落、押送条件的算术闭合。
fn delay_ok() -> bool {
    // 短尾 = 申报 - 实落，且实落 = min(申报, 空间)：三个口径必须互相闭合。
    let mut i: usize = 0;
    while i < CORPUS {
        let sh = match shape_at(i) {
            Some(v) => v,
            None => return false,
        };
        let land = match ACT_LANDED.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if sh.bad_call {
            if landed_bytes(&sh) != 0 {
                return false;
            }
        } else {
            if landed_bytes(&sh) != land {
                return false;
            }
            if sh.write_bytes.saturating_sub(land) != sh.shortfall() {
                return false;
            }
        }
        i += 1;
    }
    // O_DIRECT 形态：即时拒成立时错误不进延迟通道；去掉 direct 位则改押送同步面。
    let d = match shape_at(4) {
        Some(v) => v,
        None => return false,
    };
    if !rejects_at_write_face(&d) || defers_to_sync_face(&d) {
        return false;
    }
    let mut d2 = d;
    d2.direct = false;
    if rejects_at_write_face(&d2) || !defers_to_sync_face(&d2) {
        return false;
    }
    if !d.would_defer_without_direct() {
        return false;
    }
    // 空间足够的形态既不即时拒也不押送。
    let c = match shape_at(0) {
        Some(v) => v,
        None => return false,
    };
    if rejects_at_write_face(&c) || defers_to_sync_face(&c) || error_stranded(&c) {
        return false;
    }
    // sync_file_range 无回执：押送条件成立但领取面不存在，必须判悬置。
    let s = match shape_at(6) {
        Some(v) => v,
        None => return false,
    };
    if has_receipt(&s) || !error_stranded(&s) || !defers_to_sync_face(&s) {
        return false;
    }
    // 已消费的错误态不可再领：pending 可领、consumed 不可领。
    let u = match shape_at(7) {
        Some(v) => v,
        None => return false,
    };
    if !error_stranded(&u) || has_receipt(&u) {
        return false;
    }
    if u.prior.claimable() || !ErrState::Pending.claimable() {
        return false;
    }
    if ErrState::Idle.claimable() || ErrState::Consumed.claimable() {
        return false;
    }
    // 配额延迟：押送成立、有回执，但归因走 FS_CODE 一支。
    let q = match shape_at(5) {
        Some(v) => v,
        None => return false,
    };
    if !defers_to_sync_face(&q) || !has_receipt(&q) || error_stranded(&q) {
        return false;
    }
    // 谎报判据：短尾存在而工具退 0 的形态必须且只能出现在 Stranded 两槽。
    let mut liars: u32 = 0;
    let mut j: usize = 0;
    while j < CORPUS {
        let sh = match shape_at(j) {
            Some(v) => v,
            None => return false,
        };
        if tail_lies(&sh) {
            liars = liars.saturating_add(1);
            let a = define(&sh);
            if a.code != FS_SWAL && a.code != FS_CLEAR {
                return false;
            }
        }
        j += 1;
    }
    if liars != 2 {
        return false;
    }
    true
}

/// 判据 04：形态合法域、语料几何与死槽复用纪律。
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
        if SHAPES.len() != CORPUS {
            return false;
        }
        i += 1;
    }
    // 死槽必须整槽复用活槽形态（一槽只坏一维，坏的是申报不是事实）。
    let mut j: usize = 0;
    while j < REJECT {
        let dead = match shape_at(ACCEPT.saturating_add(j)) {
            Some(v) => v,
            None => return false,
        };
        let src = match DEAD_FROM.get(j) {
            Some(k) => match shape_at(*k) {
                Some(v) => v,
                None => return false,
            },
            None => return false,
        };
        if dead != src {
            return false;
        }
        if let Some(k) = DEAD_FROM.get(j) {
            if *k >= ACCEPT {
                return false;
            }
        }
        j += 1;
    }
    if DEAD_FROM.len() != REJECT {
        return false;
    }
    // 活槽的同步面组合：三个可文案面（fsync/fdatasync/sync_range）在活槽里全部登场，
    // 无同步面（None）由对照形态 SHAPE_DIRECT_CP 署出——四格面各有归属。
    let mut kinds_seen: [bool; 4] = [false, false, false, false];
    let mut k: usize = 0;
    while k < ACCEPT {
        let sh = match shape_at(k) {
            Some(v) => v,
            None => return false,
        };
        let rk = sh.kind.rank();
        if rk < 1 || rk > 4 {
            return false;
        }
        if let Some(slot) = kinds_seen.get_mut((rk - 1) as usize) {
            *slot = true;
        }
        k += 1;
    }
    if !kinds_seen[0] || !kinds_seen[1] || !kinds_seen[2] {
        return false;
    }
    if SHAPE_DIRECT_CP.kind != SyncKind::None {
        return false;
    }
    // 申报重建（第二形态）必须逐维等于原表。
    let mut m: usize = 0;
    while m < CORPUS {
        let a = match claim_at(m) {
            Some(v) => v,
            None => return false,
        };
        let b = match claim_of(m) {
            Some(v) => v,
            None => return false,
        };
        if a != b {
            return false;
        }
        m += 1;
    }
    true
}

/// 判据 05：九闸纵线判定——define 的产出与事实表逐维相等。
fn define_ok() -> bool {
    let mut i: usize = 0;
    while i < CORPUS {
        let sh = match shape_at(i) {
            Some(v) => v,
            None => return false,
        };
        let a = define(&sh);
        let want_stage = match ACT_STAGE.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let want_code = match ACT_CODE.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let want_value = match ACT_VALUE.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let want_note = match ACT_NOTE.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let want_default = match ACT_DEFAULT.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let want_ok = match ACT_OK.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if a.stage != want_stage || a.code != want_code || a.value != want_value {
            return false;
        }
        if a.note != want_note || a.default != want_default || a.ok() != want_ok {
            return false;
        }
        if !a.stage.is_landing() {
            return false;
        }
        if a.code == 0 && a.value != 0 {
            return false;
        }
        if a.value != sh.shortfall() && a.code != 0 && !sh.bad_call {
            return false;
        }
        i += 1;
    }
    true
}

/// 判据 06：延迟交付——Deferred 档的每一格必须落在回执闸且短尾非零；
/// Stranded 档必须沉默；两档不许互相冒充。
fn defer_ok() -> bool {
    let mut i: usize = 0;
    while i < CORPUS {
        let sh = match shape_at(i) {
            Some(v) => v,
            None => return false,
        };
        let a = define(&sh);
        let tier = Tier::of(&a);
        // 延迟报出：阶段必是 Report，短尾必大于 0，文案必报在同步面或配额面。
        if tier == Tier::Deferred {
            if a.stage != Stage::Report || a.value == 0 {
                return false;
            }
            if !a.note.blames_sync_face() {
                return false;
            }
        }
        // 悬置：一行都不打（Silent），且纵线绝不停在回执闸——没人领取就没有回执。
        if tier == Tier::Stranded {
            if a.note != Note::Silent || a.stage == Stage::Report {
                return false;
            }
        }
        // 即时拒：落调用闸，文案在写面或干脆沉默（裸面）。
        if tier == Tier::Direct && !sh.bad_call {
            if a.stage != Stage::Call {
                return false;
            }
            if !(a.note.blames_write_face() || a.note == Note::Silent) {
                return false;
            }
        }
        // 全绿档不能有归因码，也不能有短尾。
        if tier == Tier::Clean && (a.code != 0 || a.value != 0) {
            return false;
        }
        i += 1;
    }
    // 押送的形态（短尾+等待面+未消费）恰有四格；领取失败的恰有两格。
    let mut deferred_n: u32 = 0;
    let mut stranded_n: u32 = 0;
    let mut j: usize = 0;
    while j < CORPUS {
        let sh = match shape_at(j) {
            Some(v) => v,
            None => return false,
        };
        let t = Tier::of(&define(&sh));
        if t == Tier::Deferred {
            deferred_n = deferred_n.saturating_add(1);
        }
        if t == Tier::Stranded {
            stranded_n = stranded_n.saturating_add(1);
        }
        j += 1;
    }
    if deferred_n != 7 || stranded_n != 2 {
        return false;
    }
    true
}

/// 判据 07：同步面/写面切分与阶段闸门纵线（Gate 单调不倒退）。
fn kind_ok() -> bool {
    // 面切分：同一字节几何下，direct 位是唯一改变错误交付面的开关。
    let a = match shape_at(2) {
        Some(v) => v,
        None => return false,
    };
    let b = match shape_at(4) {
        Some(v) => v,
        None => return false,
    };
    let mut b_flip = b;
    b_flip.direct = false;
    if define(&a).stage != Stage::Report || define(&b).stage != Stage::Call {
        return false;
    }
    if define(&b_flip).stage != Stage::Report {
        return false;
    }
    if define(&b).value != define(&b_flip).value {
        return false;
    }
    // 闸门：正向走满九道并封印。
    let mut g = Gate::new();
    if g.rank() != 0 || g.stage().is_some() {
        return false;
    }
    let mut k: usize = 0;
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
    if g.rank() != STAGE_COUNT || g.close().is_err() || !g.is_closed() {
        return false;
    }
    // 封印后一切动作拒且留痕。
    let before = g.fingerprint();
    if g.step(Stage::Call).is_ok() || g.close().is_ok() {
        return false;
    }
    if g.fingerprint() == before || g.rejected() != 2 {
        return false;
    }
    // 倒退 → FS_STAGE；跳档 → FS_KIND；提前封印 → FS_STAGE。
    let mut h = Gate::new();
    let mut m: usize = 0;
    while m < 5 {
        if let Some(st) = Stage::ALL.get(m) {
            if h.step(*st).is_err() {
                return false;
            }
        }
        m += 1;
    }
    if h.step(Stage::Call) != Err(FS_STAGE) {
        return false;
    }
    if h.step(Stage::Closed) != Err(FS_KIND) {
        return false;
    }
    if h.close() != Err(FS_STAGE) {
        return false;
    }
    // 未起跑直接封印也判死。
    let mut n = Gate::new();
    if n.close() != Err(FS_STAGE) {
        return false;
    }
    true
}
/// 判据 08：文案逐字——同步面两式、写面一式、配额一式、用法一式，全部钉死。
fn message_ok() -> bool {
    // 同步式的共同前缀与操作数引号。
    let l1 = msg_dd_fsync("out");
    if l1.as_str() != "dd: fsync failed for 'out': No space left on device" {
        return false;
    }
    let l2 = msg_dd_fdatasync("out");
    if l2.as_str() != "dd: fdatasync failed for 'out': No space left on device" {
        return false;
    }
    let l3 = msg_dd_write("out");
    if l3.as_str() != "dd: error writing 'out': No space left on device" {
        return false;
    }
    let l4 = msg_cp_write("out");
    if l4.as_str() != "cp: error writing 'out': No space left on device" {
        return false;
    }
    let l5 = msg_dd_quota("out");
    if l5.as_str() != "dd: fsync failed for 'out': Disk quota exceeded" {
        return false;
    }
    let l6 = msg_raw_write();
    if l6.as_str() != "write: No space left on device" {
        return false;
    }
    let l7 = msg_usage_opt('z');
    if l7.as_str() != "dd: invalid option -- 'z'" {
        return false;
    }
    let l8 = msg_usage_hint();
    if l8.as_str() != "Try 'dd --help' for more information." {
        return false;
    }
    // 面归属的前缀判别：同步式与写面式互斥。
    if !is_sync_face_line(l1.as_str()) || !is_sync_face_line(l2.as_str()) {
        return false;
    }
    if is_sync_face_line(l3.as_str()) || !is_write_face_line(l3.as_str()) {
        return false;
    }
    if is_write_face_line(l1.as_str()) {
        return false;
    }
    // 引号是判据不是排版：GNU 式带单引号，裸 syscall 式不带。
    if !has_quoted_operand(l1.as_str()) || has_quoted_operand(l6.as_str()) {
        return false;
    }
    // 档位默认行与逐字面一致。
    if msg_note(Note::DdFsync) != msg_dd_fsync_default() {
        return false;
    }
    if msg_note(Note::DdFdatasync) != msg_dd_fdatasync_default() {
        return false;
    }
    if msg_note(Note::Clean).len() != 0 || msg_note(Note::Silent).len() != 0 {
        return false;
    }
    // 按形态取行：延迟槽报同步式、即时槽报写面式、悬置槽一行不打、配额槽报配额。
    let sh3 = match shape_at(2) {
        Some(v) => v,
        None => return false,
    };
    if msg_for(&sh3) != msg_dd_fsync_default() {
        return false;
    }
    let sh5 = match shape_at(4) {
        Some(v) => v,
        None => return false,
    };
    if msg_for(&sh5) != msg_dd_write("out") {
        return false;
    }
    let sh7 = match shape_at(6) {
        Some(v) => v,
        None => return false,
    };
    if msg_for(&sh7).len() != 0 {
        return false;
    }
    let sh8 = match shape_at(7) {
        Some(v) => v,
        None => return false,
    };
    if msg_for(&sh8).len() != 0 {
        return false;
    }
    let sh6 = match shape_at(5) {
        Some(v) => v,
        None => return false,
    };
    if msg_for(&sh6) != msg_dd_quota("out") {
        return false;
    }
    // cp 对照形态：同为即时拒，前缀必须换人。
    if msg_for(&SHAPE_DIRECT_CP) != msg_cp_write("out") {
        return false;
    }
    // 每一行 ENOSPC 尾巴都必须与文案卡 F11694 同源（ANCHOR 逐字）。
    if !l1.contains(ANCHOR_ERRNO_TEXT) || !l3.contains(ANCHOR_ERRNO_TEXT) {
        return false;
    }
    if ANCHOR_ERRNO_TEXT.len() != 23 {
        return false;
    }
    true
}

/// 判据 09：退出码与短尾的闭合——0/1/2 三档逐槽对齐，悬置档退 0 单独立档。
fn exit_ok() -> bool {
    let mut i: usize = 0;
    while i < CORPUS {
        let sh = match shape_at(i) {
            Some(v) => v,
            None => return false,
        };
        let want = match WANT_EXIT.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if exit_code(&sh) != want {
            return false;
        }
        i += 1;
    }
    // 三档码位都在语料中出现过（0/1 有；2 由对照调用拒给出）。
    let mut zero_seen = false;
    let mut one_seen = false;
    let mut j: usize = 0;
    while j < CORPUS {
        let sh = match shape_at(j) {
            Some(v) => v,
            None => return false,
        };
        match exit_code(&sh) {
            EXIT_OK => zero_seen = true,
            EXIT_FAIL => one_seen = true,
            _ => {}
        }
        j += 1;
    }
    if !zero_seen || !one_seen {
        return false;
    }
    // 申报折算：success → 0/1，default → 2；与 claim_exit 闭合。
    let c1 = match claim_at(0) {
        Some(v) => v,
        None => return false,
    };
    if claim_exit(&c1) != EXIT_OK {
        return false;
    }
    let c2 = match claim_at(2) {
        Some(v) => v,
        None => return false,
    };
    if claim_exit(&c2) != EXIT_FAIL {
        return false;
    }
    let c3 = match claim_at(13) {
        Some(v) => v,
        None => return false,
    };
    if claim_exit(&c3) != EXIT_USAGE {
        return false;
    }
    // 落盘字节的不可逆事实：短尾槽的实落必须等于申报减短尾。
    let mut k: usize = 0;
    while k < CORPUS {
        let sh = match shape_at(k) {
            Some(v) => v,
            None => return false,
        };
        let a = define(&sh);
        if !sh.bad_call && a.value != sh.write_bytes.saturating_sub(landed_bytes(&sh)) {
            return false;
        }
        k += 1;
    }
    true
}

/// 判据 10：矩阵分档——四档求和闭合、缺档判负、渲染行对齐。
fn matrix_ok() -> bool {
    let m = Matrix::from_cases();
    if m.total as usize != CORPUS || m.aligned() as usize != CORPUS || !m.covered() {
        return false;
    }
    if m.occupied() as usize != TIER_COUNT {
        return false;
    }
    if m.perfect_rows() as usize != TIER_COUNT {
        return false;
    }
    // 四档槽数与期望档表逐一对位。
    let mut counts: [u32; 4] = [0; 4];
    let mut i: usize = 0;
    while i < CORPUS {
        let t = match WANT_TIER.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let rk = t.rank();
        if rk < 1 || rk > 4 {
            return false;
        }
        if let Some(slot) = counts.get_mut((rk - 1) as usize) {
            *slot = slot.saturating_add(1);
        }
        let sh = match shape_at(i) {
            Some(v) => v,
            None => return false,
        };
        if Tier::of(&define(&sh)) != t {
            return false;
        }
        i += 1;
    }
    if m.count_of(Tier::Clean) != counts[0]
        || m.count_of(Tier::Deferred) != counts[1]
        || m.count_of(Tier::Direct) != counts[2]
        || m.count_of(Tier::Stranded) != counts[3]
    {
        return false;
    }
    // desync 造不平：加投不进档，covered 必须翻脸。
    let mut d = Matrix::from_cases();
    d.desync();
    if d.covered() {
        return false;
    }
    // 空矩阵不完美：缺档本身是缺陷。
    let e = Matrix::new();
    if e.perfect_rows() != 0 || e.occupied() != 0 || e.aligned() != 0 {
        return false;
    }
    if !e.covered() {
        return false;
    }
    // 单行渲染闭合：good/total 独立，空行不完美，缺行按 0。
    let mut row = Row::of(Tier::Deferred);
    row.total = 3;
    row.good = 2;
    if row.perfect() || row.miss() != 1 {
        return false;
    }
    if row.render().as_str() != "deferred:2/3" {
        return false;
    }
    if e.row(Tier::Deferred) == None {
        return false;
    }
    true
}

/// 判据 11：全绿运行、逐字渲染与红档率标定。
fn report_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if !r.green() || r.gate() != Ok(()) {
        return false;
    }
    if r.render().as_str() != RENDER_OK {
        return false;
    }
    if r.rate() != RATE_FULL || r.total != CORPUS {
        return false;
    }
    if r.accept != ACCEPT || r.reject != REJECT {
        return false;
    }
    if r.tier_sum() != CORPUS {
        return false;
    }
    // 红档标定：注入一红，千分率必须正好 929。
    let red = match run_with(Some((3usize, Fault::Estate))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if !red.is_red_calibrated() || red.green() {
        return false;
    }
    if red.render().contains("verdict=green") {
        return false;
    }
    // 越界注入必须判死。
    if run_with(Some((CORPUS, Fault::Stage))) != Err(FS_SLOT) {
        return false;
    }
    // 出口闸逐项各管一格：断一档则对应码出口。
    let mut broken = r;
    broken.total = CORPUS - 1;
    if broken.gate() != Err(FS_SLOT) {
        return false;
    }
    let mut b2 = r;
    b2.matched = b2.total - 1;
    if b2.gate() != Err(FS_STATE) {
        return false;
    }
    let mut b3 = r;
    b3.stranded = b3.stranded + 1;
    if b3.gate() != Err(FS_VALUE) {
        return false;
    }
    // 空报告 rate 归零不除零。
    let nil = Report {
        total: 0,
        accept: 0,
        reject: 0,
        clean: 0,
        deferred: 0,
        direct: 0,
        stranded: 0,
        matched: 0,
    };
    if nil.rate() != 0 {
        return false;
    }
    true
}

/// 判据 12a：语料纪律——14 槽标签唯一、死槽一槽只坏一维、六维修次序齐全。
fn corpus_ok() -> bool {
    if WANT_TAGS.len() != CORPUS || !slugs_unique(&WANT_TAGS) {
        return false;
    }
    // 后六槽的 tag 必须按六维顺序挂名。
    let want: [&str; REJECT] =
        ["estate-swap", "stage-skip", "code-swap", "value-off", "note-swap", "default-bucket"];
    let mut j: usize = 0;
    while j < REJECT {
        let idx = ACCEPT.saturating_add(j);
        let got = match WANT_TAGS.get(idx) {
            Some(v) => *v,
            None => return false,
        };
        let exp = match want.get(j) {
            Some(v) => *v,
            None => return false,
        };
        if got != exp {
            return false;
        }
        j += 1;
    }
    // 死槽与复用活槽的事实面差异恰好一位（diff_mask 只有一个 bit）。
    let mut k: usize = 0;
    while k < REJECT {
        let idx = ACCEPT.saturating_add(k);
        let sh = match shape_at(idx) {
            Some(v) => v,
            None => return false,
        };
        let actual = define(&sh);
        let claim = match claim_at(idx) {
            Some(v) => v,
            None => return false,
        };
        let expect_fault = match REJECT_FAULTS.get(k) {
            Some(v) => *v,
            None => return false,
        };
        if expect_fault.rank() != (k as u32) + 1 {
            return false;
        }
        let m = claim.diff_mask(&actual);
        if m != 1u32 << (expect_fault.rank() - 1) {
            return false;
        }
        if judge(&Case::new(sh, claim)) != expect_fault {
            return false;
        }
        k += 1;
    }
    // 活槽全部六维照实（diff_mask 为 0）。
    let mut i: usize = 0;
    while i < ACCEPT {
        let sh = match shape_at(i) {
            Some(v) => v,
            None => return false,
        };
        let claim = match claim_at(i) {
            Some(v) => v,
            None => return false,
        };
        if claim.diff_mask(&define(&sh)) != 0 || judge(&Case::new(sh, claim)) != Fault::Ok {
            return false;
        }
        let want = match LIVE_OK.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if claim.success() != want {
            return false;
        }
        i += 1;
    }
    true
}

/// 判据 12b：期望表第二形态与 FACT/CLAIM 双路一致（复写交叉校验）。
fn slots_ok() -> bool {
    if WANT_STAGE.len() != CORPUS
        || WANT_CODE.len() != CORPUS
        || WANT_NDX.len() != CORPUS
        || WANT_NOTE.len() != CORPUS
        || WANT_DEFAULT.len() != CORPUS
        || WANT_OK.len() != CORPUS
        || WANT_VALUE.len() != CORPUS
    {
        return false;
    }
    if ACT_STAGE.len() != CORPUS
        || ACT_CODE.len() != CORPUS
        || ACT_VALUE.len() != CORPUS
        || ACT_NOTE.len() != CORPUS
        || ACT_DEFAULT.len() != CORPUS
        || ACT_OK.len() != CORPUS
        || ACT_LANDED.len() != CORPUS
        || WANT_CODE_NAME.len() != CORPUS
    {
        return false;
    }
    let mut i: usize = 0;
    while i < CORPUS {
        // 申报：CLAIMS 原表 == WANT_* 重建（交叉复写抓笔误）。
        let c1 = match claim_at(i) {
            Some(v) => v,
            None => return false,
        };
        let c2 = match claim_of(i) {
            Some(v) => v,
            None => return false,
        };
        if c1 != c2 {
            return false;
        }
        // 事实：define 产出 == ACT_* 表。
        let sh = match shape_at(i) {
            Some(v) => v,
            None => return false,
        };
        let a = define(&sh);
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
        let ok = match ACT_OK.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if a.stage != st || a.code != cd || a.value != vl || a.note != nt || a.default != df {
            return false;
        }
        if a.ok() != ok {
            return false;
        }
        // 码名第二形态。
        let nm = match WANT_CODE_NAME.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if code_name(cd) != nm {
            return false;
        }
        // WANT_NDX 的秩必须与 judge 结果对位。
        if want_fault(i) != judge(&Case::new(sh, c1)) {
            return false;
        }
        i += 1;
    }
    // 六枚缺陷码互不相同且都在段位内。
    let mut p: usize = 0;
    while p < REJECT {
        let c = match REJECT_CODES.get(p) {
            Some(v) => *v,
            None => return false,
        };
        if c <= SEG_BASE || c >= SEG_BASE.saturating_add(0x0e) {
            return false;
        }
        let mut q: usize = p + 1;
        while q < REJECT {
            if let Some(x) = REJECT_CODES.get(q) {
                if *x == c {
                    return false;
                }
            }
            q += 1;
        }
        p += 1;
    }
    true
}

/// 判据 12c：账本审计——建账、乱序拒登、篡改拒登、封印重放。
fn ledger_ok() -> bool {
    let mut l = build_ledger();
    if l.audit() != Ok(()) {
        return false;
    }
    if l.len() != SAMPLES || l.is_empty() {
        return false;
    }
    if l.ok_count() != ACCEPT || l.err_count() != REJECT {
        return false;
    }
    if l.count_op(OP_STAT) != STAT_CELLS
        || l.count_op(OP_RATE) != 1
        || l.count_op(OP_DEFECT) != REJECT
    {
        return false;
    }
    // 越界与跳号：拒入账、只加 rejects。
    let mut m = Ledger::new();
    if m.register(Sample::stat(1, 5)) != Err(FS_SLOT) {
        return false;
    }
    if m.register(Sample::stat(0, 4)) != Ok(()) {
        return false;
    }
    if m.register(Sample::stat(0, 4)) != Err(FS_SLOT) {
        return false;
    }
    if m.len() != 1 || m.rejects() != 2 {
        return false;
    }
    // 篡改格（ok 与 code 互斥）拒入账。
    let tampered = Sample { slot: 1, op: OP_DEFECT, ok: true, code: FS_VALUE, value: 0 };
    if m.register(tampered) != Err(FS_UNIQ) {
        return false;
    }
    // 错值账/错码账/被改缺陷账都必须被 audit 抓到。
    if broken_value_ledger().audit() != Err(FS_VALUE) {
        return false;
    }
    if broken_code_ledger().audit() != Err(FS_UNIQ) {
        return false;
    }
    if broken_defect_ledger().audit() != Err(FS_VALUE) {
        return false;
    }
    // 封印→冻结→重放；冻结后登记拒。
    let root = l.freeze();
    if root == 0 || !l.is_sealed() || !l.is_frozen() {
        return false;
    }
    if l.replay() != Ok(root) {
        return false;
    }
    if l.register(Sample::stat(SAMPLES, 1)) != Err(FS_STATE) {
        return false;
    }
    if l.rejects() == 0 {
        return false;
    }
    // 未封印重放判死。
    let fresh = build_ledger();
    if fresh.replay() != Err(FS_STATE) {
        return false;
    }
    // 指纹非零且不同文本不同指纹。
    if fold_str(FNV_OFF, "a") == fold_str(FNV_OFF, "b") {
        return false;
    }
    if fold_str(FNV_OFF, "ab") == fold_str(FNV_OFF, "a") {
        return false;
    }
    true
}

// ---------------------------------------------------------------------------
// 出口：13 条判据
// ---------------------------------------------------------------------------

/// 本卡自检出口。恰好 13 条判据，一条一闸，名字逐字钉死。
pub fn run_unx11691_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11691_fsync_enospc");
    s.add("FS01-名称与段基址", name_ok(), "static");
    s.add("FS02-枚举与纵线", enums_ok(), "static");
    s.add("FS03-延迟分配几何", delay_ok(), "static");
    s.add("FS04-形态与合法域", shape_ok(), "static");
    s.add("FS05-九闸纵线判定", define_ok(), "static");
    s.add("FS06-延迟交付分档", defer_ok(), "static");
    s.add("FS07-同步面切分与闸门", kind_ok(), "static");
    s.add("FS08-文案四式逐字", message_ok(), "static");
    s.add("FS09-退出码与短尾", exit_ok(), "static");
    s.add("FS10-矩阵分档与渲染", matrix_ok(), "static");
    s.add("FS11-全绿运行与红档率", report_ok(), "static");
    s.add(
        "FS12-语料账本双审计",
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
        "FS13-冻结重放四注入",
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
        assert_eq!(SEG_BASE, 0x6d390);
        assert!(anchors_agree());
        assert_eq!(SEG_BASE - ANCHOR_F11681_GAP, ANCHOR_F11681_SEG);
        assert_eq!(SEG_BASE - ANCHOR_F11684_GAP, ANCHOR_F11684_SEG);
        assert_eq!(SEG_BASE - ANCHOR_F11689_GAP, ANCHOR_F11689_SEG);
        assert_eq!(ENOSPC, ANCHOR_ENOSPC);
        assert_eq!(ANCHOR_ERRNO_TEXT, "No space left on device");
    }

    #[test]
    fn thirteen_codes_are_contiguous_and_distinct() {
        assert_eq!(ALL_CODES.len(), 13);
        assert_eq!(FS_SLOT, SEG_BASE + 1);
        assert_eq!(FS_SWAL, SEG_BASE + 13);
        assert_eq!(code_offset(FS_SLOT), 1);
        assert_eq!(code_offset(FS_SWAL), 13);
        assert_eq!(code_offset(SEG_BASE), 0);
        assert_eq!(code_offset(SEG_BASE - 1), -1);
        assert!(name_ok());
    }

    #[test]
    fn gate_ranks_are_one_based_and_zero_is_not_a_stage() {
        assert_eq!(Stage::Call.rank(), 1);
        assert_eq!(Stage::Closed.rank(), 9);
        assert!(Stage::from_rank(0).is_none());
        assert!(Stage::from_rank(10).is_none());
        assert_eq!(Stage::from_rank(1), Some(Stage::Call));
        assert!(enums_ok());
    }

    #[test]
    fn landing_stages_are_exactly_three() {
        assert!(Stage::Call.is_landing());
        assert!(Stage::Report.is_landing());
        assert!(Stage::Exit.is_landing());
        assert!(!Stage::Cache.is_landing());
        assert!(!Stage::Flush.is_landing());
        assert!(!Stage::Consume.is_landing());
        assert!(!Stage::Closed.is_landing());
    }

    #[test]
    fn gate_only_advances_to_the_next_rank() {
        let mut g = Gate::new();
        assert_eq!(g.rank(), 0);
        assert_eq!(g.step(Stage::Cache), Err(FS_KIND));
        assert_eq!(g.rank(), 0);
        assert!(g.step(Stage::Call).is_ok());
        assert_eq!(g.rank(), 1);
        assert_eq!(g.step(Stage::Call), Err(FS_KIND));
    }

    #[test]
    fn rejected_moves_change_the_fingerprint() {
        let mut g = Gate::new();
        let before = g.fingerprint();
        assert_eq!(g.step(Stage::Exit), Err(FS_KIND));
        assert_ne!(g.fingerprint(), before);
        assert_eq!(g.rank(), 0);
        assert_eq!(g.rejected(), 1);
        assert_eq!(g.moves(), 1);
    }

    #[test]
    fn backward_step_is_a_stage_fault_and_sealed_gate_refuses_everything() {
        let mut g = Gate::new();
        assert!(g.walk_and_seal().is_ok());
        assert_eq!(g.step(Stage::Call), Err(FS_STATE));
        assert_eq!(g.close(), Err(FS_STATE));
        let mut h = Gate::new();
        for k in 0..5 {
            if let Some(st) = Stage::ALL.get(k) {
                assert!(h.step(*st).is_ok());
            }
        }
        assert_eq!(h.step(Stage::Call), Err(FS_STAGE));
        assert_eq!(h.close(), Err(FS_STAGE));
    }

    #[test]
    fn sync_kind_awaits_only_for_fsync_faces() {
        assert!(SyncKind::Fsync.awaits());
        assert!(SyncKind::Fdatasync.awaits());
        assert!(!SyncKind::SyncRange.awaits());
        assert!(!SyncKind::None.awaits());
        assert_eq!(SyncKind::ALL.len(), 4);
        for sk in SyncKind::ALL.iter() {
            assert_eq!(SyncKind::from_rank(sk.rank()), Some(*sk));
        }
    }

    #[test]
    fn error_state_is_claimable_only_once() {
        assert!(!ErrState::Idle.claimable());
        assert!(ErrState::Pending.claimable());
        assert!(!ErrState::Consumed.claimable());
        assert_eq!(ErrState::ALL.len(), 3);
        assert_eq!(ErrState::from_rank(4), None);
    }

    #[test]
    fn write_bytes_below_room_is_clean() {
        let sh = Shape::clean(4096, SyncKind::Fsync);
        let a = define(&sh);
        assert_eq!(a.stage, Stage::Exit);
        assert_eq!(a.code, 0);
        assert_eq!(a.value, 0);
        assert_eq!(a.note, Note::Clean);
        assert!(a.ok());
        assert_eq!(exit_code(&sh), EXIT_OK);
    }

    #[test]
    fn delayed_alloc_defers_enospc_to_fsync() {
        let sh = match shape_at(2) {
            Some(v) => v,
            None => panic!("slot missing"),
        };
        assert_eq!(sh.shortfall(), 4096);
        assert!(defers_to_sync_face(&sh));
        assert!(!rejects_at_write_face(&sh));
        let a = define(&sh);
        assert_eq!(a.stage, Stage::Report);
        assert_eq!(a.code, FS_DEFER);
        assert_eq!(a.value, 4096);
        assert_eq!(exit_code(&sh), EXIT_FAIL);
        assert_eq!(Tier::of(&a), Tier::Deferred);
    }

    #[test]
    fn fdatasync_defers_with_its_own_line() {
        let sh = match shape_at(3) {
            Some(v) => v,
            None => panic!("slot missing"),
        };
        let a = define(&sh);
        assert_eq!(a.code, FS_DEFER);
        assert_eq!(a.value, 8192);
        assert_eq!(a.note, Note::DdFdatasync);
        assert_eq!(
            msg_for(&sh).as_str(),
            "dd: fdatasync failed for 'out': No space left on device"
        );
    }

    #[test]
    fn direct_io_raises_enospc_at_the_write_face() {
        let sh = match shape_at(4) {
            Some(v) => v,
            None => panic!("slot missing"),
        };
        assert!(rejects_at_write_face(&sh));
        let a = define(&sh);
        assert_eq!(a.stage, Stage::Call);
        assert_eq!(a.code, FS_KIND);
        assert_eq!(a.note, Note::WriteErr);
        assert_eq!(Tier::of(&a), Tier::Direct);
        let mut flip = sh;
        flip.direct = false;
        assert_eq!(define(&flip).stage, Stage::Report);
        assert_eq!(define(&flip).code, FS_DEFER);
    }

    #[test]
    fn consumed_error_state_silences_the_second_report() {
        let sh = match shape_at(7) {
            Some(v) => v,
            None => panic!("slot missing"),
        };
        assert_eq!(sh.prior, ErrState::Consumed);
        assert!(error_stranded(&sh));
        let a = define(&sh);
        assert_eq!(a.code, FS_CLEAR);
        assert_eq!(a.note, Note::Silent);
        assert_eq!(exit_code(&sh), EXIT_OK);
        assert!(tail_lies(&sh));
        assert_eq!(msg_for(&sh).len(), 0);
    }

    #[test]
    fn sync_file_range_never_carries_the_receipt() {
        let sh = match shape_at(6) {
            Some(v) => v,
            None => panic!("slot missing"),
        };
        assert!(!has_receipt(&sh));
        assert!(error_stranded(&sh));
        let a = define(&sh);
        assert_eq!(a.code, FS_SWAL);
        assert_eq!(a.stage, Stage::Exit);
        assert_eq!(Tier::of(&a), Tier::Stranded);
    }

    #[test]
    fn edquot_on_the_sync_face_is_not_enospc() {
        let sh = match shape_at(5) {
            Some(v) => v,
            None => panic!("slot missing"),
        };
        assert!(sh.quota);
        let a = define(&sh);
        assert_eq!(a.code, FS_CODE);
        assert_eq!(a.note, Note::Quota);
        assert_eq!(
            msg_for(&sh).as_str(),
            "dd: fsync failed for 'out': Disk quota exceeded"
        );
        assert_eq!(errno_name(EDQUOT), "Disk quota exceeded");
    }

    #[test]
    fn bad_call_lands_at_the_gate_that_never_entered_cache() {
        let sh = Shape {
            write_bytes: 0,
            room: 0,
            direct: false,
            kind: SyncKind::Fsync,
            prior: ErrState::Idle,
            quota: false,
            tool: Tool::Dd,
            bad_call: true,
        };
        assert!(sh.domain_ok());
        let a = define(&sh);
        assert_eq!(a.stage, Stage::Call);
        assert_eq!(a.code, FS_EXIT);
        assert_eq!(a.note, Note::Usage);
        assert_eq!(exit_code(&sh), EXIT_USAGE);
        assert_eq!(landed_bytes(&sh), 0);
        assert_eq!(msg_for(&sh).as_str(), "dd: invalid option -- 'z'");
    }

    #[test]
    fn unknown_tool_falls_into_the_default_bucket() {
        let sh = Shape {
            write_bytes: 8192,
            room: 4096,
            direct: false,
            kind: SyncKind::Fsync,
            prior: ErrState::Idle,
            quota: false,
            tool: Tool::Unknown,
            bad_call: false,
        };
        let a = define(&sh);
        assert!(a.default);
        assert_eq!(a.code, FS_DEFER);
        assert_eq!(a.stage, Stage::Report);
    }

    #[test]
    fn estate_swap_is_caught_first() {
        let c = match case_at(8) {
            Some(v) => v,
            None => panic!("slot missing"),
        };
        assert_eq!(judge(&c), Fault::Estate);
        assert_eq!(want_fault(8), Fault::Estate);
    }

    #[test]
    fn stage_value_note_default_each_own_their_slot() {
        for (i, f) in [
            (9usize, Fault::Stage),
            (10usize, Fault::Code),
            (11usize, Fault::Value),
            (12usize, Fault::Note),
            (13usize, Fault::Default),
        ] {
            let c = match case_at(i) {
                Some(v) => v,
                None => panic!("slot missing"),
            };
            assert_eq!(judge(&c), f, "slot {}", i);
            assert_eq!(want_fault(i), f);
        }
    }
    #[test]
    fn corpus_tags_are_unique_and_dead_slots_rotate_one_dim() {
        assert!(corpus_ok());
        assert_eq!(WANT_TAGS.len(), CORPUS);
        assert_eq!(DEAD_FROM.len(), REJECT);
        for j in 0..REJECT {
            let dead = shape_at(ACCEPT + j).unwrap();
            let src = shape_at(DEAD_FROM[j]).unwrap();
            assert_eq!(dead, src);
            let actual = define(&dead);
            let claim = claim_at(ACCEPT + j).unwrap();
            let mask = claim.diff_mask(&actual);
            assert_eq!(mask.count_ones(), 1);
        }
    }

    #[test]
    fn second_form_tables_match_define() {
        assert!(slots_ok());
        for i in 0..CORPUS {
            let sh = shape_at(i).unwrap();
            let a = define(&sh);
            assert_eq!(a.stage, ACT_STAGE[i]);
            assert_eq!(a.code, ACT_CODE[i]);
            assert_eq!(a.value, ACT_VALUE[i]);
            assert_eq!(a.note, ACT_NOTE[i]);
            assert_eq!(a.ok(), ACT_OK[i]);
            assert_eq!(claim_at(i), claim_of(i));
        }
    }

    #[test]
    fn tier_partition_is_two_seven_two_two() {
        assert!(matrix_ok());
        let m = Matrix::from_cases();
        assert_eq!(m.count_of(Tier::Clean), 3);
        assert_eq!(m.count_of(Tier::Deferred), 7);
        assert_eq!(m.count_of(Tier::Direct), 2);
        assert_eq!(m.count_of(Tier::Stranded), 2);
        assert_eq!(m.perfect_rows(), 4);
        assert!(m.covered());
    }

    #[test]
    fn empty_row_is_never_perfect() {
        let r = Row::of(Tier::Stranded);
        assert!(r.empty());
        assert!(!r.perfect());
        assert_eq!(r.miss(), 0);
    }

    #[test]
    fn green_run_renders_the_pinned_line() {
        let r = match run() {
            Ok(v) => v,
            Err(e) => panic!("run failed {}", e),
        };
        assert!(r.green());
        assert_eq!(r.render().as_str(), RENDER_OK);
        assert_eq!(r.rate(), RATE_FULL);
        assert!(report_ok());
    }

    #[test]
    fn one_red_slot_calibrates_the_rate_at_929() {
        let r = match run_with(Some((0usize, Fault::Value))) {
            Ok(v) => v,
            Err(e) => panic!("run_with failed {}", e),
        };
        assert_eq!(r.matched, 13);
        assert_eq!(r.rate(), RED_RATE);
        assert!(r.is_red_calibrated());
        assert!(!r.green());
    }

    #[test]
    fn out_of_range_injection_is_refused() {
        assert_eq!(run_with(Some((CORPUS, Fault::Stage))), Err(FS_SLOT));
        assert_eq!(run_with(Some((CORPUS + 5, Fault::Ok))), Err(FS_SLOT));
    }

    #[test]
    fn ledger_builds_audits_and_freezes() {
        let mut l = build_ledger();
        assert_eq!(l.audit(), Ok(()));
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), ACCEPT);
        assert_eq!(l.err_count(), REJECT);
        let root = l.freeze();
        assert_ne!(root, 0);
        assert!(l.is_frozen());
        assert_eq!(l.replay(), Ok(root));
        assert!(ledger_ok());
    }

    #[test]
    fn ledger_refuses_misordered_and_tampered_cells() {
        let mut l = Ledger::new();
        assert_eq!(l.register(Sample::stat(3, 9)), Err(FS_SLOT));
        assert!(l.is_empty());
        assert_eq!(l.rejects(), 1);
        assert!(l.register(Sample::stat(0, 4)).is_ok());
        let bad = Sample { slot: 1, op: OP_RATE, ok: true, code: 0, value: 100 };
        assert_eq!(l.register(bad), Err(FS_UNIQ));
        assert_eq!(l.rejects(), 2);
        assert_eq!(l.len(), 1);
    }

    #[test]
    fn broken_ledgers_are_caught_by_audit() {
        assert_eq!(broken_value_ledger().audit(), Err(FS_VALUE));
        assert_eq!(broken_code_ledger().audit(), Err(FS_UNIQ));
        assert_eq!(broken_defect_ledger().audit(), Err(FS_VALUE));
    }

    #[test]
    fn fingerprint_folds_order_and_boundary() {
        assert_eq!(fold_byte(FNV_OFF, 0), FNV_OFF.wrapping_mul(FNV_PRIME));
        assert_ne!(fold_word(FNV_OFF, 1), fold_word(FNV_OFF, 2));
        let a = fold_str(fold_str(FNV_OFF, "ab"), "c");
        let b = fold_str(fold_str(FNV_OFF, "a"), "bc");
        assert_ne!(a, b);
        let mut g = Gate::new();
        let fp0 = g.fingerprint();
        assert_ne!(fp0, 0);
        assert!(g.step(Stage::Call).is_ok());
        assert_ne!(g.fingerprint(), fp0);
    }

    #[test]
    fn inject_four_routes_all_pass() {
        let inj = inject_four();
        assert_eq!(inj.len(), 4);
        assert!(inj[0], "table route");
        assert!(inj[1], "judge route");
        assert!(inj[2], "render route");
        assert!(inj[3], "ledger route");
        assert_eq!(INJECT_ROUTES.len(), 4);
    }

    #[test]
    fn messages_are_verbatim_and_faces_are_distinguishable() {
        assert!(message_ok());
        assert_eq!(
            msg_dd_fsync_default().as_str(),
            "dd: fsync failed for 'out': No space left on device"
        );
        assert_eq!(
            msg_dd_fdatasync_default().as_str(),
            "dd: fdatasync failed for 'out': No space left on device"
        );
        assert!(is_sync_face_line(msg_dd_fsync_default().as_str()));
        assert!(!is_sync_face_line(msg_dd_write("out").as_str()));
        assert!(is_write_face_line(msg_cp_write("out").as_str()));
        assert_eq!(msg_note(Note::Silent).len(), 0);
        assert_eq!(msg_note(Note::Usage).as_str(), "dd: invalid option -- 'z'");
    }

    #[test]
    fn exit_table_is_closed_per_slot() {
        assert!(exit_ok());
        for i in 0..CORPUS {
            let sh = shape_at(i).unwrap();
            assert_eq!(exit_code(&sh), WANT_EXIT[i], "slot {}", i);
        }
        assert_eq!(EXIT_OK, 0);
        assert_eq!(EXIT_FAIL, 1);
        assert_eq!(EXIT_USAGE, 2);
    }

    #[test]
    fn short_tail_equals_write_minus_landed_everywhere() {
        for i in 0..CORPUS {
            let sh = shape_at(i).unwrap();
            if sh.bad_call {
                continue;
            }
            assert_eq!(sh.shortfall(), sh.write_bytes - sh.landed());
            assert_eq!(landed_bytes(&sh), ACT_LANDED[i]);
            assert_eq!(define(&sh).value, sh.shortfall());
        }
    }

    #[test]
    fn domain_rules_reject_inconsistent_shapes() {
        let bad = Shape {
            write_bytes: 4096,
            room: 8192,
            direct: false,
            kind: SyncKind::Fsync,
            prior: ErrState::Pending,
            quota: false,
            tool: Tool::Dd,
            bad_call: false,
        };
        assert!(!bad.domain_ok());
        let worse = Shape {
            write_bytes: 0,
            room: 0,
            direct: false,
            kind: SyncKind::Fsync,
            prior: ErrState::Consumed,
            quota: false,
            tool: Tool::Dd,
            bad_call: true,
        };
        assert!(!worse.domain_ok());
        for i in 0..CORPUS {
            assert!(shape_at(i).unwrap().domain_ok(), "slot {}", i);
        }
    }

    #[test]
    fn enums_rank_roundtrip_and_face_arithmetic() {
        assert!(enums_ok());
        for st in Stage::ALL.iter() {
            assert_eq!(Stage::from_rank(st.rank()), Some(*st));
            assert_ne!(st.cn().len(), 0);
        }
        for nt in Note::ALL.iter() {
            assert_eq!(Note::from_rank(nt.rank()), Some(*nt));
        }
        for tl in Tool::ALL.iter() {
            assert_eq!(Tool::from_rank(tl.rank()), Some(*tl));
        }
        for tr in Tier::ALL.iter() {
            assert_eq!(Tier::from_rank(tr.rank()), Some(*tr));
            assert_ne!(tr.cn().len(), 0);
        }
        assert_eq!(SyncKind::Fdatasync.cn(), "数据同步面");
    }

    #[test]
    fn fault_codes_map_onto_the_six_reject_cells() {
        assert_eq!(REJECT_FAULTS.len(), REJECT);
        assert_eq!(REJECT_CODES.len(), REJECT);
        for (k, f) in REJECT_FAULTS.iter().enumerate() {
            assert_eq!(f.code(), REJECT_CODES[k]);
            assert!(f.is_fault());
            assert_ne!(f.why().len(), 0);
        }
        assert_eq!(Fault::Ok.code(), 0);
        assert!(!Fault::Ok.is_fault());
        assert_eq!(Fault::Estate.code(), FS_STATE);
        assert_eq!(Fault::Default.code(), FS_FAULT_DEFAULT);
    }

    #[test]
    fn verdicts_table_matches_want_ndx() {
        let v = verdicts();
        assert_eq!(v.len(), CORPUS);
        for i in 0..CORPUS {
            assert_eq!(v[i], want_fault(i), "slot {}", i);
        }
        assert!(all_claims_land());
    }

    #[test]
    fn stat_cells_cross_check_vals() {
        assert_eq!(VALS.len(), VALS_CHECK.len());
        for i in 0..VALS.len() {
            assert_eq!(VALS[i], VALS_CHECK[i], "cell {}", i);
            assert_ne!(stat_meaning(i), "越界");
        }
        assert_eq!(stat_meaning(7), "红档率 929：14 格中 13 的千分率");
        assert_eq!(VALS[7], RED_RATE);
        assert_eq!(STAT_CELLS, 7);
    }

    #[test]
    fn dec_hex_and_text_helpers_roundtrip() {
        assert_eq!(dec_text(0).as_str(), "0");
        assert_eq!(dec_text(929).as_str(), "929");
        assert_eq!(dec_text(4294967295).as_str(), "4294967295");
        assert_eq!(hex_text(0x6d390, 6).as_str(), "06d390");
        assert_eq!(hex_text(0xff, 2).as_str(), "ff");
        assert_eq!(seg_text(SEG_BASE).as_str(), "0x06d390");
        assert_eq!(u32_text(28).as_str(), "28");
        assert_eq!(i32_text(-3).as_str(), "-3");
        assert_eq!(i32_text(i32::MIN).as_str(), "-2147483648");
        assert_eq!(bool_text(true).as_str(), "true");
        assert_eq!(bool_text(false).as_str(), "false");
        let mut s = String::new();
        kv(&mut s, "total", 14);
        assert_eq!(s.as_str(), "total=14");
    }

    #[test]
    fn outcome_and_claim_fold_are_order_sensitive() {
        let a = match shape_at(2) {
            Some(v) => define(&v),
            None => panic!("slot missing"),
        };
        let f1 = a.fold(FNV_OFF);
        let f2 = a.fold(fold_word(FNV_OFF, 0));
        assert_ne!(f1, f2);
        let c = Claim::from_outcome(&a, a.ok());
        assert_eq!(c.diff_mask(&a), 0);
        assert_eq!(c.exit_hint(), EXIT_FAIL);
        let flipped = Claim::new(!a.ok(), a.stage, a.code, a.value, a.note, a.default);
        assert_eq!(flipped.diff_mask(&a), 0x01);
        assert_ne!(c.fold(FNV_OFF), flipped.fold(FNV_OFF));
    }

    #[test]
    fn render_rows_include_outcome_and_sample() {
        let a = Outcome::new(Stage::Report, FS_DEFER, 4096, Note::DdFsync, false);
        let s = a.render();
        assert!(s.contains("stage=report"));
        assert!(s.contains("code=fs-defer"));
        assert!(s.contains("tail=4096"));
        let c = Claim::new(false, Stage::Report, FS_DEFER, 4096, Note::DdFsync, false);
        assert!(c.render().contains("ok=false"));
        let cell = Sample::stat(0, 4);
        assert!(cell.render().contains("slot=0"));
        assert!(cell.self_consistent());
        // 缺陷格自洽：ok=false + code≠0 + value=0。
        assert!(Sample::defect(8, FS_STATE).self_consistent());
        // 不自洽的两种形制：把缺陷格的成败位翻成"成功"，统计格塞 0 值。
        let liar = Sample { slot: 8, op: OP_DEFECT, ok: true, code: FS_STATE, value: 0 };
        assert!(!liar.self_consistent());
        assert!(!Sample::stat(0, 0).self_consistent());
        assert_eq!(Sample::rate(7).value, RED_RATE);
    }

    #[test]
    fn ledger_render_shows_root_high_bits() {
        let mut l = build_ledger();
        let root = l.freeze();
        let s = l.render();
        assert!(s.contains("len=14"));
        assert!(s.contains("ok=8"));
        assert!(s.contains("err=6"));
        let want = hex_text(root as u32, 8);
        assert!(s.contains(want.as_str()));
    }

    #[test]
    fn code_names_and_offsets_span_the_segment() {
        assert_eq!(code_name(FS_DEFER), "fs-defer");
        assert_eq!(code_name(FS_CLEAR), "fs-clear");
        assert_eq!(code_name(FS_SWAL), "fs-swal");
        assert_eq!(code_name(FS_EXIT), "fs-exit");
        assert_eq!(code_name(SEG_BASE + 0x20), "out-of-segment");
        assert_eq!(code_offset(FS_NOTE), 7);
        assert_eq!(errno_name(ENOSPC), ANCHOR_ERRNO_TEXT);
        assert_eq!(errno_name(EBADF), "Bad file descriptor");
        assert_eq!(errno_name(999), "Unknown errno");
        assert!(errno_legible(ENOSPC));
        assert!(errno_legible(EDQUOT));
        assert!(!errno_legible(999));
    }

    #[test]
    fn direct_cp_counterpart_stays_on_the_write_face() {
        assert!(SHAPE_DIRECT_CP.domain_ok());
        let a = define(&SHAPE_DIRECT_CP);
        assert_eq!(a.stage, Stage::Call);
        assert_eq!(a.code, FS_KIND);
        assert_eq!(a.value, 12288);
        assert_eq!(Tier::of(&a), Tier::Direct);
        assert_eq!(msg_for(&SHAPE_DIRECT_CP).as_str(), "cp: error writing 'out': No space left on device");
    }

    #[test]
    fn matrix_desync_breaks_coverage() {
        let mut m = Matrix::from_cases();
        assert!(m.covered());
        m.desync();
        assert!(!m.covered());
        assert_eq!(m.total, m.aligned() + 1);
    }

    #[test]
    fn report_gate_checks_each_axis() {
        let r = match run() {
            Ok(v) => v,
            Err(_) => panic!("run must pass"),
        };
        assert_eq!(r.gate(), Ok(()));
        let mut a = r;
        a.accept = 7;
        a.reject = 6;
        assert_eq!(a.gate(), Err(FS_UNIQ));
        let mut b = r;
        b.clean = 0;
        assert_eq!(b.gate(), Err(FS_VALUE));
    }

    #[test]
    fn thirteen_checks_are_registered_and_green() {
        let cs = run_unx11691_checks();
        assert_eq!(cs.len(), 13);
        assert!(cs.all_passed());
        assert_eq!(cs.domain, "unx11691_fsync_enospc");
    }

    #[test]
    fn check_names_are_pinned_in_order() {
        let cs = run_unx11691_checks();
        let names: [&str; 13] = [
            "FS01-名称与段基址",
            "FS02-枚举与纵线",
            "FS03-延迟分配几何",
            "FS04-形态与合法域",
            "FS05-九闸纵线判定",
            "FS06-延迟交付分档",
            "FS07-同步面切分与闸门",
            "FS08-文案四式逐字",
            "FS09-退出码与短尾",
            "FS10-矩阵分档与渲染",
            "FS11-全绿运行与红档率",
            "FS12-语料账本双审计",
            "FS13-冻结重放四注入",
        ];
        for (i, want) in names.iter().enumerate() {
            let got = cs.get(i);
            assert!(got.is_some());
            if let Some(c) = got {
                assert_eq!(c.name, *want, "check {}", i);
                assert!(c.passed, "check {} red", i);
            }
        }
    }

    #[test]
    fn slot_count_and_constants_are_stable() {
        assert_eq!(slot_count(), CORPUS);
        assert_eq!(CODE_COUNT, 13);
        assert_eq!(TIER_COUNT, 4);
        assert_eq!(EXIT_RANKS, 3);
        assert_eq!(SYNC_FACE_COUNT, 4);
        assert_eq!(DEFECT_CELLS, REJECT);
        assert_eq!(VALS[6], DEFECT_CELLS as u32);
    }

    #[test]
    fn deferred_and_stranded_never_cross_report() {
        for i in 0..CORPUS {
            let sh = shape_at(i).unwrap();
            let tier = Tier::of(&define(&sh));
            if tier == Tier::Stranded {
                assert_eq!(msg_for(&sh).len(), 0, "slot {} must stay silent", i);
            }
            if tier == Tier::Deferred {
                assert_ne!(msg_for(&sh).len(), 0, "slot {} must speak", i);
                assert!(is_sync_face_line(msg_for(&sh).as_str())
                    || msg_for(&sh).contains("Disk quota exceeded"));
            }
        }
    }
}
