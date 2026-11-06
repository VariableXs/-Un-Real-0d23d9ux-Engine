//! UNX-F11698 工具退出码矩阵（ENOSPC 场景面）
//!
//! 盘满的时候，十二个工具各自怎么退出？这个问题看似只是查表，实则处处是坑：
//! 同一个 `ENOSPC`，`cp` 退 1，`sort` 退 2，`sed -i` 退 4，`df`/`du` 压根不退
//! 错、只打印一行告警。**退出码不是错误的严重度，而是工具自己对"我出了什么
//! 事"的分类**。把 1/2/4 硬压成一个"失败"，工具之间的差异就被抹平了。
//!
//! 本卡把十二个工具在同一块满盘上的收场钉成一张矩阵：
//!
//! ```text
//! $ cp  /tmp/bx-sandbox/a/x /tmp/bx-sandbox/b/y ; echo $?
//! cp: error writing '/tmp/bx-sandbox/b/y': No space left on device
//! 1
//! $ sort /tmp/bx-sandbox/a/big > /tmp/bx-sandbox/b/out ; echo $?
//! sort: cannot create temporary file in '/tmp': No space left on device
//! 2
//! $ sed -i 's/a/b/' /tmp/bx-sandbox/b/big ; echo $?
//! sed: couldn't write 4096 bytes to /tmp/bx-sandbox/b/sedAbCdE: No space left on device
//! 4
//! $ df /tmp/bx-sandbox ; echo $?
//! Filesystem  1K-blocks  Used  Available  Use%
//! bx-sandbox       4096  4096          0  100%
//! 0
//! ```
//!
//! 四行四个码：1、2、4、0。矩阵的每一格都必须同时钉住**退出码**与**errno**
//! 两档（这就是本卡标题里的"1/2 分档"：每一格判两档，一档也不许悬空）。
//!
//! ## 闸门次序（这条纵线为什么不能倒）
//!
//! 七道闸，秩 1..7，一次只许前进；倒退 → `EC_STAGE`，封账后再动 → `EC_ESTATE`：
//!
//! 1. **Check**（秩 1）——操作数齐、开关合法。用法错一律退 2，且不碰盘。
//! 2. **Open**（秩 2）——源与目标可打开。`ENOENT`/`EACCES` 在这一闸，
//!    与 ENOSPC 无关。
//! 3. **Write**（秩 3）——真正写数据。十二个工具里只有写侧工具会走到这里，
//!    `df`/`du` 是只读的，永不进入本闸。
//! 4. **Flush**（秩 4）——缓冲落盘。`ENOSPC` 最常在这里被内核交回来。
//! 5. **Close**（秩 5）——关闭与清理临时文件。`sed -i` 的临时文件就在这一闸。
//! 6. **Report**（秩 6）——errno 与文案归属。退出码在下一闸定，本闸定的是
//!    "这条 errno 到底是不是 ENOSPC"。
//! 7. **Exit**（秩 7）——退出码：1 = 普通失败，2 = 麻烦/部分完成，4 = 输入
//!    输出错，0 = 只告警。
//!
//! 次序为什么不能倒：Open 早于 Write（打不开的文件不会写满盘）；Write 早于
//! Flush（没写就没有待刷的页）；Flush 早于 Close（关之前必须结清）；Report
//! 早于 Exit（先定 errno，才谈得上用退出码给它分档）。
//!
//! ## 关键差异（本卡深度所在，谁判死谁放行）
//!
//! * **与 F11697（退出码单点）分层**：单点卡只判一个工具的码；本卡要的是
//!   **同一块满盘上十二个工具的横向矩阵**，横看工具、纵看档位，任何一格与
//!   邻格串味都算红。
//! * **与 F11687（mv 回滚）分层**：mv 卡判"复制到一半怎么收场"；本卡只把
//!   `mv` 当成十二分之一格，判它的退出码 1 是否与 `cp` 的 1 同档、与 `sort`
//!   的 2 不同档。
//! * **与 F11694（文案逐字）分层**：文案卡判句子拼写；本卡判**句子落到哪个
//!   退出码**。同一句 `No space left on device`，在 `cp` 后面是 1，在
//!   `sort` 后面是 2，在 `df` 后面是 0——句子一样，档不一样。
//! * **`df`/`du` 的 0 不是绿**：只读工具在满盘上退出 0 是正常的，但"退出 0
//!   且没有告警"就是**悬空**——用户会以为一切正常。本卡把 0 档单列为
//!   `ExitZero`，它不是绿档：0 必须带一行告警，否则判 `EC_WARNOFF`。
//! * **errno 与退出码两档分开判**：`tee` 报 `ENOSPC` 但退出 1 是对的；报
//!   `EIO` 却退出 1 就是错。两档任缺一档，矩阵就有一格悬空。

use crate::alloc::string::String;
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 段基址与联签锚
// ---------------------------------------------------------------------------

/// 本卡段基址。
pub const SEG_BASE: u32 = 0x6d420;

/// 前邻卡 F11697 段基址。
pub const ANCHOR_F11697_SEG: u32 = 0x6d410;

/// F11697 到本卡的段差。
pub const ANCHOR_F11697_GAP: u32 = 0x10;

/// 后邻卡 F11699 段基址。
pub const ANCHOR_F11699_SEG: u32 = 0x6d430;

/// 本卡到 F11699 的段差。
pub const ANCHOR_F11699_GAP: u32 = 0x10;

/// 上游卡 F11694 段基址。
pub const ANCHOR_F11694_SEG: u32 = 0x6d3d0;

/// 本卡到 F11694 的段差。
pub const ANCHOR_F11694_GAP: u32 = 0x50;

/// 上游卡 F11687 段基址。
pub const ANCHOR_F11687_SEG: u32 = 0x6d330;

/// 本卡到 F11687 的段差。
pub const ANCHOR_F11687_GAP: u32 = 0xf0;

/// `ENOSPC` 的数值。
pub const ANCHOR_ENOSPC: u32 = 28;

/// `EIO` 的数值。
pub const ANCHOR_EIO: u32 = 5;

/// `ENOSPC` 的逐字文本。
pub const ANCHOR_ENOSPC_TEXT: &str = "No space left on device";

/// `EIO` 的逐字文本。
pub const ANCHOR_EIO_TEXT: &str = "Input/output error";

// ---------------------------------------------------------------------------
// 规模常量
// ---------------------------------------------------------------------------

/// 语料槽数。
pub const CORPUS: usize = 14;

/// 台账格数。
pub const SAMPLES: usize = 14;

/// 正例格数。
pub const ACCEPT: usize = 8;

/// 反例格数。
pub const REJECT: usize = 6;

/// 档位数。
pub const TIER_COUNT: usize = 4;

/// 闸门数。
pub const STAGE_COUNT: u32 = 7;

/// 末闸秩。
pub const STAGE_LAST_RANK: u32 = 7;

/// 统计格数。
pub const STAT_CELLS: usize = 7;

/// 红档率（第八格必须等于它）。
pub const RED_RATE: u32 = 929;

/// 千分比满分。
pub const RATE_FULL: u32 = 1000;

/// 满分别名。
pub const MATCH_FULL: u32 = 1000;

/// 工具数。
pub const TOOL_COUNT: usize = 12;

/// 写侧工具数。
pub const WRITE_TOOLS: usize = 10;

/// 只读告警工具数。
pub const WARN_TOOLS: usize = 2;

/// 族内场景数。
pub const FAMILY_SCENES: usize = 12;

/// 族内退 1 的场景数。
pub const ONE_SCENES: usize = 8;

/// 族内退 2 的场景数。
pub const TWO_SCENES: usize = 1;

/// 族内退 4 的场景数。
pub const FOUR_SCENES: usize = 1;

/// 族内退 0 的场景数。
pub const ZERO_SCENES: usize = 2;

/// 统计算子号（前七格）。
pub const STAT_OPS: [u8; STAT_CELLS] = [1, 2, 3, 4, 5, 6, 7];

/// 红档格的算子号。
pub const OPID_STAT_RED: u8 = 0x08;

/// 缺陷码格的算子基号。
pub const DEFECT_OP_BASE: u8 = 0x41;

/// 一个块。
pub const BLOCK: u32 = 4096;

/// 半个块。
pub const HALF_BLOCK: u32 = 2048;

/// 沙箱前缀（探针只许落在沙箱里）。
pub const SANDBOX_PREFIX: &str = "/tmp/bx-sandbox/";

/// 源文件路径。
pub const SRC_PATH: &str = "/tmp/bx-sandbox/a/x";

/// 目的文件路径。
pub const DST_PATH: &str = "/tmp/bx-sandbox/b/y";

/// 成功退出码。
pub const EXIT_OK: u32 = 0;

/// 普通失败。
pub const EXIT_FAIL: u32 = 1;

/// 用法/麻烦。
pub const EXIT_USAGE: u32 = 2;

/// `sed` 的输入输出错。
pub const EXIT_SED: u32 = 4;

/// 未观测到退出码。
pub const EXIT_UNJUDGED: u32 = 99;

/// 退出码档数。
pub const EXIT_RANKS: usize = 4;

// ---------------------------------------------------------------------------
// 十三码
// ---------------------------------------------------------------------------

/// 槽位/用法。
pub const EC_SLOT: u32 = SEG_BASE + 0x01;
/// 成败互报。
pub const EC_ESTATE: u32 = SEG_BASE + 0x02;
/// 阶段不合。
pub const EC_STAGE: u32 = SEG_BASE + 0x03;
/// 归因码不合。
pub const EC_CODE: u32 = SEG_BASE + 0x04;
/// 数值（退出码）不合。
pub const EC_VALUE: u32 = SEG_BASE + 0x05;
/// 注记不合。
pub const EC_NOTE: u32 = SEG_BASE + 0x06;
/// 兜底档不合。
pub const EC_DEFAULT: u32 = SEG_BASE + 0x07;
/// 账本封印不合。
pub const EC_SEAL: u32 = SEG_BASE + 0x08;
/// 矩阵不合。
pub const EC_MATRIX: u32 = SEG_BASE + 0x09;
/// 一致率不合。
pub const EC_RATE: u32 = SEG_BASE + 0x0a;
/// 退出码错档。
pub const EC_EXITOFF: u32 = SEG_BASE + 0x0b;
/// errno 错档。
pub const EC_ERRNOOFF: u32 = SEG_BASE + 0x0c;
/// 告警缺失。
pub const EC_WARNOFF: u32 = SEG_BASE + 0x0d;

/// 十三码全表。
pub const ALL_CODES: [u32; 13] = [
    EC_SLOT,
    EC_ESTATE,
    EC_STAGE,
    EC_CODE,
    EC_VALUE,
    EC_NOTE,
    EC_DEFAULT,
    EC_SEAL,
    EC_MATRIX,
    EC_RATE,
    EC_EXITOFF,
    EC_ERRNOOFF,
    EC_WARNOFF,
];

/// 反例六维对应的码。
pub const REJECT_CODES: [u32; REJECT] = [
    EC_ESTATE,
    EC_STAGE,
    EC_CODE,
    EC_VALUE,
    EC_NOTE,
    EC_DEFAULT,
];

/// 正例八格统计数：族场景 / 退 1 / 退 2 / 退 4 / 退 0 / 悬空 / 一致率 /
/// 红档率（第八格必须是 `RED_RATE`）。
pub const VALS: [u32; ACCEPT] = [12, 8, 1, 1, 2, 0, 1000, 929];

// ---------------------------------------------------------------------------
// FNV-1a 指纹
// ---------------------------------------------------------------------------

/// FNV-1a 初始偏移量。
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 素数。
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一字节。
fn fold_byte(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(FNV_PRIME)
}

/// 折入一个字（小端）。
fn fold_word(h: u64, w: u64) -> u64 {
    let mut x = h;
    let mut n = w;
    let mut i = 0usize;
    while i < 8 {
        x = fold_byte(x, (n & 0xff) as u8);
        n >>= 8;
        i += 1;
    }
    x
}

/// 折入一段文本。
fn fold_str(h: u64, s: &str) -> u64 {
    let mut x = h;
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            x = fold_byte(x, *b);
        }
        i += 1;
    }
    x
}

/// 折入一个有符号数。
fn fold_i32(h: u64, v: i32) -> u64 {
    fold_word(h, v as u32 as u64)
}

/// 折入一个布尔。
fn fold_bool(h: u64, v: bool) -> u64 {
    fold_byte(h, if v { 1 } else { 0 })
}

// ---------------------------------------------------------------------------
// 文本渲染
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
        n /= 10;
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

