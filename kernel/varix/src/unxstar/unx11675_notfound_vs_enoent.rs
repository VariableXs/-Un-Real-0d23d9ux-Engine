//! UNX-F11675 notfound 与 ENOENT exec 链路对照判据
//!
//! 127 是最容易被"混账"的退出码：busybox 分发层查不到 applet 时报
//! `applet 'frobnicate' not found` 并退 127，shell 在 PATH 尽头拿不到文件时由
//! execve 返回 ENOENT、ash 报 `sh: frobnicate: not found` 同样退 127。两路在
//! **码**这一维完全同值，在**链路**这一维却南辕北辙：一路从没碰过内核
//! （applet 表是用户态字符串比对，execve 一次都没发生，errno 恒为零），另一路
//! 每个 PATH 目录都真真切切地做过 execve 系统调用、手里攥着一串 ENOENT 回执。
//! 这张卡判的就是"码同链异"这条对照线：
//!
//! * 分发层错报成 exec 层（applet 未命中却申报 errno=ENOENT、申报探过 N 个
//!   目录）——这是最阴的假证：码对了、文案族也蹭对了，唯独系统调用回执是
//!   编出来的。分发层的 execve 回执恒为零，任何非零 errno 或非零探测数在这
//!   一路都是伪证，判死；
//! * exec 层错报成分发层（PATH 搜空的 127 却报 `applet ... not found` 文案
//!   族）——把 shell 的责任推给 busybox 表，排查的人会去翻 applet 注册表而
//!   不是 PATH，判死；
//! * 126 边界：命中文件但无执行权限（EACCES=13）或格式非法（ENOEXEC=8）退
//!   126，与 127 的"根本没 execve 成功过任何候选项"分档——码不许越档；
//! * 探测数守恒：PATH 有 D 个目录、命中在第 k 个，则 execve 探了 k 次失败、
//!   第 k+1 次成功——申报的探测数与现场推定值差一都不行（`PROBE_GAP`），
//!   因为它是"逐目录序"（与 F11666 联签）的唯一可复核痕迹；
//! * 防御档（裸调用空名）在 Lex 闸就退 1、只许产 usage：它既不属于分发层
//!   notfound 也不属于 exec 层 ENOENT，混进任何一路都算链路越权。
//!
//! 与 B21 分发层主锚（F11585）的联签点在于：那张卡立"applet 表查不到就报
//! not found 退 127"的正向锚，本卡立"这条 127 与 execve 的那条 127 不许互
//! 相冒充"的对照锚；与 exec 层（F11608 exec 路径、F11664 码档、F11665 sh 文案）
//! 的联签点在于：那些卡各自立单路，本卡把两路并排放同一条纵线上逐维比对。
//!
//! ## 闸门次序（这条纵线为什么不能倒）
//!
//! `lex → route → table → scan → exec → report → exit`，七道闸秩 0..6。
//!
//! 1. `lex`：名字防线。空名（裸调用）在本闸定案退 1 产 usage——它连链路都
//!    还没选，晚一闸判就会把防御档的 usage 记成分发层的 notfound 文案。
//! 2. `route`：链路归属闸。入口方式与命中状态必须同族：分发入口只能配
//!    applet 表三态（guarded/miss/hit），exec 入口只能配文件系统四态
//!    （miss-all/perm/format/good）。不同族 = 现场本身就把两条链搅在一起，
//!    在本闸判死（ChainCross），不进后面的比对——后四闸的任何观测对这种
//!    现场都没有意义。
//! 3. `table`：applet 表查询闸（分发层专属）。查得到→放行执行，查不到→
//!    notfound 义务成立。本闸只立"有没有查名字这件事"，不观测系统调用。
//! 4. `scan`：PATH 逐目录遍历闸（exec 层专属）。探测数申报与现场推定值
//!    （MissAll=目录总数、命中=命中前失败数）必须相等；差一即 ProbeGap。
//!    它排在 table 之后：先确认走的是哪条链，才谈得上"链上探了几次"。
//! 5. `exec`：execve 回执闸。errno 观测必须与命中状态一致（MissAll→ENOENT、
//!    Perm→EACCES、Format→ENOEXEC、其余→零）。错一个码就是换了故障族，
//!    126/127 分档随之全错，所以它必须先于文案闸。
//! 6. `report`：文案族闸。报错文案的**出处**（applet 表文案族 / shell 文案族 /
//!    usage 族）必须与链路归属同族。放在最后是因为文案是链路的投影：只有
//!    链路、探测、errno 全部对账完毕，文案族不一致才只能是"冒充"而不是
//!    "上游数据坏"。
//! 7. `exit`：退出码聚合。127（两路同码）、126（命中被拒）、1（防御档）、
//!    0（正常执行）。码是前六闸的投影，任何"码对链错"的申报都要在这条
//!    纵线的前段被抓出来，不许凭码放行。
//!
//! ## 关键差异（本卡深度所在）
//!
//! * 与 F11585（B21 分发层主锚）的边界：那张卡判"分发层查不到怎么报错"，
//!   本卡判"这次 127 到底是不是分发层的"。F11585 的现场永远没有 errno，
//!   本卡给它补上"errno 非零即伪证"的反向闸。
//! * 与 F11664（notfound 退出码 127 三通道一致）的边界：那张卡把 127 当
//!   **终点**核对码值，本卡把 127 当**起点**追问链路。码值一致但链路互报，
//!   F11664 绿、本卡红——这正是两张卡必须并存的原因。
//! * 与 F11665（sh command not found 文案）的边界：那张卡判 shell 文案逐字，
//!   本卡判"这段 shell 文案有没有资格出现在这次现场"。文案逐字正确却挂在
//!   分发层现场上，是本卡判死的 ChainCross。
//! * 与 F11666（PATH 逐目录回退）的边界：那张卡立"逐目录序"的正向遍历，
//!   本卡立"探测数守恒"的复核——它数目录怎么走，本卡数回执怎么记。
//! * 与 F11679（场景×码矩阵，后继卡）的边界：那张卡把所有场景钉进码列做
//!   零悬空矩阵，本卡只钉两路 127 与 126 边界这条对照线；矩阵的全场景
//!   覆盖不在本卡展开。
//! * 本卡的排他职责：① 链路归属表（入口 × 命中 → 应然链）；② 探测数
//!   守恒（`value` 维=偏差计数）；③ errno 回执一致性；④ 文案族与链路
//!   同族。四件事在其它任何 B24 卡上都没有落点。
//! * 谁判死谁放行：链路同族、探测守恒、errno 与命中一致、文案族与链路
//!   同族、码落对档——五条件全中放行；任何一条破（RouteCross/ProbeGap/
//!   ErrnoGap/ChainCross/码越档）判死。
//!
//! 判据六维：结束阶段（stage）、退出码（code）、见证偏差数（value）、
//! 文案落点（note）、链路兑现（default）、成败申报（estate）。
//! 判序固定：状态 → 阶段 → 退出码 → 偏差 → 落点 → 链路兑现。
//!
//! 语料 14 槽：8 槽申报正确（分发 1、shell 搜索 2、拒绝 2、执行 2、防御 1），
//! 6 槽复刻同一"直接调用未知名"现场、各挂一维错报（estate/stage/code/value/
//! note/default）。账本 14 格 = 7 统计 + 1 红档率（929）+ 6 缺陷码，
//! 冻结后重放同指纹。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，`audit` 逐格
//! 对账，冻结后 `replay` 重建同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址（F11675，取自全仓分配表，不许自改）。
pub const SEG_BASE: u32 = 0x6d290;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 语料槽数。
pub const CORPUS: usize = 14;

/// 申报正确的槽数。
pub const ACCEPT: usize = 8;

/// 申报错误的槽数。
pub const REJECT: usize = 6;

/// 档位数（执行/搜索/拒绝/分发四档，即链路对照四族）。
pub const TIER_COUNT: usize = 4;

/// 与 F11664（notfound 退出码档）的段位交叉署名：那张卡的段基址。
/// 本卡不 import 对方符号，只复算位距，由 `name_ok()` 校验。
pub const ANCHOR_F11664_SEG: u32 = 0x6d1a0;
/// F11664 与本卡的位距（0x6d290 - 0x6d1a0）。
pub const ANCHOR_F11664_GAP: u32 = 0xF0;
/// 与 F11665（sh command not found 文案）的位距（0x6d290 - 0x6d1b0）。
pub const ANCHOR_F11665_GAP: u32 = 0xE0;
/// 与 F11676（裁剪矩阵重建抽样，本卡后继）的位距。
pub const ANCHOR_F11676_GAP: u32 = 0x10;

// ---------------------------------------------------------------------------
// errno 与退出码
// ---------------------------------------------------------------------------

/// errno：没有系统调用回执（分发层的恒定观测）。
pub const ERR_NONE: u32 = 0;
/// errno：文件不存在——exec 层 PATH 尽头与绝对路径缺失的回执。
pub const ERR_ENOENT: u32 = 2;
/// errno：权限拒绝——命中文件但无执行权，落 126 档。
pub const ERR_EACCES: u32 = 13;
/// errno：格式非法——命中文件但不是可执行镜像，落 126 档。
pub const ERR_ENOEXEC: u32 = 8;

/// 退出码：正常执行。
pub const EXIT_OK: u32 = 0;
/// 退出码：防御档（裸调用/空名 usage）。
pub const EXIT_GUARD: u32 = 1;
/// 退出码：命中被拒族（EACCES/ENOEXEC），POSIX 的 126 档。
pub const EXIT_REFUSED: u32 = 126;
/// 退出码：未找到族（分发层与 exec 层**同码异链**，本卡主锚）。
pub const EXIT_NOTFOUND: u32 = 127;

/// PATH 目录数的合法上界（现场观测上限，超过即坏观测）。
pub const PATH_MAX_DIRS: u32 = 128;

/// errno 合法取值数（NONE/ENOENT/EACCES/ENOEXEC 四值）。
pub const ERRNO_SET_SIZE: usize = 4;

/// busybox 版本号（本卡文案锚定的基准机版本，逐字呈现于 usage 首行）。
pub const BB_VERSION: &str = "BusyBox v1.36.1";

// ---------------------------------------------------------------------------
// 错误码（13 个，SEG_BASE + 0x01..0x0d）
// ---------------------------------------------------------------------------

/// 结束阶段申报不符。
pub const NE_ESTAGE: u32 = SEG_BASE + 0x01;
/// 退出码申报不符。
pub const NE_ECODE: u32 = SEG_BASE + 0x02;
/// 见证偏差数申报不符。
pub const NE_EVALUE: u32 = SEG_BASE + 0x03;
/// 文案落点申报不符。
pub const NE_ENOTE: u32 = SEG_BASE + 0x04;
/// 链路兑现申报不符。
pub const NE_EDEFAULT: u32 = SEG_BASE + 0x05;
/// 链路归属破断：入口族/命中族/文案族三者不同链（结构维）。
pub const NE_ECHAIN: u32 = SEG_BASE + 0x06;
/// 成败互报。
pub const NE_ESTATE: u32 = SEG_BASE + 0x07;
/// 计数不符。
pub const NE_ECOUNT: u32 = SEG_BASE + 0x08;
/// 槽位非法。
pub const NE_ESLOT: u32 = SEG_BASE + 0x09;
/// 指纹不符。
pub const NE_ESEAL: u32 = SEG_BASE + 0x0a;
/// 无观测。
pub const NE_ENOBS: u32 = SEG_BASE + 0x0b;
/// 矩阵不平。
pub const NE_EMATRIX: u32 = SEG_BASE + 0x0c;
/// 账本不平。
pub const NE_EBAL: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码（账本格的操作归类：对应七道闸 + 语料 + 重放）
// ---------------------------------------------------------------------------

/// 名字防线段。
pub const OP_LEX: u8 = 0x01;
/// 链路归属段。
pub const OP_ROUTE: u8 = 0x02;
/// applet 表查询段。
pub const OP_TABLE: u8 = 0x03;
/// PATH 遍历段。
pub const OP_SCAN: u8 = 0x04;
/// execve 回执段。
pub const OP_EXEC: u8 = 0x05;
/// 文案族段。
pub const OP_REPORT: u8 = 0x06;
/// 退出码聚合段。
pub const OP_EXIT: u8 = 0x07;
/// 语料全跑。
pub const OP_CORPUS: u8 = 0x08;
/// 账本重放。
pub const OP_REPLAY: u8 = 0x09;

// ---------------------------------------------------------------------------
// FNV-1a 指纹
// ---------------------------------------------------------------------------

/// FNV 偏移基。
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV 素数。
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个字节。
fn fold_byte(h: u64, b: u8) -> u64 {
    let mut v = h ^ (b as u64);
    let mut k = 0usize;
    while k < 8 {
        v = v.wrapping_mul(FNV_PRIME);
        k += 1;
    }
    v
}

/// 折入一个字（大端逐字节）。
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

/// 折入一个字符串。
fn fold_str(h: u64, s: &str) -> u64 {
    let bytes = s.as_bytes();
    let mut v = h;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            v = fold_byte(v, *b);
        }
        i += 1;
    }
    v
}

/// 折入一个小整数（与 fold_word 同构展宽）。
fn fold_small(h: u64, v: u8) -> u64 {
    fold_word(h, v as u64)
}

// ---------------------------------------------------------------------------
// 数字/布尔文本
// ---------------------------------------------------------------------------

/// 十进制文本（u32，逐位从后往前填，不递归）。
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

/// 定宽小写十六进制文本。
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

/// 有符号十进制文本（探测数差值可正可负，符号必须可见）。
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

// ---------------------------------------------------------------------------
// 字节基元（文案逐字比对与合法域检查用，全部手写，不借 std）
// ---------------------------------------------------------------------------

/// 文本字节数。
pub fn text_len(s: &str) -> usize {
    s.as_bytes().len()
}

/// 是否含某字节。
pub fn has_byte(s: &str, b: u8) -> bool {
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(c) = bytes.get(i) {
            if *c == b {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 逐字节全等（判据里不用 `==` 直接比 String，避免分配器行为掺进比对面）。
pub fn same_text(a: &str, b: &str) -> bool {
    let x = a.as_bytes();
    let y = b.as_bytes();
    if x.len() != y.len() {
        return false;
    }
    let mut i = 0usize;
    while i < x.len() {
        match (x.get(i), y.get(i)) {
            (Some(p), Some(q)) => {
                if p != q {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    true
}

/// 从 from 起找子串，返回首个字节下标。
pub fn find_from(hay: &str, needle: &str, from: usize) -> Option<usize> {
    let h = hay.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() || from >= h.len() || n.len() > h.len() {
        return None;
    }
    let mut i = from;
    while i + n.len() <= h.len() {
        let mut j = 0usize;
        let mut hit = true;
        while j < n.len() {
            match (h.get(i + j), n.get(j)) {
                (Some(a), Some(b)) => {
                    if a != b {
                        hit = false;
                        break;
                    }
                }
                _ => {
                    hit = false;
                    break;
                }
            }
            j += 1;
        }
        if hit {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// 名字是否含斜杠（绝对/相对路径形态——那是 exec 链路的形状，不是表名）。
pub fn name_has_slash(s: &str) -> bool {
    has_byte(s, b'/')
}

/// errno 的 strerror 文本（glibc/busybox 共用 C 库原文，逐字）。
pub fn strerror_text(e: u32) -> &'static str {
    match e {
        ERR_NONE => "(no errno)",
        ERR_ENOENT => "No such file or directory",
        ERR_EACCES => "Permission denied",
        ERR_ENOEXEC => "Exec format error",
        _ => "Unknown error",
    }
}

/// errno 是否在本卡合法域（0/2/8/13 四值）。
pub fn errno_legal(e: u32) -> bool {
    e == ERR_NONE || e == ERR_ENOENT || e == ERR_EACCES || e == ERR_ENOEXEC
}

/// 链名呈现（审计文本用它点名链路）。
pub fn lane_name(l: Lane) -> &'static str {
    match l {
        Lane::TableChain => "dispatch",
        Lane::ExecveChain => "execve",
        Lane::SilentChain => "silent",
        Lane::GuardChain => "guard",
    }
}

// ---------------------------------------------------------------------------
// 文案（真实 busybox / shell 表面措辞逐字复刻 + 本卡自研审计文案）
// ---------------------------------------------------------------------------

/// busybox 分发层未命中报错——`applet '<名>' not found`（F11661/F11674 同源模板）。
pub fn msg_applet_not_found(name: &str) -> String {
    let mut s = String::from("applet '");
    s.push_str(name);
    s.push_str("' not found");
    s
}

/// ash 外部命令未找到——`sh: <名>: not found`（与 F11665 同码同族）。
pub fn msg_sh_not_found(name: &str) -> String {
    let mut s = String::from("sh: ");
    s.push_str(name);
    s.push_str(": not found");
    s
}

/// exec 层命中但被拒的 shell 呈现——`sh: <名>: <strerror>`。
/// EACCES 与 ENOEXEC 走同一模板，只换 strerror 段，逐字可复算。
pub fn msg_sh_cannot(name: &str, errno_: u32) -> String {
    let mut s = String::from("sh: ");
    s.push_str(name);
    s.push_str(": ");
    s.push_str(strerror_text(errno_));
    s
}

/// busybox 裸调用 usage 首行（防御档义务文本）。
pub fn msg_usage_head() -> String {
    let mut s = String::from(BB_VERSION);
    s.push_str(" multi-call binary.");
    s
}

/// busybox 裸调用 usage 用法行（第二行，逐字）。
pub fn msg_usage_line() -> String {
    String::from("usage: busybox [function] [arguments]...")
}

/// 防御档全文（两行以 \n 分隔，结尾无换行）。
pub fn msg_usage_full() -> String {
    let mut s = msg_usage_head();
    s.push('\n');
    s.push_str(msg_usage_line().as_str());
    s
}

/// 本卡自研审计文案：分发层现场混进了 exec 层见证（链路冒充）。
pub fn msg_chain_cross(want: Lane, got: Lane) -> String {
    let mut s = String::from("lane attribution crossed (want ");
    s.push_str(lane_name(want));
    s.push_str(", reported ");
    s.push_str(lane_name(got));
    s.push(')');
    s
}

/// 本卡自研审计文案：探测数与现场推定值不平。
pub fn msg_probe_gap(want: u32, got: u32) -> String {
    let mut s = String::from("probe count mismatch (expected ");
    s.push_str(dec_text(want).as_str());
    s.push_str(", reported ");
    s.push_str(dec_text(got).as_str());
    s.push(')');
    s
}

/// 本卡自研审计文案：errno 回执与命中状态不一致。
pub fn msg_errno_gap(want: u32, got: u32) -> String {
    let mut s = String::from("errno witness mismatch (expected ");
    s.push_str(dec_text(want).as_str());
    s.push_str(" [");
    s.push_str(strerror_text(want));
    s.push_str("], reported ");
    s.push_str(dec_text(got).as_str());
    s.push_str(" [");
    s.push_str(strerror_text(got));
    s.push_str("])");
    s
}

/// 本卡自研审计文案：文案族与链路不同族。
pub fn msg_text_family(want: &'static str, got: &'static str) -> String {
    let mut s = String::from("message family mismatch (want ");
    s.push_str(want);
    s.push_str(", got ");
    s.push_str(got);
    s.push(')');
    s
}

/// 本卡自研审计文案：126 与 127 越档（码族错）。
pub fn msg_code_lane_drift(code: u32, lane: Lane) -> String {
    let mut s = String::from("exit code ");
    s.push_str(dec_text(code).as_str());
    s.push_str(" is incompatible with lane ");
    s.push_str(lane_name(lane));
    s
}

/// 段基址的定宽十六进制呈现（审计文本用，宽度 5，逐字 `0x` 前缀）。
pub fn seg_text() -> String {
    let mut s = String::from("0x");
    s.push_str(hex_text(SEG_BASE, 5).as_str());
    s
}

// ---------------------------------------------------------------------------
// 枚举：入口方式（这次调用怎么到达——决定谁有权产出系统调用回执）
// ---------------------------------------------------------------------------

/// 调用入口。分发入口永远不该有 execve 回执；exec 入口的每一次探测都有回执。
/// 本卡把"入口"立成链路归属的第一维，就是因为 127 的码值对它完全无感。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry {
    /// `busybox <名字>`：多路调用，applet 表是全部世界，零系统调用。
    BusyboxDispatch,
    /// `sh -c '<命令>'`：PATH 逐目录 execve 探测，每目录一张回执。
    ShellPath,
    /// 绝对/显式路径直接 execve：一次探测、一张回执，不遍历 PATH。
    AbsoluteExec,
}

impl Entry {
    /// 文本。
    pub fn text(&self) -> &'static str {
        match self {
            Entry::BusyboxDispatch => "busybox-dispatch",
            Entry::ShellPath => "shell-path",
            Entry::AbsoluteExec => "absolute-exec",
        }
    }

    /// 是否 exec 链路（有权产生 errno 与探测计数）。
    pub fn exec_lane(&self) -> bool {
        match self {
            Entry::ShellPath | Entry::AbsoluteExec => true,
            Entry::BusyboxDispatch => false,
        }
    }

    /// 是否分发链路（applet 表专属）。
    pub fn table_lane(&self) -> bool {
        match self {
            Entry::BusyboxDispatch => true,
            Entry::ShellPath | Entry::AbsoluteExec => false,
        }
    }

    /// 是否会遍历 PATH（绝对路径只打一次靶）。
    pub fn scans_path(&self) -> bool {
        match self {
            Entry::ShellPath => true,
            Entry::BusyboxDispatch | Entry::AbsoluteExec => false,
        }
    }

    /// 该入口下"未找到"的应然码：两条链同为 127（本卡主锚的同码前提）。
    pub fn miss_code(&self) -> u32 {
        match self {
            Entry::BusyboxDispatch | Entry::ShellPath | Entry::AbsoluteExec => EXIT_NOTFOUND,
        }
    }

    /// 该入口下"未找到"的文案族名。
    pub fn miss_text_family(&self) -> &'static str {
        match self {
            Entry::BusyboxDispatch => "applet-table",
            Entry::ShellPath | Entry::AbsoluteExec => "shell",
        }
    }
}

/// 入口方式在册数。
pub const ENTRY_COUNT: usize = 3;

// ---------------------------------------------------------------------------
// 枚举：命中状态（现场到底发生了什么——链路第二维）
// ---------------------------------------------------------------------------

/// 名字在世界里的真实下场。分发族三态与 exec 族四态互斥，route 闸判的就是
/// 这两族与入口的搭配是否同族。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    /// 防御档：空名/裸调用，链路尚未选定就被 lex 闸拦下。
    Guarded,
    /// 分发层：applet 表查不到（真未知、被裁、大小写错都在此族，分档归邻卡）。
    AppletMiss,
    /// 分发层：applet 表命中并成功执行。
    AppletHit,
    /// exec 层：所有候选目录全部 ENOENT（PATH 尽头）。
    MissAll,
    /// exec 层：命中文件但无执行权限（EACCES → 126）。
    HitPerm,
    /// exec 层：命中文件但镜像格式非法（ENOEXEC → 126）。
    HitFormat,
    /// exec 层：命中且 execve 成功。
    HitGood,
}

impl Hit {
    /// 文本。
    pub fn text(&self) -> &'static str {
        match self {
            Hit::Guarded => "guarded",
            Hit::AppletMiss => "applet-miss",
            Hit::AppletHit => "applet-hit",
            Hit::MissAll => "miss-all",
            Hit::HitPerm => "hit-perm",
            Hit::HitFormat => "hit-format",
            Hit::HitGood => "hit-good",
        }
    }

    /// 是否分发表族（只属于 applet 表世界）。
    pub fn table_family(&self) -> bool {
        match self {
            Hit::AppletMiss | Hit::AppletHit => true,
            Hit::Guarded | Hit::MissAll | Hit::HitPerm | Hit::HitFormat | Hit::HitGood => false,
        }
    }

    /// 是否文件系统族（只有碰过 execve 才有脸出现）。
    pub fn fs_family(&self) -> bool {
        match self {
            Hit::MissAll | Hit::HitPerm | Hit::HitFormat | Hit::HitGood => true,
            Hit::Guarded | Hit::AppletMiss | Hit::AppletHit => false,
        }
    }

    /// 是否 127 族（两链同码的那一族）。
    pub fn notfound_family(&self) -> bool {
        match self {
            Hit::AppletMiss | Hit::MissAll => true,
            Hit::Guarded | Hit::AppletHit | Hit::HitPerm | Hit::HitFormat | Hit::HitGood => false,
        }
    }

    /// 是否 126 族（命中被拒）。
    pub fn refused_family(&self) -> bool {
        match self {
            Hit::HitPerm | Hit::HitFormat => true,
            Hit::Guarded | Hit::AppletMiss | Hit::AppletHit | Hit::MissAll | Hit::HitGood => false,
        }
    }

    /// 该命中状态对应的应然 errno 回执。
    pub fn errno(&self) -> u32 {
        match self {
            Hit::MissAll => ERR_ENOENT,
            Hit::HitPerm => ERR_EACCES,
            Hit::HitFormat => ERR_ENOEXEC,
            Hit::Guarded | Hit::AppletMiss | Hit::AppletHit | Hit::HitGood => ERR_NONE,
        }
    }

    /// 该命中状态是否要求"探过目录"（exec 层检索义务）。
    pub fn wants_probe(&self) -> bool {
        match self {
            Hit::MissAll | Hit::HitPerm | Hit::HitFormat | Hit::HitGood => true,
            Hit::Guarded | Hit::AppletMiss | Hit::AppletHit => false,
        }
    }
}

/// 命中状态在册数。
pub const HIT_COUNT: usize = 7;

// ---------------------------------------------------------------------------
// 枚举：链路归属
// ---------------------------------------------------------------------------

/// 现场推定的链路归属——本卡全部"对照"的裁判基准。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lane {
    /// 分发链：applet 表是证人，零 errno、零探测。
    TableChain,
    /// exec 链：内核回执是证人，逐目录有账。
    ExecveChain,
    /// 静默链：正常执行，没有报错义务。
    SilentChain,
    /// 防御链：lex 闸就退，谁也不欠。
    GuardChain,
}

impl Lane {
    /// 文本。
    pub fn text(&self) -> &'static str {
        match self {
            Lane::TableChain => "table",
            Lane::ExecveChain => "execve",
            Lane::SilentChain => "silent",
            Lane::GuardChain => "guard",
        }
    }

    /// 该链是否有权出现 errno 回执。
    pub fn may_witness(&self) -> bool {
        match self {
            Lane::ExecveChain => true,
            Lane::TableChain | Lane::SilentChain | Lane::GuardChain => false,
        }
    }

    /// 该链的报错文案族名。
    pub fn text_family(&self) -> &'static str {
        match self {
            Lane::TableChain => "applet-table",
            Lane::ExecveChain => "shell",
            Lane::SilentChain => "none",
            Lane::GuardChain => "usage",
        }
    }

    /// 该链是否属于"127 对照族"（本卡主锚只在这两族之间对照）。
    pub fn contrast_pair(&self) -> bool {
        match self {
            Lane::TableChain | Lane::ExecveChain => true,
            Lane::SilentChain | Lane::GuardChain => false,
        }
    }
}

/// 链路在册数。
pub const LANE_COUNT: usize = 4;

// ---------------------------------------------------------------------------
// 枚举：文案出处（现场实际喊出来的那句来自哪一族）
// ---------------------------------------------------------------------------

/// 报错文案的出处族。report 闸判"文案族与链路同族"，判的就是这个字段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MsgSrc {
    /// 没有文案（正常执行）。
    NoMsg,
    /// applet 表文案族：`applet '<名>' not found`。
    TableText,
    /// shell 文案族：`sh: <名>: ...`。
    ShellText,
    /// usage 文案族：`BusyBox ... multi-call binary.`。
    UsageText,
}

impl MsgSrc {
    /// 文本。
    pub fn text(&self) -> &'static str {
        match self {
            MsgSrc::NoMsg => "none",
            MsgSrc::TableText => "applet-table",
            MsgSrc::ShellText => "shell",
            MsgSrc::UsageText => "usage",
        }
    }

    /// 是否报错族（有义务喊出声的三族之外的 NoMsg 即无声）。
    pub fn noisy(&self) -> bool {
        match self {
            MsgSrc::NoMsg => false,
            MsgSrc::TableText | MsgSrc::ShellText | MsgSrc::UsageText => true,
        }
    }

    /// 该文案族出自哪条链（NoMsg 无链）。
    pub fn home_lane(&self) -> Lane {
        match self {
            MsgSrc::TableText => Lane::TableChain,
            MsgSrc::ShellText => Lane::ExecveChain,
            MsgSrc::UsageText => Lane::GuardChain,
            MsgSrc::NoMsg => Lane::SilentChain,
        }
    }
}

/// 文案出处在册数。
pub const MSG_COUNT: usize = 4;

// ---------------------------------------------------------------------------
// 枚举：闸门阶段
// ---------------------------------------------------------------------------

/// 闸门阶段（纵线七段，秩 0..6）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// 名字防线（空名/裸调用定案）。
    Lex,
    /// 链路归属（入口族 × 命中族 同族裁决）。
    Route,
    /// applet 表查询（分发层专属）。
    Table,
    /// PATH 逐目录遍历（探测数守恒）。
    Scan,
    /// execve 回执（errno 一致性）。
    Exec,
    /// 文案族归属（链路投影）。
    Report,
    /// 退出码聚合。
    Exit,
}

impl Stage {
    /// 次序（纵线不可倒）。
    pub fn order(&self) -> u8 {
        match self {
            Stage::Lex => 0,
            Stage::Route => 1,
            Stage::Table => 2,
            Stage::Scan => 3,
            Stage::Exec => 4,
            Stage::Report => 5,
            Stage::Exit => 6,
        }
    }

    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Stage::Lex => "lex",
            Stage::Route => "route",
            Stage::Table => "table",
            Stage::Scan => "scan",
            Stage::Exec => "exec",
            Stage::Report => "report",
            Stage::Exit => "exit",
        }
    }

    /// 是否失败落点（Exit 之外都算某道闸拦下）。
    pub fn failure(&self) -> bool {
        match self {
            Stage::Exit => false,
            _ => true,
        }
    }

    /// 是否见证面闸门（scan/exec/report 三段直接对账系统调用侧证据）。
    pub fn witness_plane(&self) -> bool {
        match self {
            Stage::Scan | Stage::Exec | Stage::Report => true,
            Stage::Lex | Stage::Route | Stage::Table | Stage::Exit => false,
        }
    }

    /// 是否定案闸门（lex/route/table 三段产出链路义务，不看回执）。
    pub fn decides_lane(&self) -> bool {
        match self {
            Stage::Lex | Stage::Route | Stage::Table => true,
            Stage::Scan | Stage::Exec | Stage::Report | Stage::Exit => false,
        }
    }
}

/// 阶段总数。
pub const STAGE_COUNT: usize = 7;

// ---------------------------------------------------------------------------
// 枚举：文案落点
// ---------------------------------------------------------------------------

/// 现场形态分类。right 族 = 链路如实呈现，defect 族 = 对照线破断。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Note {
    /// 正常执行（两链皆有，无声）。
    RanOk,
    /// 分发层 notfound 如实呈现（`applet ... not found`，127）。
    AppletMiss,
    /// exec 层未找到如实呈现（`sh: ...: not found`，127）。
    PathMiss,
    /// 命中被拒如实呈现（Permission denied，126）。
    PermDenied,
    /// 镜像非法如实呈现（Exec format error，126）。
    FormatErr,
    /// 防御档 usage 如实呈现（1）。
    GuardRefused,
    /// 链路冒充：文案族/errno/命中三者不同链——判死。
    ChainCross,
    /// 探测数不守恒：PATH 遍历的账不平——判死。
    ProbeGap,
    /// errno 回执与命中状态矛盾——判死。
    ErrnoGap,
}

impl Note {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Note::RanOk => "ran-ok",
            Note::AppletMiss => "applet-miss-reported",
            Note::PathMiss => "path-miss-reported",
            Note::PermDenied => "perm-denied-reported",
            Note::FormatErr => "format-err-reported",
            Note::GuardRefused => "guard-usage",
            Note::ChainCross => "lane-crossed",
            Note::ProbeGap => "probe-gap",
            Note::ErrnoGap => "errno-gap",
        }
    }

    /// 是否工具自认成功。
    pub fn success(&self) -> bool {
        match self {
            Note::RanOk => true,
            _ => false,
        }
    }

    /// 是否如实族（链路/回执/文案三者同族的六档）。
    pub fn right_family(&self) -> bool {
        match self {
            Note::RanOk
            | Note::AppletMiss
            | Note::PathMiss
            | Note::PermDenied
            | Note::FormatErr
            | Note::GuardRefused => true,
            Note::ChainCross | Note::ProbeGap | Note::ErrnoGap => false,
        }
    }

    /// 是否对照破断族（本卡判死的三档）。
    pub fn defect_family(&self) -> bool {
        match self {
            Note::ChainCross | Note::ProbeGap | Note::ErrnoGap => true,
            _ => false,
        }
    }

    /// 是否 127 两族（同码异链的那对，本卡主锚）。
    pub fn notfound_pair(&self) -> bool {
        match self {
            Note::AppletMiss | Note::PathMiss => true,
            _ => false,
        }
    }

    /// 是否 126 两族（命中被拒对）。
    pub fn refused_pair(&self) -> bool {
        match self {
            Note::PermDenied | Note::FormatErr => true,
            _ => false,
        }
    }

    /// 落点归属的报错文案族名（审计文本用）。
    pub fn text_family(&self) -> &'static str {
        match self {
            Note::AppletMiss | Note::ChainCross => "applet-table/shell",
            Note::PathMiss | Note::PermDenied | Note::FormatErr => "shell",
            Note::GuardRefused => "usage",
            Note::RanOk => "none",
            Note::ProbeGap | Note::ErrnoGap => "none(witness)",
        }
    }
}

/// 落点在册数。
pub const NOTE_COUNT: usize = 9;

// ---------------------------------------------------------------------------
// 枚举：结果档位
// ---------------------------------------------------------------------------

/// 结果档位。四档即对照线的四族：执行 / exec 搜索 / 命中被拒 / 分发未命中。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// 码 0：正常执行。
    Ran,
    /// 码 127 且文案族为 shell：exec 层搜索族。
    Search,
    /// 码 126/1：命中被拒与防御档（非 127 的失败族）。
    Refused,
    /// 码 127 且文案族为 applet 表：分发层族。
    Dispatch,
}

impl Tier {
    /// 由结果投影。
    ///
    /// 分档只看码与文案族；红档槽仍留在现场所在档——把谎报 126 的槽挪去
    /// Refused 档，等于替申报者销毁"分发层现场"的案发现场。
    pub fn of(o: &Outcome) -> Tier {
        if o.code == EXIT_NOTFOUND {
            if o.note == Note::AppletMiss {
                Tier::Dispatch
            } else {
                Tier::Search
            }
        } else if o.code == EXIT_OK {
            Tier::Ran
        } else {
            Tier::Refused
        }
    }

    /// 次序。
    pub fn index(&self) -> usize {
        match self {
            Tier::Ran => 0,
            Tier::Search => 1,
            Tier::Refused => 2,
            Tier::Dispatch => 3,
        }
    }

    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Tier::Ran => "ran",
            Tier::Search => "search",
            Tier::Refused => "refused",
            Tier::Dispatch => "dispatch",
        }
    }

    /// 是否成功档。
    pub fn success(&self) -> bool {
        match self {
            Tier::Ran => true,
            Tier::Search | Tier::Refused | Tier::Dispatch => false,
        }
    }

    /// 该档的应然码（Dispatch/Search 共 127 是本卡主锚的码维）。
    pub fn code(&self) -> u32 {
        match self {
            Tier::Ran => EXIT_OK,
            Tier::Search => EXIT_NOTFOUND,
            Tier::Refused => EXIT_REFUSED,
            Tier::Dispatch => EXIT_NOTFOUND,
        }
    }

    /// 是否 127 双档（对照线的两族）。
    pub fn notfound_side(&self) -> bool {
        match self {
            Tier::Search | Tier::Dispatch => true,
            Tier::Ran | Tier::Refused => false,
        }
    }
}

// ---------------------------------------------------------------------------
// 枚举：申报缺陷
// ---------------------------------------------------------------------------

/// 申报缺陷。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// 一致。
    Ok,
    /// 阶段错。
    Stage,
    /// 退出码错。
    Code,
    /// 见证偏差数错。
    Value,
    /// 落点错。
    Note,
    /// 链路兑现错。
    Default,
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
            Fault::Stage => NE_ESTAGE,
            Fault::Code => NE_ECODE,
            Fault::Value => NE_EVALUE,
            Fault::Note => NE_ENOTE,
            Fault::Default => NE_EDEFAULT,
            Fault::Estate => NE_ESTATE,
            Fault::Count => NE_ECOUNT,
            Fault::Slot => NE_ESLOT,
            Fault::Seal => NE_ESEAL,
            Fault::Nobs => NE_ENOBS,
            Fault::Matrix => NE_EMATRIX,
            Fault::Bal => NE_EBAL,
        }
    }

    /// 是否非零。
    pub fn alive(&self) -> bool {
        !matches!(self, Fault::Ok)
    }
}

/// 缺陷短名。
pub fn fault_short(f: Fault) -> &'static str {
    match f {
        Fault::Ok => "ok",
        Fault::Stage => "stage",
        Fault::Code => "code",
        Fault::Value => "value",
        Fault::Note => "note",
        Fault::Default => "default",
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
// 形状（本卡输入的全部维度）
// ---------------------------------------------------------------------------

/// 一次调用的形状：入口 + 命中 + 文案出处 + PATH 现场 + 三份见证申报。
///
/// `path_dirs/hit_pos` 是**现场事实**（PATH 有几个目录、命中在第几个），
/// `probes_seen/errno_seen/msg_src` 是这次运行**被申报出来的见证**。
/// define 纵线的职责就是拿现场推定值逐维核对见证。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shape {
    /// 槽标签（唯一）。
    pub tag: &'static str,
    /// 被请求的名字（防御档为空串，exec 族可含斜杠）。
    pub name: &'static str,
    /// 入口方式。
    pub entry: Entry,
    /// 命中状态。
    pub hit: Hit,
    /// 现场 PATH 目录数（分发入口恒为 0）。
    pub path_dirs: u32,
    /// 命中目录序（MissAll 无命中恒为 0）。
    pub hit_pos: u32,
    /// 被申报的 execve 探测次数。
    pub probes_seen: u32,
    /// 被申报的 errno 回执。
    pub errno_seen: u32,
    /// 被申报的文案出处族。
    pub msg_src: MsgSrc,
}

impl Shape {
    /// 默认形状：直接调用命中正常执行。
    pub const fn blank() -> Shape {
        Shape {
            tag: "",
            name: "ls",
            entry: Entry::BusyboxDispatch,
            hit: Hit::AppletHit,
            path_dirs: 0,
            hit_pos: 0,
            probes_seen: 0,
            errno_seen: 0,
            msg_src: MsgSrc::NoMsg,
        }
    }

    /// 换"探测见证"的副本（谓词把同一现场推到不同申报）。
    pub const fn clone_probes(&self, seen: u32) -> Shape {
        Shape {
            tag: self.tag,
            name: self.name,
            entry: self.entry,
            hit: self.hit,
            path_dirs: self.path_dirs,
            hit_pos: self.hit_pos,
            probes_seen: seen,
            errno_seen: self.errno_seen,
            msg_src: self.msg_src,
        }
    }

    /// 换"errno 见证"的副本。
    pub const fn clone_errno(&self, e: u32) -> Shape {
        Shape {
            tag: self.tag,
            name: self.name,
            entry: self.entry,
            hit: self.hit,
            path_dirs: self.path_dirs,
            hit_pos: self.hit_pos,
            probes_seen: self.probes_seen,
            errno_seen: e,
            msg_src: self.msg_src,
        }
    }

    /// 换"文案出处见证"的副本（链路冒充的作案现场）。
    pub const fn clone_msg(&self, m: MsgSrc) -> Shape {
        Shape {
            tag: self.tag,
            name: self.name,
            entry: self.entry,
            hit: self.hit,
            path_dirs: self.path_dirs,
            hit_pos: self.hit_pos,
            probes_seen: self.probes_seen,
            errno_seen: self.errno_seen,
            msg_src: m,
        }
    }