/// `cp` 的写失败前缀。
pub const T_CP_PREFIX: &str = "cp: error writing '";
/// `sort` 的临时文件失败前缀。
pub const T_SORT_PREFIX: &str = "sort: cannot create temporary file in '";
/// `sed -i` 的写临时文件前缀。
pub const T_SED_PREFIX: &str = "sed: couldn't write ";
/// `tee` 的写失败前缀。
pub const T_TEE_PREFIX: &str = "tee: ";
/// `df` 的告警前缀。
pub const T_DF_PREFIX: &str = "df: warning: ";
/// `du` 的告警前缀。
pub const T_DU_PREFIX: &str = "du: warning: ";
/// 引号尾巴。
pub const T_QUOTE_TAIL: &str = "': ";
/// 空串。
pub const TEXT_EMPTY: &str = "";

/// 拼一条 `前缀 + 路径 + 尾巴` 的文本。
pub fn join_text(prefix: &str, path: &str, tail: &str) -> String {
    let mut s = String::from(prefix);
    s.push_str(path);
    s.push_str(tail);
    s
}

/// `cp` 的满盘行：`cp: error writing '<dst>': No space left on device`。
pub fn msg_cp_enospc(dst: &str) -> String {
    let mut s = join_text(T_CP_PREFIX, dst, T_QUOTE_TAIL);
    s.push_str(ANCHOR_ENOSPC_TEXT);
    s
}

/// `sort` 的临时文件满盘行。
pub fn msg_sort_enospc(dir: &str) -> String {
    let mut s = join_text(T_SORT_PREFIX, dir, T_QUOTE_TAIL);
    s.push_str(ANCHOR_ENOSPC_TEXT);
    s
}

/// `sed -i` 的写临时文件满盘行。
pub fn msg_sed_enospc(path: &str, bytes: u32) -> String {
    let mut s = String::from(T_SED_PREFIX);
    s.push_str(dec_text(bytes).as_str());
    s.push_str(" bytes to ");
    s.push_str(path);
    s.push_str(": ");
    s.push_str(ANCHOR_ENOSPC_TEXT);
    s
}

/// `tee` 的满盘行。
pub fn msg_tee_enospc(name: &str) -> String {
    let mut s = String::from(T_TEE_PREFIX);
    s.push_str(name);
    s.push_str(": ");
    s.push_str(ANCHOR_ENOSPC_TEXT);
    s
}

/// `df` 的告警行（退出 0，但必须说话）。
pub fn msg_df_warning(fs: &str) -> String {
    let mut s = String::from(T_DF_PREFIX);
    s.push_str("'");
    s.push_str(fs);
    s.push_str("' is full; availability reported as 0");
    s
}

/// `du` 的告警行。
pub fn msg_du_warning(dir: &str) -> String {
    let mut s = String::from(T_DU_PREFIX);
    s.push_str("cannot read '");
    s.push_str(dir);
    s.push_str("': ");
    s.push_str(ANCHOR_ENOSPC_TEXT);
    s
}

/// 用法错行。
pub fn msg_usage(tool: &str) -> String {
    let mut s = String::from(tool);
    s.push_str(": missing operand");
    s
}

/// 矩阵一格的成功呈现行。
pub fn msg_cell(tool: &str, code: u32, errno: u32) -> String {
    let mut s = String::from(tool);
    s.push_str(" exit=");
    s.push_str(dec_text(code).as_str());
    s.push_str(" errno=");
    s.push_str(dec_text(errno).as_str());
    s
}

// ---------------------------------------------------------------------------
// 闸门：七道
// ---------------------------------------------------------------------------

/// 本卡的七道闸。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// 秩 1：命令行成立。
    Check,
    /// 秩 2：源与目标可打开。
    Open,
    /// 秩 3：写数据。
    Write,
    /// 秩 4：缓冲落盘。
    Flush,
    /// 秩 5：关闭与清理。
    Close,
    /// 秩 6：errno 归属。
    Report,
    /// 秩 7：退出码。
    Exit,
}

impl Stage {
    /// 七道闸全表。
    pub const ALL: [Stage; 7] = [
        Stage::Check,
        Stage::Open,
        Stage::Write,
        Stage::Flush,
        Stage::Close,
        Stage::Report,
        Stage::Exit,
    ];

    /// 秩（1..=7）。
    pub fn rank(&self) -> u32 {
        match self {
            Stage::Check => 1,
            Stage::Open => 2,
            Stage::Write => 3,
            Stage::Flush => 4,
            Stage::Close => 5,
            Stage::Report => 6,
            Stage::Exit => 7,
        }
    }

    /// 短名。
    pub fn slug(&self) -> &'static str {
        match self {
            Stage::Check => "check",
            Stage::Open => "open",
            Stage::Write => "write",
            Stage::Flush => "flush",
            Stage::Close => "close",
            Stage::Report => "report",
            Stage::Exit => "exit",
        }
    }

    /// 中文名。
    pub fn cn(&self) -> &'static str {
        match self {
            Stage::Check => "命令行闸",
            Stage::Open => "打开闸",
            Stage::Write => "写闸",
            Stage::Flush => "落盘闸",
            Stage::Close => "清理闸",
            Stage::Report => "归因闸",
            Stage::Exit => "退出闸",
        }
    }

    /// 由秩还原。
    pub fn from_rank(r: u32) -> Option<Stage> {
        match r {
            1 => Some(Stage::Check),
            2 => Some(Stage::Open),
            3 => Some(Stage::Write),
            4 => Some(Stage::Flush),
            5 => Some(Stage::Close),
            6 => Some(Stage::Report),
            7 => Some(Stage::Exit),
            _ => None,
        }
    }

    /// 该闸是否可能产生 ENOSPC。
    pub fn may_enospc(&self) -> bool {
        matches!(*self, Stage::Write | Stage::Flush | Stage::Close)
    }
}

/// 十二个工具。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    /// `cp`
    Cp,
    /// `dd`
    Dd,
    /// `tee`
    Tee,
    /// `sort`
    Sort,
    /// `sed -i`
    SedI,
    /// `touch`
    Touch,
    /// `mkdir`
    Mkdir,
    /// `mv`
    Mv,
    /// `install`
    Install,
    /// `ln`
    Ln,
    /// `df`
    Df,
    /// `du`
    Du,
}

impl Tool {
    /// 十二工具全表。
    pub const ALL: [Tool; 12] = [
        Tool::Cp,
        Tool::Dd,
        Tool::Tee,
        Tool::Sort,
        Tool::SedI,
        Tool::Touch,
        Tool::Mkdir,
        Tool::Mv,
        Tool::Install,
        Tool::Ln,
        Tool::Df,
        Tool::Du,
    ];

    /// 短名。
    pub fn slug(&self) -> &'static str {
        match self {
            Tool::Cp => "cp",
            Tool::Dd => "dd",
            Tool::Tee => "tee",
            Tool::Sort => "sort",
            Tool::SedI => "sed-i",
            Tool::Touch => "touch",
            Tool::Mkdir => "mkdir",
            Tool::Mv => "mv",
            Tool::Install => "install",
            Tool::Ln => "ln",
            Tool::Df => "df",
            Tool::Du => "du",
        }
    }

    /// 由短名还原。
    pub fn from_slug(s: &str) -> Option<Tool> {
        let table = Tool::ALL;
        let mut i = 0usize;
        while i < TOOL_COUNT {
            if let Some(t) = table.get(i) {
                if t.slug() == s {
                    return Some(*t);
                }
            }
            i += 1;
        }
        None
    }

    /// 该工具在满盘上的退出码。
    pub const fn code(&self) -> u32 {
        match self {
            Tool::Cp => EXIT_FAIL,
            Tool::Dd => EXIT_FAIL,
            Tool::Tee => EXIT_FAIL,
            Tool::Sort => EXIT_USAGE,
            Tool::SedI => EXIT_SED,
            Tool::Touch => EXIT_FAIL,
            Tool::Mkdir => EXIT_FAIL,
            Tool::Mv => EXIT_FAIL,
            Tool::Install => EXIT_FAIL,
            Tool::Ln => EXIT_FAIL,
            Tool::Df => EXIT_OK,
            Tool::Du => EXIT_OK,
        }
    }

    /// 该工具在满盘上的 errno（只读工具为 0）。
    pub const fn errno(&self) -> u32 {
        match self {
            Tool::Df => 0,
            Tool::Du => 0,
            _ => ANCHOR_ENOSPC,
        }
    }

    /// 该工具退出时是否必须带告警。
    pub const fn warning(&self) -> bool {
        matches!(*self, Tool::Df | Tool::Du)
    }

    /// 该工具是否写数据。
    pub fn writes_data(&self) -> bool {
        !matches!(*self, Tool::Df | Tool::Du)
    }

    /// 该工具是否只读。
    pub fn read_only(&self) -> bool {
        !self.writes_data()
    }

    /// 该工具的退出码档位文本。
    pub fn bucket_slug(&self) -> &'static str {
        match self.code() {
            EXIT_FAIL => "one",
            EXIT_USAGE => "two",
            EXIT_SED => "four",
            _ => "zero",
        }
    }
}

// ---------------------------------------------------------------------------
// 注记与判维
// ---------------------------------------------------------------------------

/// 矩阵一格的收场注记。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Note {
    /// 退 1 档（普通失败）。
    One,
    /// 退 2 档（麻烦/部分完成）。
    Two,
    /// 退 4 档（输入输出错）。
    Four,
    /// 退 0 档（只读工具，必须带告警）。
    Zero,
    /// 退出码错档。
    ExitOff,
    /// errno 错档。
    ErrnoOff,
    /// 告警缺失。
    WarnOff,
    /// 用法错。
    Usage,
    /// 未判档。
    Nil,
}

impl Note {
    /// 九档全表。
    pub const ALL: [Note; 9] = [
        Note::One,
        Note::Two,
        Note::Four,
        Note::Zero,
        Note::ExitOff,
        Note::ErrnoOff,
        Note::WarnOff,
        Note::Usage,
        Note::Nil,
    ];

    /// 短名。
    pub fn slug(&self) -> &'static str {
        match self {
            Note::One => "exit-one",
            Note::Two => "exit-two",
            Note::Four => "exit-four",
            Note::Zero => "exit-zero",
            Note::ExitOff => "exit-off",
            Note::ErrnoOff => "errno-off",
            Note::WarnOff => "warn-off",
            Note::Usage => "usage",
            Note::Nil => "nil",
        }
    }

    /// 中文名。
    pub fn cn(&self) -> &'static str {
        match self {
            Note::One => "退一档",
            Note::Two => "退二档",
            Note::Four => "退四档",
            Note::Zero => "退零档",
            Note::ExitOff => "码错档",
            Note::ErrnoOff => "errno 错档",
            Note::WarnOff => "告警缺失",
            Note::Usage => "用法错",
            Note::Nil => "未判",
        }
    }

    /// 秩。
    pub fn rank(&self) -> u32 {
        match self {
            Note::One => 1,
            Note::Two => 2,
            Note::Four => 3,
            Note::Zero => 4,
            Note::ExitOff => 5,
            Note::ErrnoOff => 6,
            Note::WarnOff => 7,
            Note::Usage => 8,
            Note::Nil => 9,
        }
    }

    /// 由秩还原。
    pub fn from_rank(r: u32) -> Option<Note> {
        match r {
            1 => Some(Note::One),
            2 => Some(Note::Two),
            3 => Some(Note::Four),
            4 => Some(Note::Zero),
            5 => Some(Note::ExitOff),
            6 => Some(Note::ErrnoOff),
            7 => Some(Note::WarnOff),
            8 => Some(Note::Usage),
            9 => Some(Note::Nil),
            _ => None,
        }
    }

    /// 由退出码得档位注记。
    pub fn of_exit(code: u32) -> Note {
        match code {
            EXIT_FAIL => Note::One,
            EXIT_USAGE => Note::Two,
            EXIT_SED => Note::Four,
            _ => Note::Zero,
        }
    }

    /// 是否是矩阵内的正常档。
    pub fn is_cell(&self) -> bool {
        matches!(*self, Note::One | Note::Two | Note::Four | Note::Zero)
    }

    /// 是否是错档。
    pub fn is_off(&self) -> bool {
        matches!(*self, Note::ExitOff | Note::ErrnoOff | Note::WarnOff)
    }
}

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
            Fault::Estate => EC_ESTATE,
            Fault::Stage => EC_STAGE,
            Fault::Code => EC_CODE,
            Fault::Value => EC_VALUE,
            Fault::Note => EC_NOTE,
            Fault::Default => EC_DEFAULT,
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
            Fault::Ok => "两档全合",
            Fault::Estate => "失败被报成成功，或成功被报成失败",
            Fault::Stage => "清理闸的事实被记到退出闸",
            Fault::Code => "错档已被判死，申报却报无码",
            Fault::Value => "退出码数值与现场差一档",
            Fault::Note => "退二与退四互认，档位串味",
            Fault::Default => "可判定的格子被丢进未判档",
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
// 四档：退一 / 退二 / 退四 / 退零
// ---------------------------------------------------------------------------