    /// 换"入口方式"的副本（双路对照复算）。
    pub const fn clone_entry(&self, e: Entry) -> Shape {
        Shape {
            tag: self.tag,
            name: self.name,
            entry: e,
            hit: self.hit,
            path_dirs: self.path_dirs,
            hit_pos: self.hit_pos,
            probes_seen: self.probes_seen,
            errno_seen: self.errno_seen,
            msg_src: self.msg_src,
        }
    }

    /// 现场推定的探测次数：PATH 尽头=目录总数；命中=命中前失败数；
    /// 绝对路径=单次靶；分发层=零（本卡的第一对照律）。
    pub fn truth_probes(&self) -> u32 {
        match self.entry {
            Entry::BusyboxDispatch => 0,
            Entry::AbsoluteExec => 0,
            Entry::ShellPath => match self.hit {
                Hit::MissAll => self.path_dirs,
                Hit::HitPerm | Hit::HitFormat | Hit::HitGood => self.hit_pos,
                Hit::Guarded | Hit::AppletMiss | Hit::AppletHit => 0,
            },
        }
    }

    /// 现场推定的 errno 回执。
    pub fn truth_errno(&self) -> u32 {
        if self.entry.table_lane() {
            return ERR_NONE;
        }
        self.hit.errno()
    }

    /// 现场推定的应然码。
    pub fn truth_code(&self) -> u32 {
        match self.hit {
            Hit::Guarded => EXIT_GUARD,
            Hit::AppletMiss | Hit::MissAll => EXIT_NOTFOUND,
            Hit::HitPerm | Hit::HitFormat => EXIT_REFUSED,
            Hit::AppletHit | Hit::HitGood => EXIT_OK,
        }
    }

    /// 现场推定的应然落点。
    pub fn right_note(&self) -> Note {
        match self.hit {
            Hit::Guarded => Note::GuardRefused,
            Hit::AppletMiss => Note::AppletMiss,
            Hit::MissAll => Note::PathMiss,
            Hit::HitPerm => Note::PermDenied,
            Hit::HitFormat => Note::FormatErr,
            Hit::AppletHit | Hit::HitGood => Note::RanOk,
        }
    }

    /// 现场推定的文案出处义务。
    pub fn want_msg(&self) -> MsgSrc {
        match self.msg_obligation() {
            MsgSrc::NoMsg => MsgSrc::NoMsg,
            MsgSrc::TableText => MsgSrc::TableText,
            MsgSrc::ShellText => MsgSrc::ShellText,
            MsgSrc::UsageText => MsgSrc::UsageText,
        }
    }

    /// 文案义务（与 want_msg 同式，单列给对照审计复算）。
    pub fn msg_obligation(&self) -> MsgSrc {
        match self.hit {
            Hit::AppletMiss => MsgSrc::TableText,
            Hit::MissAll | Hit::HitPerm | Hit::HitFormat => MsgSrc::ShellText,
            Hit::Guarded => MsgSrc::UsageText,
            Hit::AppletHit | Hit::HitGood => MsgSrc::NoMsg,
        }
    }

    /// 现场推定的链路归属。
    pub fn lane(&self) -> Lane {
        match self.hit {
            Hit::Guarded => Lane::GuardChain,
            Hit::AppletMiss => Lane::TableChain,
            Hit::AppletHit => Lane::SilentChain,
            Hit::MissAll | Hit::HitPerm | Hit::HitFormat | Hit::HitGood => Lane::ExecveChain,
        }
    }

    /// 入口族与命中族是否同族（route 闸的裁判式）。
    pub fn lane_consistent(&self) -> bool {
        match self.entry {
            Entry::BusyboxDispatch => self.hit == Hit::Guarded || self.hit.table_family(),
            Entry::ShellPath | Entry::AbsoluteExec => self.hit.fs_family(),
        }
    }

    /// 探测见证偏差（绝对差，饱和运算）。
    pub fn probe_delta(&self) -> u32 {
        let t = self.truth_probes();
        let s = self.probes_seen;
        if s > t {
            s - t
        } else {
            t - s
        }
    }

    /// errno 见证偏差（0 或 1）。
    pub fn errno_delta(&self) -> u32 {
        if self.errno_seen == self.truth_errno() {
            0
        } else {
            1
        }
    }

    /// 文案族偏差（0 或 1）。
    pub fn msg_delta(&self) -> u32 {
        if self.msg_src == self.want_msg() {
            0
        } else {
            1
        }
    }

    /// 见证三元组总偏差（value 维的现场定义）。
    pub fn witness_delta(&self) -> u32 {
        self.probe_delta()
            .saturating_add(self.errno_delta())
            .saturating_add(self.msg_delta())
    }

    /// 形状是否合法可建（只禁结构自相矛盾，见证谎报是观测不是坏形状）。
    pub fn valid(&self) -> bool {
        if !errno_legal(self.errno_seen) {
            return false;
        }
        if self.path_dirs > PATH_MAX_DIRS || self.probes_seen > PATH_MAX_DIRS {
            return false;
        }
        if self.hit_pos > self.path_dirs {
            return false;
        }
        // 分发入口没有 PATH 现场。
        if self.entry.table_lane() && self.path_dirs != 0 {
            return false;
        }
        // 绝对入口遍历 PATH 也是越权。
        if self.entry == Entry::AbsoluteExec && self.path_dirs != 0 {
            return false;
        }
        // 空名只许站在防御档。
        if self.name.is_empty() && self.hit != Hit::Guarded {
            return false;
        }
        if !self.name.is_empty() && self.hit == Hit::Guarded {
            return false;
        }
        // 防御档只许分发入口。
        if self.hit == Hit::Guarded && self.entry != Entry::BusyboxDispatch {
            return false;
        }
        // 表族名字不许带斜杠（那是路径形态，归 exec 链）。
        if self.hit.table_family() && name_has_slash(self.name) {
            return false;
        }
        // 分发入口不配文件系统族命中，反之亦然（route 的合法域投影）。
        if !self.lane_consistent() {
            // 例外：故意制造的链路搅局现场留给 define 判死，形状域仍要能表达
            // "分发入口 + 文件系统命中"这种伪证现场——但仅限 errno/msg 见证面，
            // hit 本身与入口异族即坏形状。
            return false;
        }
        // MissAll 无命中位；命中族必须有目录现场（shell 路）。
        if self.hit == Hit::MissAll && self.hit_pos != 0 {
            return false;
        }
        if self.entry == Entry::ShellPath && self.hit.fs_family() && self.path_dirs == 0 {
            return false;
        }
        // 分发层的探测与 errno 现场恒零（现场事实层面不许有伪 PATH）。
        if self.entry.table_lane() && (self.probes_seen > PATH_MAX_DIRS) {
            return false;
        }
        true
    }

    /// 指纹：对全部判定字段敏感。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_str(h, self.tag);
        h = fold_str(h, self.name);
        h = fold_str(h, self.entry.text());
        h = fold_str(h, self.hit.text());
        h = fold_word(h, self.path_dirs as u64);
        h = fold_word(h, self.hit_pos as u64);
        h = fold_word(h, self.probes_seen as u64);
        h = fold_word(h, self.errno_seen as u64);
        fold_str(h, self.msg_src.text())
    }
}

// ---------------------------------------------------------------------------
// 结果
// ---------------------------------------------------------------------------

/// 一次判定的结果（六维）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// 结束阶段（最后拦下这次调用的闸）。
    pub stage: Stage,
    /// 退出码（0/1/126/127）。
    pub code: u32,
    /// 见证偏差数（正确对账恒为 0）。
    pub value: u32,
    /// 文案落点。
    pub note: Note,
    /// 链路兑现位：现场如实走完纵线且有报错义务并为真。
    pub default: bool,
}

impl Outcome {
    /// 空白结果。
    pub const fn blank() -> Outcome {
        Outcome { stage: Stage::Exit, code: EXIT_OK, value: 0, note: Note::RanOk, default: false }
    }

    /// 是否成功（码 0 且落点属成功族）。
    pub fn ok(&self) -> bool {
        self.code == EXIT_OK && self.note.success()
    }

    /// 是否失败。
    pub fn failed(&self) -> bool {
        !self.ok()
    }

    /// 是否 127 族。
    pub fn notfound(&self) -> bool {
        self.code == EXIT_NOTFOUND
    }

    /// 是否 126 族。
    pub fn refused(&self) -> bool {
        self.code == EXIT_REFUSED
    }

    /// 链路兑现归因：兑现位为真必须"停在出口闸、零偏差、落点属如实族"。
    pub fn attributed(&self) -> bool {
        self.default && self.stage == Stage::Exit && self.value == 0 && self.note.right_family()
    }

    /// 码与落点是否同族（127 只配未找到两档与破断档，0 只配 RanOk）。
    pub fn code_note_coherent(&self) -> bool {
        match self.code {
            EXIT_NOTFOUND => {
                self.note == Note::AppletMiss
                    || self.note == Note::PathMiss
                    || self.note == Note::ChainCross
                    || self.note == Note::ProbeGap
                    || self.note == Note::ErrnoGap
            }
            EXIT_REFUSED => {
                self.note == Note::PermDenied
                    || self.note == Note::FormatErr
                    || self.note == Note::ChainCross
                    || self.note == Note::ProbeGap
                    || self.note == Note::ErrnoGap
            }
            EXIT_OK => self.note == Note::RanOk || self.note.defect_family(),
            EXIT_GUARD => self.note == Note::GuardRefused || self.note == Note::ChainCross,
            _ => false,
        }
    }

    /// 码与链是否相容（对照线的码维不变量）。
    pub fn lane_code_ok(&self, lane: Lane) -> bool {
        match lane {
            Lane::TableChain => self.code == EXIT_NOTFOUND || self.code == EXIT_OK || self.code == EXIT_GUARD,
            Lane::ExecveChain => {
                self.code == EXIT_NOTFOUND || self.code == EXIT_REFUSED || self.code == EXIT_OK
            }
            Lane::GuardChain => self.code == EXIT_GUARD,
            Lane::SilentChain => self.code == EXIT_OK,
        }
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.stage.order() as u64);
        h = fold_word(h, self.code as u64);
        h = fold_word(h, self.value as u64);
        h = fold_str(h, self.note.name());
        fold_word(h, self.default as u64)
    }
}

// ---------------------------------------------------------------------------
// 定义纵线（lex → route → table → scan → exec → report → exit）
// ---------------------------------------------------------------------------

/// 判定一次调用在"分发层 vs exec 层双路 127 对照"合同下的收尾结论。
///
/// 每一闸只在上一闸放行后才执行：先定链路（route），再对探测账（scan），
/// 再对回执账（exec），最后对文案账（report）。次序颠倒会把"分发层伪证
/// errno"误报成文案族问题，掩盖链路冒充本身——那是本卡最危险的假证形态。
pub fn define(s: &Shape) -> Outcome {
    let code = s.truth_code();
    let note = s.right_note();
    let lane = s.lane();
    // ---- lex：防御档（链路未选定就退，不许进任何对照） ----
    if s.hit == Hit::Guarded {
        return Outcome { stage: Stage::Lex, code: EXIT_GUARD, value: 0, note: Note::GuardRefused, default: false };
    }
    // ---- route：入口族与命中族不同族即链路搅局（现场级冒充，判死） ----
    if !s.lane_consistent() {
        return Outcome { stage: Stage::Route, code, value: 1, note: Note::ChainCross, default: false };
    }
    // ---- scan：探测数守恒（exec 层的逐目录账，与 F11666 联签） ----
    if s.entry.scans_path() || s.entry == Entry::AbsoluteExec {
        let dp = s.probe_delta();
        if dp > 0 {
            return Outcome { stage: Stage::Scan, code, value: dp, note: Note::ProbeGap, default: false };
        }
    } else {
        // 分发层的多探一分都是伪证（applet 表世界没有 execve）。
        if s.probes_seen != 0 {
            return Outcome { stage: Stage::Scan, code, value: s.probes_seen, note: Note::ProbeGap, default: false };
        }
    }
    // ---- exec：errno 回执与命中一致 ----
    if s.errno_delta() > 0 {
        return Outcome { stage: Stage::Exec, code, value: s.errno_delta(), note: Note::ErrnoGap, default: false };
    }
    // ---- report：文案族与链路同族 ----
    if s.msg_delta() > 0 {
        return Outcome { stage: Stage::Report, code, value: s.msg_delta(), note: Note::ChainCross, default: false };
    }
    // ---- exit：全闸放行，兑现位=有报错义务且如实呈现 ----
    let obligation = note == Note::AppletMiss || note == Note::PathMiss || note == Note::PermDenied
        || note == Note::FormatErr;
    let _ = lane;
    Outcome { stage: Stage::Exit, code, value: 0, note, default: obligation }
}

/// 现场文案（note → 真实文本；成功且无声的落点返回空串）。
pub fn message(s: &Shape, o: &Outcome) -> String {
    match o.note {
        Note::RanOk => String::new(),
        Note::AppletMiss => msg_applet_not_found(s.name),
        Note::PathMiss => msg_sh_not_found(s.name),
        Note::PermDenied => msg_sh_cannot(s.name, ERR_EACCES),
        Note::FormatErr => msg_sh_cannot(s.name, ERR_ENOEXEC),
        Note::GuardRefused => msg_usage_full(),
        Note::ChainCross => msg_chain_cross(s.lane(), s.msg_src.home_lane()),
        Note::ProbeGap => msg_probe_gap(s.truth_probes(), s.probes_seen),
        Note::ErrnoGap => msg_errno_gap(s.truth_errno(), s.errno_seen),
    }
}

/// 对照审计行：只有破断形态才有内容，正确现场返回空串。
pub fn contrast_report(s: &Shape, o: &Outcome) -> String {
    if !o.note.defect_family() {
        return String::new();
    }
    let mut t = String::from("lane=");
    t.push_str(lane_name(s.lane()));
    t.push_str(" want-msg=");
    t.push_str(s.want_msg().text());
    t.push_str(" got-msg=");
    t.push_str(s.msg_src.text());
    t.push_str(" probes=");
    t.push_str(dec_text(s.probes_seen).as_str());
    t.push('/');
    t.push_str(dec_text(s.truth_probes()).as_str());
    t.push_str(" errno=");
    t.push_str(dec_text(s.errno_seen).as_str());
    t.push('/');
    t.push_str(dec_text(s.truth_errno()).as_str());
    t
}

// ---------------------------------------------------------------------------
// 申报与用例
// ---------------------------------------------------------------------------

/// 一槽申报。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Claim {
    /// 结束阶段。
    pub stage: Stage,
    /// 退出码。
    pub code: u32,
    /// 见证偏差数。
    pub value: u32,
    /// 文案落点。
    pub note: Note,
    /// 链路兑现位。
    pub default: bool,
}

impl Claim {
    /// 空白申报。
    pub const fn blank() -> Claim {
        Claim { stage: Stage::Exit, code: EXIT_OK, value: 0, note: Note::RanOk, default: false }
    }