/// 收场四档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    /// 退出码 1。
    ExitOne,
    /// 退出码 2。
    ExitTwo,
    /// 退出码 4。
    ExitFour,
    /// 退出码 0（必须带告警）。
    ExitZero,
}

impl Tier {
    /// 四档全表。
    pub const ALL: [Tier; 4] = [
        Tier::ExitOne,
        Tier::ExitTwo,
        Tier::ExitFour,
        Tier::ExitZero,
    ];

    /// 秩。
    pub fn rank(&self) -> u32 {
        match self {
            Tier::ExitOne => 1,
            Tier::ExitTwo => 2,
            Tier::ExitFour => 3,
            Tier::ExitZero => 4,
        }
    }

    /// 短名。
    pub fn slug(&self) -> &'static str {
        match self {
            Tier::ExitOne => "one",
            Tier::ExitTwo => "two",
            Tier::ExitFour => "four",
            Tier::ExitZero => "zero",
        }
    }

    /// 中文名。
    pub fn cn(&self) -> &'static str {
        match self {
            Tier::ExitOne => "退一档",
            Tier::ExitTwo => "退二档",
            Tier::ExitFour => "退四档",
            Tier::ExitZero => "退零档",
        }
    }

    /// 该档对应的退出码。
    pub fn code(&self) -> u32 {
        match self {
            Tier::ExitOne => EXIT_FAIL,
            Tier::ExitTwo => EXIT_USAGE,
            Tier::ExitFour => EXIT_SED,
            Tier::ExitZero => EXIT_OK,
        }
    }

    /// 由秩还原。
    pub fn from_rank(r: u32) -> Option<Tier> {
        match r {
            1 => Some(Tier::ExitOne),
            2 => Some(Tier::ExitTwo),
            3 => Some(Tier::ExitFour),
            4 => Some(Tier::ExitZero),
            _ => None,
        }
    }

    /// 绿档只有前三档：退 0 必须带告警，否则是悬空。
    pub fn is_green(&self) -> bool {
        matches!(*self, Tier::ExitOne | Tier::ExitTwo | Tier::ExitFour)
    }

    /// 由一次结果归档：按观测到的退出码分档。
    pub fn of(o: &Outcome) -> Tier {
        if o.default || o.note == Note::Nil || o.note == Note::Usage {
            return Tier::ExitZero;
        }
        match o.value {
            EXIT_FAIL => Tier::ExitOne,
            EXIT_USAGE => Tier::ExitTwo,
            EXIT_SED => Tier::ExitFour,
            _ => Tier::ExitZero,
        }
    }
}

// ---------------------------------------------------------------------------
// 域模型：Shape / Outcome / Claim / Case
// ---------------------------------------------------------------------------

/// 矩阵一格的完整现场。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Shape {
    /// 工具。
    pub tool: Tool,
    /// 观测到的 errno。
    pub errno: u32,
    /// 观测到的退出码。
    pub exit: u32,
    /// 是否打印了告警。
    pub warning: bool,
    /// 命令行是否非法。
    pub bad_call: bool,
    /// 兜底档。
    pub default: bool,
}

impl Shape {
    /// 一格"全对"的现场（退出码与 errno 都按工具表）。
    pub const fn cell(tool: Tool) -> Shape {
        Shape {
            tool,
            errno: tool.errno(),
            exit: tool.code(),
            warning: tool.warning(),
            bad_call: false,
            default: false,
        }
    }

    /// 一格退出码错档的现场。
    pub fn wrong_exit(tool: Tool, exit: u32) -> Shape {
        Shape {
            tool,
            errno: tool.errno(),
            exit,
            warning: tool.warning(),
            bad_call: false,
            default: false,
        }
    }

    /// 一格 errno 错档的现场。
    pub fn wrong_errno(tool: Tool, errno: u32) -> Shape {
        Shape {
            tool,
            errno,
            exit: tool.code(),
            warning: tool.warning(),
            bad_call: false,
            default: false,
        }
    }

    /// 一格漏告警的现场。
    pub fn silent(tool: Tool) -> Shape {
        Shape {
            tool,
            errno: tool.errno(),
            exit: tool.code(),
            warning: false,
            bad_call: false,
            default: false,
        }
    }

    /// 兜底型。
    pub const fn unjudged(tool: Tool) -> Shape {
        Shape {
            tool,
            errno: tool.errno(),
            exit: EXIT_UNJUDGED,
            warning: tool.warning(),
            bad_call: false,
            default: true,
        }
    }

    /// 该格是否真的撞了满盘。
    pub fn enospc(&self) -> bool {
        self.errno == ANCHOR_ENOSPC
    }

    /// 该格是否是"退 0 且无告警"的悬空。
    pub fn dangling(&self) -> bool {
        self.exit == EXIT_OK && !self.warning
    }

    /// 域合法性。
    pub fn domain_ok(&self) -> bool {
        if self.errno > 64 {
            return false;
        }
        if self.bad_call {
            return self.exit == EXIT_USAGE && !self.default;
        }
        if self.tool.read_only() {
            return self.errno == 0;
        }
        if self.errno == 0 && self.exit != EXIT_UNJUDGED {
            return false;
        }
        if self.exit == EXIT_UNJUDGED {
            return self.default;
        }
        true
    }

    /// 形状指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_str(h, self.tool.slug());
        h = fold_word(h, self.errno as u64);
        h = fold_word(h, self.exit as u64);
        h = fold_bool(h, self.warning);
        h = fold_bool(h, self.bad_call);
        h = fold_bool(h, self.default);
        h
    }
}