    /// 是否申报成功（码 0 且落点属成功族）。
    pub fn success(&self) -> bool {
        self.code == EXIT_OK && self.note.success()
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
        Case { shape: Shape::blank(), claim: Claim::blank() }
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

/// 8 个可正确申报的现场 + 6 个复刻现场（复刻自 dispatch-miss-127）。
///
/// 复刻槽的**形状逐字相同**——六维错报全部只发生在申报面；这保证红档的
/// 现场族仍是分发层（Tier::of 落 Dispatch），谎报不改变案发现场。
#[rustfmt::skip]
pub const SHAPES: [Shape; CORPUS] = [
    Shape { tag: "dispatch-miss-127", name: "frobnicate", entry: Entry::BusyboxDispatch,
        hit: Hit::AppletMiss, path_dirs: 0, hit_pos: 0,
        probes_seen: 0, errno_seen: 0, msg_src: MsgSrc::TableText },
    Shape { tag: "shell-exhaust-127", name: "frobnicate", entry: Entry::ShellPath,
        hit: Hit::MissAll, path_dirs: 4, hit_pos: 0,
        probes_seen: 4, errno_seen: 2, msg_src: MsgSrc::ShellText },
    Shape { tag: "abs-enoent-127", name: "/tmp/nope", entry: Entry::AbsoluteExec,
        hit: Hit::MissAll, path_dirs: 0, hit_pos: 0,
        probes_seen: 0, errno_seen: 2, msg_src: MsgSrc::ShellText },
    Shape { tag: "shell-perm-126", name: "tool", entry: Entry::ShellPath,
        hit: Hit::HitPerm, path_dirs: 5, hit_pos: 2,
        probes_seen: 2, errno_seen: 13, msg_src: MsgSrc::ShellText },
    Shape { tag: "shell-format-126", name: "img", entry: Entry::ShellPath,
        hit: Hit::HitFormat, path_dirs: 5, hit_pos: 0,
        probes_seen: 0, errno_seen: 8, msg_src: MsgSrc::ShellText },
    Shape { tag: "dispatch-ran-0", name: "ls", entry: Entry::BusyboxDispatch,
        hit: Hit::AppletHit, path_dirs: 0, hit_pos: 0,
        probes_seen: 0, errno_seen: 0, msg_src: MsgSrc::NoMsg },
    Shape { tag: "shell-ran-0", name: "ls", entry: Entry::ShellPath,
        hit: Hit::HitGood, path_dirs: 3, hit_pos: 1,
        probes_seen: 1, errno_seen: 0, msg_src: MsgSrc::NoMsg },
    Shape { tag: "dispatch-guard-1", name: "", entry: Entry::BusyboxDispatch,
        hit: Hit::Guarded, path_dirs: 0, hit_pos: 0,
        probes_seen: 0, errno_seen: 0, msg_src: MsgSrc::UsageText },
    Shape { tag: "estate-swap", name: "frobnicate", entry: Entry::BusyboxDispatch,
        hit: Hit::AppletMiss, path_dirs: 0, hit_pos: 0,
        probes_seen: 0, errno_seen: 0, msg_src: MsgSrc::TableText },
    Shape { tag: "stage-swap", name: "frobnicate", entry: Entry::BusyboxDispatch,
        hit: Hit::AppletMiss, path_dirs: 0, hit_pos: 0,
        probes_seen: 0, errno_seen: 0, msg_src: MsgSrc::TableText },
    Shape { tag: "code-swap", name: "frobnicate", entry: Entry::BusyboxDispatch,
        hit: Hit::AppletMiss, path_dirs: 0, hit_pos: 0,
        probes_seen: 0, errno_seen: 0, msg_src: MsgSrc::TableText },
    Shape { tag: "value-swap", name: "frobnicate", entry: Entry::BusyboxDispatch,
        hit: Hit::AppletMiss, path_dirs: 0, hit_pos: 0,
        probes_seen: 0, errno_seen: 0, msg_src: MsgSrc::TableText },
    Shape { tag: "note-swap", name: "frobnicate", entry: Entry::BusyboxDispatch,
        hit: Hit::AppletMiss, path_dirs: 0, hit_pos: 0,
        probes_seen: 0, errno_seen: 0, msg_src: MsgSrc::TableText },
    Shape { tag: "default-swap", name: "frobnicate", entry: Entry::BusyboxDispatch,
        hit: Hit::AppletMiss, path_dirs: 0, hit_pos: 0,
        probes_seen: 0, errno_seen: 0, msg_src: MsgSrc::TableText },
];

/// 与 `SHAPES` 一一对应的申报。
///
/// 前 8 槽逐维等于现场；后 6 槽各只错一维，错的那维依次是
/// estate/stage/code/value/note/default。note-swap 是本卡主锚的显式形态：
/// 码同为 127、成败同为败，只把分发层文案族换成 shell 文案族——
/// 除本卡外没有任何 B24 卡能抓住这种"同码换链"冒充。
#[rustfmt::skip]
pub const CLAIMS: [Claim; CORPUS] = [
    Claim { stage: Stage::Exit, code: 127, value: 0, note: Note::AppletMiss, default: true },
    Claim { stage: Stage::Exit, code: 127, value: 0, note: Note::PathMiss, default: true },
    Claim { stage: Stage::Exit, code: 127, value: 0, note: Note::PathMiss, default: true },
    Claim { stage: Stage::Exit, code: 126, value: 0, note: Note::PermDenied, default: true },
    Claim { stage: Stage::Exit, code: 126, value: 0, note: Note::FormatErr, default: true },
    Claim { stage: Stage::Exit, code: 0, value: 0, note: Note::RanOk, default: false },
    Claim { stage: Stage::Exit, code: 0, value: 0, note: Note::RanOk, default: false },
    Claim { stage: Stage::Lex, code: 1, value: 0, note: Note::GuardRefused, default: false },
    Claim { stage: Stage::Exit, code: 0, value: 0, note: Note::RanOk, default: false },
    Claim { stage: Stage::Table, code: 127, value: 0, note: Note::AppletMiss, default: true },
    Claim { stage: Stage::Exit, code: 126, value: 0, note: Note::AppletMiss, default: true },
    Claim { stage: Stage::Exit, code: 127, value: 3, note: Note::AppletMiss, default: true },
    Claim { stage: Stage::Exit, code: 127, value: 0, note: Note::PathMiss, default: true },
    Claim { stage: Stage::Exit, code: 127, value: 0, note: Note::AppletMiss, default: false },
];

/// 后 6 槽的期望缺陷。
pub const REJECT_FAULTS: [Fault; REJECT] =
    [Fault::Estate, Fault::Stage, Fault::Code, Fault::Value, Fault::Note, Fault::Default];

/// 后 6 槽的缺陷码。
pub const REJECT_CODES: [u32; REJECT] = [
    NE_ESTATE,
    NE_ESTAGE,
    NE_ECODE,
    NE_EVALUE,
    NE_ENOTE,
    NE_EDEFAULT,
];

/// 账本 8 个成功格的值（总数、成、败、执行档、搜索档、拒绝档、分发档、红档 ppm）。
pub const VALS: [u32; ACCEPT] = [14, 8, 6, 2, 2, 3, 1, 929];

/// 语料表。
pub fn cases() -> [Case; CORPUS] {
    let mut out = [Case::blank(); CORPUS];
    let mut i = 0usize;
    while i < CORPUS {
        if let (Some(s), Some(c)) = (SHAPES.get(i), CLAIMS.get(i)) {
            out[i] = Case { shape: *s, claim: *c };
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
        return Err(NE_ENOBS);
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

/// 对判：状态 → 阶段 → 退出码 → 偏差数 → 落点 → 链路兑现。
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
    let mut s = String::from(c.shape.name);
    s.push_str(" entry=");
    s.push_str(c.shape.entry.text());
    s.push_str(" hit=");
    s.push_str(c.shape.hit.text());
    s.push_str(" lane=");
    s.push_str(lane_name(c.shape.lane()));
    s.push_str(" stage=");
    s.push_str(o.stage.name());
    s.push_str(" exit=");
    s.push_str(dec_text(o.code).as_str());
    s.push_str(" delta=");
    s.push_str(dec_text(o.value).as_str());
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

/// 14 槽的期望结束阶段（校验器按纵线实际会走到哪一闸）。
const WANT_STAGE: [Stage; CORPUS] = [
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
    Stage::Lex,
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
];

/// 14 槽的期望退出码。
const WANT_CODE: [u32; CORPUS] = [127, 127, 127, 126, 126, 0, 0, 1, 127, 127, 127, 127, 127, 127];

/// 14 槽的期望见证偏差数。
const WANT_VALUE: [u32; CORPUS] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

/// 14 槽的期望文案落点。
const WANT_NOTE: [Note; CORPUS] = [
    Note::AppletMiss,
    Note::PathMiss,
    Note::PathMiss,
    Note::PermDenied,
    Note::FormatErr,
    Note::RanOk,
    Note::RanOk,
    Note::GuardRefused,
    Note::AppletMiss,
    Note::AppletMiss,
    Note::AppletMiss,
    Note::AppletMiss,
    Note::AppletMiss,
    Note::AppletMiss,
];

/// 14 槽的期望链路兑现位。
const WANT_DEFAULT: [bool; CORPUS] = [
    true, true, true, true, true, false, false, false, true, true, true, true, true, true,
];

/// 14 槽标签。
const WANT_TAGS: [&str; CORPUS] = [
    "dispatch-miss-127",
    "shell-exhaust-127",
    "abs-enoent-127",
    "shell-perm-126",
    "shell-format-126",
    "dispatch-ran-0",
    "shell-ran-0",
    "dispatch-guard-1",
    "estate-swap",
    "stage-swap",
    "code-swap",
    "value-swap",
    "note-swap",
    "default-swap",
];

/// 八格申报正确的下标。
const ACCEPT_SLOTS: [usize; ACCEPT] = [0usize, 1, 2, 3, 4, 5, 6, 7];

/// 六格申报错误的下标。
const REJECT_SLOTS: [usize; REJECT] = [8usize, 9, 10, 11, 12, 13];

/// 复刻槽（后六槽）的标签后缀。
const SWAP_SUFFIX: &str = "-swap";

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

    /// 该档是否只由复刻槽变红（本卡立卡理由：活槽不许有红）。
    pub fn reds_are_replicas(&self) -> bool {
        self.red() == 0 || self.red() == REJECT
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
/// 分档只看**实际结果**落进哪一档（码 + 文案族），申报正确与否单独计。
/// 本卡红档槽全部复刻自 dispatch-miss-127，其现场仍是分发层，因此全部落
/// Dispatch 档——127 双档（Dispatch=7、Search=2）在矩阵上并排呈现，正是
/// "码同链异"的可见化。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Matrix {
    /// 行（ran/search/refused/dispatch）。
    pub rows: [Row; TIER_COUNT],
}

impl Matrix {
    /// 空矩阵。
    pub const fn blank() -> Matrix {
        Matrix { rows: [Row::blank(); TIER_COUNT] }
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

    /// 覆盖是否平（每档都有槽——四档即对照四族，缺一族即矩阵不平/悬空）。
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

    /// 127 双档合计（分发 + 搜索——对照线的主战场）。
    pub fn notfound_slots(&self) -> usize {
        let mut n = 0usize;
        if let Some(r) = self.rows.get(Tier::Search.index()) {
            n += r.total;
        }
        if let Some(r) = self.rows.get(Tier::Dispatch.index()) {
            n += r.total;
        }
        n
    }

    /// 渲染全矩阵。
    pub fn render(&self) -> String {
        let tiers = [Tier::Ran, Tier::Search, Tier::Refused, Tier::Dispatch];
        let mut s = String::new();
        let mut i = 0usize;
        while i < tiers.len() {
            if let Some(t) = tiers.get(i) {
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
    /// 执行档正确数。
    pub ran: u32,
    /// 搜索档正确数。
    pub search: u32,
    /// 拒绝档正确数。
    pub refused: u32,
    /// 分发档正确数。
    pub dispatch: u32,
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
            Err(NE_ECOUNT)
        }
    }

    /// 码 0 档合计（执行）。
    pub fn zero_cells(&self) -> u32 {
        self.ran
    }

    /// 127 双档正确数（对照兑现面：两路 127 都如实归链）。
    pub fn notfound_cells(&self) -> u32 {
        self.dispatch + self.search
    }

    /// 渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("ne-exec corpus=");
        s.push_str(u32_text(self.total).as_str());
        s.push_str(" accept=");
        s.push_str(u32_text(self.accept).as_str());
        s.push_str(" reject=");
        s.push_str(u32_text(self.reject).as_str());
        s.push_str(" ran=");
        s.push_str(u32_text(self.ran).as_str());
        s.push_str(" search=");
        s.push_str(u32_text(self.search).as_str());
        s.push_str(" refused=");
        s.push_str(u32_text(self.refused).as_str());
        s.push_str(" dispatch=");
        s.push_str(u32_text(self.dispatch).as_str());
        s.push_str(" match=");
        s.push_str(u32_text(self.matched).as_str());
        s.push_str(" rate=");
        s.push_str(u32_text(self.rate()).as_str());
        s.push_str("ppm\n");
        s
    }
}

/// 全绿渲染的逐字钉版。
const RENDER_OK: &str =
    "ne-exec corpus=14 accept=8 reject=6 ran=2 search=2 refused=3 dispatch=1 match=14 rate=1000ppm\n";

/// 全跑。
pub fn run() -> Result<Report, u32> {
    run_with(None)
}

/// 可注入一槽缺陷的全跑。
///
/// 注入的作用是把"期望表"本身也放进被测面：强行宣称某槽犯了另一种缺陷，
/// 报表就必须掉 matched、rate 随之离开 1000。若注入后仍报全绿，说明 matched
/// 是硬编码的，而不是算出来的。
pub fn run_with(over: Option<(usize, Fault)>) -> Result<Report, u32> {
    if let Some((i, _)) = over {
        if i >= CORPUS {
            return Err(NE_ESLOT);
        }
    }
    let cs = cases();
    let mut total = 0u32;
    let mut accept = 0u32;
    let mut reject = 0u32;
    let mut ran = 0u32;
    let mut search = 0u32;
    let mut refused = 0u32;
    let mut dispatch = 0u32;
    let mut matched = 0u32;
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return Err(NE_ENOBS),
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
            None => return Err(NE_ENOBS),
        };
        total += 1;
        if f == want {
            matched += 1;
        }
        if f == Fault::Ok {
            accept += 1;
            match Tier::of(&a) {
                Tier::Ran => ran += 1,
                Tier::Search => search += 1,
                Tier::Refused => refused += 1,
                Tier::Dispatch => dispatch += 1,
            }
        } else {
            reject += 1;
        }
        i += 1;
    }
    Ok(Report { total, accept, reject, ran, search, refused, dispatch, matched })
}

// ---------------------------------------------------------------------------
// 阶段闸门
// ---------------------------------------------------------------------------

/// 单调阶段闸门。
///
/// 观测纵线是 `lex → route → table → scan → exec → report → exit`，
/// 七段只能向前。回退（例如已经对完文案账又声称停在探测闸）说明申报者
/// 不是按闸顺序观测的，而是拿着码倒推链路——这类申报在 127 双路上最危险，
/// 因为码那一维看起来完全合理。
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
        Gate { last: -1, steps: 0, closed: false }
    }

    /// 走一段。
    pub fn step(&mut self, s: Stage) -> Result<(), u32> {
        if self.closed {
            return Err(NE_ESTATE);
        }
        let o = s.order() as i32;
        if o < self.last {
            return Err(NE_ESTAGE);
        }
        self.last = o;
        self.steps = self.steps.saturating_add(1);
        Ok(())
    }

    /// 关闭，返回段数。
    pub fn close(&mut self) -> Result<u32, u32> {
        if self.closed {
            return Err(NE_ESTATE);
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
    /// 槽位（必须严格等于当前长度）。
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
        Sample { slot: 0, op: 0, ok: false, code: 0, value: 0 }
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

    /// 登记一格；槽位不等于当前长度、越界或已冻结一律拒绝并计数。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(NE_ESLOT);
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
        self.len - self.ok_count()
    }

    /// 逐格对账：槽位连续、成格值等于 `VALS`、败格码等于 `REJECT_CODES`、
    /// 败格值必归零。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != ACCEPT || self.err_count() != REJECT {
            return Err(NE_EBAL);
        }
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(NE_EBAL),
            };
            if s.slot as usize != i {
                return Err(NE_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(NE_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(NE_EVALUE);
                        }
                    }
                    None => return Err(NE_EBAL),
                }
                vi += 1;
            } else {
                if s.value != 0 {
                    return Err(NE_EVALUE);
                }
                match REJECT_CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(NE_EVALUE);
                        }
                    }
                    None => return Err(NE_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != ACCEPT || ci != REJECT {
            return Err(NE_EBAL);
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
            return Err(NE_EBAL);
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
            Err(NE_ESEAL)
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
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 建账本：7 格统计 + 1 格红档率（929）+ 6 格缺陷码。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let r = run();
    let _ = l.register(count_cell(0, OP_CORPUS, r.map(|x| x.total)));
    let _ = l.register(count_cell(1, OP_CORPUS, r.map(|x| x.accept)));
    let _ = l.register(count_cell(2, OP_CORPUS, r.map(|x| x.reject)));
    let _ = l.register(count_cell(3, OP_LEX, r.map(|x| x.ran)));
    let _ = l.register(count_cell(4, OP_SCAN, r.map(|x| x.search)));
    let _ = l.register(count_cell(5, OP_EXEC, r.map(|x| x.refused)));
    let _ = l.register(count_cell(6, OP_TABLE, r.map(|x| x.dispatch)));

    let red = run_with(Some((0, Fault::Code)));
    let _ = l.register(count_cell(7, OP_REPLAY, red.map(|x| x.rate())));

    let cs = cases();
    let mut i = 0usize;
    while i < REJECT {
        if let Some(c) = cs.get(i + ACCEPT) {
            let f = judge(c);
            let _ = l.register(Sample {
                slot: (ACCEPT + i) as u8,
                op: OP_REPORT,
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

/// 四路注入：形状表、判据、渲染、账本。
///
/// 四路各自独立，任何一路静默退化都会在这里变成 false：
/// 1. 表路——段基址、枚举、链路归属矩阵、形状合法域；
/// 2. 判路——七闸纵线、双路 127 对照、对判、矩阵；
/// 3. 渲染路——语料、期望表、红档、文案、释义；
/// 4. 账本路——阶段闸门、逐格对账、冻结重放。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = name_ok() && enums_ok() && lanes_ok() && shape_ok();
    out[1] = define_ok() && contrast_ok() && judge_ok() && matrix_ok();
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

/// 段基址与位距的交叉署名复算（本卡与 B24/B21 邻卡的段位闭合）。
pub fn anchors_agree() -> bool {
    if SEG_BASE != 0x6d290 {
        return false;
    }
    if ANCHOR_F11664_SEG.checked_add(ANCHOR_F11664_GAP) != Some(SEG_BASE) {
        return false;
    }
    if ANCHOR_F11665_GAP != SEG_BASE - ANCHOR_F11664_SEG - 0x10 {
        return false;
    }
    ANCHOR_F11676_GAP == 0x10
}

// ---------------------------------------------------------------------------
// 判据基元
// ---------------------------------------------------------------------------

/// 名称与段基址：常量链、尺寸、errno/退出码阶梯、操作码、交叉署名一致性。
fn name_ok() -> bool {
    if SEG_BASE != 0x6d290 {
        return false;
    }
    if NE_ESTAGE != SEG_BASE + 0x01
        || NE_ECODE != SEG_BASE + 0x02
        || NE_EVALUE != SEG_BASE + 0x03
        || NE_ENOTE != SEG_BASE + 0x04
        || NE_EDEFAULT != SEG_BASE + 0x05
        || NE_ECHAIN != SEG_BASE + 0x06
    {
        return false;
    }
    if NE_ESTATE != SEG_BASE + 0x07
        || NE_ECOUNT != SEG_BASE + 0x08
        || NE_ESLOT != SEG_BASE + 0x09
        || NE_ESEAL != SEG_BASE + 0x0a
        || NE_ENOBS != SEG_BASE + 0x0b
        || NE_EMATRIX != SEG_BASE + 0x0c
        || NE_EBAL != SEG_BASE + 0x0d
    {
        return false;
    }
    if CORPUS != 14 || SAMPLES != 14 || ACCEPT != 8 || REJECT != 6 || TIER_COUNT != 4 {
        return false;
    }
    if ERR_NONE != 0 || ERR_ENOENT != 2 || ERR_EACCES != 13 || ERR_ENOEXEC != 8 {
        return false;
    }
    if EXIT_OK != 0 || EXIT_GUARD != 1 || EXIT_REFUSED != 126 || EXIT_NOTFOUND != 127 {
        return false;
    }
    if OP_LEX != 1 || OP_ROUTE != 2 || OP_TABLE != 3 || OP_SCAN != 4 {
        return false;
    }
    if OP_EXEC != 5 || OP_REPORT != 6 || OP_EXIT != 7 || OP_CORPUS != 8 || OP_REPLAY != 9 {
        return false;
    }
    if STAGE_COUNT != 7 || ENTRY_COUNT != 3 || HIT_COUNT != 7 || LANE_COUNT != 4 || MSG_COUNT != 4
    {
        return false;
    }
    if NOTE_COUNT != 9 || ERRNO_SET_SIZE != 4 || PATH_MAX_DIRS != 128 {
        return false;
    }
    if !anchors_agree() {
        return false;
    }
    if strerror_text(ERR_ENOENT) != "No such file or directory" {
        return false;
    }
    if strerror_text(ERR_EACCES) != "Permission denied" {
        return false;
    }
    if strerror_text(ERR_ENOEXEC) != "Exec format error" {
        return false;
    }
    if strerror_text(99) != "Unknown error" {
        return false;
    }
    if !errno_legal(ERR_ENOENT) || errno_legal(9) {
        return false;
    }
    if BB_VERSION != "BusyBox v1.36.1" {
        return false;
    }
    let mut v = String::from("applet '");
    v.push_str("x");
    same_text(v.as_str(), "applet 'x'") == false && seg_text().as_str() == "0x6d290"
}

/// 枚举：七闸次序、落点三族不重不漏、档位投影、入口/命中族方向、缺陷码互斥。
fn enums_ok() -> bool {
    let stages = [
        Stage::Lex,
        Stage::Route,
        Stage::Table,
        Stage::Scan,
        Stage::Exec,
        Stage::Report,
        Stage::Exit,
    ];
    if stages.len() != STAGE_COUNT {
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
        if i + 1 == stages.len() {
            if s.failure() {
                return false;
            }
        } else if !s.failure() {
            return false;
        }
        // 见证面三段：scan/exec/report。
        if s.witness_plane() != (i == 3 || i == 4 || i == 5) {
            return false;
        }
        // 定案三段：lex/route/table。
        if s.decides_lane() != (i < 3) {
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

    let notes = [
        Note::RanOk,
        Note::AppletMiss,
        Note::PathMiss,
        Note::PermDenied,
        Note::FormatErr,
        Note::GuardRefused,
        Note::ChainCross,
        Note::ProbeGap,
        Note::ErrnoGap,
    ];
    if notes.len() != NOTE_COUNT {
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
        // 两族互斥且完备：如实族 + 破断族 = 恰一。
        if a.right_family() == a.defect_family() {
            return false;
        }
        // 127 双档必须都是如实族（对照线主锚）。
        if a.notfound_pair() && !a.right_family() {
            return false;
        }
        if a.refused_pair() && !a.right_family() {
            return false;
        }
        if a.success() && (a.defect_family() || !a.right_family()) {
            return false;
        }
        if a.success() != (a == Note::RanOk) {
            return false;
        }
        n += 1;
    }
    if Note::AppletMiss.name() != "applet-miss-reported" {
        return false;
    }
    if Note::PathMiss.name() != "path-miss-reported" {
        return false;
    }
    // 127 双档同码是本卡主锚的枚举级证明。
    if Note::AppletMiss.notfound_pair() != Note::PathMiss.notfound_pair() {
        return false;
    }
    if Note::AppletMiss.notfound_pair() == false {
        return false;
    }

    let tiers = [Tier::Ran, Tier::Search, Tier::Refused, Tier::Dispatch];
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
        // 127 双档必须码值全等（对照的码维）。
        if x.notfound_side() != (t == 1 || t == 3) {
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
    if Tier::Dispatch.code() != Tier::Search.code() {
        return false;
    }
    if Tier::Refused.code() != EXIT_REFUSED {
        return false;
    }

    // 入口方向：两路 miss 同码 127；只有 shell 路遍历 PATH。
    if Entry::BusyboxDispatch.miss_code() != EXIT_NOTFOUND {
        return false;
    }
    if Entry::ShellPath.miss_code() != EXIT_NOTFOUND {
        return false;
    }
    if Entry::AbsoluteExec.miss_code() != EXIT_NOTFOUND {
        return false;
    }
    if Entry::BusyboxDispatch.exec_lane() {
        return false;
    }
    if !Entry::BusyboxDispatch.table_lane() {
        return false;
    }
    if !Entry::AbsoluteExec.exec_lane() || Entry::AbsoluteExec.table_lane() {
        return false;
    }
    if !Entry::ShellPath.scans_path() {
        return false;
    }
    if Entry::AbsoluteExec.scans_path() || Entry::BusyboxDispatch.scans_path() {
        return false;
    }
    if Entry::BusyboxDispatch.miss_text_family() != "applet-table" {
        return false;
    }
    if Entry::ShellPath.miss_text_family() != "shell" {
        return false;
    }

    // 命中族方向：表族与文件系统族互斥；errno 阶梯钉死。
    let hits = [
        Hit::Guarded,
        Hit::AppletMiss,
        Hit::AppletHit,
        Hit::MissAll,
        Hit::HitPerm,
        Hit::HitFormat,
        Hit::HitGood,
    ];
    if hits.len() != HIT_COUNT {
        return false;
    }
    let mut h = 0usize;
    while h < hits.len() {
        let x = match hits.get(h) {
            Some(v) => *v,
            None => return false,
        };
        if x.table_family() && x.fs_family() {
            return false;
        }
        if x.notfound_family() && x.refused_family() {
            return false;
        }
        if !errno_legal(x.errno()) {
            return false;
        }
        if x.wants_probe() && !x.fs_family() {
            return false;
        }
        if x.fs_family() && !x.wants_probe() {
            return false;
        }
        let mut y = h + 1;
        while y < hits.len() {
            if let Some(z) = hits.get(y) {
                if x.text() == z.text() {
                    return false;
                }
            }
            y += 1;
        }
        h += 1;
    }
    if Hit::MissAll.errno() != ERR_ENOENT || Hit::HitPerm.errno() != ERR_EACCES {
        return false;
    }
    if Hit::HitFormat.errno() != ERR_ENOEXEC || Hit::AppletMiss.errno() != ERR_NONE {
        return false;
    }
    if !Hit::AppletMiss.notfound_family() || Hit::AppletHit.notfound_family() {
        return false;
    }
    if !Hit::MissAll.fs_family() || Hit::MissAll.table_family() {
        return false;
    }

    // 文案族归链：NoMsg 归静默、TableText 归分发、ShellText 归 exec、Usage 归防御。
    if MsgSrc::NoMsg.home_lane() != Lane::SilentChain {
        return false;
    }
    if MsgSrc::TableText.home_lane() != Lane::TableChain {
        return false;
    }
    if MsgSrc::ShellText.home_lane() != Lane::ExecveChain {
        return false;
    }
    if MsgSrc::UsageText.home_lane() != Lane::GuardChain {
        return false;
    }
    if MsgSrc::NoMsg.noisy() {
        return false;
    }
    if !MsgSrc::TableText.noisy() || !MsgSrc::ShellText.noisy() {
        return false;
    }
    // 链路对照对：只有分发链与 exec 链构成 127 对照。
    if !Lane::TableChain.contrast_pair() || !Lane::ExecveChain.contrast_pair() {
        return false;
    }
    if Lane::SilentChain.contrast_pair() || Lane::GuardChain.contrast_pair() {
        return false;
    }
    if Lane::TableChain.may_witness() {
        return false;
    }
    if !Lane::ExecveChain.may_witness() {
        return false;
    }

    let faults = [
        Fault::Ok,
        Fault::Stage,
        Fault::Code,
        Fault::Value,
        Fault::Note,
        Fault::Default,
        Fault::Estate,
        Fault::Count,
        Fault::Slot,
        Fault::Seal,
        Fault::Nobs,
        Fault::Matrix,
        Fault::Bal,
    ];
    let mut f = 0usize;
    let mut seen = [0u32; 13];
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
        && Fault::Stage.code() == NE_ESTAGE
        && Fault::Estate.code() == NE_ESTATE
        && Fault::Bal.code() == NE_EBAL
}

/// 链路归属矩阵：入口 × 命中 → 合法搭配表、链路推定、码族相容。
fn lanes_ok() -> bool {
    // 分发入口的合法命中域 = 表族 + 防御档；exec 入口 = 文件系统族。
    let dispatch_ok_hits = [Hit::Guarded, Hit::AppletMiss, Hit::AppletHit];
    let exec_ok_hits = [Hit::MissAll, Hit::HitPerm, Hit::HitFormat, Hit::HitGood];
    let mut i = 0usize;
    while i < dispatch_ok_hits.len() {
        let h = match dispatch_ok_hits.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let mut s = Shape::blank();
        s.entry = Entry::BusyboxDispatch;
        s.hit = h;
        s.name = if h == Hit::Guarded { "" } else { "ls" };
        if !s.lane_consistent() {
            return false;
        }
        if h == Hit::Guarded && s.lane() != Lane::GuardChain {
            return false;
        }
        if h == Hit::AppletMiss && s.lane() != Lane::TableChain {
            return false;
        }
        if h == Hit::AppletHit && s.lane() != Lane::SilentChain {
            return false;
        }
        i += 1;
    }
    let mut j = 0usize;
    while j < exec_ok_hits.len() {
        let h = match exec_ok_hits.get(j) {
            Some(v) => *v,
            None => return false,
        };
        let mut s = Shape::blank();
        s.entry = Entry::ShellPath;
        s.hit = h;
        s.name = "tool";
        s.path_dirs = 2;
        s.hit_pos = if s.hit == Hit::HitGood { 1 } else { 0 };
        if s.hit == Hit::MissAll {
            s.hit_pos = 0;
        }
        if !s.lane_consistent() {
            return false;
        }
        if s.lane() != Lane::ExecveChain {
            return false;
        }
        j += 1;
    }
    // 搅局搭配必须被 route 裁判式拒绝（分发入口 + 文件系统命中）。
    let crossed = [Hit::MissAll, Hit::HitPerm, Hit::HitFormat, Hit::HitGood];
    let mut k = 0usize;
    while k < crossed.len() {
        let mut s = Shape::blank();
        s.entry = Entry::BusyboxDispatch;
        if let Some(h) = crossed.get(k) {
            s.hit = *h;
        }
        if s.lane_consistent() {
            return false;
        }
        k += 1;
    }
    // exec 入口 + 表族命中同样是搅局。
    let tb = [Hit::AppletMiss, Hit::AppletHit];
    let mut m = 0usize;
    while m < tb.len() {
        let mut s = Shape::blank();
        s.entry = Entry::AbsoluteExec;
        if let Some(h) = tb.get(m) {
            s.hit = *h;
        }
        if s.lane_consistent() {
            return false;
        }
        m += 1;
    }
    // 探测守恒表：truth_probes 对六族现场的取值。
    let exhaust = Shape {
        tag: "p",
        name: "x",
        entry: Entry::ShellPath,
        hit: Hit::MissAll,
        path_dirs: 6,
        hit_pos: 0,
        probes_seen: 6,
        errno_seen: 2,
        msg_src: MsgSrc::ShellText,
    };
    if exhaust.truth_probes() != 6 || exhaust.probe_delta() != 0 {
        return false;
    }
    let abs_miss = exhaust.clone_entry(Entry::AbsoluteExec);
    if abs_miss.truth_probes() != 0 {
        return false;
    }
    let dispatch = Shape {
        tag: "p",
        name: "frobnicate",
        entry: Entry::BusyboxDispatch,
        hit: Hit::AppletMiss,
        path_dirs: 0,
        hit_pos: 0,
        probes_seen: 0,
        errno_seen: 0,
        msg_src: MsgSrc::TableText,
    };
    // 分发层的探测义务恒零——对照律第一条。
    if dispatch.truth_probes() != 0 {
        return false;
    }
    if dispatch.clone_probes(3).probe_delta() != 3 {
        return false;
    }
    // errno 阶梯：命中族到回执的映射不可漂。
    if dispatch.truth_errno() != ERR_NONE || exhaust.truth_errno() != ERR_ENOENT {
        return false;
    }
    let perm = Shape { hit: Hit::HitPerm, path_dirs: 4, hit_pos: 3, probes_seen: 3, errno_seen: 13, ..exhaust };
    if perm.truth_errno() != ERR_EACCES || perm.truth_probes() != 3 {
        return false;
    }
    let fmt = Shape { hit: Hit::HitFormat, hit_pos: 0, probes_seen: 0, errno_seen: 8, ..perm };
    if fmt.truth_errno() != ERR_ENOEXEC {
        return false;
    }
    // 文案义务表：四族命中的文案出处一一钉死。
    if dispatch.want_msg() != MsgSrc::TableText {
        return false;
    }
    if exhaust.want_msg() != MsgSrc::ShellText || perm.want_msg() != MsgSrc::ShellText {
        return false;
    }
    let guard = Shape {
        tag: "g",
        name: "",
        entry: Entry::BusyboxDispatch,
        hit: Hit::Guarded,
        path_dirs: 0,
        hit_pos: 0,
        probes_seen: 0,
        errno_seen: 0,
        msg_src: MsgSrc::UsageText,
    };
    if guard.want_msg() != MsgSrc::UsageText || guard.truth_code() != EXIT_GUARD {
        return false;
    }
    let good = Shape { hit: Hit::AppletHit, name: "ls", msg_src: MsgSrc::NoMsg, ..guard };
    if good.want_msg() != MsgSrc::NoMsg || good.truth_code() != EXIT_OK {
        return false;
    }
    // 码与链相容表（lane_code_ok）逐格点验。
    let o27 = Outcome { stage: Stage::Exit, code: EXIT_NOTFOUND, value: 0, note: Note::AppletMiss, default: true };
    if !o27.lane_code_ok(Lane::TableChain) || !o27.lane_code_ok(Lane::ExecveChain) {
        return false;
    }
    let o26 = Outcome { code: EXIT_REFUSED, note: Note::PermDenied, ..o27 };
    if o26.lane_code_ok(Lane::TableChain) {
        return false;
    }
    if !o26.lane_code_ok(Lane::ExecveChain) {
        return false;
    }
    let o1 = Outcome { stage: Stage::Lex, code: EXIT_GUARD, note: Note::GuardRefused, default: false, value: 0 };
    if !o1.lane_code_ok(Lane::GuardChain) || o1.lane_code_ok(Lane::SilentChain) {
        return false;
    }
    true
}

/// 形状合法域：空白合法、逐项破坏必判死、守恒口径、clone 系保序。
fn shape_ok() -> bool {
    let base = Shape::blank();
    if !base.valid() {
        return false;
    }
    if base.truth_code() != EXIT_OK || base.lane() != Lane::SilentChain {
        return false;
    }
    // 非法 errno 见证。
    let mut a = base.clone_errno(9);
    if a.valid() {
        return false;
    }
    // PATH 目录数越界。
    a = Shape { path_dirs: PATH_MAX_DIRS + 1, ..a.clone_errno(0) };
    if a.valid() {
        return false;
    }
    // 命中序越过目录总数。
    let shell = Shape {
        tag: "s",
        name: "ls",
        entry: Entry::ShellPath,
        hit: Hit::HitGood,
        path_dirs: 3,
        hit_pos: 4,
        probes_seen: 3,
        errno_seen: 0,
        msg_src: MsgSrc::NoMsg,
    };
    if shell.valid() {
        return false;
    }
    // 分发入口谎报 PATH 现场。
    let fake_path = Shape { path_dirs: 5, ..base };
    if fake_path.valid() {
        return false;
    }
    // 空名配非防御档。
    let empty_bad = Shape { name: "", hit: Hit::AppletMiss, ..base };
    if empty_bad.valid() {
        return false;
    }
    // 防御档配非空名。
    let guard_name = Shape { hit: Hit::Guarded, name: "ls", ..base };
    if guard_name.valid() {
        return false;
    }
    // 表名字带斜杠（路径形态混进表族）。
    let slashy = Shape { name: "bin/ls", hit: Hit::AppletMiss, ..base };
    if slashy.valid() {
        return false;
    }
    // MissAll 不许带命中位。
    let miss_pos = Shape {
        tag: "m",
        name: "x",
        entry: Entry::ShellPath,
        hit: Hit::MissAll,
        path_dirs: 4,
        hit_pos: 2,
        probes_seen: 4,
        errno_seen: 2,
        msg_src: MsgSrc::ShellText,
    };
    if miss_pos.valid() {
        return false;
    }
    // shell 路文件系统族必须有 PATH 现场。
    let no_path = Shape {
        tag: "n",
        name: "x",
        entry: Entry::ShellPath,
        hit: Hit::HitPerm,
        path_dirs: 0,
        hit_pos: 0,
        probes_seen: 0,
        errno_seen: 13,
        msg_src: MsgSrc::ShellText,
    };
    if no_path.valid() {
        return false;
    }
    // 合法现场全通过：双路 127 与 126 族各造一个。
    let ok_dispatch = Shape {
        tag: "d",
        name: "frobnicate",
        entry: Entry::BusyboxDispatch,
        hit: Hit::AppletMiss,
        path_dirs: 0,
        hit_pos: 0,
        probes_seen: 0,
        errno_seen: 0,
        msg_src: MsgSrc::TableText,
    };
    if !ok_dispatch.valid() {
        return false;
    }
    let ok_shell = Shape {
        tag: "e",
        name: "frobnicate",
        entry: Entry::ShellPath,
        hit: Hit::MissAll,
        path_dirs: 4,
        hit_pos: 0,
        probes_seen: 4,
        errno_seen: 2,
        msg_src: MsgSrc::ShellText,
    };
    if !ok_shell.valid() {
        return false;
    }
    // 见证谎报不是坏形状（申报面归 judge，观测面归 define）：
    // 分发层吹嘘探了 3 次——形状仍合法，必须由 define 的 scan 闸抓。
    let liar = ok_dispatch.clone_probes(3);
    if !liar.valid() {
        return false;
    }
    // clone 系保序：换见证不动场景骨架。
    let c1 = ok_shell.clone_errno(13);
    c1.entry == ok_shell.entry && c1.hit == ok_shell.hit && c1.tag == ok_shell.tag && c1.path_dirs == ok_shell.path_dirs
}

/// 纵线七闸：各闸各拦各的形态，正确现场放行到 exit，闸间优先级不可倒。
fn define_ok() -> bool {
    // 基准分发层现场（正确见证）。
    let base = match case_at(0) {
        Some(c) => c.shape,
        None => return false,
    };
    let o = define(&base);
    if o.stage != Stage::Exit || o.code != EXIT_NOTFOUND || o.value != 0 {
        return false;
    }
    if o.note != Note::AppletMiss || !o.default || !o.attributed() {
        return false;
    }
    if !o.code_note_coherent() {
        return false;
    }
    // 分发层伪证探测：applet 表世界吹嘘探了 3 个目录 → scan 闸 ProbeGap。
    let liar = base.clone_probes(3);
    let ol = define(&liar);
    if ol.stage != Stage::Scan || ol.note != Note::ProbeGap || ol.value != 3 {
        return false;
    }
    if ol.default || ol.code != EXIT_NOTFOUND {
        return false;
    }
    // 分发层伪证 errno：查名失败却报 ENOENT → exec 闸 ErrnoGap。
    let fake_errno = base.clone_errno(ERR_ENOENT);
    let of_ = define(&fake_errno);
    if of_.stage != Stage::Exec || of_.note != Note::ErrnoGap || of_.value != 1 {
        return false;
    }
    // 文案族冒充：分发现场喊 shell 文案 → report 闸 ChainCross。
    let cross_msg = base.clone_msg(MsgSrc::ShellText);
    let om = define(&cross_msg);
    if om.stage != Stage::Report || om.note != Note::ChainCross || om.value != 1 {
        return false;
    }
    // 闸间优先级：探测账坏先于 errno 账坏，errno 账坏先于文案账坏。
    let all_bad = base.clone_probes(2).clone_errno(ERR_EACCES).clone_msg(MsgSrc::UsageText);
    let oa = define(&all_bad);
    if oa.stage != Stage::Scan || oa.note != Note::ProbeGap || oa.value != 2 {
        return false;
    }
    let errno_and_msg = all_bad.clone_probes(0);
    let oe = define(&errno_and_msg);
    if oe.stage != Stage::Exec || oe.note != Note::ErrnoGap {
        return false;
    }
    // route 闸：入口与命中异族在最前段就被拦下，不进后面的账。
    let crossed = Shape {
        tag: "route-cross",
        name: "frobnicate",
        entry: Entry::BusyboxDispatch,
        hit: Hit::MissAll,
        path_dirs: 0,
        hit_pos: 0,
        probes_seen: 0,
        errno_seen: 0,
        msg_src: MsgSrc::ShellText,
    };
    let orc = define(&crossed);
    if orc.stage != Stage::Route || orc.note != Note::ChainCross || orc.value != 1 {
        return false;
    }
    // lex 闸：防御档不进对照、不落 exit。
    let guard = match case_at(7) {
        Some(c) => c.shape,
        None => return false,
    };
    let og = define(&guard);
    if og.stage != Stage::Lex || og.code != EXIT_GUARD || og.note != Note::GuardRefused {
        return false;
    }
    if og.default || og.value != 0 {
        return false;
    }
    // shell 路正确现场放行：耗尽 127 与命中 126。
    let exhaust = match case_at(1) {
        Some(c) => c.shape,
        None => return false,
    };
    let ox = define(&exhaust);
    if ox.stage != Stage::Exit || ox.code != EXIT_NOTFOUND || ox.note != Note::PathMiss {
        return false;
    }
    if !ox.default || ox.value != 0 {
        return false;
    }
    // 探测短报：PATH 四格只报两格 → scan 闸差二。
    let under = exhaust.clone_probes(2);
    let ou = define(&under);
    if ou.stage != Stage::Scan || ou.note != Note::ProbeGap || ou.value != 2 {
        return false;
    }
    // 命中被拒：perm 现场 errno 换 ENOENT 即换码族，exec 闸抓。
    let perm = match case_at(3) {
        Some(c) => c.shape,
        None => return false,
    };
    let op = define(&perm);
    if op.stage != Stage::Exit || op.code != EXIT_REFUSED || op.note != Note::PermDenied {
        return false;
    }
    let perm_errno = perm.clone_errno(ERR_ENOENT);
    let ope = define(&perm_errno);
    if ope.stage != Stage::Exec || ope.note != Note::ErrnoGap {
        return false;
    }
    // 126 两族放行。
    let fmt = match case_at(4) {
        Some(c) => c.shape,
        None => return false,
    };
    let of2 = define(&fmt);
    if of2.code != EXIT_REFUSED || of2.note != Note::FormatErr || !of2.default {
        return false;
    }
    // 双路正常执行：码 0、无声、兑现位假。
    let dr = match case_at(5) {
        Some(c) => c.shape,
        None => return false,
    };
    let od = define(&dr);
    if od.stage != Stage::Exit || od.code != EXIT_OK || od.default || !od.ok() {
        return false;
    }
    let sr = match case_at(6) {
        Some(c) => c.shape,
        None => return false,
    };
    let os = define(&sr);
    if os.note != Note::RanOk || os.attributed() {
        return false;
    }
    // 执行现场乱喊文案：RanOk 场景报 shell 文案 → report 闸 ChainCross。
    let noisy = sr.clone_msg(MsgSrc::ShellText);
    let on = define(&noisy);
    if on.stage != Stage::Report || on.note != Note::ChainCross || on.code != EXIT_OK {
        return false;
    }
    if !on.code_note_coherent() {
        return false;
    }
    // 指纹稳定：同形状两次判定全等，形状指纹不随复算漂移；换见证必换判定指纹。
    let twin = match case_at(0) {
        Some(c) => c.shape,
        None => return false,
    };
    if twin.seal() != base.seal() || define(&twin).seal() != o.seal() {
        return false;
    }
    if base.clone_probes(0).seal() != base.seal() {
        return false;
    }
    define(&base.clone_probes(3)).seal() != o.seal()
}

/// 双路 127 对照主锚：码同、链异、文案族异、见证族异；126 边界与防御档不混线。
fn contrast_ok() -> bool {
    let dispatch = match case_at(0) {
        Some(c) => c.shape,
        None => return false,
    };
    let shell = match case_at(1) {
        Some(c) => c.shape,
        None => return false,
    };
    let abs = match case_at(2) {
        Some(c) => c.shape,
        None => return false,
    };
    let od = define(&dispatch);
    let os = define(&shell);
    let oa = define(&abs);
    // 主锚一：三路 127 码值全等（"双路同码"是前提而不是结论）。
    if od.code != EXIT_NOTFOUND || os.code != EXIT_NOTFOUND || oa.code != EXIT_NOTFOUND {
        return false;
    }
    // 主锚二：码同链异——分发链与 exec 链的落点族必须不同。
    if od.note == os.note || od.note != Note::AppletMiss || os.note != Note::PathMiss {
        return false;
    }
    if dispatch.lane() == shell.lane() {
        return false;
    }
    if !dispatch.lane().contrast_pair() || !shell.lane().contrast_pair() {
        return false;
    }
    // 主锚三：分发链见证恒零，exec 链见证有账。
    if dispatch.truth_probes() != 0 || dispatch.truth_errno() != ERR_NONE {
        return false;
    }
    if shell.truth_probes() != 4 || shell.truth_errno() != ERR_ENOENT {
        return false;
    }
    // 绝对路径单靶：不遍历 PATH（探测零但 errno 有值——第三个对照格）。
    if abs.truth_probes() != 0 || abs.truth_errno() != ERR_ENOENT {
        return false;
    }
    if abs.entry.scans_path() {
        return false;
    }
    // 文案逐字对照：applet 表文案族与 shell 文案族不可混认。
    let td = message(&dispatch, &od);
    let ts = message(&shell, &os);
    if !same_text(td.as_str(), "applet 'frobnicate' not found") {
        return false;
    }
    if !same_text(ts.as_str(), "sh: frobnicate: not found") {
        return false;
    }
    if find_from(td.as_str(), "applet '", 0).is_none() {
        return false;
    }
    if find_from(ts.as_str(), "sh: ", 0).is_none() {
        return false;
    }
    // 两族文案共享的 "not found" 尾巴（码同链异的文案投影：同尾异头）。
    if find_from(td.as_str(), "not found", 0).is_none() {
        return false;
    }
    if find_from(ts.as_str(), "not found", 0).is_none() {
        return false;
    }
    // 链路冒充的两种方向都要判死。
    let d_says_shell = define(&dispatch.clone_msg(MsgSrc::ShellText));
    let s_says_table = define(&shell.clone_msg(MsgSrc::TableText));
    if d_says_shell.note != Note::ChainCross || s_says_table.note != Note::ChainCross {
        return false;
    }
    if d_says_shell.stage != Stage::Report || s_says_table.stage != Stage::Report {
        return false;
    }
    // 126 边界：命中被拒不进 127 对照族。
    let perm = match case_at(3) {
        Some(c) => c.shape,
        None => return false,
    };
    let o26 = define(&perm);
    if o26.code != EXIT_REFUSED || Tier::of(&o26) != Tier::Refused {
        return false;
    }
    if Tier::of(&o26).notfound_side() {
        return false;
    }
    // 防御档不属对照对。
    let guard = match case_at(7) {
        Some(c) => c.shape,
        None => return false,
    };
    let og = define(&guard);
    if guard.lane().contrast_pair() {
        return false;
    }
    if og.code != EXIT_GUARD || og.stage != Stage::Lex || og.note != Note::GuardRefused {
        return false;
    }
    if og.default || og.value != 0 {
        return false;
    }
    if Tier::of(&og) != Tier::Refused {
        return false;
    }
    // 档位镜像：分发落 Dispatch、shell/绝对落 Search，两档共 127。
    if Tier::of(&od) != Tier::Dispatch || Tier::of(&os) != Tier::Search {
        return false;
    }
    if Tier::of(&oa) != Tier::Search {
        return false;
    }
    Tier::of(&od).code() == Tier::of(&os).code() && Tier::of(&os).notfound_side()
}

// ---------------------------------------------------------------------------
// NE07 判据基元：申报对判次序与期望表复算
// ---------------------------------------------------------------------------

/// 对判纵线必须与契约 §1 一致：Estate → Stage → Code → Value → Note → Default。
/// 这里用同一现场的"多维齐错"申报把每一级优先权都钉一遍：
/// - 败局谎报成功（连阶段带码全换）——先判 Estate，绝不许滑到 Code 维；
/// - 成败同报、阶段先错——Stage 必须在 Code 之前截住；
/// - 码对偏错——Value 在 Note 之前；落点对兑现错——Note 在 Default 之前。
/// 次序一乱，"同码换链"（note-swap）就会在别的维上露不出来，本卡主锚即失效。
fn judge_ok() -> bool {
    let cs = cases();
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let want = match expected(i) {
            Some(v) => v,
            None => return false,
        };
        // 每槽实判必须逐字等于期望缺陷表（这正是 run 的 matched 计数前提）。
        if judge(&c) != want {
            return false;
        }
        // 方法面对自由函数的委托不许分叉。
        if c.judge() != judge(&c) {
            return false;
        }
        if c.ok() != (want == Fault::Ok) {
            return false;
        }
        if c.tag() != c.shape.tag {
            return false;
        }
        if c.actual() != define(&c.shape) {
            return false;
        }
        i += 1;
    }
    // expected 的下标面：0..8 恒 Ok，8..14 顺次挂 REJECT_FAULTS，越界无值。
    let mut a = 0usize;
    while a < ACCEPT {
        if expected(a) != Some(Fault::Ok) {
            return false;
        }
        a += 1;
    }
    let mut j = 0usize;
    while j < REJECT {
        match (expected(ACCEPT + j), REJECT_FAULTS.get(j)) {
            (Some(x), Some(y)) => {
                if x != *y {
                    return false;
                }
            }
            _ => return false,
        }
        j += 1;
    }
    if expected(CORPUS).is_some() {
        return false;
    }
    // cover：恰 14 放行，少一格/多一格都判无观测。
    if cover(CORPUS) != Ok(()) {
        return false;
    }
    if cover(ACCEPT) != Err(NE_ENOBS) {
        return false;
    }
    if cover(CORPUS + 1) != Err(NE_ENOBS) {
        return false;
    }
    // ---- 次序实验台：全部拿 slot0（分发层 127 现场）做底 ----
    let base = match case_at(0) {
        Some(v) => v,
        None => return false,
    };
    // 多维齐错的"报成功"：阶段/码/落点/兑现全不同，仍必须最先落 Estate。
    let liar = Case {
        shape: base.shape,
        claim: Claim { stage: Stage::Exit, code: 0, value: 0, note: Note::RanOk, default: false },
    };
    if judge(&liar) != Fault::Estate {
        return false;
    }
    // 成败同报（码 126 依旧败局）：Stage 先于 Code 截住。
    let stage_then_code = Case {
        shape: base.shape,
        claim: Claim { stage: Stage::Table, code: 126, value: 0, note: Note::PathMiss, default: false },
    };
    if stage_then_code.actual().ok() != stage_then_code.claim.success() {
        return false;
    }
    if judge(&stage_then_code) != Fault::Stage {
        return false;
    }
    // 阶段对、码与落点齐错：Code 先于 Note。
    let code_then_note = Case {
        shape: base.shape,
        claim: Claim { stage: Stage::Exit, code: 126, value: 0, note: Note::PathMiss, default: true },
    };
    if judge(&code_then_note) != Fault::Code {
        return false;
    }
    // 码对、偏错加落点错：Value 先于 Note（主锚维不许被吞进 Value 维）。
    let value_then_note = Case {
        shape: base.shape,
        claim: Claim { stage: Stage::Exit, code: 127, value: 2, note: Note::PathMiss, default: true },
    };
    if judge(&value_then_note) != Fault::Value {
        return false;
    }
    // 本卡主锚形态：码同为 127、成败同为败、偏同为零，只换文案族——
    // 必须干净地落在 Note 维。
    let note_then_default = Case {
        shape: base.shape,
        claim: Claim { stage: Stage::Exit, code: 127, value: 0, note: Note::PathMiss, default: false },
    };
    if judge(&note_then_default) != Fault::Note {
        return false;
    }
    // 落点对、仅兑现位错：最后一维 Default 兜底。
    let only_default = Case {
        shape: base.shape,
        claim: Claim { stage: Stage::Exit, code: 127, value: 0, note: Note::AppletMiss, default: false },
    };
    if judge(&only_default) != Fault::Default {
        return false;
    }
    // 全维如实：六闸都放行。
    if judge(&base) != Fault::Ok {
        return false;
    }
    // Fault::code 映射与判据段一致：判出的缺陷必须能落回本段错误码。
    if judge(&liar).code() != NE_ESTATE {
        return false;
    }
    if judge(&note_then_default).code() != NE_ENOTE {
        return false;
    }
    if base.judge().code() != 0 {
        return false;
    }
    // 空白用例自洽：blank 形状配 blank 申报必判 Ok。
    if judge(&Case::blank()) != Fault::Ok {
        return false;
    }
    true
}

/// 矩阵与报表：四档行合计、覆盖平满、注入掉绿、渲染逐字。
///
/// `127 双档并排`（Dispatch 行 7 槽、Search 行 2 槽）是"码同链异"的可见化；
/// 红档只许出现在复刻槽所在的 Dispatch 行——活槽行一旦有红，说明诚实现场
/// 被申报面污染，那是报表造假而不是判据通过。
fn matrix_ok() -> bool {
    let m = Matrix::of();
    if m.slots() != CORPUS {
        return false;
    }
    if m.goods() != ACCEPT {
        return false;
    }
    if !m.covered() {
        return false;
    }
    // 四档合计钉死：执行 2、搜索 2、拒绝 3、分发 7（六复刻槽留在案发档）。
    let ran = match m.row(Tier::Ran) {
        Some(v) => v,
        None => return false,
    };
    let search = match m.row(Tier::Search) {
        Some(v) => v,
        None => return false,
    };
    let refused = match m.row(Tier::Refused) {
        Some(v) => v,
        None => return false,
    };
    let dispatch = match m.row(Tier::Dispatch) {
        Some(v) => v,
        None => return false,
    };
    if ran.total != 2 || ran.good != 2 {
        return false;
    }
    if search.total != 2 || search.good != 2 {
        return false;
    }
    if refused.total != 3 || refused.good != 3 {
        return false;
    }
    if dispatch.total != REJECT + 1 || dispatch.good != 1 {
        return false;
    }
    // 全对档只有前三行；红只许在复刻槽堆里，且恰为 REJECT 个。
    if m.perfect_rows() != 3 {
        return false;
    }
    if dispatch.red() != REJECT || !dispatch.reds_are_replicas() {
        return false;
    }
    if !ran.reds_are_replicas() || !search.reds_are_replicas() || !refused.reds_are_replicas() {
        return false;
    }
    // 127 双档合计：分发 7 + 搜索 2 = 9（对照线主战场的槽数）。
    if m.notfound_slots() != 9 {
        return false;
    }
    // Row 的防御语义：空行不算全对；good 越过 total 的红数折叠回 total。
    let blank_row = Row::blank();
    if blank_row.perfect() || blank_row.red() != 0 {
        return false;
    }
    let odd = Row { total: 2, good: 3 };
    if odd.red() != 2 || odd.perfect() {
        return false;
    }
    // Tier::of 投影规则逐条复算（分档只看码 + 落点族，与申报无关）。
    let o_dis = Outcome { stage: Stage::Exit, code: 127, value: 0, note: Note::AppletMiss, default: true };
    let o_sea = Outcome { stage: Stage::Exit, code: 127, value: 0, note: Note::PathMiss, default: true };
    let o_cross = Outcome { stage: Stage::Report, code: 127, value: 1, note: Note::ChainCross, default: false };
    let o_ran = Outcome { stage: Stage::Exit, code: 0, value: 0, note: Note::RanOk, default: false };
    let o_ref = Outcome { stage: Stage::Exit, code: 126, value: 0, note: Note::PermDenied, default: true };
    let o_gua = Outcome { stage: Stage::Lex, code: 1, value: 0, note: Note::GuardRefused, default: false };
    if Tier::of(&o_dis) != Tier::Dispatch {
        return false;
    }
    if Tier::of(&o_sea) != Tier::Search || Tier::of(&o_cross) != Tier::Search {
        return false;
    }
    if Tier::of(&o_ran) != Tier::Ran {
        return false;
    }
    if Tier::of(&o_ref) != Tier::Refused || Tier::of(&o_gua) != Tier::Refused {
        return false;
    }
    // 码维不变量：Dispatch 与 Search 共 127——同码是前提，不是结论。
    if Tier::Search.code() != EXIT_NOTFOUND || Tier::Dispatch.code() != EXIT_NOTFOUND {
        return false;
    }
    if Tier::Search.code() != Tier::Dispatch.code() {
        return false;
    }
    if !(Tier::Search.notfound_side() && Tier::Dispatch.notfound_side()) {
        return false;
    }
    if Tier::Ran.notfound_side() || Tier::Refused.notfound_side() {
        return false;
    }
    if Tier::Ran.code() != EXIT_OK || Tier::Refused.code() != EXIT_REFUSED {
        return false;
    }
    // 矩阵渲染逐行钉版：四行、每行带 red= 字段。
    let mt = m.render();
    let mts = mt.as_str();
    if find_from(mts, "ran total=2 good=2 red=0", 0).is_none() {
        return false;
    }
    if find_from(mts, "search total=2 good=2 red=0", 0).is_none() {
        return false;
    }
    if find_from(mts, "refused total=3 good=3 red=0", 0).is_none() {
        return false;
    }
    if find_from(mts, "dispatch total=7 good=1 red=6", 0).is_none() {
        return false;
    }
    let mut nl = 0usize;
    let mb = mt.as_bytes();
    let mut k = 0usize;
    while k < mb.len() {
        if let Some(b) = mb.get(k) {
            if *b == b'\n' {
                nl += 1;
            }
        }
        k += 1;
    }
    if nl != TIER_COUNT {
        return false;
    }
    // 报表与矩阵互锁：accept 侧四档计数与矩阵 good 数必须逐档相等。
    let rep = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if rep.ran != ran.good as u32 || rep.search != search.good as u32 {
        return false;
    }
    if rep.refused != refused.good as u32 || rep.dispatch != dispatch.good as u32 {
        return false;
    }
    if rep.total as usize != CORPUS || rep.accept as usize != ACCEPT || rep.reject as usize != REJECT {
        return false;
    }
    if rep.matched as usize != CORPUS || !rep.green() {
        return false;
    }
    if rep.gate() != Ok(()) || rep.rate() != 1000 {
        return false;
    }
    if rep.zero_cells() != 2 || rep.notfound_cells() != 3 {
        return false;
    }
    // 全绿渲染逐字钉版（报表造假在这一行当场现形）。
    if !same_text(rep.render().as_str(), RENDER_OK) {
        return false;
    }
    // 注入面：把 slot0 的期望强行改成 Code——matched 掉到 13、红档率 929、闸门红。
    let inj = match run_with(Some((0, Fault::Code))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if inj.matched != 13 || inj.accept != 7 || inj.reject != 7 {
        return false;
    }
    if inj.dispatch != 0 || inj.green() {
        return false;
    }
    if inj.gate() != Err(NE_ECOUNT) {
        return false;
    }
    if inj.rate() != 929 {
        return false;
    }
    // 红档率的第二形式复算：公式直算与 rate() 必须同值。
    let manual = Report {
        total: 14,
        accept: 7,
        reject: 7,
        ran: 2,
        search: 2,
        refused: 3,
        dispatch: 0,
        matched: 13,
    };
    if manual.rate() != 929 || !same_text(manual.render().as_str(), inj.render().as_str()) {
        return false;
    }
    // 零分母防御：total=0 的率必须归零而不是炸。
    let empty = Report {
        total: 0,
        accept: 0,
        reject: 0,
        ran: 0,
        search: 0,
        refused: 0,
        dispatch: 0,
        matched: 0,
    };
    if empty.rate() != 0 || empty.green() {
        return false;
    }
    // 同值注入是无操作注入：报表不许变样（防"注入必掉绿"的假信号）。
    let same = match run_with(Some((9, Fault::Stage))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if !same.green() || !same_text(same.render().as_str(), rep.render().as_str()) {
        return false;
    }
    // 越界槽注入：显式 E*SLOT，而不是静默全绿。
    match run_with(Some((CORPUS, Fault::Code))) {
        Err(c) => {
            if c != NE_ESLOT {
                return false;
            }
        }
        Ok(_) => return false,
    }
    // run() 与 run_with(None) 同形。
    let none = match run_with(None) {
        Ok(v) => v,
        Err(_) => return false,
    };
    same_text(none.render().as_str(), rep.render().as_str())
}

/// 标签是否以 `-swap` 收尾（逐字节比，不借 std 的 ends_with）。
fn tag_is_swap(t: &str) -> bool {
    let tb = t.as_bytes();
    let sb = SWAP_SUFFIX.as_bytes();
    if tb.len() < sb.len() {
        return false;
    }
    let start = tb.len() - sb.len();
    let mut j = 0usize;
    while j < sb.len() {
        match (tb.get(start + j), sb.get(j)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        j += 1;
    }
    true
}

/// 语料纪律（契约 §2）：14 槽全合法可建、标签唯一、复刻槽现场逐字等于案发槽、
/// 前 8 槽申报逐维镜像现场、后 6 槽一槽只坏一维。
///
/// 复刻槽的意义：六份红档必须在**案发现场完全相同**的前提下只让申报面出错，
/// 这样红档的档位归属（Dispatch）才是被"复刻"锁死的——谎报不会挪动现场，
/// 挪动现场的谎报等于替申报者销毁证据。
fn corpus_ok() -> bool {
    if SHAPES.len() != CORPUS || CLAIMS.len() != CORPUS {
        return false;
    }
    // 每一槽都得是合法现场（越权形状混进语料，判据就失去了可复算性）。
    let mut i = 0usize;
    while i < CORPUS {
        match SHAPES.get(i) {
            Some(s) => {
                if !s.valid() {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    // 标签两两不同（语料的身份面）。
    let mut p = 0usize;
    while p < CORPUS {
        let mut q = p + 1;
        while q < CORPUS {
            match (SHAPES.get(p), SHAPES.get(q)) {
                (Some(x), Some(y)) => {
                    if same_text(x.tag, y.tag) {
                        return false;
                    }
                }
                _ => return false,
            }
            q += 1;
        }
        p += 1;
    }
    // 活槽不带 -swap 后缀、死槽必须带（复刻身份写在标签上，不许藏）。
    let mut k = 0usize;
    while k < ACCEPT {
        match SHAPES.get(k) {
            Some(s) => {
                if tag_is_swap(s.tag) {
                    return false;
                }
            }
            None => return false,
        }
        k += 1;
    }
    let mut r = ACCEPT;
    while r < CORPUS {
        match SHAPES.get(r) {
            Some(s) => {
                if !tag_is_swap(s.tag) {
                    return false;
                }
            }
            None => return false,
        }
        r += 1;
    }
    // 复刻槽与案发槽（slot0）的现场字段逐字相同，只许标签不同。
    let src = match SHAPES.get(0) {
        Some(v) => *v,
        None => return false,
    };
    let mut d = ACCEPT;
    while d < CORPUS {
        let rep = match SHAPES.get(d) {
            Some(v) => *v,
            None => return false,
        };
        if rep.name != src.name
            || rep.entry != src.entry
            || rep.hit != src.hit
            || rep.path_dirs != src.path_dirs
            || rep.hit_pos != src.hit_pos
            || rep.probes_seen != src.probes_seen
            || rep.errno_seen != src.errno_seen
            || rep.msg_src != src.msg_src
        {
            return false;
        }
        if same_text(rep.tag, src.tag) {
            return false;
        }
        d += 1;
    }
    // 前 8 槽：申报逐维等于现场（诚实镜像是本卡的对照组）。
    let mut e = 0usize;
    while e < ACCEPT {
        let c = match case_at(e) {
            Some(v) => v,
            None => return false,
        };
        let a = c.actual();
        let cl = c.claim;
        if a.stage != cl.stage || a.code != cl.code || a.value != cl.value {
            return false;
        }
        if a.note != cl.note || a.default != cl.default {
            return false;
        }
        if judge(&c) != Fault::Ok {
            return false;
        }
        e += 1;
    }
    // 后 6 槽：一槽只坏一维；Estate 槽（首格）是唯一允许波及其他维的形态，
    // 因为"败局报成功"本身就必须换掉码与落点。
    let mut j = 0usize;
    while j < REJECT {
        let idx = ACCEPT + j;
        let c = match case_at(idx) {
            Some(v) => v,
            None => return false,
        };
        let a = c.actual();
        let cl = c.claim;
        let same_estate = a.ok() == cl.success();
        let d_stage = a.stage != cl.stage;
        let d_code = a.code != cl.code;
        let d_value = a.value != cl.value;
        let d_note = a.note != cl.note;
        let d_default = a.default != cl.default;
        let n_diff = (d_stage as u8)
            .saturating_add(d_code as u8)
            .saturating_add(d_value as u8)
            .saturating_add(d_note as u8)
            .saturating_add(d_default as u8);
        let want = match REJECT_FAULTS.get(j) {
            Some(v) => *v,
            None => return false,
        };
        if want == Fault::Estate {
            if j != 0 || same_estate {
                return false;
            }
        } else {
            if !same_estate || n_diff != 1 {
                return false;
            }
            let dim_hit = match want {
                Fault::Stage => d_stage,
                Fault::Code => d_code,
                Fault::Value => d_value,
                Fault::Note => d_note,
                Fault::Default => d_default,
                _ => false,
            };
            if !dim_hit {
                return false;
            }
        }
        if judge(&c) != want {
            return false;
        }
        j += 1;
    }
    // note-swap 是本卡主锚的显式形态：同码 127、同败局，只换链路文案族。
    if let Some(c) = case_at(ACCEPT + 4) {
        let a = c.actual();
        if a.code != EXIT_NOTFOUND || c.claim.code != EXIT_NOTFOUND {
            return false;
        }
        if a.note != Note::AppletMiss || c.claim.note != Note::PathMiss {
            return false;
        }
        if !a.note.notfound_pair() || !c.claim.note.notfound_pair() {
            return false;
        }
    } else {
        return false;
    }
    // 克隆子现场保骨架：clone_* 只许动自己那一维（现场与见证分离的前提）。
    if let Some(s) = SHAPES.get(1) {
        let back = s.clone_probes(s.probes_seen).clone_errno(s.errno_seen).clone_msg(s.msg_src).clone_entry(s.entry);
        if back != *s {
            return false;
        }
        if s.clone_probes(99).probes_seen != 99 || s.clone_probes(99).errno_seen != s.errno_seen {
            return false;
        }
    } else {
        return false;
    }
    // 空白用例：blank 形状如实申报即全绿，任何语料外的用例不许先天带红。
    judge(&Case::blank()) == Fault::Ok
}

/// 期望表第二形式（WANT_*）与申报面（CLAIMS）、现场面（define）的三方互检。
///
/// 契约 §2 要求把申报表用第二种形式复写一遍：这里 WANT_* 五维表 + 标签表
/// 独立手敲，任何一处抄错都会与 `define` 的推定或 `judge` 的截点相抵。
/// accept/reject 下标表同样独立成面——分区不闭合就是语料在灌水。
fn slots_ok() -> bool {
    if WANT_STAGE.len() != CORPUS
        || WANT_CODE.len() != CORPUS
        || WANT_VALUE.len() != CORPUS
        || WANT_NOTE.len() != CORPUS
        || WANT_DEFAULT.len() != CORPUS
        || WANT_TAGS.len() != CORPUS
    {
        return false;
    }
    let cs = cases();
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        // 标签面：语料身份与期望表一致，case_at 与 cases 两条通道同值。
        let t = match WANT_TAGS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if !same_text(c.tag(), t) {
            return false;
        }
        if let Some(by_at) = case_at(i) {
            if !same_text(by_at.tag(), t) {
                return false;
            }
        } else {
            return false;
        }
        // 五维现场面复算：define 的推定必须逐维等于第二形式表。
        let a = c.actual();
        if let Some(w) = WANT_STAGE.get(i) {
            if a.stage != *w {
                return false;
            }
        } else {
            return false;
        }
        if let Some(w) = WANT_CODE.get(i) {
            if a.code != *w {
                return false;
            }
        } else {
            return false;
        }
        if let Some(w) = WANT_VALUE.get(i) {
            if a.value != *w {
                return false;
            }
        } else {
            return false;
        }
        if let Some(w) = WANT_NOTE.get(i) {
            if a.note != *w {
                return false;
            }
        } else {
            return false;
        }
        if let Some(w) = WANT_DEFAULT.get(i) {
            if a.default != *w {
                return false;
            }
        } else {
            return false;
        }
        i += 1;
    }
    // 分区表：ACCEPT_SLOTS 严格升序 0..8，REJECT_SLOTS 严格 8..14。
    let mut p = 0usize;
    while p < ACCEPT {
        match ACCEPT_SLOTS.get(p) {
            Some(v) => {
                if *v != p {
                    return false;
                }
            }
            None => return false,
        }
        p += 1;
    }
    let mut q = 0usize;
    while q < REJECT {
        match REJECT_SLOTS.get(q) {
            Some(v) => {
                if *v != ACCEPT + q {
                    return false;
                }
            }
            None => return false,
        }
        q += 1;
    }
    // 每槽恰落一区：分区计数和为 14 且互斥。
    let mut slot = 0usize;
    while slot < CORPUS {
        let in_accept = ACCEPT_SLOTS.contains(&slot);
        let in_reject = REJECT_SLOTS.contains(&slot);
        if in_accept == in_reject {
            return false;
        }
        let c = match case_at(slot) {
            Some(v) => v,
            None => return false,
        };
        if in_accept && judge(&c) != Fault::Ok {
            return false;
        }
        if in_reject && !judge(&c).alive() {
            return false;
        }
        if in_accept != (expected(slot) == Some(Fault::Ok)) {
            return false;
        }
        slot += 1;
    }
    // 后缀面：复刻槽标签必须 -swap 收尾，活槽必须不带。
    let mut w = 0usize;
    while w < CORPUS {
        let t = match WANT_TAGS.get(w) {
            Some(v) => *v,
            None => return false,
        };
        if w < ACCEPT {
            if tag_is_swap(t) {
                return false;
            }
        } else if !tag_is_swap(t) {
            return false;
        }
        w += 1;
    }
    // 边界：case_at 越界归 None，末槽可达。
    if case_at(CORPUS).is_some() {
        return false;
    }
    if case_at(CORPUS - 1).is_none() {
        return false;
    }
    // 127 双族期望互异：活槽前二格的期望落点必须分属两链（主锚的第二形式）。
    match (WANT_NOTE.get(0), WANT_NOTE.get(1)) {
        (Some(x), Some(y)) => {
            if *x != Note::AppletMiss || *y != Note::PathMiss {
                return false;
            }
            if !x.notfound_pair() || !y.notfound_pair() || x == y {
                return false;
            }
        }
        _ => return false,
    }
    // 期望码表与 Entry::miss_code 恒等：三入口的未找到码全同。
    if !(WANT_CODE.get(0) == Some(&127)
        && WANT_CODE.get(1) == Some(&127)
        && WANT_CODE.get(2) == Some(&127))
    {
        return false;
    }
    Entry::BusyboxDispatch.miss_code() == Entry::ShellPath.miss_code()
        && Entry::ShellPath.miss_code() == Entry::AbsoluteExec.miss_code()
        && Entry::AbsoluteExec.miss_code() == EXIT_NOTFOUND
}

/// 红档显式：REJECT 两表、VALS 八格、缺陷码阶梯与 Fault 全码面。
///
/// 第 8 格红档率是本卡对"报表造假"的直接防御：它**不是**统计数，而是
/// 一次真实注入（把案发槽的期望换成 Code）后 `rate()` 的算出值。若哪天
/// run_with 的 matched 变成硬编码，这一格立刻对不上 929。
fn reds_ok() -> bool {
    if REJECT_FAULTS.len() != REJECT || REJECT_CODES.len() != REJECT || VALS.len() != ACCEPT {
        return false;
    }
    // 六缺陷次序逐字钉死（契约 §1：Estate → Stage → Code → Value → Note → Default）。
    let order = [Fault::Estate, Fault::Stage, Fault::Code, Fault::Value, Fault::Note, Fault::Default];
    let mut i = 0usize;
    while i < REJECT {
        match (REJECT_FAULTS.get(i), order.get(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    // 缺陷码：非零、两两不同、与 Fault::code 同槽、全部落在本段阶梯内。
    i = 0usize;
    while i < REJECT {
        let c = match REJECT_CODES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let f = match REJECT_FAULTS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if c == 0 || c != f.code() {
            return false;
        }
        if c <= SEG_BASE || c > SEG_BASE.saturating_add(0x0d) {
            return false;
        }
        let mut j = i + 1;
        while j < REJECT {
            match REJECT_CODES.get(j) {
                Some(o) => {
                    if *o == c {
                        return false;
                    }
                }
                None => return false,
            }
            j += 1;
        }
        i += 1;
    }
    // 六格的具名映射逐个点名（防止阶梯整体错位一格）。
    if REJECT_CODES.get(0) != Some(&NE_ESTATE)
        || REJECT_CODES.get(1) != Some(&NE_ESTAGE)
        || REJECT_CODES.get(2) != Some(&NE_ECODE)
    {
        return false;
    }
    if REJECT_CODES.get(3) != Some(&NE_EVALUE)
        || REJECT_CODES.get(4) != Some(&NE_ENOTE)
        || REJECT_CODES.get(5) != Some(&NE_EDEFAULT)
    {
        return false;
    }
    // VALS 前 7 格与报表同槽：总数、成、败、执行、搜索、拒绝、分发。
    let rep = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if VALS.get(0) != Some(&(CORPUS as u32)) {
        return false;
    }
    if VALS.get(1) != Some(&(ACCEPT as u32)) || VALS.get(2) != Some(&(REJECT as u32)) {
        return false;
    }
    if VALS.get(3) != Some(&rep.ran) || VALS.get(4) != Some(&rep.search) {
        return false;
    }
    if VALS.get(5) != Some(&rep.refused) || VALS.get(6) != Some(&rep.dispatch) {
        return false;
    }
    // 四档统计格之和必等于成格数（灌水/漏档都过不了这一行）。
    let tier_sum = match (VALS.get(3), VALS.get(4), VALS.get(5), VALS.get(6)) {
        (Some(a), Some(b), Some(c), Some(d)) => (*a)
            .saturating_add(*b)
            .saturating_add(*c)
            .saturating_add(*d),
        _ => return false,
    };
    if tier_sum != ACCEPT as u32 {
        return false;
    }
    // 第 8 格：红档率 929——必须等于一次真实注入后的算出值，而不是绿灯率。
    let inj = match run_with(Some((0, Fault::Code))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if VALS.get(7) != Some(&929) {
        return false;
    }
    if VALS.get(7) != Some(&inj.rate()) {
        return false;
    }
    if inj.rate() == rep.rate() || inj.rate() == 1000 {
        return false;
    }
    if inj.rate() == inj.matched {
        return false;
    }
    // 千分公式的独立复算：(13*1000 + 14/2) / 14 = 929。
    if (13u32.saturating_mul(1000).saturating_add(7)) / 14 != 929 {
        return false;
    }
    // Fault 全码面：Ok 归零、其余十二档非零且两两互异，alive 与码值一致。
    let all_faults = [
        Fault::Ok,
        Fault::Stage,
        Fault::Code,
        Fault::Value,
        Fault::Note,
        Fault::Default,
        Fault::Estate,
        Fault::Count,
        Fault::Slot,
        Fault::Seal,
        Fault::Nobs,
        Fault::Matrix,
        Fault::Bal,
    ];
    if all_faults.len() != 13 {
        return false;
    }
    let mut p = 0usize;
    while p < all_faults.len() {
        let f = match all_faults.get(p) {
            Some(v) => *v,
            None => return false,
        };
        if f.alive() != (f.code() != 0) {
            return false;
        }
        if (f == Fault::Ok) != (f.code() == 0) {
            return false;
        }
        let short = fault_short(f);
        if short.is_empty() {
            return false;
        }
        let mut q = p + 1;
        while q < all_faults.len() {
            let g = match all_faults.get(q) {
                Some(v) => *v,
                None => return false,
            };
            if f.code() == g.code() && f != g {
                return false;
            }
            if same_text(fault_short(f), fault_short(g)) && f != g {
                return false;
            }
            q += 1;
        }
        p += 1;
    }
    // 段内 13 阶梯逐字：SEG_BASE+1 .. +0x0d 一格不缺。
    let ladder = [
        NE_ESTAGE,
        NE_ECODE,
        NE_EVALUE,
        NE_ENOTE,
        NE_EDEFAULT,
        NE_ECHAIN,
        NE_ESTATE,
        NE_ECOUNT,
        NE_ESLOT,
        NE_ESEAL,
        NE_ENOBS,
        NE_EMATRIX,
        NE_EBAL,
    ];
    let mut k = 0usize;
    while k < ladder.len() {
        let want = match SEG_BASE.checked_add((k as u32) + 1) {
            Some(v) => v,
            None => return false,
        };
        match ladder.get(k) {
            Some(v) => {
                if *v != want {
                    return false;
                }
            }
            None => return false,
        }
        k += 1;
    }
    true
}

/// 文案逐字：真实世界四族报错 + 本卡三族审计文案 + 义务面全表遍历。
///
/// 「notfound 与 ENOENT exec 链路对照」判据的文案半壁江山就在这一条：
/// `applet '...' not found` 与 `sh: ...: not found` 必须一字不差、头异尾同；
/// 126 两族共用 `sh: <名>: <strerror>` 模板只换 strerror 段。
fn message_ok() -> bool {
    // 真实报错文本逐字钉版（大小写、单引号、冒号空格都是判据面）。
    if !same_text(msg_applet_not_found("frobnicate").as_str(), "applet 'frobnicate' not found") {
        return false;
    }
    if !same_text(msg_sh_not_found("frobnicate").as_str(), "sh: frobnicate: not found") {
        return false;
    }
    if !same_text(msg_sh_cannot("tool", ERR_EACCES).as_str(), "sh: tool: Permission denied") {
        return false;
    }
    if !same_text(msg_sh_cannot("img", ERR_ENOEXEC).as_str(), "sh: img: Exec format error") {
        return false;
    }
    // usage 两行逐字 + 拼合形态：恰一个换行、结尾无换行。
    if !same_text(BB_VERSION, "BusyBox v1.36.1") {
        return false;
    }
    let head = msg_usage_head();
    let line = msg_usage_line();
    if !same_text(head.as_str(), "BusyBox v1.36.1 multi-call binary.") {
        return false;
    }
    if !same_text(line.as_str(), "usage: busybox [function] [arguments]...") {
        return false;
    }
    let full = msg_usage_full();
    let fs = full.as_str();
    if find_from(fs, head.as_str(), 0) != Some(0) {
        return false;
    }
    let want_pos = match text_len(head.as_str()).checked_add(1) {
        Some(v) => v,
        None => return false,
    };
    if find_from(fs, line.as_str(), 0) != Some(want_pos) {
        return false;
    }
    let mut nl = 0usize;
    let fb = full.as_bytes();
    let mut bi = 0usize;
    while bi < fb.len() {
        if let Some(b) = fb.get(bi) {
            if *b == b'\n' {
                nl += 1;
            }
        }
        bi += 1;
    }
    if nl != 1 {
        return false;
    }
    match fb.get(fb.len().saturating_sub(1)) {
        Some(b) => {
            if *b == b'\n' {
                return false;
            }
        }
        None => return false,
    }
    // strerror 表与 errno 合法域（0/2/8/13 四值，其余拒收）。
    if !same_text(strerror_text(ERR_NONE), "(no errno)") {
        return false;
    }
    if !same_text(strerror_text(ERR_ENOENT), "No such file or directory") {
        return false;
    }
    if !same_text(strerror_text(ERR_EACCES), "Permission denied") {
        return false;
    }
    if !same_text(strerror_text(ERR_ENOEXEC), "Exec format error") {
        return false;
    }
    if !same_text(strerror_text(9), "Unknown error") {
        return false;
    }
    if !(errno_legal(0) && errno_legal(2) && errno_legal(8) && errno_legal(13)) {
        return false;
    }
    if errno_legal(1) || errno_legal(9) || errno_legal(127) {
        return false;
    }
    // message() 的如实六落点逐字复算（语料槽直接驱动）。
    let honest = [
        (0usize, "applet 'frobnicate' not found"),
        (1usize, "sh: frobnicate: not found"),
        (2usize, "sh: /tmp/nope: not found"),
        (3usize, "sh: tool: Permission denied"),
        (4usize, "sh: img: Exec format error"),
    ];
    let mut h = 0usize;
    while h < honest.len() {
        let (idx, want) = match honest.get(h) {
            Some(v) => *v,
            None => return false,
        };
        let c = match case_at(idx) {
            Some(v) => v,
            None => return false,
        };
        if !same_text(message(&c.shape, &c.actual()).as_str(), want) {
            return false;
        }
        h += 1;
    }
    if let Some(c) = case_at(7) {
        if !same_text(message(&c.shape, &c.actual()).as_str(), fs) {
            return false;
        }
    } else {
        return false;
    }
    // 无声档：成功且静默的落点必须拿空串（不许有"成功也喊话"的档）。
    if let Some(c) = case_at(5) {
        if !message(&c.shape, &c.actual()).is_empty() {
            return false;
        }
    } else {
        return false;
    }
    if let Some(c) = case_at(6) {
        if !message(&c.shape, &c.actual()).is_empty() {
            return false;
        }
    } else {
        return false;
    }
    // 三个破断形态的审计文案逐字（方向词 want/reported 不许互换）。
    let s1 = match SHAPES.get(1) {
        Some(v) => *v,
        None => return false,
    };
    let s0 = match SHAPES.get(0) {
        Some(v) => *v,
        None => return false,
    };
    let cross = s1.clone_msg(MsgSrc::TableText);
    let oc = define(&cross);
    if oc.note != Note::ChainCross || oc.stage != Stage::Report {
        return false;
    }
    if !same_text(message(&cross, &oc).as_str(), "lane attribution crossed (want execve, reported dispatch)") {
        return false;
    }
    // 反方向链路冒充（分发层现场喊 shell 文案）——主锚的另一个方向。
    let cross0 = s0.clone_msg(MsgSrc::ShellText);
    let oc0 = define(&cross0);
    if oc0.note != Note::ChainCross || oc0.stage != Stage::Report {
        return false;
    }
    if !same_text(message(&cross0, &oc0).as_str(), "lane attribution crossed (want dispatch, reported execve)") {
        return false;
    }
    let gap = s1.clone_probes(2);
    let og = define(&gap);
    if og.note != Note::ProbeGap || og.stage != Stage::Scan || og.value != 2 {
        return false;
    }
    if !same_text(message(&gap, &og).as_str(), "probe count mismatch (expected 4, reported 2)") {
        return false;
    }
    let eg = s1.clone_errno(13);
    let oe = define(&eg);
    if oe.note != Note::ErrnoGap || oe.stage != Stage::Exec {
        return false;
    }
    if !same_text(
        message(&eg, &oe).as_str(),
        "errno witness mismatch (expected 2 [No such file or directory], reported 13 [Permission denied])",
    ) {
        return false;
    }
    // 族名与漂移两条辅助审计文本逐字。
    if !same_text(msg_text_family("applet-table", "shell").as_str(), "message family mismatch (want applet-table, got shell)") {
        return false;
    }
    if !same_text(msg_code_lane_drift(EXIT_REFUSED, Lane::TableChain).as_str(), "exit code 126 is incompatible with lane dispatch") {
        return false;
    }
    // contrast_report：诚实现场空串，破断现场带见证分数且无换行。
    if !contrast_report(&s0, &define(&s0)).is_empty() {
        return false;
    }
    if !contrast_report(&s1, &define(&s1)).is_empty() {
        return false;
    }
    let cr = contrast_report(&gap, &og);
    let crs = cr.as_str();
    if find_from(crs, "lane=execve", 0).is_none() {
        return false;
    }
    if find_from(crs, "want-msg=shell", 0).is_none() {
        return false;
    }
    if find_from(crs, "got-msg=shell", 0).is_none() {
        return false;
    }
    if find_from(crs, "probes=2/4", 0).is_none() {
        return false;
    }
    if find_from(crs, "errno=2/2", 0).is_none() {
        return false;
    }
    if has_byte(crs, b'\n') {
        return false;
    }
    // 义务面全表：文案非空当且仅当落点不是成功静默族。
    let cs = cases();
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let m = message(&c.shape, &c.actual());
        if m.is_empty() != (c.actual().note == Note::RanOk) {
            return false;
        }
        i += 1;
    }
    // 段位文本逐字：0x6d290 定宽五位。
    if !same_text(seg_text().as_str(), "0x6d290") {
        return false;
    }
    true
}

/// 逐槽释义：报表记录的是现场真值而不是申报面，六维分段逐字可定位。
///
/// detail 的独立性在这里被钉死：code-swap 槽的报告仍然写 exit=127、
/// value-swap 槽仍写 delta=0、note-swap 槽仍写 applet-miss-reported——
/// 若哪天 detail 改读 claim，红档的案卷就会被申报者的谎言覆写。
fn detail_ok() -> bool {
    let cs = cases();
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let a = c.actual();
        let d = detail(&c);
        let ds = d.as_str();
        let db = d.as_bytes();
        if db.is_empty() {
            return false;
        }
        // 行尾恰一个换行（渲染对齐的前提）。
        match db.get(db.len() - 1) {
            Some(b) => {
                if *b != b'\n' {
                    return false;
                }
            }
            None => return false,
        }
        // 行首：有名槽以名字开头，防御档空名以 " entry=" 开头。
        if c.shape.name.is_empty() {
            if find_from(ds, " entry=", 0) != Some(0) {
                return false;
            }
        } else if find_from(ds, c.shape.name, 0) != Some(0) {
            return false;
        }
        // 六个分段维度逐字在场（entry/hit/lane/stage/exit/delta/note）。
        let mut needle = String::from(" entry=");
        needle.push_str(c.shape.entry.text());
        if find_from(ds, needle.as_str(), 0).is_none() {
            return false;
        }
        needle = String::from(" hit=");
        needle.push_str(c.shape.hit.text());
        if find_from(ds, needle.as_str(), 0).is_none() {
            return false;
        }
        needle = String::from(" lane=");
        needle.push_str(lane_name(c.shape.lane()));
        if find_from(ds, needle.as_str(), 0).is_none() {
            return false;
        }
        needle = String::from(" stage=");
        needle.push_str(a.stage.name());
        if find_from(ds, needle.as_str(), 0).is_none() {
            return false;
        }
        needle = String::from(" exit=");
        needle.push_str(dec_text(a.code).as_str());
        if find_from(ds, needle.as_str(), 0).is_none() {
            return false;
        }
        needle = String::from(" delta=");
        needle.push_str(dec_text(a.value).as_str());
        if find_from(ds, needle.as_str(), 0).is_none() {
            return false;
        }
        needle = String::from(" note=");
        needle.push_str(a.note.name());
        if find_from(ds, needle.as_str(), 0).is_none() {
            return false;
        }
        // 报错义务槽带引号文案段；静默槽永无 msg 段。
        let has_msg = find_from(ds, " msg=\"", 0).is_some();
        if has_msg == (a.note == Note::RanOk) {
            return false;
        }
        i += 1;
    }
    // code-swap 槽：报告说真值 127，不说申报的 126。
    if let Some(c) = case_at(10) {
        let d = detail(&c);
        if find_from(d.as_str(), " exit=127", 0).is_none() {
            return false;
        }
        if find_from(d.as_str(), " exit=126", 0).is_some() {
            return false;
        }
    } else {
        return false;
    }
    // value-swap 槽：delta 反映现场零偏差而不是申报的 3。
    if let Some(c) = case_at(11) {
        let d = detail(&c);
        if find_from(d.as_str(), " delta=0", 0).is_none() {
            return false;
        }
        if find_from(d.as_str(), " delta=3", 0).is_some() {
            return false;
        }
    } else {
        return false;
    }
    // note-swap 槽（主锚）：案卷落点仍是分发族，谎报的 shell 族不入案卷。
    if let Some(c) = case_at(12) {
        let d = detail(&c);
        if find_from(d.as_str(), "note=applet-miss-reported", 0).is_none() {
            return false;
        }
        if find_from(d.as_str(), "path-miss-reported", 0).is_some() {
            return false;
        }
    } else {
        return false;
    }
    // 复刻六槽的释义与案发槽逐字相同（现场一致是复刻的前提）。
    let c0 = match case_at(0) {
        Some(v) => v,
        None => return false,
    };
    let d0 = detail(&c0);
    let mut j = ACCEPT;
    while j < CORPUS {
        let c = match case_at(j) {
            Some(v) => v,
            None => return false,
        };
        if !same_text(detail(&c).as_str(), d0.as_str()) {
            return false;
        }
        j += 1;
    }
    // 前八槽两两互异（八份现场各占一档，语料不许内卷）。
    let mut p = 0usize;
    while p < ACCEPT {
        let mut q = p + 1;
        while q < ACCEPT {
            let x = match case_at(p) {
                Some(v) => v,
                None => return false,
            };
            let y = match case_at(q) {
                Some(v) => v,
                None => return false,
            };
            if same_text(detail(&x).as_str(), detail(&y).as_str()) {
                return false;
            }
            q += 1;
        }
        p += 1;
    }
    // 防御档案卷内 usage 两行俱在（嵌入换行不毁定位）。
    if let Some(c) = case_at(7) {
        let d = detail(&c);
        let ds = d.as_str();
        if find_from(ds, "BusyBox v1.36.1 multi-call binary.", 0).is_none() {
            return false;
        }
        if find_from(ds, "usage: busybox [function] [arguments]...", 0).is_none() {
            return false;
        }
    } else {
        return false;
    }
    // 双 127 案卷：同尾异头（码同链异的文案投影在逐槽释义里也要可见）。
    if let (Some(cd), Some(csh)) = (case_at(0), case_at(1)) {
        let dd = detail(&cd);
        let dsh = detail(&csh);
        if find_from(dd.as_str(), "applet 'frobnicate' not found", 0).is_none() {
            return false;
        }
        if find_from(dsh.as_str(), "sh: frobnicate: not found", 0).is_none() {
            return false;
        }
        if find_from(dd.as_str(), "sh: ", 0).is_some() {
            return false;
        }
        if find_from(dsh.as_str(), "applet '", 0).is_some() {
            return false;
        }
    } else {
        return false;
    }
    true
}

/// 阶段闸门：单调纵线 lex→…→exit，倒退判阶段错，关闸后一切操作判状态错。
fn gate_ok() -> bool {
    let mut g = Gate::new();
    if g.last() != -1 || g.steps() != 0 || g.closed() {
        return false;
    }
    let ladder = [
        Stage::Lex,
        Stage::Route,
        Stage::Table,
        Stage::Scan,
        Stage::Exec,
        Stage::Report,
        Stage::Exit,
    ];
    let mut i = 0usize;
    while i < ladder.len() {
        let s = match ladder.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if g.step(s) != Ok(()) {
            return false;
        }
        if g.steps() != (i as u32).saturating_add(1) {
            return false;
        }
        if g.last() != s.order() as i32 {
            return false;
        }
        i += 1;
    }
    match g.close() {
        Ok(n) => {
            if n != STAGE_COUNT as u32 {
                return false;
            }
        }
        Err(_) => return false,
    }
    if !g.closed() {
        return false;
    }
    // 关闸后一切 step/close 判状态错，且状态不许再被改动。
    if g.step(Stage::Exit) != Err(NE_ESTATE) {
        return false;
    }
    if g.close() != Err(NE_ESTATE) {
        return false;
    }
    if g.steps() != 7 || g.last() != 6 {
        return false;
    }
    // 倒退判阶段错：被拒的那一步不改状态。
    let mut h = Gate::new();
    if h.step(Stage::Exit) != Ok(()) {
        return false;
    }
    if h.step(Stage::Table) != Err(NE_ESTAGE) {
        return false;
    }
    if h.last() != Stage::Exit.order() as i32 || h.steps() != 1 {
        return false;
    }
    // 同秩视为前进（单调不减，而不是严格递增）。
    if h.step(Stage::Exit) != Ok(()) {
        return false;
    }
    if h.steps() != 2 {
        return false;
    }
    // 空闸可直接关闭：零段也合法，关闭后同样落锁。
    let mut z = Gate::new();
    match z.close() {
        Ok(n) => {
            if n != 0 {
                return false;
            }
        }
        Err(_) => return false,
    }
    if z.step(Stage::Lex) != Err(NE_ESTATE) {
        return false;
    }
    // Copy 语义：闸门副本的推进不回写原件。
    let before = h;
    let mut dup = h;
    if dup.step(Stage::Exit) != Ok(()) {
        return false;
    }
    if h.steps() != before.steps() || h.last() != before.last() {
        return false;
    }
    // 倒退被拒之后闸门仍可前进（拒步不是关闸）。
    if h.step(Stage::Exit) != Ok(()) {
        return false;
    }
    true
}

/// 手工建账 14 格（对 `build_ledger` 的第二形式复写，兼作破损注入面）。
///
/// bad=0 正常；bad=1 首成格值 +1；bad=2 首败格码 +1；
/// bad=3 第二成格码非零；bad=4 首败格值非零。四种破损都要被 audit 点名。
fn hand_ledger(bad: u8) -> Ledger {
    let mut l = Ledger::new();
    let mut i = 0usize;
    while i < ACCEPT {
        let mut value = match VALS.get(i) {
            Some(x) => *x,
            None => 0,
        };
        let mut code = 0u32;
        if bad == 1 && i == 0 {
            value = value.saturating_add(1);
        }
        if bad == 3 && i == 1 {
            code = NE_EBAL;
        }
        let op: u8 = match i {
            0 | 1 | 2 => OP_CORPUS,
            3 => OP_LEX,
            4 => OP_SCAN,
            5 => OP_EXEC,
            6 => OP_TABLE,
            _ => OP_REPLAY,
        };
        let _ = l.register(Sample { slot: i as u8, op, ok: true, code, value });
        i += 1;
    }
    let mut j = 0usize;
    while j < REJECT {
        let idx = ACCEPT + j;
        let mut code = match REJECT_CODES.get(j) {
            Some(x) => *x,
            None => 0,
        };
        let mut value = 0u32;
        if bad == 2 && j == 0 {
            code = code.saturating_add(1);
        }
        if bad == 4 && j == 0 {
            value = 1;
        }
        let _ = l.register(Sample { slot: idx as u8, op: OP_REPORT, ok: false, code, value });
        j += 1;
    }
    l
}

/// 账本：槽位严格连号、逐格对账、两形式建账同指纹、冻结后拒登记、重放必等。
///
/// 账本是本卡的"观测持久面"：VALS 第 8 格（929）在账本里与报表里必须
/// 同值，任何一格被涂改都要在 audit/replay 两道独立复核下露出来。
fn ledger_ok() -> bool {
    // 空账本自面。
    let e = Ledger::new();
    if e.len() != 0 || !e.is_empty() || e.sample(0).is_some() {
        return false;
    }
    if e.rejects() != 0 || e.is_frozen() || e.root() != 0 {
        return false;
    }
    if e.ok_count() != 0 || e.err_count() != 0 {
        return false;
    }
    if e.audit() != Err(NE_EBAL) {
        return false;
    }
    // 跳号登记：拒收、计数、不入账，三件事必须同时成立。
    let mut l = Ledger::new();
    let skip = Sample { slot: 1, op: OP_CORPUS, ok: true, code: 0, value: 14 };
    if l.register(skip) != Err(NE_ESLOT) {
        return false;
    }
    if l.len() != 0 || l.rejects() != 1 {
        return false;
    }
    let first = Sample { slot: 0, op: OP_CORPUS, ok: true, code: 0, value: 14 };
    if l.register(first) != Ok(()) {
        return false;
    }
    if l.len() != 1 {
        return false;
    }
    // 回号登记（slot0 重复）同样被连号纪律拒之门外。
    if l.register(first) != Err(NE_ESLOT) {
        return false;
    }
    if l.len() != 1 || l.rejects() != 2 {
        return false;
    }
    if let Some(s) = l.sample(0) {
        if s.value != 14 {
            return false;
        }
    } else {
        return false;
    }
    // 满账：build_ledger 的逐格对账（槽、成败、值、码、op 五面）。
    let lb = build_ledger();
    if lb.len() != SAMPLES || lb.ok_count() != ACCEPT || lb.err_count() != REJECT {
        return false;
    }
    if lb.audit() != Ok(()) {
        return false;
    }
    let want_ops = [
        OP_CORPUS,
        OP_CORPUS,
        OP_CORPUS,
        OP_LEX,
        OP_SCAN,
        OP_EXEC,
        OP_TABLE,
        OP_REPLAY,
        OP_REPORT,
        OP_REPORT,
        OP_REPORT,
        OP_REPORT,
        OP_REPORT,
        OP_REPORT,
    ];
    if want_ops.len() != SAMPLES {
        return false;
    }
    let mut i = 0usize;
    while i < SAMPLES {
        let s = match lb.sample(i) {
            Some(v) => v,
            None => return false,
        };
        if s.slot as usize != i {
            return false;
        }
        match want_ops.get(i) {
            Some(w) => {
                if s.op != *w {
                    return false;
                }
            }
            None => return false,
        }
        if i < ACCEPT {
            if !s.ok || s.code != 0 {
                return false;
            }
            match VALS.get(i) {
                Some(w) => {
                    if s.value != *w {
                        return false;
                    }
                }
                None => return false,
            }
        } else {
            if s.ok || s.value != 0 {
                return false;
            }
            match REJECT_CODES.get(i - ACCEPT) {
                Some(w) => {
                    if s.code != *w {
                        return false;
                    }
                }
                None => return false,
            }
        }
        i += 1;
    }
    if lb.sample(SAMPLES).is_some() {
        return false;
    }
    // 统计格与报表同槽：账本抄报表，两形式必须互咬。
    let rep = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let cells = [
        (0usize, rep.total),
        (1, rep.accept),
        (2, rep.reject),
        (3, rep.ran),
        (4, rep.search),
        (5, rep.refused),
        (6, rep.dispatch),
    ];
    let mut c = 0usize;
    while c < cells.len() {
        let (idx, want) = match cells.get(c) {
            Some(v) => *v,
            None => return false,
        };
        if let Some(s) = lb.sample(idx) {
            if s.value != want {
                return false;
            }
        } else {
            return false;
        }
        c += 1;
    }
    // 红档格：929 与注入算出值在账本里同槽（VALS 第 8 格的账面对账）。
    let inj = match run_with(Some((0, Fault::Code))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if let Some(s7) = lb.sample(7) {
        if s7.value != 929 || s7.value != inj.rate() {
            return false;
        }
        if s7.op != OP_REPLAY || !s7.ok {
            return false;
        }
    } else {
        return false;
    }
    // 第二形式建账与 build_ledger 指纹逐格相同（手写面与生成面闭合）。
    if hand_ledger(0).seal() != lb.seal() {
        return false;
    }
    // 指纹确定且非零。
    let s1 = lb.seal();
    if s1 == 0 || build_ledger().seal() != s1 {
        return false;
    }
    // 冻结：root 等于冻结前指纹、落锁、拒登、拒双冻、重放恒等。
    let mut f = build_ledger();
    let pre = f.seal();
    let root = match f.freeze() {
        Ok(r) => r,
        Err(_) => return false,
    };
    if root != pre || f.root() != root || !f.is_frozen() {
        return false;
    }
    let before_rej = f.rejects();
    if f.register(Sample::blank()) != Err(NE_ESLOT) {
        return false;
    }
    if f.rejects() != before_rej.saturating_add(1) {
        return false;
    }
    if f.freeze() != Err(NE_EBAL) {
        return false;
    }
    if f.replay() != Ok(root) {
        return false;
    }
    // 重放不依赖冻结态：未冻账本同样过 replay。
    if build_ledger().replay() != Ok(s1) {
        return false;
    }
    // 四种破损都要被 audit 点名，且破损账本冻结后重放必红。
    let mut t1 = hand_ledger(1);
    if t1.audit() != Err(NE_EVALUE) {
        return false;
    }
    let _ = t1.freeze();
    if t1.replay() != Err(NE_ESEAL) {
        return false;
    }
    if hand_ledger(2).audit() != Err(NE_EVALUE) {
        return false;
    }
    if hand_ledger(3).audit() != Err(NE_EVALUE) {
        return false;
    }
    if hand_ledger(4).audit() != Err(NE_EVALUE) {
        return false;
    }
    // 缺格账（13 格）直接红：长度不是能糊弄的维度。
    let hl = hand_ledger(0);
    let mut sh = Ledger::new();
    let mut k = 0usize;
    while k < SAMPLES - 1 {
        if let Some(s) = hl.sample(k) {
            if sh.register(s) != Ok(()) {
                return false;
            }
        } else {
            return false;
        }
        k += 1;
    }
    if sh.audit() != Err(NE_EBAL) {
        return false;
    }
    // 满账之后越界格（slot 14）进不来。
    let mut full = hand_ledger(0);
    if full.register(Sample { slot: 14, op: OP_REPORT, ok: false, code: NE_EBAL, value: 0 }) != Err(NE_ESLOT)
    {
        return false;
    }
    if full.rejects() != 1 || full.len() != SAMPLES {
        return false;
    }
    // Sample::blank() 默认形安全（零形状，不会伪装成成格）。
    let b = Sample::blank();
    b.slot == 0 && b.op == 0 && !b.ok && b.code == 0 && b.value == 0
}


// ---------------------------------------------------------------------------
// 域自检入口（13 条闸门，与 NE 段 13 阶错误码一一对位）
// ---------------------------------------------------------------------------

/// 本卡域自检：13 条判据逐项过闸。
///
/// 每条判据都跑真实谓词（不是常量 true），红项文案直接指向破断维；
/// 第 13 条把账本与四路注入合成总闸——任何一路静默退化都会让它变红。
pub fn run_unx11675_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11675_notfound_vs_enoent");
    s.add(
        "NE01-名称与段基址",
        name_ok() && anchors_agree(),
        "SEG_BASE 0x6d290/13 阶梯/与 F11664 位距 0xF0、F11676 邻位 0x10 交叉署名",
    );
    s.add(
        "NE02-枚举与纵线",
        enums_ok(),
        "七闸秩 0..6/落点九档三族互斥/四链四档/入口命中同族表",
    );
    s.add(
        "NE03-链路归属矩阵",
        lanes_ok(),
        "分发层见证恒零、绝对路径单靶、shell 逐目录有账、跨族申报判死",
    );
    s.add(
        "NE04-形状合法域",
        shape_ok(),
        "入口族与命中族同族才可建形/越权现场（伪 PATH、空名错位）拒收",
    );
    s.add(
        "NE05-七闸纵线",
        define_ok(),
        "lex→route→table→scan→exec→report→exit 逐闸放行，破断落点与偏差数精确",
    );
    s.add(
        "NE06-双路127对照",
        contrast_ok(),
        "同码异链：applet 表文案与 sh 文案对仗、见证族有无论、126/防御档不混线",
    );
    s.add(
        "NE07-申报对判",
        judge_ok(),
        "六维次序 Estate 先行，note-swap 同码换链干净落 Note 维",
    );
    s.add(
        "NE08-矩阵统计",
        matrix_ok(),
        "四档 2/2/3/7、红只在复刻槽、注入掉绿 929、渲染逐字 RENDER_OK",
    );
    s.add(
        "NE09-语料与渲染",
        corpus_ok() && slots_ok(),
        "14 槽全合法、八镜像六单维、复刻现场逐字同案发、WANT 五表第二形式互咬",
    );
    s.add(
        "NE10-红档显式",
        reds_ok(),
        "REJECT_CODES 互异同槽、VALS 前七统计第八钉 929、Fault 全码面闭合",
    );
    s.add(
        "NE11-文案逐字",
        message_ok(),
        "applet/sh/strerror/usage 四族真实文本 + 三族审计文案逐字复算",
    );
    s.add(
        "NE12-释义与账本",
        detail_ok() && ledger_ok(),
        "案卷记真值不记谎言；账本连号、双形式同指纹、冻结重放、四破损皆红",
    );
    let mut l = build_ledger();
    let a = l.audit() == Ok(());
    let b = match l.freeze() {
        Ok(r) => r != 0 && l.is_frozen() && l.replay() == Ok(r),
        Err(_) => false,
    };
    let inj = inject_four();
    let four = match inj.get(0..4) {
        Some(w) => w.iter().all(|v| *v),
        None => false,
    };
    s.add(
        "NE13-冻结重放四注入",
        a && b && four,
        "建账→对账→冻结→重放全绿，且表路/判路/渲染路/账本路四注入全真",
    );
    s
}

// --P3NEXT--

/// 调试用逐谓词探针（交付前删除）。
pub fn __probe() -> [bool; 15] {
    [
        name_ok(),
        enums_ok(),
        lanes_ok(),
        shape_ok(),
        define_ok(),
        contrast_ok(),
        judge_ok(),
        matrix_ok(),
        corpus_ok(),
        slots_ok(),
        reds_ok(),
        message_ok(),
        detail_ok(),
        gate_ok(),
        ledger_ok(),
    ]
}