/// 事实面。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Outcome {
    /// 纵线落点阶段。
    pub stage: Stage,
    /// 归因码（0 = 无违诺）。
    pub code: u32,
    /// 观测到的退出码。
    pub value: u32,
    /// 收场注记。
    pub note: Note,
    /// 兜底档。
    pub default: bool,
    /// 该格是否判对。
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

    /// 是否悬空（退 0 无告警或未判）。
    pub fn dangling(&self) -> bool {
        self.default || self.note == Note::Nil || self.note == Note::WarnOff
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
        s.push_str(" exit=");
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
    /// 申报的退出码。
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
// 本卡的不变量：一格的收场
// ---------------------------------------------------------------------------

/// 由现场推出事实面。
///
/// 四步，次序即闸门：用法 → 兜底 → 退出码 → errno → 告警。
/// 退出码先于 errno：一格"码对了、errno 错了"与"errno 对了、码错了"是两
/// 种不同的红，前者落在退出闸，后者落在归因闸。
pub fn define(s: &Shape) -> Outcome {
    // 闸 1 Check：用法错一律退 2，且不碰盘。
    if s.bad_call {
        return Outcome::breach(Stage::Check, EC_SLOT, EXIT_USAGE, Note::Usage, false);
    }
    // 兜底档：可判定的格子被丢进未判。
    if s.default {
        return Outcome::new(Stage::Report, 0, EXIT_UNJUDGED, Note::Nil, true, false);
    }
    // 闸 7 Exit：退出码与工具表不合。
    if s.exit != s.tool.code() {
        return Outcome::breach(Stage::Exit, EC_EXITOFF, s.exit, Note::ExitOff, false);
    }
    // 闸 6 Report：errno 与工具表不合。
    if s.errno != s.tool.errno() {
        return Outcome::breach(Stage::Report, EC_ERRNOOFF, s.exit, Note::ErrnoOff, false);
    }
    // 闸 5 Close：只读工具的 0 退出必须带告警。
    if s.warning != s.tool.warning() {
        return Outcome::breach(Stage::Close, EC_WARNOFF, s.exit, Note::WarnOff, false);
    }
    // 两档全合。
    Outcome::pass(Stage::Exit, s.exit, Note::of_exit(s.exit))
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
        None => Shape::cell(Tool::Cp),
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

/// 覆盖校验。
pub fn cover(n: usize) -> Result<(), u32> {
    if n > CORPUS {
        return Err(EC_SLOT);
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

/// 槽 8 的现场：`cp` 报 ENOSPC 却退出 0（伪成功）。
const SHAPE_CP_ZERO: Shape = Shape {
    tool: Tool::Cp,
    errno: ANCHOR_ENOSPC,
    exit: EXIT_OK,
    warning: false,
    bad_call: false,
    default: false,
};

/// 槽 10 的现场：`tee` 报的是 EIO 而不是 ENOSPC。
const SHAPE_TEE_EIO: Shape = Shape {
    tool: Tool::Tee,
    errno: ANCHOR_EIO,
    exit: EXIT_FAIL,
    warning: false,
    bad_call: false,
    default: false,
};

/// 十四槽现场。
pub const SHAPES: [Shape; CORPUS] = [
    Shape {
        tool: Tool::Cp,
        errno: ANCHOR_ENOSPC,
        exit: EXIT_FAIL,
        warning: false,
        bad_call: false,
        default: false,
    },
    Shape {
        tool: Tool::Dd,
        errno: ANCHOR_ENOSPC,
        exit: EXIT_FAIL,
        warning: false,
        bad_call: false,
        default: false,
    },
    Shape {
        tool: Tool::Tee,
        errno: ANCHOR_ENOSPC,
        exit: EXIT_FAIL,
        warning: false,
        bad_call: false,
        default: false,
    },
    Shape {
        tool: Tool::Sort,
        errno: ANCHOR_ENOSPC,
        exit: EXIT_USAGE,
        warning: false,
        bad_call: false,
        default: false,
    },
    Shape {
        tool: Tool::SedI,
        errno: ANCHOR_ENOSPC,
        exit: EXIT_SED,
        warning: false,
        bad_call: false,
        default: false,
    },
    Shape {
        tool: Tool::Touch,
        errno: ANCHOR_ENOSPC,
        exit: EXIT_FAIL,
        warning: false,
        bad_call: false,
        default: false,
    },
    Shape {
        tool: Tool::Df,
        errno: 0,
        exit: EXIT_OK,
        warning: true,
        bad_call: false,
        default: false,
    },
    Shape {
        tool: Tool::Du,
        errno: 0,
        exit: EXIT_OK,
        warning: true,
        bad_call: false,
        default: false,
    },
    SHAPE_CP_ZERO,
    Shape::cell(Tool::Dd),
    SHAPE_TEE_EIO,
    Shape::cell(Tool::Sort),
    Shape::cell(Tool::SedI),
    Shape::unjudged(Tool::Mkdir),
];

/// 十四格申报。前八格与 `define(SHAPES[i])` 逐维相同，后六格各坏一维。
pub const CLAIMS: [Claim; CORPUS] = [
    Claim::new("cp-exit-one", true, Stage::Exit, 0, 1, Note::One, false),
    Claim::new("dd-exit-one", true, Stage::Exit, 0, 1, Note::One, false),
    Claim::new("tee-exit-one", true, Stage::Exit, 0, 1, Note::One, false),
    Claim::new("sort-exit-two", true, Stage::Exit, 0, 2, Note::Two, false),
    Claim::new("sed-exit-four", true, Stage::Exit, 0, 4, Note::Four, false),
    Claim::new("touch-exit-one", true, Stage::Exit, 0, 1, Note::One, false),
    Claim::new("df-exit-zero", true, Stage::Exit, 0, 0, Note::Zero, false),
    Claim::new("du-exit-zero", true, Stage::Exit, 0, 0, Note::Zero, false),
    // 死 1 estate-swap：事实是伪成功（有 ENOSPC 却退 0），申报却报成功。
    Claim::new(
        "estate-swap",
        true,
        Stage::Exit,
        EC_EXITOFF,
        0,
        Note::ExitOff,
        false,
    ),
    // 死 2 stage-skip：dd 两档全对，申报却说归因闸才收场。
    Claim::new("stage-skip", true, Stage::Report, 0, 1, Note::One, false),
    // 死 3 code-swallow：EIO 已被判 errno 错档，申报却报无码。
    Claim::new(
        "code-swallow",
        false,
        Stage::Report,
        0,
        1,
        Note::ErrnoOff,
        false,
    ),
    // 死 4 value-off：sort 退 2，申报退 1。
    Claim::new("value-off", true, Stage::Exit, 0, 1, Note::Two, false),
    // 死 5 note-swap：sed 退 4，申报成退 2 档。
    Claim::new("note-swap", true, Stage::Exit, 0, 4, Note::Two, false),
    // 死 6 default-bucket：可判定的格子被丢进未判档。
    Claim::new(
        "default-bucket",
        false,
        Stage::Report,
        0,
        EXIT_UNJUDGED,
        Note::Nil,
        false,
    ),
];

// ---------------------------------------------------------------------------
// 期望表
// ---------------------------------------------------------------------------

/// 期望阶段。
pub const WANT_STAGE: [Stage; CORPUS] = [
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
    Stage::Report,
    Stage::Exit,
    Stage::Exit,
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
    0,
    0,
    EC_EXITOFF,
    0,
    EC_ERRNOOFF,
    0,
    0,
    0,
];

/// 期望缺陷维序位。
pub const WANT_NDX: [usize; CORPUS] = [6, 6, 6, 6, 6, 6, 6, 6, 0, 1, 2, 3, 4, 5];

/// 期望注记。
pub const WANT_NOTE: [Note; CORPUS] = [
    Note::One,
    Note::One,
    Note::One,
    Note::Two,
    Note::Four,
    Note::One,
    Note::Zero,
    Note::Zero,
    Note::ExitOff,
    Note::One,
    Note::ErrnoOff,
    Note::Two,
    Note::Four,
    Note::Nil,
];

/// 期望兜底位。
pub const WANT_DEFAULT: [bool; CORPUS] = [
    false, false, false, false, false, false, false, false, false, false, false, false, false, true,
];

/// 期望短名。
pub const WANT_TAGS: [&str; CORPUS] = [
    "cp-exit-one",
    "dd-exit-one",
    "tee-exit-one",
    "sort-exit-two",
    "sed-exit-four",
    "touch-exit-one",
    "df-exit-zero",
    "du-exit-zero",
    "estate-swap",
    "stage-skip",
    "code-swallow",
    "value-off",
    "note-swap",
    "default-bucket",
];

/// 期望退出码。
pub const WANT_VALUE: [u32; CORPUS] = [1, 1, 1, 2, 4, 1, 0, 0, 0, 1, 1, 2, 4, 99];

/// 期望成败。
pub const WANT_SUCCESS: [bool; CORPUS] = [
    true, true, true, true, true, true, true, true, false, true, false, true, true, false,
];

/// 期望现场退出码。
pub const WANT_EXIT: [u32; CORPUS] = [1, 1, 1, 2, 4, 1, 0, 0, 0, 1, 1, 2, 4, 99];

// ---------------------------------------------------------------------------
// 族内场景：十二个工具各一格
// ---------------------------------------------------------------------------

/// 族内统一种子。
pub const FAMILY_SEED: u32 = 0x6873_0001;

/// 第 `i` 条场景（12 条，一条一个工具）。
pub fn scene_at(i: usize) -> Option<Shape> {
    let table = Tool::ALL;
    match table.get(i) {
        Some(t) => Some(Shape::cell(*t)),
        None => None,
    }
}

/// 全族场景。
pub fn scenes() -> [Shape; FAMILY_SCENES] {
    let mut out = [Shape::cell(Tool::Cp); FAMILY_SCENES];
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

/// 该场景是否落进自己的档。
pub fn scene_anchored(i: usize) -> Option<bool> {
    match scene_at(i) {
        Some(s) => Some(Tier::of(&define(&s)).code() == s.exit),
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

/// 族内每条场景都必须自洽且不悬空。
pub fn scenes_agree() -> bool {
    let mut i = 0usize;
    while i < FAMILY_SCENES {
        let c = match scene_case(i) {
            Some(x) => x,
            None => return false,
        };
        if judge(&c) != Fault::Ok {
            return false;
        }
        if c.actual().ok() != c.claim.success() {
            return false;
        }
        if c.actual().dangling() {
            return false;
        }
        if !c.actual().note.is_cell() {
            return false;
        }
        i += 1;
    }
    true
}

/// 族场景按档计数。
pub fn scene_counts() -> (usize, usize, usize, usize) {
    let mut one = 0usize;
    let mut two = 0usize;
    let mut four = 0usize;
    let mut zero = 0usize;
    let mut i = 0usize;
    while i < FAMILY_SCENES {
        if let Some(s) = scene_at(i) {
            match Tier::of(&define(&s)) {
                Tier::ExitOne => one += 1,
                Tier::ExitTwo => two += 1,
                Tier::ExitFour => four += 1,
                Tier::ExitZero => zero += 1,
            }
        }
        i += 1;
    }
    (one, two, four, zero)
}

/// 族内悬空格数（必须为 0）。
pub fn scene_dangling_count() -> usize {
    let mut n = 0usize;
    let mut i = 0usize;
    while i < FAMILY_SCENES {
        if let Some(s) = scene_at(i) {
            if define(&s).dangling() {
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

/// 联签锚自洽。
pub fn anchors_agree() -> bool {
    if ANCHOR_F11697_SEG.checked_add(ANCHOR_F11697_GAP) != Some(SEG_BASE) {
        return false;
    }
    if SEG_BASE.checked_add(ANCHOR_F11699_GAP) != Some(ANCHOR_F11699_SEG) {
        return false;
    }
    if ANCHOR_F11694_SEG.checked_add(ANCHOR_F11694_GAP) != Some(SEG_BASE) {
        return false;
    }
    if ANCHOR_F11687_SEG.checked_add(ANCHOR_F11687_GAP) != Some(SEG_BASE) {
        return false;
    }
    ANCHOR_ENOSPC == 28
        && ANCHOR_EIO == 5
        && ANCHOR_ENOSPC_TEXT == "No space left on device"
        && ANCHOR_EIO_TEXT == "Input/output error"
}

// ---------------------------------------------------------------------------
// 档位与矩阵
// ---------------------------------------------------------------------------

/// 一档的统计行。
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

/// 四档矩阵：横看语料槽，纵看退出码档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Matrix {
    rows: [Row; TIER_COUNT],
}

impl Matrix {
    /// 由语料构造。
    pub fn build() -> Matrix {
        let table = Tier::ALL;
        let mut rows = [Row {
            tier: Tier::ExitOne,
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

/// 全绿渲染文本（逐字钉住）。
pub const RENDER_OK: &str = "exit corpus=14 accept=8 reject=6 one=8 two=1 four=1 zero=2 dangling=0 match=12 rate=1000permille\n";

/// 一次退出码矩阵结算报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Report {
    /// 结算分母。
    pub total: usize,
    /// 正例格数。
    pub accept: usize,
    /// 反例格数。
    pub reject: usize,
    /// 退 1 场景数。
    pub one: usize,
    /// 退 2 场景数。
    pub two: usize,
    /// 退 4 场景数。
    pub four: usize,
    /// 退 0 场景数。
    pub zero: usize,
    /// 计入分子的场景数。
    pub matched: usize,
    /// 违约计数。
    pub conflicts: usize,
    /// 悬空格数。
    pub dangling: usize,
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
        self.conflicts == 0
            && self.dangling == 0
            && self.rate() == RATE_FULL
            && self.accept == ACCEPT
            && self.reject == REJECT
            && self.one + self.two + self.four + self.zero == FAMILY_SCENES
            && self.matched == FAMILY_SCENES
    }

    /// 红项计数。
    pub fn reds(&self) -> u32 {
        (self.conflicts as u32) + (self.dangling as u32)
    }

    /// 闸门。
    pub fn gate(&self) -> Result<(), u32> {
        if self.total == 0 {
            return Err(EC_MATRIX);
        }
        if self.dangling > 0 {
            return Err(EC_WARNOFF);
        }
        if self.conflicts > 0 {
            return Err(EC_NOTE);
        }
        if self.rate() < RATE_FULL {
            return Err(EC_RATE);
        }
        if self.one + self.two + self.four + self.zero != FAMILY_SCENES {
            return Err(EC_MATRIX);
        }
        Ok(())
    }

    /// 四档格数。
    pub fn tier_cells(&self) -> u32 {
        (self.one as u32) + (self.two as u32) + (self.four as u32) + (self.zero as u32)
    }

    /// 红档格数。
    pub fn red_cells(&self) -> u32 {
        (self.dangling as u32) + (self.conflicts as u32)
    }

    /// 逐字渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("exit corpus=");
        s.push_str(dec_text((self.accept + self.reject) as u32).as_str());
        s.push_str(" accept=");
        s.push_str(dec_text(self.accept as u32).as_str());
        s.push_str(" reject=");
        s.push_str(dec_text(self.reject as u32).as_str());
        s.push_str(" one=");
        s.push_str(dec_text(self.one as u32).as_str());
        s.push_str(" two=");
        s.push_str(dec_text(self.two as u32).as_str());
        s.push_str(" four=");
        s.push_str(dec_text(self.four as u32).as_str());
        s.push_str(" zero=");
        s.push_str(dec_text(self.zero as u32).as_str());
        s.push_str(" dangling=");
        s.push_str(dec_text(self.dangling as u32).as_str());
        s.push_str(" match=");
        s.push_str(dec_text(self.matched as u32).as_str());
        s.push_str(" rate=");
        s.push_str(dec_text(self.rate()).as_str());
        s.push_str("permille\n");
        s
    }
}

/// 空报告。
pub fn empty_report() -> Report {
    Report {
        total: 0,
        accept: 0,
        reject: 0,
        one: 0,
        two: 0,
        four: 0,
        zero: 0,
        matched: 0,
        conflicts: 0,
        dangling: 0,
    }
}

/// 两路结算。
fn settle(over: Option<(usize, Fault)>) -> Report {
    let mut rep = Report {
        total: FAMILY_SCENES,
        accept: ACCEPT,
        reject: REJECT,
        one: 0,
        two: 0,
        four: 0,
        zero: 0,
        matched: 0,
        conflicts: 0,
        dangling: 0,
    };
    let mut i = 0usize;
    while i < FAMILY_SCENES {
        let s = match scene_at(i) {
            Some(x) => x,
            None => return empty_report(),
        };
        let o = define(&s);
        match Tier::of(&o) {
            Tier::ExitOne => rep.one += 1,
            Tier::ExitTwo => rep.two += 1,
            Tier::ExitFour => rep.four += 1,
            Tier::ExitZero => rep.zero += 1,
        }
        if o.ok() {
            rep.matched += 1;
        } else {
            rep.conflicts += 1;
        }
        if o.dangling() {
            rep.dangling += 1;
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

/// 主结算。
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
            return Err(EC_SLOT);
        }
    }
    Ok(settle(over))
}

/// 报告与矩阵互相印证。
pub fn report_agrees_with_matrix() -> bool {
    let rep = settle(None);
    let m = Matrix::build();
    let one = match m.row(Tier::ExitOne) {
        Some(r) => r.good,
        None => 0,
    };
    let zero = match m.row(Tier::ExitZero) {
        Some(r) => r.good,
        None => 0,
    };
    rep.one == ONE_SCENES && rep.zero == ZERO_SCENES && one > 0 && zero > 0
}

/// 红档率标定。
pub fn red_rate_calibrated() -> bool {
    let base = settle(None);
    let bad = match run_with(Some((3, Fault::Value))) {
        Ok(r) => r,
        Err(_) => return false,
    };
    if bad.matched >= base.matched {
        return false;
    }
    RED_RATE == 929
}

// ---------------------------------------------------------------------------
// 阶段闸门：Check→Open→Write→Flush→Close→Report→Exit
// ---------------------------------------------------------------------------

/// 一次矩阵结算的走闸状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Gate {
    current: Option<Stage>,
    closed: bool,
    steps: u32,
    refusals: u32,
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
            return Err(EC_ESTATE);
        }
        if let Some(cur) = self.current {
            if s.rank() < cur.rank() {
                self.refusals += 1;
                return Err(EC_STAGE);
            }
        }
        self.root = fold_word(self.root, s.rank() as u64);
        self.root = fold_str(self.root, s.slug());
        self.current = Some(s);
        self.steps += 1;
        Ok(())
    }

    /// 封账。
    pub fn close(&mut self) -> Result<u64, u32> {
        if self.closed {
            self.refusals += 1;
            return Err(EC_ESTATE);
        }
        if self.current.is_none() {
            return Err(EC_STAGE);
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
                None => return Err(EC_SLOT),
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
                None => return Err(EC_SLOT),
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
// 账本：逐格登记矩阵统计量与缺陷码
// ---------------------------------------------------------------------------

/// 一格台账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sample {
    /// 槽号。
    pub slot: u8,
    /// 算子号。
    pub op: u8,
    /// 成功还是败。
    pub ok: bool,
    /// 错误码（成功格必须为 0）。
    pub code: u32,
    /// 统计值（败格必须为 0）。
    pub value: u32,
}

impl Sample {
    /// 指纹。
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

/// 十四格账本 = 七统计 + 一红档 + 六缺陷码。
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
            return Err(EC_ESTATE);
        }
        if self.len >= SAMPLES {
            self.rejects += 1;
            return Err(EC_SLOT);
        }
        if s.slot as usize != self.len {
            self.rejects += 1;
            return Err(EC_SLOT);
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

    /// 冻结后的指纹。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 逐格复核。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES {
            return Err(EC_SLOT);
        }
        let stat_last = match STAT_CELLS.checked_add(1) {
            Some(x) => x,
            None => return Err(EC_SLOT),
        };
        let mut i = 0usize;
        while i < stat_last {
            let s = match self.sample(i) {
                Some(x) => x,
                None => return Err(EC_SLOT),
            };
            if !s.ok {
                return Err(EC_ESTATE);
            }
            if s.code != 0 {
                return Err(EC_CODE);
            }
            let want_value = match VALS.get(i) {
                Some(v) => *v,
                None => return Err(EC_SLOT),
            };
            if s.value != want_value {
                return Err(EC_VALUE);
            }
            let want_op = match self.stat_op_want(i) {
                Some(v) => v,
                None => return Err(EC_SLOT),
            };
            if s.op != want_op {
                return Err(EC_CODE);
            }
            i += 1;
        }
        let mut j = 0usize;
        while j < REJECT {
            let cell = match STAT_CELLS.checked_add(1).and_then(|x| x.checked_add(j)) {
                Some(x) => x,
                None => return Err(EC_SLOT),
            };
            let s = match self.sample(cell) {
                Some(x) => x,
                None => return Err(EC_SLOT),
            };
            if s.ok {
                return Err(EC_ESTATE);
            }
            if s.value != 0 {
                return Err(EC_VALUE);
            }
            let want_code = match REJECT_CODES.get(j) {
                Some(c) => *c,
                None => return Err(EC_SLOT),
            };
            if s.code != want_code {
                return Err(EC_CODE);
            }
            let want_op = match DEFECT_OP_BASE.checked_add(j as u8) {
                Some(x) => x,
                None => return Err(EC_SLOT),
            };
            if s.op != want_op {
                return Err(EC_CODE);
            }
            let mut k = j + 1usize;
            while k < REJECT {
                if let Some(other) = REJECT_CODES.get(k) {
                    if *other == want_code {
                        return Err(EC_CODE);
                    }
                }
                k += 1;
            }
            j += 1;
        }
        Ok(())
    }

    /// 第一处不合规的格子与原因。
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

    /// 冻结：先复核再折叠。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(EC_ESTATE);
        }
        match self.audit() {
            Ok(()) => {}
            Err(code) => return Err(code),
        }
        let h = self.seal();
        if h == 0 {
            return Err(EC_SEAL);
        }
        self.root = h;
        self.frozen = true;
        Ok(h)
    }

    /// 重放：以同一批格子重建账本，比指纹。
    pub fn replay(&self) -> Result<u64, u32> {
        if !self.frozen {
            return Err(EC_ESTATE);
        }
        let mut fresh = Ledger::new();
        let mut i = 0usize;
        while i < self.len {
            let s = match self.sample(i) {
                Some(x) => x,
                None => return Err(EC_SEAL),
            };
            match fresh.register(s) {
                Ok(()) => {}
                Err(_) => return Err(EC_SEAL),
            }
            i += 1;
        }
        let h = fresh.seal();
        if h != self.root {
            return Err(EC_SEAL);
        }
        Ok(h)
    }

    /// 改动一格（仅未冻结时允许）。
    pub fn poke(&mut self, i: usize, value: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(EC_ESTATE);
        }
        match self.slots.get_mut(i) {
            Some(cell) => {
                match *cell {
                    Some(ref mut s) => {
                        s.value = value;
                    }
                    None => return Err(EC_SLOT),
                }
                Ok(())
            }
            None => Err(EC_SLOT),
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

/// 十四格标准台账。
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

/// 注入一维错的台账。
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

/// judge 的 Estate 优先性自检。
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
        EC_SLOT,
        a.value.wrapping_add(7),
        Note::WarnOff,
        true,
    );
    let c = Case::new(s, wrong);
    judge(&c) == Fault::Estate
}

/// 申报表第一格的标签。
pub fn head_tag() -> &'static str {
    match WANT_TAGS.get(0) {
        Some(x) => *x,
        None => "cp-exit-one",
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
// 十二条判据谓词（全部私有，只在 run_unx11698_checks 里汇总）
// ---------------------------------------------------------------------------

/// 01 名称与段基址：常量层自洽。
fn name_ok() -> bool {
    if SEG_BASE != 0x6d420 {
        return false;
    }
    if CORPUS != SAMPLES || ACCEPT + REJECT != CORPUS {
        return false;
    }
    if TIER_COUNT != 4 || EXIT_RANKS != 4 {
        return false;
    }
    if STAGE_COUNT as usize != Stage::ALL.len() || STAGE_LAST_RANK != 7 {
        return false;
    }
    if STAT_CELLS + 1 != ACCEPT || STAT_CELLS + 1 + REJECT != SAMPLES {
        return false;
    }
    if RED_RATE != 929 || RATE_FULL != 1000 || MATCH_FULL != 1000 {
        return false;
    }
    if FAMILY_SCENES != ONE_SCENES + TWO_SCENES + FOUR_SCENES + ZERO_SCENES {
        return false;
    }
    if ONE_SCENES != 8 || TWO_SCENES != 1 || FOUR_SCENES != 1 || ZERO_SCENES != 2 {
        return false;
    }
    if TOOL_COUNT != Tool::ALL.len() || TOOL_COUNT != 12 {
        return false;
    }
    if WRITE_TOOLS + WARN_TOOLS != TOOL_COUNT {
        return false;
    }
    if !SANDBOX_PREFIX.starts_with("/tmp/bx-sandbox/") {
        return false;
    }
    if EXIT_OK != 0 || EXIT_FAIL != 1 || EXIT_USAGE != 2 || EXIT_SED != 4 {
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
        if f.code() != c || REJECT_FAULTS.get(k) != Some(&f) {
            return false;
        }
        if fault_ndx(f) != k || ndx_fault(k) != Some(f) {
            return false;
        }
        k += 1;
    }
    Fault::Ok.code() == 0 && ndx_fault(6) == Some(Fault::Ok)
}

/// 03 工具码表：十二个工具各自的退出码与 errno 必须成表。
fn tools_ok() -> bool {
    let mut writes = 0usize;
    let mut warns = 0usize;
    let mut i = 0usize;
    while i < Tool::ALL.len() {
        let t = match Tool::ALL.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let code = t.code();
        if code != EXIT_OK && code != EXIT_FAIL && code != EXIT_USAGE && code != EXIT_SED {
            return false;
        }
        let bucket = match code {
            EXIT_FAIL => "one",
            EXIT_USAGE => "two",
            EXIT_SED => "four",
            _ => "zero",
        };
        if t.bucket_slug() != bucket {
            return false;
        }
        if t.writes_data() {
            writes += 1;
            if t.errno() != ANCHOR_ENOSPC {
                return false;
            }
            if t.warning() {
                return false;
            }
        } else {
            warns += 1;
            if t.errno() != 0 || !t.warning() {
                return false;
            }
        }
        if t.writes_data() == t.read_only() {
            return false;
        }
        if Tool::from_slug(t.slug()) != Some(t) {
            return false;
        }
        let mut j = i + 1usize;
        while j < Tool::ALL.len() {
            if let Some(o) = Tool::ALL.get(j) {
                if o.slug() == t.slug() {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    writes == WRITE_TOOLS && warns == WARN_TOOLS
}

/// 04 十二格两档：每一格的退出码档与 errno 档都必须判到，不许悬空。
fn cells_ok() -> bool {
    let mut one = 0usize;
    let mut two = 0usize;
    let mut four = 0usize;
    let mut zero = 0usize;
    let mut i = 0usize;
    while i < Tool::ALL.len() {
        let t = match Tool::ALL.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let s = Shape::cell(t);
        let o = define(&s);
        if !o.ok() || o.code != 0 || o.stage != Stage::Exit {
            return false;
        }
        if o.value != t.code() || o.note != Note::of_exit(t.code()) {
            return false;
        }
        if o.dangling() || s.dangling() {
            return false;
        }
        match Tier::of(&o) {
            Tier::ExitOne => one += 1,
            Tier::ExitTwo => two += 1,
            Tier::ExitFour => four += 1,
            Tier::ExitZero => zero += 1,
        }
        if s.enospc() != t.writes_data() {
            return false;
        }
        i += 1;
    }
    one == 8 && two == 1 && four == 1 && zero == 2
}

/// 05 errno 归因：errno 错档落在归因闸，与退出码分档互不掩盖。
fn errno_ok() -> bool {
    let a = define(&Shape::wrong_errno(Tool::Tee, ANCHOR_EIO));
    if a.ok() || a.code != EC_ERRNOOFF || a.stage != Stage::Report {
        return false;
    }
    if a.note != Note::ErrnoOff || !a.note.is_off() {
        return false;
    }
    if a.value != EXIT_FAIL {
        return false;
    }
    let b = define(&Shape::wrong_errno(Tool::Df, ANCHOR_ENOSPC));
    if b.ok() || b.code != EC_ERRNOOFF || b.stage != Stage::Report || b.value != EXIT_OK {
        return false;
    }
    let c = define(&Shape::wrong_errno(Tool::Cp, ANCHOR_EIO));
    if c.code != EC_ERRNOOFF || c.note != Note::ErrnoOff {
        return false;
    }
    if Stage::Report.rank() != 6 || Stage::Report.may_enospc() {
        return false;
    }
    // 同一句 errno 在两个工具上退出码不同：errno 对不代表码对。
    if Tool::Cp.errno() != Tool::Sort.errno() || Tool::Cp.code() == Tool::Sort.code() {
        return false;
    }
    Note::ErrnoOff.is_off() && !Note::ErrnoOff.is_cell()
}

/// 06 退出码分档：码错档落在退出闸，1/2/4/0 四档各有其码。
fn exit_ok() -> bool {
    let cases = [
        (Shape::wrong_exit(Tool::Cp, EXIT_OK), EXIT_OK),
        (Shape::wrong_exit(Tool::Sort, EXIT_SED), EXIT_SED),
        (Shape::wrong_exit(Tool::SedI, EXIT_FAIL), EXIT_FAIL),
        (Shape::wrong_exit(Tool::Df, EXIT_FAIL), EXIT_FAIL),
    ];
    let mut i = 0usize;
    while i < cases.len() {
        let (s, want) = match cases.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let o = define(&s);
        if o.ok() || o.code != EC_EXITOFF || o.stage != Stage::Exit {
            return false;
        }
        if o.note != Note::ExitOff || o.value != want {
            return false;
        }
        i += 1;
    }
    if Stage::Exit.rank() != STAGE_LAST_RANK {
        return false;
    }
    let mut t = 1u32;
    while t <= 4 {
        let tier = match Tier::from_rank(t) {
            Some(x) => x,
            None => return false,
        };
        if tier.rank() != t || tier.slug().is_empty() {
            return false;
        }
        if !tier.is_green() && tier != Tier::ExitZero {
            return false;
        }
        t += 1;
    }
    Note::ExitOff.is_off() && Tier::from_rank(5).is_none()
}

/// 07 零档告警：只读工具退 0 必须带告警，否则悬空。
fn warn_ok() -> bool {
    let df = define(&Shape::silent(Tool::Df));
    if df.ok() || df.code != EC_WARNOFF || df.stage != Stage::Close {
        return false;
    }
    if df.note != Note::WarnOff || !df.dangling() {
        return false;
    }
    let du = define(&Shape::silent(Tool::Du));
    if du.code != EC_WARNOFF || du.note != Note::WarnOff || !du.dangling() {
        return false;
    }
    let good = define(&Shape::cell(Tool::Df));
    if good.dangling() || !good.ok() || good.note != Note::Zero {
        return false;
    }
    if Tier::ExitZero.is_green() {
        return false;
    }
    if Tier::ExitOne.is_green() == false || !Tier::ExitFour.is_green() {
        return false;
    }
    // 写侧工具退 0 同样是悬空（cp 伪成功）。
    let cp0 = Shape::wrong_exit(Tool::Cp, EXIT_OK);
    if !cp0.dangling() {
        return false;
    }
    if !Note::WarnOff.is_off() || Note::Zero.is_off() {
        return false;
    }
    matches!(Stage::Close.may_enospc(), true) && !Stage::Check.may_enospc()
}

/// 08 文案逐字：四档的句子各有其形，句子一样档不一样。
fn text_ok() -> bool {
    let cp = msg_cp_enospc(DST_PATH);
    if cp.as_str() != "cp: error writing '/tmp/bx-sandbox/b/y': No space left on device" {
        return false;
    }
    let so = msg_sort_enospc("/tmp");
    if so.as_str() != "sort: cannot create temporary file in '/tmp': No space left on device" {
        return false;
    }
    let se = msg_sed_enospc("/tmp/bx-sandbox/b/sedAbCdE", BLOCK);
    if !se.as_str().starts_with(T_SED_PREFIX) || !se.as_str().contains("4096 bytes to") {
        return false;
    }
    if !se.as_str().contains(ANCHOR_ENOSPC_TEXT) {
        return false;
    }
    let te = msg_tee_enospc(DST_PATH);
    if !te.as_str().starts_with(T_TEE_PREFIX) || !te.as_str().contains(ANCHOR_ENOSPC_TEXT) {
        return false;
    }
    let df = msg_df_warning("bx-sandbox");
    if df.as_str() != "df: warning: 'bx-sandbox' is full; availability reported as 0" {
        return false;
    }
    let du = msg_du_warning("/tmp/bx-sandbox");
    if !du.as_str().starts_with(T_DU_PREFIX) || !du.as_str().contains(ANCHOR_ENOSPC_TEXT) {
        return false;
    }
    if msg_usage("cp").as_str() != "cp: missing operand" {
        return false;
    }
    if msg_cell("sort", EXIT_USAGE, ANCHOR_ENOSPC).as_str() != "sort exit=2 errno=28" {
        return false;
    }
    if join_text("a", "b", "c").as_str() != "abc" {
        return false;
    }
    if ANCHOR_EIO_TEXT != "Input/output error" || ANCHOR_ENOSPC_TEXT != "No space left on device"
    {
        return false;
    }
    // 同一句尾巴，四条前缀互不相同。
    cp.as_str() != so.as_str()
        && so.as_str() != se.as_str()
        && se.as_str() != te.as_str()
        && df.as_str() != du.as_str()
}

/// 09 语料十四槽互校。
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
        if expected(i) != Some(c.fault()) || !c.shape.domain_ok() {
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
        i += 1;
    }
    cover(CORPUS).is_ok() && cover(CORPUS + 1) == Err(EC_SLOT)
}

/// 10 判序六维互斥：死槽各坏一维，掩码恰好一位。
fn judge_ok() -> bool {
    let mut k = ACCEPT;
    while k < CORPUS {
        let c = match case_at(k) {
            Some(x) => x,
            None => return false,
        };
        let f = c.fault();
        if f == Fault::Ok || f != expected_reject_ndx(k) {
            return false;
        }
        if c.claim.diff_mask(&c.actual()) != 1u32 << fault_ndx(f) {
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
        i += 1;
    }
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

/// 11 四档矩阵覆盖。
fn matrix_ok() -> bool {
    let m = Matrix::build();
    if m.slots() != CORPUS * TIER_COUNT || !m.covered() {
        return false;
    }
    let one = match m.row(Tier::ExitOne) {
        Some(r) => r,
        None => return false,
    };
    let two = match m.row(Tier::ExitTwo) {
        Some(r) => r,
        None => return false,
    };
    let four = match m.row(Tier::ExitFour) {
        Some(r) => r,
        None => return false,
    };
    let zero = match m.row(Tier::ExitZero) {
        Some(r) => r,
        None => return false,
    };
    if one.total != CORPUS || two.total != CORPUS || four.total != CORPUS || zero.total != CORPUS
    {
        return false;
    }
    if one.good != 4 || two.good != 1 || four.good != 1 || zero.good != 4 {
        return false;
    }
    if m.goods() != 10 || m.red_total() != CORPUS * TIER_COUNT - 10 {
        return false;
    }
    if m.perfect_rows() != 0 {
        return false;
    }
    if !m.render().as_str().contains("one=") || !m.render().as_str().contains("zero=") {
        return false;
    }
    report_agrees_with_matrix()
}

/// 12 全绿渲染与闸门：报告一行一字不许改，闸门不许倒退。
fn render_ok() -> bool {
    let rep = match run() {
        Ok(r) => r,
        Err(_) => return false,
    };
    if !rep.green() || rep.render().as_str() != RENDER_OK {
        return false;
    }
    if rep.rate() != RATE_FULL || rep.reds() != 0 || rep.dangling != 0 {
        return false;
    }
    if rep.one != ONE_SCENES || rep.two != TWO_SCENES {
        return false;
    }
    if rep.four != FOUR_SCENES || rep.zero != ZERO_SCENES {
        return false;
    }
    if rep.tier_cells() != FAMILY_SCENES as u32 {
        return false;
    }
    let empty = empty_report();
    if empty.green() || empty.gate() != Err(EC_MATRIX) || empty.rate() != 0 {
        return false;
    }
    if run_with(Some((CORPUS, Fault::Value))) != Err(EC_SLOT) {
        return false;
    }
    match run_with(Some((3, Fault::Value))) {
        Ok(bad) => {
            if !(bad.conflicts == 1 && bad.matched + 1 == rep.matched) {
                return false;
            }
        }
        Err(_) => return false,
    }
    match Gate::full_pass() {
        Ok(h) => {
            if h == 0 {
                return false;
            }
        }
        Err(_) => return false,
    }
    if Gate::walk(&[Stage::Exit, Stage::Check]) != Err(EC_STAGE) {
        return false;
    }
    let mut g = Gate::walk(&[Stage::Check, Stage::Open, Stage::Write]);
    let mut gate = match g {
        Ok(ref mut x) => {
            if x.stage() != Some(Stage::Write) || x.steps() != 3 {
                return false;
            }
            let h = match x.close() {
                Ok(v) => v,
                Err(_) => return false,
            };
            if h == 0 || !x.is_closed() {
                return false;
            }
            if x.step(Stage::Flush) != Err(EC_ESTATE) {
                return false;
            }
            true
        }
        Err(_) => false,
    };
    g = Ok(Gate::new());
    if let Ok(ref mut fresh) = g {
        if fresh.step(Stage::Flush).is_err() {
            gate = false;
        }
        // 单档之后 close 必须成功并封账；封账后再走档报 ESTATE。
        if fresh.close().is_err() || !fresh.is_closed() {
            gate = false;
        }
        if fresh.step(Stage::Close) != Err(EC_ESTATE) {
            gate = false;
        }
    }
    if !gate {
        return false;
    }
    let mut lone = Gate::new();
    if lone.close() != Err(EC_STAGE) {
        return false;
    }
    if Gate::new().render().as_str() != "gate stage=none steps=0 refusals=0 closed=false" {
        return false;
    }
    Gate::walk(&[Stage::Check, Stage::Check]).is_ok()
}

/// 13 之前：账本逐格复核、冻结、重放、错位拒收。
fn ledger_ok() -> bool {
    let mut l = build_ledger();
    if l.len() != SAMPLES || l.is_empty() || l.audit() != Ok(()) {
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
        slot: 2,
        op: 1,
        ok: true,
        code: 0,
        value: 1,
    };
    if bad.register(s) != Err(EC_SLOT) || bad.rejects() != 1 {
        return false;
    }
    let h = match l.freeze() {
        Ok(x) => x,
        Err(_) => return false,
    };
    if h == 0 || l.root() != h || !l.is_frozen() || l.replay() != Ok(h) {
        return false;
    }
    if l.freeze() != Err(EC_ESTATE) || l.poke(0, 1) != Err(EC_ESTATE) {
        return false;
    }
    l.render().as_str().contains("len=14") && l.render().as_str().contains("frozen=true")
}

/// 汇总：十三条判据，一条不多一条不少。
pub fn run_unx11698_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11698_exit_code_matrix_enospc");
    s.add("EC01-名称与段基址", name_ok(), "static");
    s.add("EC02-十三码互异", codes_ok(), "static");
    s.add("EC03-工具码表", tools_ok(), "static");
    s.add("EC04-十二格两档", cells_ok(), "static");
    s.add("EC05-errno 归因", errno_ok(), "static");
    s.add("EC06-退出码分档", exit_ok(), "static");
    s.add("EC07-零档告警", warn_ok(), "static");
    s.add("EC08-文案逐字", text_ok(), "static");
    s.add("EC09-语料十四槽互校", corpus_ok(), "static");
    s.add("EC10-判序六维互斥", judge_ok(), "static");
    s.add("EC11-四档矩阵覆盖", matrix_ok(), "static");
    s.add("EC12-全绿渲染与闸门", render_ok(), "static");
    s.add(
        "EC13-冻结重放四注入",
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
// 手写自检：三十四条，逐条钉住本卡的每一个不变量
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t01_seg_base_is_pinned() {
        assert_eq!(SEG_BASE, 0x6d420);
        assert_eq!(ANCHOR_F11697_SEG + ANCHOR_F11697_GAP, SEG_BASE);
        assert_eq!(SEG_BASE + ANCHOR_F11699_GAP, ANCHOR_F11699_SEG);
        assert_eq!(ANCHOR_F11694_SEG + ANCHOR_F11694_GAP, SEG_BASE);
        assert_eq!(ANCHOR_F11687_SEG + ANCHOR_F11687_GAP, SEG_BASE);
        assert!(anchors_agree());
    }

    #[test]
    fn t02_thirteen_codes_sit_on_the_segment() {
        assert_eq!(ALL_CODES.len(), 13);
        let mut i = 0usize;
        while i < ALL_CODES.len() {
            assert_eq!(ALL_CODES[i], SEG_BASE + i as u32 + 1);
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
            assert_eq!(fault_ndx(faults[k]), k);
            assert_eq!(ndx_fault(k), Some(faults[k]));
            k += 1;
        }
        assert_eq!(Fault::Ok.code(), 0);
        assert_eq!(fault_ndx(Fault::Ok), 6);
        assert_eq!(ndx_fault(6), Some(Fault::Ok));
        assert_eq!(ndx_fault(7), None);
    }

    #[test]
    fn t05_vals_are_the_pinned_statistics() {
        assert_eq!(VALS.len(), ACCEPT);
        assert_eq!(VALS[0] as usize, FAMILY_SCENES);
        assert_eq!(VALS[1] as usize, ONE_SCENES);
        assert_eq!(VALS[2] as usize, TWO_SCENES);
        assert_eq!(VALS[3] as usize, FOUR_SCENES);
        assert_eq!(VALS[4] as usize, ZERO_SCENES);
        assert_eq!(VALS[5], 0);
        assert_eq!(VALS[6], RATE_FULL);
        assert_eq!(VALS[7], RED_RATE);
    }

    #[test]
    fn t06_corpus_has_eight_live_slots() {
        assert_eq!(CORPUS, 14);
        assert_eq!(SAMPLES, 14);
        assert_eq!(ACCEPT + REJECT, CORPUS);
        assert_eq!(cases().len(), CORPUS);
        assert!(case_at(13).is_some());
        assert!(case_at(14).is_none());
        let mut i = 0usize;
        while i < ACCEPT {
            let c = cases()[i];
            assert_eq!(c.fault(), Fault::Ok);
            assert!(c.actual().ok());
            assert!(!c.actual().dangling());
            i += 1;
        }
    }

    #[test]
    fn t07_dead_slots_break_exactly_one_dimension() {
        let cs = cases();
        let mut k = ACCEPT;
        while k < CORPUS {
            let c = cs[k];
            let f = c.fault();
            assert_ne!(f, Fault::Ok);
            assert_eq!(f, Fault::reject_faults()[k - ACCEPT]);
            assert_eq!(c.claim.diff_mask(&c.actual()), 1u32 << fault_ndx(f));
            k += 1;
        }
    }

    #[test]
    fn t08_slot_eight_is_a_false_success() {
        let c = cases()[8];
        let a = c.actual();
        assert_eq!(c.tag(), "estate-swap");
        assert!(c.claim.success());
        assert!(!a.ok());
        assert_eq!(a.stage, Stage::Exit);
        assert_eq!(a.code, EC_EXITOFF);
        assert_eq!(a.value, EXIT_OK);
        assert_eq!(a.note, Note::ExitOff);
        assert!(!a.dangling());
        assert_eq!(c.fault(), Fault::Estate);
    }

    #[test]
    fn t09_slot_nine_lands_on_the_wrong_gate() {
        let c = cases()[9];
        let a = c.actual();
        assert_eq!(c.tag(), "stage-skip");
        assert_eq!(a.stage, Stage::Exit);
        assert_eq!(c.claim.stage, Stage::Report);
        assert!(a.ok());
        assert_eq!(c.fault(), Fault::Stage);
        assert_eq!(c.claim.diff_mask(&a), 0x02);
    }

    #[test]
    fn t10_slot_ten_swallows_the_errno() {
        let c = cases()[10];
        let a = c.actual();
        assert_eq!(c.tag(), "code-swallow");
        assert_eq!(c.shape.errno, ANCHOR_EIO);
        assert_eq!(a.stage, Stage::Report);
        assert_eq!(a.code, EC_ERRNOOFF);
        assert_eq!(a.value, EXIT_FAIL);
        assert_eq!(a.note, Note::ErrnoOff);
        assert_eq!(c.claim.code, 0);
        assert_eq!(c.fault(), Fault::Code);
    }

    #[test]
    fn t11_slot_eleven_off_by_one_tier() {
        let c = cases()[11];
        let a = c.actual();
        assert_eq!(c.tag(), "value-off");
        assert_eq!(a.value, EXIT_USAGE);
        assert_eq!(c.claim.value, EXIT_FAIL);
        assert_eq!(a.note, Note::Two);
        assert_eq!(c.claim.note, Note::Two);
        assert_eq!(c.fault(), Fault::Value);
        assert_eq!(c.claim.diff_mask(&a), 0x08);
    }

    #[test]
    fn t12_slot_twelve_swaps_two_and_four() {
        let c = cases()[12];
        let a = c.actual();
        assert_eq!(c.tag(), "note-swap");
        assert_eq!(a.value, EXIT_SED);
        assert_eq!(a.note, Note::Four);
        assert_eq!(c.claim.note, Note::Two);
        assert_eq!(c.claim.value, EXIT_SED);
        assert_eq!(c.fault(), Fault::Note);
        assert_eq!(c.claim.diff_mask(&a), 0x10);
    }

    #[test]
    fn t13_slot_thirteen_falls_into_default() {
        let c = cases()[13];
        let a = c.actual();
        assert_eq!(c.tag(), "default-bucket");
        assert!(c.shape.default);
        assert!(a.default);
        assert_eq!(a.stage, Stage::Report);
        assert_eq!(a.value, EXIT_UNJUDGED);
        assert_eq!(a.note, Note::Nil);
        assert!(a.dangling());
        assert_eq!(c.fault(), Fault::Default);
    }

    #[test]
    fn t14_usage_faults_land_on_check() {
        let s = Shape {
            tool: Tool::Cp,
            errno: ANCHOR_ENOSPC,
            exit: EXIT_FAIL,
            warning: false,
            bad_call: true,
            default: false,
        };
        let o = define(&s);
        assert!(!o.ok());
        assert_eq!(o.stage, Stage::Check);
        assert_eq!(o.code, EC_SLOT);
        assert_eq!(o.value, EXIT_USAGE);
        assert_eq!(o.note, Note::Usage);
        assert!(!o.default);
        assert!(!o.dangling());
        // 用法错不进四档，按 Tier::of 归零档。
        assert_eq!(Tier::of(&o), Tier::ExitZero);
    }

    #[test]
    fn t15_unjudged_cells_are_dangling() {
        let s = Shape::unjudged(Tool::Mkdir);
        let o = define(&s);
        assert!(!o.ok());
        assert_eq!(o.stage, Stage::Report);
        assert_eq!(o.code, 0);
        assert_eq!(o.value, EXIT_UNJUDGED);
        assert_eq!(o.note, Note::Nil);
        assert!(o.default);
        assert!(o.dangling());
        assert_eq!(Tier::of(&o), Tier::ExitZero);
    }

    #[test]
    fn t16_exit_code_faults_land_on_exit_gate() {
        let a = define(&Shape::wrong_exit(Tool::Cp, EXIT_OK));
        assert_eq!(a.stage, Stage::Exit);
        assert_eq!(a.code, EC_EXITOFF);
        assert_eq!(a.value, EXIT_OK);
        assert_eq!(a.note, Note::ExitOff);
        let b = define(&Shape::wrong_exit(Tool::Sort, EXIT_FAIL));
        assert_eq!(b.code, EC_EXITOFF);
        assert_eq!(b.value, EXIT_FAIL);
        assert_eq!(b.note, Note::ExitOff);
    }

    #[test]
    fn t17_errno_faults_land_on_report_gate() {
        let a = define(&Shape::wrong_errno(Tool::Tee, ANCHOR_EIO));
        assert_eq!(a.stage, Stage::Report);
        assert_eq!(a.code, EC_ERRNOOFF);
        assert_eq!(a.value, EXIT_FAIL);
        assert_eq!(a.note, Note::ErrnoOff);
        let b = define(&Shape::wrong_errno(Tool::Df, ANCHOR_ENOSPC));
        assert_eq!(b.stage, Stage::Report);
        assert_eq!(b.code, EC_ERRNOOFF);
        assert_eq!(b.value, EXIT_OK);
        assert_eq!(b.note, Note::ErrnoOff);
    }

    #[test]
    fn t18_missing_warning_lands_on_close_gate() {
        let o = define(&Shape::silent(Tool::Df));
        assert!(!o.ok());
        assert_eq!(o.stage, Stage::Close);
        assert_eq!(o.code, EC_WARNOFF);
        assert_eq!(o.value, EXIT_OK);
        assert_eq!(o.note, Note::WarnOff);
        assert!(o.dangling());
        let du = define(&Shape::silent(Tool::Du));
        assert_eq!(du.code, EC_WARNOFF);
        assert_eq!(du.note, Note::WarnOff);
    }

    #[test]
    fn t19_every_tool_cell_is_graded() {
        let mut one = 0usize;
        let mut two = 0usize;
        let mut four = 0usize;
        let mut zero = 0usize;
        let mut i = 0usize;
        while i < TOOL_COUNT {
            let t = Tool::ALL[i];
            let o = define(&Shape::cell(t));
            assert!(o.ok());
            assert_eq!(o.stage, Stage::Exit);
            assert_eq!(o.code, 0);
            assert_eq!(o.value, t.code());
            assert_eq!(o.note, Note::of_exit(t.code()));
            assert!(!o.dangling());
            match Tier::of(&o) {
                Tier::ExitOne => one += 1,
                Tier::ExitTwo => two += 1,
                Tier::ExitFour => four += 1,
                Tier::ExitZero => zero += 1,
            }
            i += 1;
        }
        assert_eq!(one, 8);
        assert_eq!(two, 1);
        assert_eq!(four, 1);
        assert_eq!(zero, 2);
    }

    #[test]
    fn t20_tool_table_is_twelve_wide() {
        assert_eq!(Tool::ALL.len(), TOOL_COUNT);
        assert_eq!(TOOL_COUNT, 12);
        let mut writes = 0usize;
        let mut warns = 0usize;
        let mut i = 0usize;
        while i < TOOL_COUNT {
            let t = Tool::ALL[i];
            assert_eq!(Tool::from_slug(t.slug()), Some(t));
            if t.writes_data() {
                writes += 1;
                assert!(!t.warning());
                assert_eq!(t.errno(), ANCHOR_ENOSPC);
            } else {
                warns += 1;
                assert!(t.read_only());
                assert!(t.warning());
                assert_eq!(t.errno(), 0);
            }
            i += 1;
        }
        assert_eq!(writes, WRITE_TOOLS);
        assert_eq!(warns, WARN_TOOLS);
        assert_eq!(Tool::Df.bucket_slug(), "zero");
        assert_eq!(Tool::Sort.bucket_slug(), "two");
        assert_eq!(Tool::SedI.bucket_slug(), "four");
        assert_eq!(Tool::Cp.bucket_slug(), "one");
    }

    #[test]
    fn t21_tiers_are_four_with_zero_not_green() {
        assert_eq!(Tier::ALL.len(), TIER_COUNT);
        let mut r = 1u32;
        while r <= 4 {
            let t = Tier::from_rank(r);
            assert!(t.is_some());
            assert_eq!(t.map(|x| x.rank()), Some(r));
            assert_eq!(t.map(|x| x.code()), Some(match r {
                1 => EXIT_FAIL,
                2 => EXIT_USAGE,
                3 => EXIT_SED,
                _ => EXIT_OK,
            }));
            r += 1;
        }
        assert!(Tier::from_rank(5).is_none());
        assert!(Tier::ExitOne.is_green());
        assert!(Tier::ExitTwo.is_green());
        assert!(Tier::ExitFour.is_green());
        assert!(!Tier::ExitZero.is_green());
    }

    #[test]
    fn t22_notes_are_nine_with_three_off() {
        assert_eq!(Note::ALL.len(), 9);
        let mut i = 0usize;
        let mut cells = 0usize;
        let mut offs = 0usize;
        while i < Note::ALL.len() {
            let n = Note::ALL[i];
            assert_eq!(Note::from_rank(n.rank()), Some(n));
            if n.is_cell() {
                cells += 1;
            }
            if n.is_off() {
                offs += 1;
            }
            i += 1;
        }
        assert_eq!(cells, 4);
        assert_eq!(offs, 3);
        assert_eq!(Note::of_exit(EXIT_FAIL), Note::One);
        assert_eq!(Note::of_exit(EXIT_USAGE), Note::Two);
        assert_eq!(Note::of_exit(EXIT_SED), Note::Four);
        assert_eq!(Note::of_exit(EXIT_OK), Note::Zero);
        assert_eq!(Note::Usage.rank(), 8);
        assert_eq!(Note::Nil.rank(), 9);
    }

    #[test]
    fn t23_stages_are_seven_in_order() {
        assert_eq!(Stage::ALL.len(), 7);
        assert_eq!(STAGE_COUNT as usize, Stage::ALL.len());
        assert_eq!(STAGE_LAST_RANK, 7);
        let mut r = 1u32;
        while r <= STAGE_LAST_RANK {
            let s = Stage::from_rank(r);
            assert!(s.is_some());
            assert_eq!(s.map(|x| x.rank()), Some(r));
            r += 1;
        }
        assert!(Stage::from_rank(0).is_none());
        assert!(Stage::from_rank(8).is_none());
        assert!(!Stage::Check.may_enospc());
        assert!(!Stage::Open.may_enospc());
        assert!(Stage::Write.may_enospc());
        assert!(Stage::Flush.may_enospc());
        assert!(Stage::Close.may_enospc());
        assert!(!Stage::Report.may_enospc());
        assert!(!Stage::Exit.may_enospc());
        assert_eq!(Stage::Exit.rank(), STAGE_LAST_RANK);
    }

    #[test]
    fn t24_judge_order_is_fixed() {
        let s = Shape::cell(Tool::Cp);
        let a = define(&s);
        let all_wrong = Claim::new(
            "all-wrong",
            !a.ok(),
            Stage::Check,
            EC_SLOT,
            a.value.wrapping_add(3),
            Note::WarnOff,
            true,
        );
        assert_eq!(judge(&Case::new(s, all_wrong)), Fault::Estate);
        let stage_off = Claim::new("stage-off", a.ok(), Stage::Check, 0, a.value, a.note, false);
        assert_eq!(judge(&Case::new(s, stage_off)), Fault::Stage);
        let code_off = Claim::new("code-off", a.ok(), a.stage, EC_CODE, a.value, a.note, false);
        assert_eq!(judge(&Case::new(s, code_off)), Fault::Code);
        let value_off = Claim::new("value-off", a.ok(), a.stage, 0, a.value + 1, a.note, false);
        assert_eq!(judge(&Case::new(s, value_off)), Fault::Value);
        let note_off = Claim::new("note-off", a.ok(), a.stage, 0, a.value, Note::Two, false);
        assert_eq!(judge(&Case::new(s, note_off)), Fault::Note);
        let def_off = Claim::new("def-off", a.ok(), a.stage, 0, a.value, a.note, true);
        assert_eq!(judge(&Case::new(s, def_off)), Fault::Default);
        let copy = Claim::from_outcome(&a, a.ok(), "copy");
        assert_eq!(judge(&Case::new(s, copy)), Fault::Ok);
    }

    #[test]
    fn t25_expected_table_matches_the_ndx_table() {
        assert_eq!(WANT_NDX.len(), CORPUS);
        assert_eq!(expected(0), Some(Fault::Ok));
        assert_eq!(expected(7), Some(Fault::Ok));
        assert_eq!(expected(8), Some(Fault::Estate));
        assert_eq!(expected(9), Some(Fault::Stage));
        assert_eq!(expected(10), Some(Fault::Code));
        assert_eq!(expected(11), Some(Fault::Value));
        assert_eq!(expected(12), Some(Fault::Note));
        assert_eq!(expected(13), Some(Fault::Default));
        assert_eq!(expected(14), None);
        assert_eq!(cover(CORPUS), Ok(()));
        assert_eq!(cover(CORPUS + 1), Err(EC_SLOT));
    }

    #[test]
    fn t26_matrix_covers_four_tiers() {
        let m = Matrix::build();
        assert_eq!(m.slots(), CORPUS * TIER_COUNT);
        assert_eq!(m.slots(), 56);
        assert!(m.covered());
        let one = m.row(Tier::ExitOne);
        let two = m.row(Tier::ExitTwo);
        let four = m.row(Tier::ExitFour);
        let zero = m.row(Tier::ExitZero);
        assert_eq!(one.map(|r| r.good), Some(4));
        assert_eq!(two.map(|r| r.good), Some(1));
        assert_eq!(four.map(|r| r.good), Some(1));
        assert_eq!(zero.map(|r| r.good), Some(4));
        assert_eq!(one.map(|r| r.total), Some(CORPUS));
        assert_eq!(m.goods(), 10);
        assert_eq!(m.red_total(), CORPUS * TIER_COUNT - 10);
        assert_eq!(m.perfect_rows(), 0);
        assert_eq!(m.render().as_str(), "matrix one=4/14 two=1/14 four=1/14 zero=4/14");
    }

    #[test]
    fn t27_report_renders_verbatim() {
        let rep = run();
        assert!(rep.is_ok());
        if let Ok(r) = rep {
            assert_eq!(r.render().as_str(), RENDER_OK);
            assert!(r.green());
            assert_eq!(r.rate(), RATE_FULL);
            assert_eq!(r.reds(), 0);
            assert_eq!(r.dangling, 0);
            assert_eq!(r.tier_cells(), FAMILY_SCENES as u32);
            assert_eq!(r.red_cells(), 0);
            assert_eq!(r.gate(), Ok(()));
        }
    }

    #[test]
    fn t28_empty_report_is_not_green() {
        let e = empty_report();
        assert!(!e.green());
        assert_eq!(e.rate(), 0);
        assert_eq!(e.gate(), Err(EC_MATRIX));
        assert_eq!(e.tier_cells(), 0);
        assert_eq!(e.reds(), 0);
        assert_eq!(e.render().as_str(), "exit corpus=0 accept=0 reject=0 one=0 two=0 four=0 zero=0 dangling=0 match=0 rate=0permille\n");
    }

    #[test]
    fn t29_injected_fault_lowers_the_rate() {
        assert_eq!(run_with(Some((CORPUS, Fault::Value))), Err(EC_SLOT));
        match run_with(Some((3, Fault::Value))) {
            Ok(bad) => {
                assert_eq!(bad.conflicts, 1);
                assert_eq!(bad.matched, 11);
                assert_eq!(bad.rate(), 917);
                assert_eq!(bad.red_cells(), 1);
                assert!(!bad.green());
                assert_eq!(bad.gate(), Err(EC_NOTE));
                assert_eq!(bad.dangling, 0);
            }
            Err(_) => panic!("slot 3 must settle"),
        }
        assert!(red_rate_calibrated());
        assert!(report_agrees_with_matrix());
    }

    #[test]
    fn t30_gate_refuses_regressions_and_replay_after_close() {
        match Gate::full_pass() {
            Ok(h) => assert_ne!(h, 0),
            Err(_) => panic!("full pass must close"),
        }
        assert_eq!(Gate::walk(&[Stage::Exit, Stage::Check]), Err(EC_STAGE));
        assert!(Gate::walk(&[Stage::Check, Stage::Check]).is_ok());
        let mut g = Gate::new();
        assert_eq!(g.close(), Err(EC_STAGE));
        assert_eq!(g.render().as_str(), "gate stage=none steps=0 refusals=0 closed=false");
        assert_eq!(g.step(Stage::Flush), Ok(()));
        let h = g.close();
        assert!(h.is_ok());
        assert!(g.is_closed());
        assert_eq!(g.step(Stage::Close), Err(EC_ESTATE));
        assert_eq!(g.refusals(), 1);
    }

    #[test]
    fn t31_ledger_holds_fourteen_cells() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert!(!l.is_empty());
        assert_eq!(l.ok_count(), STAT_CELLS + 1);
        assert_eq!(l.err_count(), REJECT);
        assert_eq!(l.audit(), Ok(()));
        assert_eq!(l.first_mismatch(), None);
        assert!(!l.is_frozen());
        assert_eq!(l.rejects(), 0);
        assert!(l.render().as_str().contains("len=14"));
        let head = l.sample(0);
        assert_eq!(head.map(|s| s.slot), Some(0));
        assert_eq!(head.map(|s| s.ok), Some(true));
        assert_eq!(head.map(|s| s.value), Some(VALS[0]));
    }

    #[test]
    fn t32_ledger_rejects_misplaced_slots() {
        let mut l = Ledger::new();
        assert!(l.is_empty());
        assert_eq!(l.audit(), Err(EC_SLOT));
        let s = Sample {
            slot: 2,
            op: 1,
            ok: true,
            code: 0,
            value: 1,
        };
        assert_eq!(l.register(s), Err(EC_SLOT));
        assert_eq!(l.rejects(), 1);
        assert_eq!(l.len(), 0);
        assert_eq!(l.poke(0, 1), Err(EC_SLOT));
        let mut full = Ledger::new();
        let mut i = 0usize;
        while i < SAMPLES {
            let x = Sample {
                slot: i as u8,
                op: 1,
                ok: true,
                code: 0,
                value: 1,
            };
            assert_eq!(full.register(x), Ok(()));
            i += 1;
        }
        let over = Sample {
            slot: SAMPLES as u8,
            op: 1,
            ok: true,
            code: 0,
            value: 1,
        };
        assert_eq!(full.register(over), Err(EC_SLOT));
        assert_eq!(full.len(), SAMPLES);
    }

    #[test]
    fn t33_ledger_freezes_and_replays() {
        let mut l = build_ledger();
        let h = l.freeze();
        assert!(h.is_ok());
        let root = match h {
            Ok(x) => x,
            Err(_) => 0,
        };
        assert_ne!(root, 0);
        assert!(l.is_frozen());
        assert_eq!(l.root(), root);
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(EC_ESTATE));
        assert_eq!(l.poke(0, 1), Err(EC_ESTATE));
        assert!(l.render().as_str().contains("frozen=true"));
        let unfrozen = build_ledger();
        assert_eq!(unfrozen.replay(), Err(EC_ESTATE));
    }

    #[test]
    fn t34_broken_ledgers_fail_audit() {
        assert!(build_ledger_broken(0).audit().is_err());
        assert!(build_ledger_broken(1).audit().is_err());
        assert!(build_ledger_broken(2).audit().is_err());
        assert!(build_ledger_broken(0).first_mismatch().is_some());
        assert_eq!(build_ledger_broken(9).audit(), Ok(()));
        let mut b = build_ledger_broken(0);
        assert_eq!(b.freeze(), Err(EC_VALUE));
    }

    #[test]
    fn t35_four_routes_agree() {
        let four = inject_four();
        assert_eq!(four.len(), 4);
        let mut i = 0usize;
        while i < four.len() {
            assert!(four[i]);
            i += 1;
        }
        assert!(scenes_agree());
        assert_eq!(scene_dangling_count(), 0);
    }

    #[test]
    fn t36_text_helpers_are_fixed_width() {
        assert_eq!(u32_text(0).as_str(), "0");
        assert_eq!(u32_text(929).as_str(), "929");
        assert_eq!(dec_text(1000).as_str(), "1000");
        assert_eq!(hex_text(0x420, 6).as_str(), "000420");
        assert_eq!(hex_text(0x0d, 2).as_str(), "0d");
        assert_eq!(seg_text(SEG_BASE).as_str(), "0x06d420");
        assert_eq!(seg_text(0).as_str(), "0x000000");
        assert_eq!(permille_text(RATE_FULL).as_str(), "1000permille");
        assert_eq!(bool_text(true).as_str(), "true");
        assert_eq!(bool_text(false).as_str(), "false");
        assert_eq!(i32_text(-5).as_str(), "-5");
        assert_eq!(join_text("a", "b", "c").as_str(), "abc");
    }

    #[test]
    fn t37_messages_are_verbatim() {
        assert_eq!(
            msg_cp_enospc(DST_PATH).as_str(),
            "cp: error writing '/tmp/bx-sandbox/b/y': No space left on device"
        );
        assert_eq!(
            msg_sort_enospc("/tmp").as_str(),
            "sort: cannot create temporary file in '/tmp': No space left on device"
        );
        assert_eq!(
            msg_sed_enospc("/tmp/bx-sandbox/b/sedX", BLOCK).as_str(),
            "sed: couldn't write 4096 bytes to /tmp/bx-sandbox/b/sedX: No space left on device"
        );
        assert_eq!(
            msg_tee_enospc(DST_PATH).as_str(),
            "tee: /tmp/bx-sandbox/b/y: No space left on device"
        );
        assert_eq!(
            msg_df_warning("bx-sandbox").as_str(),
            "df: warning: 'bx-sandbox' is full; availability reported as 0"
        );
        assert_eq!(
            msg_du_warning("/tmp/bx-sandbox").as_str(),
            "du: warning: cannot read '/tmp/bx-sandbox': No space left on device"
        );
        assert_eq!(msg_usage("cp").as_str(), "cp: missing operand");
        assert_eq!(msg_cell("sort", EXIT_USAGE, 28).as_str(), "sort exit=2 errno=28");
        assert!(SRC_PATH.starts_with(SANDBOX_PREFIX));
        assert!(DST_PATH.starts_with(SANDBOX_PREFIX));
    }

    #[test]
    fn t38_outcome_and_claim_render_are_stable() {
        let o = define(&Shape::cell(Tool::Cp));
        assert_eq!(
            o.render().as_str(),
            "stage=exit code=0x000000 exit=1 note=exit-one ok=true"
        );
        let c = cases()[0];
        assert_eq!(
            c.claim.render().as_str(),
            "cp-exit-one ok=true exit 0x000000 1 exit-one"
        );
        let s = Sample {
            slot: 0,
            op: 1,
            ok: true,
            code: 0,
            value: 12,
        };
        assert_eq!(
            s.render().as_str(),
            "slot 0 op 01 true code 00000 value 12"
        );
        let d = define(&Shape::unjudged(Tool::Mkdir));
        assert!(d.render().as_str().contains("note=nil"));
        assert_eq!(o.seal(), define(&Shape::cell(Tool::Cp)).seal());
    }

    #[test]
    fn t39_scene_family_is_self_consistent() {
        assert_eq!(scenes().len(), FAMILY_SCENES);
        assert_eq!(scene_counts(), (8, 1, 1, 2));
        let mut i = 0usize;
        while i < FAMILY_SCENES {
            assert_eq!(scene_anchored(i), Some(true));
            assert!(scene_seal(i).is_some());
            if let Some(c) = scene_case(i) {
                assert_eq!(judge(&c), Fault::Ok);
                assert_eq!(c.tag(), head_tag());
            }
            i += 1;
        }
        assert_eq!(scene_at(FAMILY_SCENES), None);
        assert!(scene_case(FAMILY_SCENES).is_none());
        assert!(scene_seal(FAMILY_SCENES).is_none());
        assert_eq!(head_tag(), "cp-exit-one");
        assert_eq!(head_val(), 12);
    }

    #[test]
    fn t40_checkset_has_thirteen_checks() {
        let s = run_unx11698_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
        assert_eq!(s.passed_count(), 13);
        assert_eq!(s.failed_count(), 0);
        match s.get(0) {
            Some(c) => assert_eq!(c.name, "EC01-名称与段基址"),
            None => panic!("check 0 missing"),
        }
        match s.get(12) {
            Some(c) => assert_eq!(c.name, "EC13-冻结重放四注入"),
            None => panic!("check 12 missing"),
        }
        let mut i = 0usize;
        while i < 13 {
            match s.get(i) {
                Some(c) => assert!(c.passed),
                None => panic!("check missing"),
            }
            i += 1;
        }
    }

    #[test]
    fn t41_stage_ranks_are_one_to_seven() {
        let mut i = 0usize;
        while i < Stage::ALL.len() {
            let st = Stage::ALL[i];
            assert_eq!(st.rank() as usize, i + 1);
            assert!(!st.slug().is_empty());
            assert!(!st.cn().is_empty());
            i += 1;
        }
        assert_eq!(Stage::ALL[5], Stage::Report);
        assert_eq!(Stage::ALL[6], Stage::Exit);
    }

    #[test]
    fn t42_cell_domain_is_bounded() {
        let s = Shape::cell(Tool::Cp);
        assert!(s.domain_ok());
        assert_eq!(s.enospc(), true);
        assert_eq!(s.dangling(), false);
        let z = Shape::cell(Tool::Df);
        assert_eq!(z.enospc(), false);
        assert_eq!(z.dangling(), false);
        let bad = Shape {
            tool: Tool::Cp,
            errno: 65,
            exit: EXIT_FAIL,
            warning: false,
            bad_call: false,
            default: false,
        };
        assert!(!bad.domain_ok());
        assert!(Shape::cell(Tool::Du).seal() != Shape::cell(Tool::Df).seal());
    }
}
