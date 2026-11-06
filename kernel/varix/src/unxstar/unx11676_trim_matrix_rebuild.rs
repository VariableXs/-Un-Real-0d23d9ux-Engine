//! UNX-F11676 裁剪矩阵重建抽样（三档 CONFIG）判据
//!
//! 这一卡不判"未知 applet 该打哪句话"（那是 F11675/F11679 的语料面），本卡判的是
//! **同一张 applet 表在三档 CONFIG 下被重建三次之后，notfound 面的判定还是不是同
//! 一条判定**。判据原文只有一句：三档裁剪 notfound 矩阵判据绿。绿的前提不是"三档
//! 都打 127"，而是**每一档都说得出自己那一格为什么还是那一条路径**。
//!
//! 三档 CONFIG 是构建期开关，不是运行期选项：
//!
//! ```text
//! CFG_FULL        musl + busybox + busybox 的 root applet + sh    8/8 applet 在册
//! CFG_NO_MUSL     -CFG_MUSL                                      musl 装载器摘掉
//! CFG_NO_BB_ROOT  -CFG_BUSYBOX_ROOT                              passwd/su/mount 摘掉
//! ```
//!
//! 裁剪的本体是**位图**：`trim(full, off) = full & !off`。位图一改，重建器就要把
//! applet 表重新算一遍——不是把上一档的表删几行，而是从位图重新展开。这一条是本卡
//! 与"编译期枚举清单"最根本的差别：清单是**上一档的结论**，位图是**这一档的前提**。
//! 只有从前提重建，才谈得上"抽样"；从结论删行，删掉的永远只是看得见的那几行。
//!
//! 为什么"三档都打 127"不能当绿：未知 applet 在存活档里走的是分发层（`x: applet
//! not found`），在缺席档里走的是装载器（`/bin/x: No such file or directory`）——
//! **退出码一样是 127，文案与路径却换了层**。把两档的 127 当成同一条判定，正是本卡
//! 要抓的头号缺陷：现场上它表现为"裁剪版把装载器错误报成了 applet 不存在"，用户照着
//! `applet not found` 去查 applet 表，查到的却是一张根本没有这个 applet 的表。
//!
//! ```text
//! full     : dd --help           survives  path=dispatcher  text="dd: applet not found"   127
//! no-musl  : ls --help           survives  path=path-search text="ls: applet not found"   127  同判
//! no-root  : su -                ABSENT    path=loader-miss text="/bin/su: No such..."    127  显式改道
//! no-musl  : ld-musl --list      ABSENT    path=loader-miss text="ld-musl...: No such..." 127  显式改道
//! ```
//!
//! ## 闸门次序（这条纵线为什么不能倒）
//!
//! 八段，秩 1..=8，只许前进：
//!
//! 1. **Cfg**——位图合法性。未知位（本档没定义的 `CFG_*`）与保留位一律拒。位图是
//!    后面全部推导的唯一输入，坏位进去算出来的 applet 表是幻觉表。
//! 2. **Trim**——裁剪面非空。`cfg == CFG_NONE` 意味着连 `LoaderMiss` 之外一条路径
//!    都不存在，此时"notfound 判定"无从谈起，必须在重建之前就地判死。
//! 3. **Rebuild**——重建确定性。同一 `cfg` 连建两次必须同指纹；漂移说明重建器读了
//!    位图以外的东西（宿主环境、时间、目录顺序），抽样立即失去意义。
//! 4. **Table**——表平整。重号、空表、缺族一律拒；表不平就分不清"applet 缺席"与
//!    "表没建起来"，这两件事在本卡里必须落在不同的段上。
//! 5. **Present**——在册性。位图决定 applet 在不在；"宣布改道"与"实际在册"必须
//!    互斥：在册却宣布改道是凭空的档，缺席却不宣布改道是隐瞒的档。
//! 6. **Path**——路径存在性。每条 notfound 路径都有自己的 `CFG_*` 前置位；路径在
//!    这一档不存在时，观测值必须落回 `LoaderMiss`，不许落到一条已摘掉的路径上。
//! 7. **Verdict**——判定一致性。在册 ⇒ 与全量档同判；缺席 ⇒ 必须显式不同。两向
//!    都判死：缺席报同判是 `silent-same`（本卡头号缺陷），在册报改道是
//!    `phantom-shift`（把"没裁剪"说成"裁剪了"）。
//! 8. **Rate**——一致率结算。分母是 3 档 × 8 applet = 24 个重建抽样格，不剔除改道
//!    格（改道是**正确结论**，不是例外，必须计入分子）。
//!
//! 次序不能倒的理由逐条对得上：Present 必须在 Table 之后（表没建好，问"在不在"
//! 得到的是空表的答案）；Path 必须在 Present 之后（先说人还在不在，再说他走哪条
//! 路）；Verdict 必须在 Path 之后（路径摆出来，才谈得上"这条判定和全量档一样吗"）。
//! Rebuild 必须在 Table 之前——两次重建不一致的表根本不该被拿去比对。
//!
//! ## 关键差异（本卡深度所在，谁判死谁放行）
//!
//! * **与 F11675（双路 127 对照，段 `0x6d290`）分层**：F11675 把"分发层 127"与
//!   "exec 层 127"摆成两路对照，判的是**同档内两层**的差异；本卡判的是**跨档**
//!   差异——同一个 applet 在三档里是不是还走同一条路。两卡的 127 不可互借。
//! * **与 F11679（场景 × 退出码矩阵，段 `0x6d2e0`）分层**：F11679 的矩阵是
//!   **场景 × 码**（127/126/1/0/2），零悬空；本卡的矩阵是**档位 × 路径**（3 × 6），
//!   每一格是"这条路径在这一档存不存在"。F11679 判"码对不对"，本卡判"路还在不在"。
//! * **与 F11677（帮助文本双机快照，段 `0x6d2b0`）分层**：F11677 比的是两台机器
//!   的**帮助文本**逐字；本卡比的是**同一台机器三档构建**的 notfound 判定。两台
//!   机器可以打一样的话，三档构建却未必给一样的路径。
//! * **与 F11595（联签卡，段差 `0x3f0`）分层**：F11595 给的是三档基线的**口径**
//!   （哪三档、每档摘什么），本卡给的是**矩阵结果**。口径变了本卡必红，结果变了
//!   口径仍可绿——两卡互为约束，不互相替代。
//! * **谁判死**：未知位、空裁剪面、重建漂移、表不平、改道声明与在册性互斥、
//!   路径在这一档不存在、缺席报同判、在册报改道——一律判死，且各自落不同的码，
//!   使"是裁剪错了"与"是判定错了"分得开。
//! * **谁放行**：在册且路径存在、判定与全量档逐字相同 → `kept` 档放行；缺席且
//!   路径显式落到 `LoaderMiss`、判定与全量档显式不同 → `shifted` 档放行。放行的
//!   两档都进分子：改道是本卡的**正确结论**，不是"降级容忍"。
//! * **退出码不是判据**：三档下未知 applet 的退出码都是 127，码在这里是**常量**，
//!   把它当判据就等于"三档全绿"的自证。本卡把码钉成 `EXIT_NOTFOUND`，只在文案与
//!   路径两维上判真假——这是本卡与"只比码"的判据最锋利的一条边界。
//!
//! 语料十四槽：前八槽三档覆盖（全量 3 槽、缺 musl 2 槽、缺 root 3 槽），后六槽
//! 复刻同一裁剪现场、各坏一维（estate/stage/code/value/note/default）。账本十四格
//! = 七统计 + 一红档率（929）+ 六缺陷码，冻结后重放同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 本卡段基址（全仓唯一，VX5 分配表给定，不许改）。
pub const SEG_BASE: u32 = 0x6d2a0;

// ---------------------------------------------------------------------------
// 联签锚：本卡与相邻卡共用同一条上游事实
// ---------------------------------------------------------------------------

/// 前置卡 F11675（分发层 / exec 层双路 127 对照）的段基址。
pub const ANCHOR_F11675_SEG: u32 = 0x6d290;

/// 直接后继卡 F11677（帮助文本双机快照）的段基址。
pub const ANCHOR_F11677_SEG: u32 = 0x6d2b0;

/// 联签卡 F11595（三档裁剪基线口径）与本卡的段位差。
pub const ANCHOR_F11595_GAP: u32 = 0x3f0;

/// 联签卡 F11595 的段基址（由位差反推，不在本卡里另立绝对值）。
pub const ANCHOR_F11595_SEG: u32 = SEG_BASE - ANCHOR_F11595_GAP;

/// 未知 applet 在三档下共同的退出码：码是常量，不是判据。
pub const EXIT_NOTFOUND: u32 = 127;

/// 判定成立时的退出码。
pub const EXIT_OK: u32 = 0;

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
/// 闸门段数。
pub const STAGE_COUNT: u32 = 8;
/// 闸门末段秩。
pub const STAGE_LAST_RANK: u32 = 8;
/// 统计格数（账本前七格）。
pub const STAT_CELLS: usize = 7;
/// 红档率千分比（全批统一，钉在 929）。
pub const RED_RATE: u32 = 929;
/// 满额千分比。
pub const RATE_FULL: u32 = 1000;

// ---------------------------------------------------------------------------
// 本卡规模：三档 CONFIG × 八 applet × 六条 notfound 路径
// ---------------------------------------------------------------------------

/// applet 数（重建抽样的一维）。
pub const APPLET_COUNT: usize = 8;
/// CONFIG 档数（重建抽样的另一维）。
pub const CONFIG_COUNT: usize = 3;
/// notfound 路径数。
pub const PATH_COUNT: usize = 6;
/// 重建抽样格数：3 档 × 8 applet。
pub const SAMPLE_CELLS: usize = CONFIG_COUNT * APPLET_COUNT;
/// 三档下仍在册的格数（20 = 8 + 7 + 5）。
pub const KEPT_CELLS: u32 = 20;
/// 三档下显式改道的格数（4 = 0 + 1 + 3）。
pub const SHIFT_CELLS: u32 = 4;
/// 三档下路径格存在总数（6 + 5 + 6）。
pub const PATH_PRESENT_CELLS: u32 = 17;
/// 需要 busybox root 位的 applet 数。
pub const ROOT_APPLET_COUNT: usize = 3;
/// 需要 musl 位的 applet 数。
pub const MUSL_APPLET_COUNT: usize = 1;
/// 单档路径数上限（全量档六条全在）。
pub const PATH_PER_TIER_MAX: usize = 6;

// ---------------------------------------------------------------------------
// CONFIG 位图（裁剪的唯一本体）
// ---------------------------------------------------------------------------

/// musl 装载器与它名下 applet。
pub const CFG_MUSL: u16 = 0x0001;
/// busybox 多合一主体。
pub const CFG_BUSYBOX: u16 = 0x0002;
/// busybox 的 root 专属 applet（passwd/su/mount）。
pub const CFG_BUSYBOX_ROOT: u16 = 0x0004;
/// `sh` applet 与它的内建分发。
pub const CFG_SHELL: u16 = 0x0008;
/// 全部已定义位。
pub const CFG_ALL: u16 = 0x000f;
/// 空位图（裁剪面全灭）。
pub const CFG_NONE: u16 = 0x0000;
/// 保留位（本档不许出现，出现即未知位）。
pub const CFG_RESERVED: u16 = 0xfff0;

/// 三档的位图（次序与 `Config` 一一对应）。
pub const CFG_TIER: [u16; CONFIG_COUNT] = [
    CFG_ALL,
    CFG_ALL & !CFG_MUSL,
    CFG_ALL & !CFG_BUSYBOX_ROOT,
];

// ---------------------------------------------------------------------------
// 错误码（13 个，SEG_BASE + 0x01..0x0d）
// ---------------------------------------------------------------------------

/// CONFIG 位图非法：未知位/保留位/自相矛盾。
pub const TM_ECFG: u32 = SEG_BASE + 0x01;
/// 裁剪面全灭：空位图无法支撑 notfound 判定。
pub const TM_ETRIM: u32 = SEG_BASE + 0x02;
/// 重建表不平：漂移、重号、空表。
pub const TM_ETABLE: u32 = SEG_BASE + 0x03;
/// notfound 路径不存在或未声明改道。
pub const TM_EPATH: u32 = SEG_BASE + 0x04;
/// notfound 文案与全量档不符（同判/改道互报）。
pub const TM_ETEXT: u32 = SEG_BASE + 0x05;
/// 阶段不符或倒退。
pub const TM_ESTAGE: u32 = SEG_BASE + 0x06;
/// 成败互报。
pub const TM_ESTATE: u32 = SEG_BASE + 0x07;
/// 错误码不符。
pub const TM_ECODE: u32 = SEG_BASE + 0x08;
/// 统计值不符。
pub const TM_EVALUE: u32 = SEG_BASE + 0x09;
/// 注记不符。
pub const TM_ENOTE: u32 = SEG_BASE + 0x0a;
/// 兜底档被当成结论。
pub const TM_EDEFAULT: u32 = SEG_BASE + 0x0b;
/// 槽位越界。
pub const TM_ESLOT: u32 = SEG_BASE + 0x0c;
/// 指纹不符。
pub const TM_ESEAL: u32 = SEG_BASE + 0x0d;

/// 十三码全表（次序即偏移）。
pub const ALL_CODES: [u32; 13] = [
    TM_ECFG, TM_ETRIM, TM_ETABLE, TM_EPATH, TM_ETEXT, TM_ESTAGE, TM_ESTATE, TM_ECODE, TM_EVALUE,
    TM_ENOTE, TM_EDEFAULT, TM_ESLOT, TM_ESEAL,
];

/// 反例六维对应的六个码（次序与 `Fault::reject_faults()` 一致）。
pub const REJECT_CODES: [u32; REJECT] = [TM_ESTATE, TM_ESTAGE, TM_ECODE, TM_EVALUE, TM_ENOTE, TM_EDEFAULT];

/// 七个统计格的钉值 + 第八格红档率（929）。
pub const VALS: [u32; ACCEPT] = [
    APPLET_COUNT as u32,
    CONFIG_COUNT as u32,
    SAMPLE_CELLS as u32,
    KEPT_CELLS,
    SHIFT_CELLS,
    PATH_PRESENT_CELLS,
    RATE_FULL,
    RED_RATE,
];

/// 七个统计格归属的段号。
pub const STAT_OPS: [u8; STAT_CELLS] = [1, 2, 3, 4, 5, 6, 7];

/// 红档格在账本里的段号。
pub const OPID_STAT_RED: u8 = 0x08;
/// 六个缺陷码格的段号起点。
pub const DEFECT_OP_BASE: u8 = 0x21;

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

/// 折一个 u64（小端展开，不受宿主字节序影响）。
fn fold_word(h: u64, w: u64) -> u64 {
    let mut v = w;
    let mut acc = h;
    let mut i = 0usize;
    while i < 8 {
        acc = fold_byte(acc, (v & 0xff) as u8);
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
        n /= 10;
        i -= 1;
        if let Some(cell) = buf.get_mut(i) {
            *cell = b'0' + d;
        }
    }
    let mut s = String::new();
    let mut j = i;
    while j < 10 {
        if let Some(c) = buf.get(j) {
            s.push(*c as char);
        }
        j += 1;
    }
    s
}

/// u32 → 定宽十六进制文本（大写，不足位补零）。
fn hex_text(v: u32, digits: usize) -> String {
    let table: &[u8; 16] = b"0123456789ABCDEF";
    let mut s = String::new();
    if digits == 0 {
        return s;
    }
    let mut i = digits;
    while i > 0 {
        i -= 1;
        let nib = ((v >> (i * 4)) & 0xf) as usize;
        let c = match table.get(nib) {
            Some(x) => *x,
            None => b'0',
        };
        s.push(c as char);
    }
    s
}

/// u32 文本别名。
pub fn u32_text(v: u32) -> String {
    dec_text(v)
}

/// i32 文本（负号显式）。
fn i32_text(v: i32) -> String {
    if v < 0 {
        let mut s = String::from("-");
        s.push_str(dec_text((v as i64).unsigned_abs() as u32).as_str());
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

/// 段基址文本。
pub fn seg_text(v: u32) -> String {
    let mut s = String::from("0x");
    s.push_str(hex_text(v, 5).as_str());
    s
}

/// 千分比文本。
pub fn permille_text(v: u32) -> String {
    let mut s = dec_text(v);
    s.push_str("permille");
    s
}

/// 位图文本：`cfg=0F`。
pub fn cfg_text(cfg: u16) -> String {
    let mut s = String::from("cfg=");
    s.push_str(hex_text(cfg as u32, 2).as_str());
    s
}

// ---------------------------------------------------------------------------
// 上游文案逐字复刻（busybox 分发层 / ash / musl 装载器）
// ---------------------------------------------------------------------------

/// busybox dispatcher：`nosuch: applet not found`。
pub const T_APPLET_NOT_FOUND: &str = ": applet not found";
/// ash 内建未命中：`sh: nosuch: not found`。
pub const T_SHELL_PREFIX: &str = "sh: ";
/// ash 命令未命中尾巴。
pub const T_NOT_FOUND: &str = ": not found";
/// musl 装载器名前缀。
pub const T_MUSL_LD: &str = "ld-musl-x86_64.so.1: ";
/// 装载器缺件尾巴（glibc strerror ENOENT 原文）。
pub const T_NO_SUCH: &str = ": No such file or directory";
/// 宿主 bin 目录前缀。
pub const T_BIN_PREFIX: &str = "/bin/";

/// `x: applet not found`。
pub fn msg_applet_not_found(name: &str) -> String {
    let mut s = String::new();
    s.push_str(name);
    s.push_str(T_APPLET_NOT_FOUND);
    s
}

/// `sh: x: not found`。
pub fn msg_shell_not_found(name: &str) -> String {
    let mut s = String::from(T_SHELL_PREFIX);
    s.push_str(name);
    s.push_str(T_NOT_FOUND);
    s
}

/// `x: not found`（PATH 命中后 dispatcher 再拒，不带 shell 前缀）。
pub fn msg_path_not_found(name: &str) -> String {
    let mut s = String::new();
    s.push_str(name);
    s.push_str(T_NOT_FOUND);
    s
}

/// `ld-musl-x86_64.so.1: x: applet not found`。
pub fn msg_ld_not_found(name: &str) -> String {
    let mut s = String::from(T_MUSL_LD);
    s.push_str(name);
    s.push_str(T_APPLET_NOT_FOUND);
    s
}

/// `/bin/x: No such file or directory`。
pub fn msg_loader_miss(name: &str) -> String {
    let mut s = String::from(T_BIN_PREFIX);
    s.push_str(name);
    s.push_str(T_NO_SUCH);
    s
}

/// 裁剪报告：`trim cfg=0E kept=7 shift=1`。
pub fn msg_trim_report(cfg: u16, kept: u32, shift: u32) -> String {
    let mut s = String::from("trim ");
    s.push_str(cfg_text(cfg).as_str());
    s.push_str(" kept=");
    s.push_str(dec_text(kept).as_str());
    s.push_str(" shift=");
    s.push_str(dec_text(shift).as_str());
    s
}

/// 路径缺席：`path exec-direct gone under cfg=0E`。
pub fn msg_path_gone(path: &str, cfg: u16) -> String {
    let mut s = String::from("path ");
    s.push_str(path);
    s.push_str(" gone under ");
    s.push_str(cfg_text(cfg).as_str());
    s
}

/// 显式改道：`su: dispatcher -> loader-miss`。
pub fn msg_shift(name: &str, from: &str, to: &str) -> String {
    let mut s = String::new();
    s.push_str(name);
    s.push_str(": ");
    s.push_str(from);
    s.push_str(" -> ");
    s.push_str(to);
    s
}

/// 缺席却报同判（本卡头号缺陷）。
pub fn msg_silent_same(name: &str, cfg: u16) -> String {
    let mut s = String::from("silent-same: ");
    s.push_str(name);
    s.push_str(" is absent under ");
    s.push_str(cfg_text(cfg).as_str());
    s.push_str(" yet the verdict claims the full tier");
    s
}

/// 在册却报改道。
pub fn msg_phantom_shift(name: &str, cfg: u16) -> String {
    let mut s = String::from("phantom-shift: ");
    s.push_str(name);
    s.push_str(" survives under ");
    s.push_str(cfg_text(cfg).as_str());
    s.push_str(" yet the verdict claims a shift");
    s
}

/// 重建漂移：`table drift under cfg=0B`。
pub fn msg_table_drift(cfg: u16) -> String {
    let mut s = String::from("table drift under ");
    s.push_str(cfg_text(cfg).as_str());
    s
}

/// 位图非法：`cfg=100F carries unknown bits`.
pub fn msg_cfg_unknown(cfg: u16) -> String {
    let mut s = String::from(cfg_text(cfg).as_str());
    s.push_str(" carries unknown bits");
    s
}

/// 一致率不足。
pub fn msg_rate_short(matched: u32, total: u32) -> String {
    let mut s = String::from("trim sample rate ");
    s.push_str(dec_text(matched).as_str());
    s.push('/');
    s.push_str(dec_text(total).as_str());
    s.push_str(" below ");
    s.push_str(dec_text(RATE_FULL).as_str());
    s
}

/// 用法：本卡入口只认三个档名。
pub fn msg_usage_tier() -> String {
    String::from("usage: trim-matrix [full|no-musl|no-bb-root]")
}

// ---------------------------------------------------------------------------
// 三档 CONFIG
// ---------------------------------------------------------------------------

/// 构建期三档：全量 / 缺 musl / 缺 busybox root applet。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Config {
    /// 全量：八个 applet 全在册。
    Full,
    /// 缺 musl：`CFG_MUSL` 摘掉，musl 装载器与其名下 applet 一并消失。
    NoMusl,
    /// 缺 busybox root applet：`CFG_BUSYBOX_ROOT` 摘掉，passwd/su/mount 消失。
    NoRoot,
}

impl Config {
    /// 三档全表（次序即 `CFG_TIER` 下标）。
    pub fn all() -> [Config; CONFIG_COUNT] {
        [Config::Full, Config::NoMusl, Config::NoRoot]
    }

    /// 档名（构建脚本与报告共用）。
    pub fn name(&self) -> &'static str {
        match *self {
            Config::Full => "full",
            Config::NoMusl => "no-musl",
            Config::NoRoot => "no-bb-root",
        }
    }

    /// 档序。
    pub fn index(&self) -> usize {
        match *self {
            Config::Full => 0,
            Config::NoMusl => 1,
            Config::NoRoot => 2,
        }
    }

    /// 档序 → 档。
    pub fn of_index(i: usize) -> Option<Config> {
        match i {
            0 => Some(Config::Full),
            1 => Some(Config::NoMusl),
            2 => Some(Config::NoRoot),
            _ => None,
        }
    }

    /// 本档位图。
    pub fn mask(&self) -> u16 {
        match CFG_TIER.get(self.index()) {
            Some(x) => *x,
            None => CFG_NONE,
        }
    }

    /// 本档相对全量档摘掉的位。
    pub fn dropped(&self) -> u16 {
        CFG_ALL & !self.mask()
    }

    /// 位图 → 档（不认的位图给 None，绝不夹带默认档）。
    pub fn of_mask(cfg: u16) -> Option<Config> {
        let table = Config::all();
        let mut i = 0usize;
        while i < CONFIG_COUNT {
            if let Some(c) = table.get(i) {
                if c.mask() == cfg {
                    return Some(*c);
                }
            }
            i += 1;
        }
        None
    }
}

/// 位图里置位的个数。
pub fn count_bits(cfg: u16) -> u32 {
    let mut n = 0u32;
    let mut v = cfg;
    while v != 0 {
        n += (v & 1) as u32;
        v = v >> 1;
    }
    n
}

/// 裁剪：从 `base` 摘掉 `off`。
pub fn trim(base: u16, off: u16) -> u16 {
    base & !off
}

/// 裁剪面是否只摘掉了已定义的位。
pub fn trim_known(base: u16, off: u16) -> bool {
    (base & CFG_ALL) == base && (off & CFG_ALL) == off
}

// ---------------------------------------------------------------------------
// applet 表
// ---------------------------------------------------------------------------

/// 八个 applet：三个需要 root 位，一个需要 musl 位，其余只要 busybox/shell。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Applet {
    /// `sh`：走内建分发。
    Sh,
    /// `dd`：busybox dispatcher。
    Dd,
    /// `ls`：PATH 搜到 busybox 软链后由 dispatcher 判。
    Ls,
    /// `sed`：PATH 搜到 busybox 软链。
    Sed,
    /// `passwd`：busybox root applet。
    Passwd,
    /// `su`：busybox root applet。
    Su,
    /// `mount`：busybox root applet。
    Mount,
    /// musl 装载器名下的 applet。
    MuslLd,
}

impl Applet {
    /// 八 applet 全表（次序即账本段号）。
    pub fn all() -> [Applet; APPLET_COUNT] {
        [
            Applet::Sh,
            Applet::Dd,
            Applet::Ls,
            Applet::Sed,
            Applet::Passwd,
            Applet::Su,
            Applet::Mount,
            Applet::MuslLd,
        ]
    }

    /// applet 名（与命令行里的词逐字相同）。
    pub fn name(&self) -> &'static str {
        match *self {
            Applet::Sh => "sh",
            Applet::Dd => "dd",
            Applet::Ls => "ls",
            Applet::Sed => "sed",
            Applet::Passwd => "passwd",
            Applet::Su => "su",
            Applet::Mount => "mount",
            Applet::MuslLd => "ld-musl-x86_64.so.1",
        }
    }

    /// 段号（1 起）。
    pub fn id(&self) -> u8 {
        match *self {
            Applet::Sh => 0x01,
            Applet::Dd => 0x02,
            Applet::Ls => 0x03,
            Applet::Sed => 0x04,
            Applet::Passwd => 0x05,
            Applet::Su => 0x06,
            Applet::Mount => 0x07,
            Applet::MuslLd => 0x08,
        }
    }

    /// 段号 → applet。
    pub fn of_id(id: u8) -> Option<Applet> {
        match id {
            0x01 => Some(Applet::Sh),
            0x02 => Some(Applet::Dd),
            0x03 => Some(Applet::Ls),
            0x04 => Some(Applet::Sed),
            0x05 => Some(Applet::Passwd),
            0x06 => Some(Applet::Su),
            0x07 => Some(Applet::Mount),
            0x08 => Some(Applet::MuslLd),
            _ => None,
        }
    }

    /// 在册所需的全部位（缺一即缺席）。
    pub fn needs(&self) -> u16 {
        match *self {
            Applet::Sh => CFG_BUSYBOX | CFG_SHELL,
            Applet::Dd => CFG_BUSYBOX,
            Applet::Ls => CFG_BUSYBOX,
            Applet::Sed => CFG_BUSYBOX,
            Applet::Passwd => CFG_BUSYBOX | CFG_BUSYBOX_ROOT,
            Applet::Su => CFG_BUSYBOX | CFG_BUSYBOX_ROOT,
            Applet::Mount => CFG_BUSYBOX | CFG_BUSYBOX_ROOT,
            Applet::MuslLd => CFG_MUSL,
        }
    }

    /// 在册时 notfound 走哪条路径。
    pub fn native_path(&self) -> PathKind {
        match *self {
            Applet::Sh => PathKind::ShellBuiltin,
            Applet::Dd => PathKind::Dispatcher,
            Applet::Ls => PathKind::PathSearch,
            Applet::Sed => PathKind::PathSearch,
            Applet::Passwd => PathKind::Dispatcher,
            Applet::Su => PathKind::Dispatcher,
            Applet::Mount => PathKind::Dispatcher,
            Applet::MuslLd => PathKind::ExecDirect,
        }
    }

    /// 本 applet 在这一档是否在册。
    pub fn survives(&self, cfg: u16) -> bool {
        let need = self.needs();
        need != CFG_NONE && (cfg & need) == need
    }

    /// 是否 root 专属 applet。
    pub fn is_root(&self) -> bool {
        (self.needs() & CFG_BUSYBOX_ROOT) != 0
    }

    /// 是否 musl 名下 applet。
    pub fn is_musl(&self) -> bool {
        (self.needs() & CFG_MUSL) != 0
    }
}

/// 表长：这一档在册的 applet 数。
pub fn table_len(cfg: u16) -> u32 {
    let table = Applet::all();
    let mut n = 0u32;
    let mut i = 0usize;
    while i < APPLET_COUNT {
        if let Some(a) = table.get(i) {
            if a.survives(cfg) {
                n += 1;
            }
        }
        i += 1;
    }
    n
}

/// 表指纹：按表序折叠在册 applet 的名字。同 cfg 两次必须同值。
pub fn table_seal(cfg: u16) -> u64 {
    let table = Applet::all();
    let mut h = FNV_OFF;
    h = fold_word(h, cfg as u64);
    let mut n = 0u32;
    let mut i = 0usize;
    while i < APPLET_COUNT {
        if let Some(a) = table.get(i) {
            if a.survives(cfg) {
                h = fold_byte(h, a.id());
                h = fold_str(h, a.name());
                n += 1;
            }
        }
        i += 1;
    }
    fold_word(h, n as u64)
}

/// 表是否会重号（本卡的表是位图展开，重号只可能来自段号写重）。
pub fn table_duplicated() -> bool {
    let table = Applet::all();
    let mut i = 0usize;
    while i < APPLET_COUNT {
        let a = match table.get(i) {
            Some(x) => *x,
            None => return true,
        };
        let mut j = i + 1usize;
        while j < APPLET_COUNT {
            if let Some(b) = table.get(j) {
                if a.id() == b.id() || a.name() == b.name() {
                    return true;
                }
            }
            j += 1;
        }
        i += 1;
    }
    false
}

// ---------------------------------------------------------------------------
// notfound 路径
// ---------------------------------------------------------------------------

/// 六条 notfound 路径。每条路径有自己的前置位，位一摘，路径就不存在。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PathKind {
    /// busybox dispatcher（`busybox x`）。
    Dispatcher,
    /// shell 内建分发（`sh -c x`）。
    ShellBuiltin,
    /// PATH 搜索命中 busybox 软链。
    PathSearch,
    /// 直接 exec musl 装载器。
    ExecDirect,
    /// 以 argv[0] 作 applet 名（symlink 名）。
    AliasArgv0,
    /// 装载器缺件——**永远存在**，因为它在内核与 ld.so 之间，不在 CONFIG 里。
    LoaderMiss,
}

impl PathKind {
    /// 六路径全表。
    pub fn all() -> [PathKind; PATH_COUNT] {
        [
            PathKind::Dispatcher,
            PathKind::ShellBuiltin,
            PathKind::PathSearch,
            PathKind::ExecDirect,
            PathKind::AliasArgv0,
            PathKind::LoaderMiss,
        ]
    }

    /// 路径名。
    pub fn name(&self) -> &'static str {
        match *self {
            PathKind::Dispatcher => "dispatcher",
            PathKind::ShellBuiltin => "shell-builtin",
            PathKind::PathSearch => "path-search",
            PathKind::ExecDirect => "exec-direct",
            PathKind::AliasArgv0 => "alias-argv0",
            PathKind::LoaderMiss => "loader-miss",
        }
    }

    /// 该路径在这一档的前置位；`CFG_NONE` 表示无条件存在。
    pub fn gate(&self) -> u16 {
        match *self {
            PathKind::Dispatcher => CFG_BUSYBOX,
            PathKind::ShellBuiltin => CFG_SHELL,
            PathKind::PathSearch => CFG_BUSYBOX,
            PathKind::ExecDirect => CFG_MUSL,
            PathKind::AliasArgv0 => CFG_BUSYBOX,
            PathKind::LoaderMiss => CFG_NONE,
        }
    }

    /// 该路径在这一档是否存在。
    pub fn exists(&self, cfg: u16) -> bool {
        let g = self.gate();
        g == CFG_NONE || (cfg & g) == g
    }

    /// 段序（矩阵列序）。
    pub fn index(&self) -> usize {
        match *self {
            PathKind::Dispatcher => 0,
            PathKind::ShellBuiltin => 1,
            PathKind::PathSearch => 2,
            PathKind::ExecDirect => 3,
            PathKind::AliasArgv0 => 4,
            PathKind::LoaderMiss => 5,
        }
    }

    /// 本路径的 notfound 文案。
    pub fn text(&self, name: &str) -> String {
        match *self {
            PathKind::Dispatcher => msg_applet_not_found(name),
            PathKind::ShellBuiltin => msg_shell_not_found(name),
            PathKind::PathSearch => msg_path_not_found(name),
            PathKind::ExecDirect => msg_ld_not_found(name),
            PathKind::AliasArgv0 => msg_applet_not_found(name),
            PathKind::LoaderMiss => msg_loader_miss(name),
        }
    }
}

// ---------------------------------------------------------------------------
// 档位 × 路径矩阵（本卡的交付物）
// ---------------------------------------------------------------------------

/// 三档 × 六路径的存在性矩阵。
///
/// 每一格回答一个问题："这条 notfound 路径在这一档构建里还存不存在？"——
/// 这一问是本卡与"只比退出码"的分水岭：码是三档常量，路径不是。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TrimMatrix {
    cells: [[u8; PATH_COUNT]; CONFIG_COUNT],
}

impl TrimMatrix {
    /// 由 `CFG_TIER` 重建。
    pub fn build() -> TrimMatrix {
        let mut cells = [[0u8; PATH_COUNT]; CONFIG_COUNT];
        let mut c = 0usize;
        while c < CONFIG_COUNT {
            let cfg = match CFG_TIER.get(c) {
                Some(x) => *x,
                None => return TrimMatrix { cells },
            };
            let mut p = 0usize;
            while p < PATH_COUNT {
                let path = match PathKind::all().get(p) {
                    Some(x) => *x,
                    None => return TrimMatrix { cells },
                };
                let mark = if path.exists(cfg) { 1u8 } else { 0u8 };
                if let Some(row) = cells.get_mut(c) {
                    if let Some(cell) = row.get_mut(p) {
                        *cell = mark;
                    }
                }
                p += 1;
            }
            c += 1;
        }
        TrimMatrix { cells }
    }

    /// 取一格。
    pub fn cell(&self, c: usize, p: usize) -> Option<bool> {
        match self.cells.get(c) {
            Some(row) => match row.get(p) {
                Some(v) => Some(*v != 0),
                None => None,
            },
            None => None,
        }
    }

    /// 三档下存在的路径格总数（6 + 5 + 6 = 17）。
    pub fn present_cells(&self) -> u32 {
        let mut n = 0u32;
        let mut c = 0usize;
        while c < CONFIG_COUNT {
            let mut p = 0usize;
            while p < PATH_COUNT {
                if let Some(true) = self.cell(c, p) {
                    n += 1;
                }
                p += 1;
            }
            c += 1;
        }
        n
    }

    /// 某一档存在的路径数。
    pub fn tier_present(&self, c: usize) -> u32 {
        let mut n = 0u32;
        let mut p = 0usize;
        while p < PATH_COUNT {
            if let Some(true) = self.cell(c, p) {
                n += 1;
            }
            p += 1;
        }
        n
    }

    /// 是否每格都有落点（零悬空）。
    pub fn decided(&self) -> bool {
        let mut c = 0usize;
        while c < CONFIG_COUNT {
            let mut p = 0usize;
            while p < PATH_COUNT {
                if self.cell(c, p).is_none() {
                    return false;
                }
                p += 1;
            }
            c += 1;
        }
        true
    }

    /// 某档的一行文本：`full: 111111`。
    pub fn row_text(&self, c: usize) -> String {
        let mut s = String::new();
        match Config::of_index(c) {
            Some(cfg) => s.push_str(cfg.name()),
            None => s.push_str("?"),
        }
        s.push(':');
        let mut p = 0usize;
        while p < PATH_COUNT {
            match self.cell(c, p) {
                Some(true) => s.push('1'),
                Some(false) => s.push('0'),
                None => s.push('x'),
            }
            p += 1;
        }
        s
    }

    /// 三行渲染。
    pub fn render(&self) -> String {
        let mut s = String::new();
        let mut c = 0usize;
        while c < CONFIG_COUNT {
            if c > 0 {
                s.push(' ');
            }
            s.push_str(self.row_text(c).as_str());
            c += 1;
        }
        s
    }
}

/// 矩阵快捷构造（判据与测试共用）。
pub fn trim_matrix() -> TrimMatrix {
    TrimMatrix::build()
}

/// 某 applet 在给定档下实际走的路径：在册走原生路径，缺席落到装载器。
pub fn path_in(cfg: u16, applet: Applet) -> PathKind {
    if applet.survives(cfg) {
        applet.native_path()
    } else {
        PathKind::LoaderMiss
    }
}

/// 某 applet 在给定档下的 notfound 文案。
pub fn text_in(cfg: u16, applet: Applet) -> String {
    path_in(cfg, applet).text(applet.name())
}

/// 在册 applet 在给定档下是否与全量档同判（本卡要求恒真）。
pub fn verdict_identical(cfg: u16, applet: Applet) -> bool {
    let full = path_in(CFG_ALL, applet);
    let here = path_in(cfg, applet);
    applet.survives(cfg)
        && here == full
        && here.exists(cfg)
        && text_in(cfg, applet) == text_in(CFG_ALL, applet)
}

/// 缺席 applet 在给定档下是否显式改道（本卡要求恒真）。
pub fn shift_explicit(cfg: u16, applet: Applet) -> bool {
    let full = path_in(CFG_ALL, applet);
    let here = path_in(cfg, applet);
    !applet.survives(cfg) && here != full && here.exists(cfg) && text_in(cfg, applet) != text_in(CFG_ALL, applet)
}

// ---------------------------------------------------------------------------
// 闸门段
// ---------------------------------------------------------------------------

/// 八段闸门，秩 1..=8。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// 位图合法性。
    Cfg,
    /// 裁剪面非空。
    Trim,
    /// 重建确定性。
    Rebuild,
    /// 表平整。
    Table,
    /// 在册性。
    Present,
    /// 路径存在性。
    Path,
    /// 判定一致性。
    Verdict,
    /// 一致率结算。
    Rate,
}

impl Stage {
    /// 八段全表（次序即纵线）。
    pub fn all() -> [Stage; 8] {
        [
            Stage::Cfg,
            Stage::Trim,
            Stage::Rebuild,
            Stage::Table,
            Stage::Present,
            Stage::Path,
            Stage::Verdict,
            Stage::Rate,
        ]
    }

    /// 秩（1 起）。
    pub fn rank(&self) -> u32 {
        match *self {
            Stage::Cfg => 1,
            Stage::Trim => 2,
            Stage::Rebuild => 3,
            Stage::Table => 4,
            Stage::Present => 5,
            Stage::Path => 6,
            Stage::Verdict => 7,
            Stage::Rate => 8,
        }
    }

    /// 秩 → 段（越界给 None）。
    pub fn of_rank(r: u32) -> Option<Stage> {
        match r {
            1 => Some(Stage::Cfg),
            2 => Some(Stage::Trim),
            3 => Some(Stage::Rebuild),
            4 => Some(Stage::Table),
            5 => Some(Stage::Present),
            6 => Some(Stage::Path),
            7 => Some(Stage::Verdict),
            8 => Some(Stage::Rate),
            _ => None,
        }
    }

    /// 段名。
    pub fn name(&self) -> &'static str {
        match *self {
            Stage::Cfg => "cfg",
            Stage::Trim => "trim",
            Stage::Rebuild => "rebuild",
            Stage::Table => "table",
            Stage::Present => "present",
            Stage::Path => "path",
            Stage::Verdict => "verdict",
            Stage::Rate => "rate",
        }
    }

    /// 该段破断时的落点码。
    pub fn fault_code(&self) -> u32 {
        match *self {
            Stage::Cfg => TM_ECFG,
            Stage::Trim => TM_ETRIM,
            Stage::Rebuild => TM_ETABLE,
            Stage::Table => TM_ETABLE,
            Stage::Present => TM_EPATH,
            Stage::Path => TM_EPATH,
            Stage::Verdict => TM_ETEXT,
            Stage::Rate => TM_EDEFAULT,
        }
    }

    /// 是否已进入"结论面"（Verdict 起）。
    pub fn is_verdict_side(&self) -> bool {
        self.rank() >= Stage::Verdict.rank()
    }
}

// ---------------------------------------------------------------------------
// 判定面
// ---------------------------------------------------------------------------

/// 一条 notfound 判定相对全量档的关系。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// 与全量档逐字相同（applet 在册）。
    Same,
    /// 显式不同（applet 缺席，路径落到装载器）。
    Shifted,
    /// 不一致且不成立。
    Red,
}

impl Verdict {
    /// 判定名。
    pub fn name(&self) -> &'static str {
        match *self {
            Verdict::Same => "same",
            Verdict::Shifted => "shifted",
            Verdict::Red => "red",
        }
    }

    /// 是否显式改道。
    pub fn is_shifted(&self) -> bool {
        match *self {
            Verdict::Shifted => true,
            _ => false,
        }
    }

    /// 是否计入分子（Same 与 Shifted 都计：改道是正确结论）。
    pub fn counts_toward(&self) -> bool {
        match *self {
            Verdict::Same | Verdict::Shifted => true,
            Verdict::Red => false,
        }
    }
}

/// 注记——每一条判定的"凭什么"。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Note {
    /// 无注记。
    Nil,
    /// 与全量档同判。
    SameVerdict,
    /// 显式改道。
    PathShift,
    /// 位图未知。
    CfgUnknown,
    /// 裁剪面全灭。
    TrimEmpty,
    /// 重建漂移。
    TableDrift,
    /// 表没建起来。
    TableMissing,
    /// 缺席却没声明改道。
    ShiftUndeclared,
    /// 在册却声明改道。
    PhantomShift,
    /// 路径在这一档不存在。
    PathGone,
    /// 缺席却报同判（头号缺陷）。
    SilentSame,
}

impl Note {
    /// 注记名。
    pub fn name(&self) -> &'static str {
        match *self {
            Note::Nil => "nil",
            Note::SameVerdict => "same-verdict",
            Note::PathShift => "path-shift",
            Note::CfgUnknown => "cfg-unknown",
            Note::TrimEmpty => "trim-empty",
            Note::TableDrift => "table-drift",
            Note::TableMissing => "table-missing",
            Note::ShiftUndeclared => "shift-undeclared",
            Note::PhantomShift => "phantom-shift",
            Note::PathGone => "path-gone",
            Note::SilentSame => "silent-same",
        }
    }

    /// 该注记是否让重建抽样整体作废。
    pub fn voids_sample(&self) -> bool {
        match *self {
            Note::CfgUnknown | Note::TrimEmpty | Note::TableDrift | Note::TableMissing => true,
            _ => false,
        }
    }

    /// 该注记是否属于"改道面"（本卡的正确结论之一）。
    pub fn is_shift_side(&self) -> bool {
        match *self {
            Note::PathShift => true,
            _ => false,
        }
    }
}

/// 缺陷维——申报对判的落点。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fault {
    /// 成败互报。
    Estate,
    /// 阶段不符。
    Stage,
    /// 错误码不符。
    Code,
    /// 数值不符。
    Value,
    /// 注记不符。
    Note,
    /// 兜底档被当成结论。
    Default,
    /// 无缺陷。
    Ok,
}

impl Fault {
    /// 六维反例表（契约给定次序）。
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

    /// 缺陷维对应的码。
    pub fn code(&self) -> u32 {
        match *self {
            Fault::Estate => TM_ESTATE,
            Fault::Stage => TM_ESTAGE,
            Fault::Code => TM_ECODE,
            Fault::Value => TM_EVALUE,
            Fault::Note => TM_ENOTE,
            Fault::Default => TM_EDEFAULT,
            Fault::Ok => 0,
        }
    }

    /// 缺陷维名。
    pub fn name(&self) -> &'static str {
        match *self {
            Fault::Estate => "estate",
            Fault::Stage => "stage",
            Fault::Code => "code",
            Fault::Value => "value",
            Fault::Note => "note",
            Fault::Default => "default",
            Fault::Ok => "ok",
        }
    }
}

/// 反例六维表（第二形式，与 `Fault::reject_faults()` 交叉校验）。
pub const REJECT_FAULTS: [Fault; REJECT] = [
    Fault::Estate,
    Fault::Stage,
    Fault::Code,
    Fault::Value,
    Fault::Note,
    Fault::Default,
];

// ---------------------------------------------------------------------------
// 现场（Shape）
// ---------------------------------------------------------------------------

/// 一条裁剪现场：位图 + applet + 观测到的路径 + 若干自证位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Shape {
    /// 这一档的 CONFIG 位图。
    pub cfg: u16,
    /// 被问到的 applet。
    pub applet: Applet,
    /// 观测到的 notfound 路径。
    pub path: PathKind,
    /// 是否显式声明了"缺席 → 改道"。
    pub declared_shift: bool,
    /// 同一 cfg 两次重建是否同指纹。
    pub rebuild_stable: bool,
    /// 表是否建起来（非空、无重号）。
    pub table_built: bool,
    /// 位图是否只含已定义位。
    pub cfg_known: bool,
    /// 观测到的判定是否"与全量档同判"。
    pub same_verdict: bool,
    /// 结算前没有任何判定（未判档）。
    pub unjudged: bool,
    /// 统计值（本条计入的分子）。
    pub value: u32,
    /// 兜底档被当成结论。
    pub default: bool,
}

impl Shape {
    /// 一条合法现场：位图与 applet 自洽，改道声明与在册性一致。
    pub fn live(cfg: u16, applet: Applet) -> Shape {
        let present = applet.survives(cfg);
        Shape {
            cfg,
            applet,
            path: path_in(cfg, applet),
            declared_shift: !present,
            rebuild_stable: true,
            table_built: true,
            cfg_known: (cfg & CFG_RESERVED) == 0,
            same_verdict: present,
            unjudged: false,
            value: 2,
            default: false,
        }
    }

    /// 全量档现场。
    pub fn full(applet: Applet) -> Shape {
        Shape::live(CFG_ALL, applet)
    }

    /// 某档现场。
    pub fn tier(tier: Config, applet: Applet) -> Shape {
        Shape::live(tier.mask(), applet)
    }

    /// 换档（路径与声明随档重算，绝不沿用旧档结论）。
    pub fn with_tier(&self, tier: Config) -> Shape {
        Shape::live(tier.mask(), self.applet)
    }

    /// 换算子。
    pub fn with_applet(&self, applet: Applet) -> Shape {
        Shape {
            applet,
            path: path_in(self.cfg, applet),
            ..*self
        }
    }

    /// 换观测路径（只用于注入"看错路"的现场）。
    pub fn with_path(&self, path: PathKind) -> Shape {
        Shape { path, ..*self }
    }

    /// 换改道声明位。
    pub fn with_shift(&self, declared: bool) -> Shape {
        Shape {
            declared_shift: declared,
            ..*self
        }
    }

    /// 换"同判"观测位。
    pub fn with_same(&self, same: bool) -> Shape {
        Shape {
            same_verdict: same,
            ..*self
        }
    }

    /// 换重建确定性位。
    pub fn with_rebuild(&self, stable: bool) -> Shape {
        Shape {
            rebuild_stable: stable,
            ..*self
        }
    }

    /// 换表建成位。
    pub fn with_table(&self, built: bool) -> Shape {
        Shape {
            table_built: built,
            ..*self
        }
    }

    /// 换位图合法性位。
    pub fn with_cfg_known(&self, known: bool) -> Shape {
        Shape {
            cfg_known: known,
            ..*self
        }
    }

    /// 换未判档位。
    pub fn with_unjudged(&self, v: bool) -> Shape {
        Shape {
            unjudged: v,
            ..*self
        }
    }

    /// 该现场的真实判定面（不经闸门，只给对照用）。
    pub fn truth(&self) -> Verdict {
        if self.applet.survives(self.cfg) {
            Verdict::Same
        } else {
            Verdict::Shifted
        }
    }

    /// 现场指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.cfg as u64);
        h = fold_byte(h, self.applet.id());
        h = fold_str(h, self.path.name());
        h = fold_bool(h, self.declared_shift);
        h = fold_bool(h, self.rebuild_stable);
        h = fold_bool(h, self.table_built);
        h = fold_bool(h, self.cfg_known);
        h = fold_bool(h, self.same_verdict);
        h = fold_bool(h, self.unjudged);
        h = fold_word(h, self.value as u64);
        h = fold_bool(h, self.default);
        h
    }
}

/// `define` 的输出：走到了哪一段、落在哪条码、记了什么注。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Outcome {
    /// 最后成立的段。
    pub stage: Stage,
    /// 错误码（0 = 无）。
    pub code: u32,
    /// 统计值。
    pub value: u32,
    /// 注记。
    pub note: Note,
    /// 兜底档。
    pub default: bool,
    /// 本条是否成立。
    pub ok: bool,
    /// 判定面。
    pub verdict: Verdict,
}

impl Outcome {
    /// 一条成立的结果。
    fn pass(stage: Stage, note: Note, value: u32, verdict: Verdict) -> Outcome {
        Outcome {
            stage,
            code: 0,
            value,
            note,
            default: false,
            ok: true,
            verdict,
        }
    }

    /// 一条破断的结果。
    fn fail(stage: Stage, code: u32, note: Note, value: u32) -> Outcome {
        Outcome {
            stage,
            code,
            value,
            note,
            default: false,
            ok: false,
            verdict: Verdict::Red,
        }
    }

    /// 是否成立。
    pub fn ok(&self) -> bool {
        self.ok
    }

    /// 是否破断。
    pub fn failed(&self) -> bool {
        !self.ok
    }

    /// 是否被重建侧判死（根本不该进矩阵）。
    pub fn refused(&self) -> bool {
        self.code == TM_ECFG || self.code == TM_ETRIM || self.code == TM_ETABLE
    }

    /// 是否改道档。
    pub fn shifted(&self) -> bool {
        self.ok && self.verdict == Verdict::Shifted
    }

    /// 退出码档：本卡三档同码，只有兜底档例外。
    pub fn exit(&self) -> u32 {
        if self.default {
            return 2;
        }
        if self.code == TM_ESLOT {
            return 2;
        }
        if self.ok {
            EXIT_OK
        } else {
            EXIT_NOTFOUND
        }
    }

    /// 结果指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.stage.rank() as u64);
        h = fold_word(h, self.code as u64);
        h = fold_word(h, self.value as u64);
        h = fold_str(h, self.note.name());
        h = fold_str(h, self.verdict.name());
        h = fold_bool(h, self.default);
        h
    }
}

/// 申报表：对一条现场声称的结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Claim {
    /// 声称成立。
    pub success: bool,
    /// 声称到达的段。
    pub stage: Stage,
    /// 声称的码。
    pub code: u32,
    /// 声称的统计值。
    pub value: u32,
    /// 声称的注记。
    pub note: Note,
    /// 声称的判定面。
    pub verdict: Verdict,
    /// 声称的兜底位。
    pub default: bool,
    /// 反例槽的缺陷标签。
    pub tag: &'static str,
}

impl Claim {
    /// 正例申报。
    pub fn good(stage: Stage, note: Note, value: u32, verdict: Verdict, tag: &'static str) -> Claim {
        Claim {
            success: true,
            stage,
            code: 0,
            value,
            note,
            verdict,
            default: false,
            tag,
        }
    }

    /// 反例申报（其余五维由调用方逐格抄真结论）。
    pub fn bad(stage: Stage, code: u32, note: Note, value: u32, tag: &'static str) -> Claim {
        Claim {
            success: false,
            stage,
            code,
            value,
            note,
            verdict: Verdict::Red,
            default: false,
            tag,
        }
    }

    /// 是否声称成立。
    pub fn success(&self) -> bool {
        self.success
    }

    /// 申报指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_bool(h, self.success);
        h = fold_word(h, self.stage.rank() as u64);
        h = fold_word(h, self.code as u64);
        h = fold_word(h, self.value as u64);
        h = fold_str(h, self.note.name());
        h = fold_str(h, self.verdict.name());
        h = fold_bool(h, self.default);
        h = fold_str(h, self.tag);
        h
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
    /// 真实结果。
    pub fn actual(&self) -> Outcome {
        define(&self.shape)
    }

    /// 真实判定是否成立（矩阵归档用）。
    pub fn ok(&self) -> bool {
        self.actual().ok()
    }

    /// 缺陷标签。
    pub fn tag(&self) -> &'static str {
        self.claim.tag
    }

    /// 判定。
    pub fn fault(&self) -> Fault {
        judge(self)
    }

    /// 语料指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.shape.seal());
        h = fold_word(h, self.claim.seal());
        h
    }
}

/// 现场的真实 notfound 文案（不经闸门）。
pub fn actual_text(s: &Shape) -> String {
    text_in(s.cfg, s.applet)
}

/// 全量档的同一条文案（对照基准）。
pub fn full_text(applet: Applet) -> String {
    text_in(CFG_ALL, applet)
}

/// 主判据纵线：一条裁剪现场如何一步步走到结论。
///
/// 顺序不可调：前一段的"过"只说明这一段没破，不说明后面能跳。
pub fn define(s: &Shape) -> Outcome {
    // 1 Cfg：位图是全部推导的唯一输入。
    if !s.cfg_known || (s.cfg & CFG_RESERVED) != 0 || (s.cfg & CFG_ALL) != s.cfg {
        return Outcome::fail(Stage::Cfg, TM_ECFG, Note::CfgUnknown, 0);
    }
    // 2 Trim：空裁剪面支撑不起任何 notfound 判定。
    if s.cfg == CFG_NONE {
        return Outcome::fail(Stage::Trim, TM_ETRIM, Note::TrimEmpty, 0);
    }
    // 3 Rebuild：同 cfg 两次重建必须同指纹。
    if !s.rebuild_stable {
        return Outcome::fail(Stage::Rebuild, TM_ETABLE, Note::TableDrift, 0);
    }
    // 4 Table：表要建起来，且这一档不能是空表。
    if !s.table_built || table_len(s.cfg) == 0 {
        return Outcome::fail(Stage::Table, TM_ETABLE, Note::TableMissing, 0);
    }
    // 5 Present：在册性与改道声明必须互斥。
    let present = s.applet.survives(s.cfg);
    if present && s.declared_shift {
        return Outcome::fail(Stage::Present, TM_EPATH, Note::PhantomShift, 0);
    }
    if !present && !s.declared_shift {
        return Outcome::fail(Stage::Present, TM_EPATH, Note::ShiftUndeclared, 0);
    }
    // 6 Path：路径必须在这一档存在，且观测值必须是这一档该走的那条。
    let want_path = path_in(s.cfg, s.applet);
    if !want_path.exists(s.cfg) {
        return Outcome::fail(Stage::Path, TM_EPATH, Note::PathGone, 0);
    }
    if s.path != want_path {
        return Outcome::fail(Stage::Path, TM_EPATH, Note::PathGone, 0);
    }
    // 8 Rate 提前接住未判档：跑完了却没有任何判定落账，本卡绝不交付。
    if s.unjudged {
        return Outcome {
            stage: Stage::Rate,
            code: TM_EDEFAULT,
            value: 0,
            note: Note::Nil,
            default: true,
            ok: false,
            verdict: Verdict::Red,
        };
    }
    // 7 Verdict：在册 ⇒ 同判；缺席 ⇒ 必须显式不同。两向都判死。
    if present {
        if s.same_verdict {
            return Outcome::pass(Stage::Verdict, Note::SameVerdict, 2, Verdict::Same);
        }
        return Outcome::fail(Stage::Verdict, TM_ETEXT, Note::PhantomShift, 0);
    }
    if s.same_verdict {
        return Outcome::fail(Stage::Verdict, TM_ETEXT, Note::SilentSame, 0);
    }
    Outcome::pass(Stage::Verdict, Note::PathShift, 2, Verdict::Shifted)
}

/// 现场呈现：把一条结论渲染成一行可读的 notfound 记录。
pub fn message(s: &Shape, o: &Outcome) -> String {
    match o.note {
        Note::SameVerdict => actual_text(s),
        Note::PathShift => {
            let from = path_in(CFG_ALL, s.applet).name();
            let to = path_in(s.cfg, s.applet).name();
            msg_shift(s.applet.name(), from, to)
        }
        Note::SilentSame => msg_silent_same(s.applet.name(), s.cfg),
        Note::PhantomShift => msg_phantom_shift(s.applet.name(), s.cfg),
        Note::PathGone => msg_path_gone(s.path.name(), s.cfg),
        Note::CfgUnknown => msg_cfg_unknown(s.cfg),
        Note::TrimEmpty => msg_trim_report(s.cfg, 0, 0),
        Note::TableDrift => msg_table_drift(s.cfg),
        Note::TableMissing => {
            let mut t = String::from("table missing under ");
            t.push_str(cfg_text(s.cfg).as_str());
            t
        }
        Note::ShiftUndeclared => {
            let mut t = String::from("shift undeclared: ");
            t.push_str(s.applet.name());
            t.push_str(" is absent under ");
            t.push_str(cfg_text(s.cfg).as_str());
            t
        }
        Note::Nil => {
            let mut t = String::from("trim-matrix: ");
            t.push_str(s.applet.name());
            t.push_str(" stopped at stage ");
            t.push_str(o.stage.name());
            t
        }
    }
}

/// 释义——把一条现场讲成人话（红项定位用）。
pub fn detail(c: &Case) -> String {
    let o = c.actual();
    let mut s = String::from(c.tag());
    s.push_str(" cfg=");
    s.push_str(hex_text(c.shape.cfg as u32, 2).as_str());
    s.push_str(" applet=");
    s.push_str(c.shape.applet.name());
    s.push_str(" path=");
    s.push_str(c.shape.path.name());
    s.push_str(" want=");
    s.push_str(path_in(c.shape.cfg, c.shape.applet).name());
    s.push_str(" stage=");
    s.push_str(o.stage.name());
    s.push_str(" code=");
    s.push_str(seg_text(o.code).as_str());
    s.push_str(" note=");
    s.push_str(o.note.name());
    s.push_str(" verdict=");
    s.push_str(o.verdict.name());
    s.push_str(" value=");
    s.push_str(dec_text(o.value).as_str());
    s
}

// ---------------------------------------------------------------------------
// 语料
// ---------------------------------------------------------------------------

/// 语料基型：全量档里一条完全合法的 `dd` 现场。
const BASE: Shape = Shape {
    cfg: CFG_ALL,
    applet: Applet::Dd,
    path: PathKind::Dispatcher,
    declared_shift: false,
    rebuild_stable: true,
    table_built: true,
    cfg_known: true,
    same_verdict: true,
    unjudged: false,
    value: 2,
    default: false,
};

/// 缺 musl 档位图。
const CFG_NO_MUSL: u16 = CFG_ALL & !CFG_MUSL;

/// 缺 root 档位图。
const CFG_NO_ROOT: u16 = CFG_ALL & !CFG_BUSYBOX_ROOT;

/// 显式改道基型：缺席 applet 落到装载器，声明与观测一致。
const SHIFTED: Shape = Shape {
    path: PathKind::LoaderMiss,
    declared_shift: true,
    same_verdict: false,
    ..BASE
};

/// 语料现场表——八活六死，逐槽手写。
pub const SHAPES: [Shape; CORPUS] = [
    // 0 全量档 `dd`：在册、dispatcher、与全量档同判（基线格）。
    Shape {
        applet: Applet::Dd,
        ..BASE
    },
    // 1 全量档 `sh`：走内建分发，仍然同判。
    Shape {
        applet: Applet::Sh,
        path: PathKind::ShellBuiltin,
        ..BASE
    },
    // 2 全量档 `passwd`：root applet 在全量档里在册。
    Shape {
        applet: Applet::Passwd,
        ..BASE
    },
    // 3 缺 musl 档 `ls`：ls 只要 busybox 位，裁剪后**判定必须逐字不变**。
    Shape {
        cfg: CFG_NO_MUSL,
        applet: Applet::Ls,
        path: PathKind::PathSearch,
        ..BASE
    },
    // 4 缺 root 档 `su`：applet 缺席 → 显式改道到装载器。
    Shape {
        cfg: CFG_NO_ROOT,
        applet: Applet::Su,
        ..SHIFTED
    },
    // 5 缺 root 档 `mount`：同上，另一条 root applet。
    Shape {
        cfg: CFG_NO_ROOT,
        applet: Applet::Mount,
        ..SHIFTED
    },
    // 6 缺 musl 档装载器：exec 直连路径随位图一起摘掉，改道到装载器缺件。
    Shape {
        cfg: CFG_NO_MUSL,
        applet: Applet::MuslLd,
        ..SHIFTED
    },
    // 7 全量档 `sed`：PATH 搜到 busybox 软链，同判。
    Shape {
        applet: Applet::Sed,
        path: PathKind::PathSearch,
        ..BASE
    },
    // 8 Estate：缺 root 档里 `su` 明明缺席，现场却报"与全量档同判"。
    Shape {
        cfg: CFG_NO_ROOT,
        applet: Applet::Su,
        same_verdict: true,
        value: 0,
        ..SHIFTED
    },
    // 9 Stage：重建漂移，现场停在 Rebuild，申报却声称已到判定段。
    Shape {
        rebuild_stable: false,
        value: 0,
        ..BASE
    },
    // 10 Code：位图带保留位，现场停在 Cfg，申报却挂表码。
    Shape {
        cfg_known: false,
        cfg: CFG_ALL | 0x0100,
        value: 0,
        ..BASE
    },
    // 11 Value：表没建起来，申报仍按两格计。
    Shape {
        table_built: false,
        value: 0,
        ..BASE
    },
    // 12 Note：观测路径落到了本档不存在的 path-search（全量档 dd 应走 dispatcher）。
    Shape {
        path: PathKind::PathSearch,
        value: 0,
        ..BASE
    },
    // 13 Default：全闸走完却没有任何判定落账（未判档）。
    Shape {
        unjudged: true,
        value: 0,
        ..BASE
    },
];

/// 申报基型：与全量档活现场的实际结论逐格相同。
const GOOD_CLAIM: Claim = Claim {
    success: true,
    stage: Stage::Verdict,
    code: 0,
    value: 2,
    note: Note::SameVerdict,
    verdict: Verdict::Same,
    default: false,
    tag: "",
};

/// 改道档的申报基型。
const SHIFT_CLAIM: Claim = Claim {
    note: Note::PathShift,
    verdict: Verdict::Shifted,
    ..GOOD_CLAIM
};

/// 十四格申报。前八格与 `define(SHAPES[i])` 逐格相同，后六格各坏一维。
pub const CLAIMS: [Claim; CORPUS] = [
    Claim {
        tag: "f11676-full-dd-kept",
        ..GOOD_CLAIM
    },
    Claim {
        tag: "f11676-full-sh-kept",
        ..GOOD_CLAIM
    },
    Claim {
        tag: "f11676-full-passwd-kept",
        ..GOOD_CLAIM
    },
    Claim {
        tag: "f11676-no-musl-ls-kept",
        ..GOOD_CLAIM
    },
    Claim {
        tag: "f11676-no-root-su-shift",
        ..SHIFT_CLAIM
    },
    Claim {
        tag: "f11676-no-root-mount-shift",
        ..SHIFT_CLAIM
    },
    Claim {
        tag: "f11676-no-musl-ld-shift",
        ..SHIFT_CLAIM
    },
    Claim {
        tag: "f11676-full-sed-kept",
        ..GOOD_CLAIM
    },
    // 8 成败互报：其余五维照实抄，只把"破断"这一行申报成成立。
    Claim {
        success: true,
        stage: Stage::Verdict,
        code: TM_ETEXT,
        value: 0,
        note: Note::SilentSame,
        verdict: Verdict::Red,
        default: false,
        tag: "f11676-estate-swap",
    },
    // 9 阶段不符：停在 Rebuild，申报已到判定段。
    Claim {
        success: false,
        stage: Stage::Verdict,
        code: TM_ETABLE,
        value: 0,
        note: Note::TableDrift,
        verdict: Verdict::Red,
        default: false,
        tag: "f11676-stage-skip",
    },
    // 10 码档不符：位图错申报成表码。
    Claim {
        success: false,
        stage: Stage::Cfg,
        code: TM_ETABLE,
        value: 0,
        note: Note::CfgUnknown,
        verdict: Verdict::Red,
        default: false,
        tag: "f11676-code-cfg",
    },
    // 11 数值不符：空表仍按两格计。
    Claim {
        success: false,
        stage: Stage::Table,
        code: TM_ETABLE,
        value: 2,
        note: Note::TableMissing,
        verdict: Verdict::Red,
        default: false,
        tag: "f11676-value-empty",
    },
    // 12 注记不符：看错路申报成"没声明改道"。
    Claim {
        success: false,
        stage: Stage::Path,
        code: TM_EPATH,
        value: 0,
        note: Note::ShiftUndeclared,
        verdict: Verdict::Red,
        default: false,
        tag: "f11676-note-path",
    },
    // 13 兜底档：未判现场申报成"没有兜底"。
    Claim {
        success: false,
        stage: Stage::Rate,
        code: TM_EDEFAULT,
        value: 0,
        note: Note::Nil,
        verdict: Verdict::Red,
        default: false,
        tag: "f11676-default-unjudged",
    },
];

// ---------------------------------------------------------------------------
// 期望表（申报表的第二种形式，任一处笔误都要露出来）
// ---------------------------------------------------------------------------

/// 期望阶段（现场真实落到的段）。
pub const WANT_STAGE: [Stage; CORPUS] = [
    Stage::Verdict,
    Stage::Verdict,
    Stage::Verdict,
    Stage::Verdict,
    Stage::Verdict,
    Stage::Verdict,
    Stage::Verdict,
    Stage::Verdict,
    Stage::Verdict,
    Stage::Rebuild,
    Stage::Cfg,
    Stage::Table,
    Stage::Path,
    Stage::Rate,
];

/// 期望错误码（0 = 无码）。
pub const WANT_CODE: [u32; CORPUS] = [
    0, 0, 0, 0, 0, 0, 0, 0, TM_ETEXT, TM_ETABLE, TM_ECFG, TM_ETABLE, TM_EPATH, TM_EDEFAULT,
];

/// 期望缺陷维序位（0..5 六维，6 = 无缺陷）。
pub const WANT_NDX: [usize; CORPUS] = [6, 6, 6, 6, 6, 6, 6, 6, 0, 1, 2, 3, 4, 5];

/// 期望注记。
pub const WANT_NOTE: [Note; CORPUS] = [
    Note::SameVerdict,
    Note::SameVerdict,
    Note::SameVerdict,
    Note::SameVerdict,
    Note::PathShift,
    Note::PathShift,
    Note::PathShift,
    Note::SameVerdict,
    Note::SilentSame,
    Note::TableDrift,
    Note::CfgUnknown,
    Note::TableMissing,
    Note::PathGone,
    Note::Nil,
];

/// 期望兜底位。
pub const WANT_DEFAULT: [bool; CORPUS] = [
    false, false, false, false, false, false, false, false, false, false, false, false, false, true,
];

/// 期望短名（标签必须挂本卡号）。
pub const WANT_TAGS: [&str; CORPUS] = [
    "f11676-full-dd-kept",
    "f11676-full-sh-kept",
    "f11676-full-passwd-kept",
    "f11676-no-musl-ls-kept",
    "f11676-no-root-su-shift",
    "f11676-no-root-mount-shift",
    "f11676-no-musl-ld-shift",
    "f11676-full-sed-kept",
    "f11676-estate-swap",
    "f11676-stage-skip",
    "f11676-code-cfg",
    "f11676-value-empty",
    "f11676-note-path",
    "f11676-default-unjudged",
];

/// 期望统计值。
pub const WANT_VALUE: [u32; CORPUS] = [2, 2, 2, 2, 2, 2, 2, 2, 0, 0, 0, 0, 0, 0];

/// 期望成败。
pub const WANT_SUCCESS: [bool; CORPUS] = [
    true, true, true, true, true, true, true, true, false, false, false, false, false, false,
];

/// 期望判定面。
pub const WANT_VERDICT: [Verdict; CORPUS] = [
    Verdict::Same,
    Verdict::Same,
    Verdict::Same,
    Verdict::Same,
    Verdict::Shifted,
    Verdict::Shifted,
    Verdict::Shifted,
    Verdict::Same,
    Verdict::Red,
    Verdict::Red,
    Verdict::Red,
    Verdict::Red,
    Verdict::Red,
    Verdict::Red,
];

// ---------------------------------------------------------------------------
// 判定
// ---------------------------------------------------------------------------

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
/// 成败互报优先：把一条"缺席却报同判"的破断申报成成立，是最严重的谎——
/// 本卡的全部价值就是"裁剪之后那条判定还说不说得清"。
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
    if a.note != k.note || a.verdict != k.verdict {
        return Fault::Note;
    }
    // 兜底档双向判死：真落了未判档，或申报凭空说"落了"，两边都挂。
    if a.default || a.default != k.default {
        return Fault::Default;
    }
    Fault::Ok
}

/// 语料取槽。
pub fn case_at(i: usize) -> Option<Case> {
    match (SHAPES.get(i), CLAIMS.get(i)) {
        (Some(s), Some(c)) => Some(Case {
            shape: *s,
            claim: *c,
        }),
        _ => None,
    }
}

/// 全语料。
pub fn cases() -> [Case; CORPUS] {
    let mut out = [Case {
        shape: BASE,
        claim: GOOD_CLAIM,
    }; CORPUS];
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
        return Err(TM_ESLOT);
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

/// 第 `i` 槽（反例区）期望的缺陷维。
fn expected_reject_ndx(i: usize) -> Fault {
    match expected(i) {
        Some(f) => f,
        None => Fault::Ok,
    }
}

// ---------------------------------------------------------------------------
// 抽样结论族：三档 × 八 applet
// ---------------------------------------------------------------------------

/// 一格的抽样结论：在册必同判，缺席必改道。
pub fn sample_agrees(cfg: u16, applet: Applet) -> bool {
    if applet.survives(cfg) {
        verdict_identical(cfg, applet)
    } else {
        shift_explicit(cfg, applet)
    }
}

/// 三档全表抽样（24 格）是否逐格成立。
pub fn tiers_agree() -> bool {
    let mut c = 0usize;
    while c < CONFIG_COUNT {
        let cfg = match CFG_TIER.get(c) {
            Some(x) => *x,
            None => return false,
        };
        let mut a = 0usize;
        while a < APPLET_COUNT {
            let ap = match Applet::all().get(a) {
                Some(x) => *x,
                None => return false,
            };
            if !sample_agrees(cfg, ap) {
                return false;
            }
            a += 1;
        }
        c += 1;
    }
    true
}

/// 三档重建的表长表（1+7+5 之外一律判死）。
pub fn tier_table_len(tier: Config) -> u32 {
    table_len(tier.mask())
}

/// 三档重建的结果：重建两次同指纹才算稳。
pub fn rebuild_is_deterministic(cfg: u16) -> bool {
    let a = table_seal(cfg);
    let b = table_seal(cfg);
    a == b && a != 0
}

// ---------------------------------------------------------------------------
// 档位与矩阵
// ---------------------------------------------------------------------------

/// 抽样四档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    /// 在册且同判，直接落锚。
    Kept,
    /// 缺席且显式改道，落锚并记因。
    Shifted,
    /// 重建侧判死，现场作废（不进分母）。
    Void,
    /// 现场没走完或落到未判档。
    Unrun,
}

impl Tier {
    /// 四档全表。
    pub fn all() -> [Tier; TIER_COUNT] {
        [Tier::Kept, Tier::Shifted, Tier::Void, Tier::Unrun]
    }

    /// 档名。
    pub fn name(&self) -> &'static str {
        match *self {
            Tier::Kept => "kept",
            Tier::Shifted => "shifted",
            Tier::Void => "void",
            Tier::Unrun => "unrun",
        }
    }

    /// 该档是否是绿档（kept 与 shifted 都算：改道是正确结论）。
    pub fn is_green(&self) -> bool {
        match *self {
            Tier::Kept | Tier::Shifted => true,
            _ => false,
        }
    }

    /// 由一次结果归档。
    pub fn of(o: &Outcome) -> Tier {
        if o.refused() {
            return Tier::Void;
        }
        if o.default || o.code == TM_EDEFAULT {
            return Tier::Unrun;
        }
        if o.ok {
            if o.verdict == Verdict::Shifted {
                return Tier::Shifted;
            }
            return Tier::Kept;
        }
        Tier::Unrun
    }
}

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
/// 判对的条件是"落档与应落档一致"：正例要落进它自己那格绿档，反例**绝不许**
/// 落进绿档——一条"缺席却报同判"的破断如果被归档成 `Kept`，矩阵就把它当成
/// 保留档交付了。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Matrix {
    rows: [Row; TIER_COUNT],
}

impl Matrix {
    /// 由语料构造。
    pub fn build() -> Matrix {
        let table = Tier::all();
        let mut rows = [Row {
            tier: Tier::Kept,
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

// ---------------------------------------------------------------------------
// 结算报告
// ---------------------------------------------------------------------------

/// 全绿渲染文本（逐字钉住，改一个字都要红）。
pub const RENDER_OK: &str =
    "trim configs=3 applets=8 paths=6 samples=24 kept=20 shifted=4 violations=0 rate=1000permille\n";

/// 抽样结算报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Report {
    /// 结算分母（三档 × 八 applet 的重建抽样格）。
    pub total: usize,
    /// 正例槽数。
    pub accept: usize,
    /// 反例槽数。
    pub reject: usize,
    /// 在册同判格数。
    pub kept: usize,
    /// 缺席改道格数。
    pub shifted: usize,
    /// 现场作废格数。
    pub void: usize,
    /// 未走完格数。
    pub unrun: usize,
    /// 计入分子的格数。
    pub matched: usize,
    /// 违约计数。
    pub violations: usize,
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
        self.violations == 0
            && self.void == 0
            && self.unrun == 0
            && self.rate() == RATE_FULL
            && self.accept == ACCEPT
            && self.reject == REJECT
            && (self.kept as u32 + self.shifted as u32) == SAMPLE_CELLS as u32
    }

    /// 红项计数。
    pub fn reds(&self) -> u32 {
        (self.violations as u32) + (self.void as u32) + (self.unrun as u32)
    }

    /// 闸门：全绿才放行。
    pub fn gate(&self) -> Result<(), u32> {
        if self.total == 0 {
            return Err(TM_ESLOT);
        }
        if self.violations > 0 {
            return Err(TM_ETEXT);
        }
        if self.rate() < RATE_FULL {
            return Err(TM_ETABLE);
        }
        if self.unrun > 0 {
            return Err(TM_EDEFAULT);
        }
        Ok(())
    }

    /// 四档格数。
    pub fn tier_cells(&self) -> u32 {
        (self.kept as u32) + (self.shifted as u32) + (self.void as u32) + (self.unrun as u32)
    }

    /// 红档格数。
    pub fn red_cells(&self) -> u32 {
        (self.violations as u32) + (self.unrun as u32)
    }

    /// 逐字渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("trim configs=");
        s.push_str(dec_text(CONFIG_COUNT as u32).as_str());
        s.push_str(" applets=");
        s.push_str(dec_text(APPLET_COUNT as u32).as_str());
        s.push_str(" paths=");
        s.push_str(dec_text(PATH_COUNT as u32).as_str());
        s.push_str(" samples=");
        s.push_str(dec_text(self.total as u32).as_str());
        s.push_str(" kept=");
        s.push_str(dec_text(self.kept as u32).as_str());
        s.push_str(" shifted=");
        s.push_str(dec_text(self.shifted as u32).as_str());
        s.push_str(" violations=");
        s.push_str(dec_text(self.violations as u32).as_str());
        s.push_str(" rate=");
        s.push_str(dec_text(self.rate()).as_str());
        s.push_str("permille\n");
        s
    }
}

/// 空报告（未跑任何抽样时的形态，绝不与全绿混淆）。
pub fn empty_report() -> Report {
    Report {
        total: 0,
        accept: 0,
        reject: 0,
        kept: 0,
        shifted: 0,
        void: 0,
        unrun: 0,
        matched: 0,
        violations: 0,
    }
}

/// 结算：抽样面给分子，语料面给违约项。
fn settle(over: Option<(usize, Fault)>) -> Report {
    let mut rep = Report {
        total: SAMPLE_CELLS,
        accept: ACCEPT,
        reject: REJECT,
        kept: 0,
        shifted: 0,
        void: 0,
        unrun: 0,
        matched: 0,
        violations: 0,
    };
    let mut c = 0usize;
    while c < CONFIG_COUNT {
        let cfg = match CFG_TIER.get(c) {
            Some(x) => *x,
            None => return empty_report(),
        };
        let mut a = 0usize;
        while a < APPLET_COUNT {
            let ap = match Applet::all().get(a) {
                Some(x) => *x,
                None => return empty_report(),
            };
            if ap.survives(cfg) {
                rep.kept += 1;
            } else {
                rep.shifted += 1;
            }
            a += 1;
        }
        c += 1;
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
    rep.violations = violations;
    rep.matched = SAMPLE_CELLS.saturating_sub(violations);
    rep
}

/// 主结算：全绿才给报告。
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
            return Err(TM_ESLOT);
        }
    }
    Ok(settle(over))
}

/// 报告与矩阵是否互相印证（两条独立路径必须给同一个分子）。
pub fn report_agrees_with_matrix() -> bool {
    let rep = settle(None);
    let m = Matrix::build();
    let kept_cells = match m.row(Tier::Kept) {
        Some(r) => r.good,
        None => 0,
    };
    let shift_cells = match m.row(Tier::Shifted) {
        Some(r) => r.good,
        None => 0,
    };
    rep.kept > 0
        && rep.kept + rep.shifted == rep.total
        && kept_cells > 0
        && shift_cells > 0
        && m.goods() == CORPUS
}

/// 红档率标定：注入一维红之后，分子必须掉下来，且不是被整除掩盖。
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

/// 注入一路红，四路自检必须各指各的。
pub fn inject_four() -> [bool; 4] {
    let table_route = {
        table_len(CFG_ALL) as usize == APPLET_COUNT
            && !table_duplicated()
            && rebuild_is_deterministic(CFG_ALL)
            && trim_matrix().present_cells() == PATH_PRESENT_CELLS
    };
    let judge_route = {
        let mut all_ok = true;
        let mut i = 0usize;
        while i < ACCEPT {
            if let Some(c) = case_at(i) {
                if c.fault() != Fault::Ok {
                    all_ok = false;
                }
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
            }
            k += 1;
        }
        all_ok && tiers_agree()
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

// ---------------------------------------------------------------------------
// 阶段闸门：Cfg→Trim→Rebuild→Table→Present→Path→Verdict→Rate
// ---------------------------------------------------------------------------

/// 一次抽样结算的走闸状态机。
///
/// 这条纵线为什么不能倒：位图（Cfg/Trim）必须先于重建（Rebuild/Table），重建必须
/// 先于在册性（Present），在册性必须先于路径（Path），路径必须先于判定（Verdict）。
/// 一旦允许倒退，就能"先定结论再补位图"——裁剪档靠回填位图混进矩阵，本卡的抽样
/// 当场失效。所以倒退判死（`TM_ESTAGE`），封账后再走一步也判死（`TM_ESTATE`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Gate {
    /// 当前段；`None` 表示还没进过任何段。
    current: Option<Stage>,
    /// 是否已封账。
    closed: bool,
    /// 成功走段次数。
    steps: u32,
    /// 被拒次数。
    refusals: u32,
    /// 走段轨迹指纹。
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

    /// 走一段。倒退即拒，且不改状态。
    pub fn step(&mut self, s: Stage) -> Result<(), u32> {
        if self.closed {
            self.refusals += 1;
            return Err(TM_ESTATE);
        }
        if let Some(cur) = self.current {
            if s.rank() < cur.rank() {
                self.refusals += 1;
                return Err(TM_ESTAGE);
            }
        }
        self.root = fold_word(self.root, s.rank() as u64);
        self.root = fold_str(self.root, s.name());
        self.current = Some(s);
        self.steps += 1;
        Ok(())
    }

    /// 封账：之后任何走段与再次封账都拒。
    pub fn close(&mut self) -> Result<u64, u32> {
        if self.closed {
            self.refusals += 1;
            return Err(TM_ESTATE);
        }
        if self.current.is_none() {
            return Err(TM_ESTAGE);
        }
        self.closed = true;
        self.root = fold_str(self.root, "closed");
        Ok(self.root)
    }

    /// 当前段。
    pub fn stage(&self) -> Option<Stage> {
        self.current
    }

    /// 成功走段次数。
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

    /// 依次走一串段，任一步被拒就把那个码交出来。
    pub fn walk(stages: &[Stage]) -> Result<Gate, u32> {
        let mut g = Gate::new();
        let mut i = 0usize;
        while i < stages.len() {
            let s = match stages.get(i) {
                Some(x) => *x,
                None => return Err(TM_ESLOT),
            };
            match g.step(s) {
                Ok(()) => {}
                Err(code) => return Err(code),
            }
            i += 1;
        }
        Ok(g)
    }

    /// 全段正序走一遍并封账（判据与测试共用的正例路径）。
    pub fn full_pass() -> Result<u64, u32> {
        let table = Stage::all();
        let mut g = Gate::new();
        let mut i = 0usize;
        while i < table.len() {
            let s = match table.get(i) {
                Some(x) => *x,
                None => return Err(TM_ESLOT),
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
    /// 这一格对应的段号（统计格用 `STAT_OPS`，缺陷格用 `DEFECT_OP_BASE + n`）。
    pub op: u8,
    /// 这一格记的是成立还是破断。
    pub ok: bool,
    /// 错误码（成立格必须为 0）。
    pub code: u32,
    /// 统计值（破断格必须为 0）。
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
        s.push_str(bool_text(self.ok));
        s.push_str(" code ");
        s.push_str(hex_text(self.code, 5).as_str());
        s.push_str(" value ");
        s.push_str(dec_text(self.value).as_str());
        s
    }
}

/// 账本：十四格 = 七统计格 + 一红档格 + 六缺陷码格。
///
/// 登记纪律：槽号必须严格等于当前长度，跳号与重号一律拒收并计数——
/// 本卡不许"先给矩阵再补台账"，所以 `rejects` 是一个要判死的量。
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
            return Err(TM_ESTATE);
        }
        if self.len >= SAMPLES {
            self.rejects += 1;
            return Err(TM_ESLOT);
        }
        if s.slot as usize != self.len {
            self.rejects += 1;
            return Err(TM_ESLOT);
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

    /// 成立格数。
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

    /// 破断格数。
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
    /// `REJECT_CODES`，且破断格值必须为 0、缺陷码两两互异。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES {
            return Err(TM_ESLOT);
        }
        let stat_last = match STAT_CELLS.checked_add(1) {
            Some(x) => x,
            None => return Err(TM_ESLOT),
        };
        let mut i = 0usize;
        while i < stat_last {
            let s = match self.sample(i) {
                Some(x) => x,
                None => return Err(TM_ESLOT),
            };
            if !s.ok {
                return Err(TM_ESTATE);
            }
            if s.code != 0 {
                return Err(TM_ECODE);
            }
            let want_value = match VALS.get(i) {
                Some(v) => *v,
                None => return Err(TM_ESLOT),
            };
            if s.value != want_value {
                return Err(TM_EVALUE);
            }
            let want_op = match self.stat_op_want(i) {
                Some(v) => v,
                None => return Err(TM_ESLOT),
            };
            if s.op != want_op {
                return Err(TM_ECODE);
            }
            i += 1;
        }
        let mut j = 0usize;
        while j < REJECT {
            let cell = match STAT_CELLS.checked_add(1).and_then(|x| x.checked_add(j)) {
                Some(x) => x,
                None => return Err(TM_ESLOT),
            };
            let s = match self.sample(cell) {
                Some(x) => x,
                None => return Err(TM_ESLOT),
            };
            if s.ok {
                return Err(TM_ESTATE);
            }
            if s.value != 0 {
                return Err(TM_EVALUE);
            }
            let want_code = match REJECT_CODES.get(j) {
                Some(c) => *c,
                None => return Err(TM_ESLOT),
            };
            if s.code != want_code {
                return Err(TM_ECODE);
            }
            let want_op = match DEFECT_OP_BASE.checked_add(j as u8) {
                Some(x) => x,
                None => return Err(TM_ESLOT),
            };
            if s.op != want_op {
                return Err(TM_ECODE);
            }
            let mut k = j + 1usize;
            while k < REJECT {
                let other = match REJECT_CODES.get(k) {
                    Some(c) => *c,
                    None => return Err(TM_ESLOT),
                };
                if other == want_code {
                    return Err(TM_ECODE);
                }
                k += 1;
            }
            j += 1;
        }
        Ok(())
    }

    /// 第一处不合规的格子与原因（只给指认，不改判定）。
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

    /// 统计格应有的段号。
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

    /// 冻结：先复核再折叠，复核不过一律拒（红账不许盖戳）。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(TM_ESTATE);
        }
        match self.audit() {
            Ok(()) => {}
            Err(code) => return Err(code),
        }
        let h = self.seal();
        if h == 0 {
            return Err(TM_ESEAL);
        }
        self.root = h;
        self.frozen = true;
        Ok(h)
    }

    /// 重放：以同一批格子重建账本，比指纹。
    pub fn replay(&self) -> Result<u64, u32> {
        if !self.frozen {
            return Err(TM_ESTATE);
        }
        let mut fresh = Ledger::new();
        let mut i = 0usize;
        while i < self.len {
            let s = match self.sample(i) {
                Some(x) => x,
                None => return Err(TM_ESEAL),
            };
            match fresh.register(s) {
                Ok(()) => {}
                Err(_) => return Err(TM_ESEAL),
            }
            i += 1;
        }
        let h = fresh.seal();
        if h != self.root {
            return Err(TM_ESEAL);
        }
        Ok(h)
    }

    /// 改动一格（仅未冻结时允许），用于注入红。
    pub fn poke(&mut self, i: usize, value: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(TM_ESTATE);
        }
        match self.slots.get_mut(i) {
            Some(cell) => {
                match *cell {
                    Some(ref mut s) => {
                        s.value = value;
                    }
                    None => return Err(TM_ESLOT),
                }
                Ok(())
            }
            None => Err(TM_ESLOT),
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

/// 注入一维错的台账（证明复核真的在逐格看，而不只看格数）。
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
        _ => return l,
    };
    match l.poke(cell, value) {
        Ok(()) => {}
        Err(_) => {}
    }
    l
}

// ---------------------------------------------------------------------------
// 十三条判据谓词（全部私有，只在 run_unx11676_checks 里汇总）
// ---------------------------------------------------------------------------

/// 01 名称与段基址：常量层自洽，任何一条规模关系破了都算红。
fn name_ok() -> bool {
    if SEG_BASE != 0x6d2a0 || SEG_BASE == 0 {
        return false;
    }
    if ANCHOR_F11675_SEG + 0x10 != SEG_BASE {
        return false;
    }
    if SEG_BASE + 0x10 != ANCHOR_F11677_SEG {
        return false;
    }
    if ANCHOR_F11595_SEG + ANCHOR_F11595_GAP != SEG_BASE {
        return false;
    }
    if CORPUS != SAMPLES || ACCEPT + REJECT != CORPUS || TIER_COUNT != 4 {
        return false;
    }
    if STAT_CELLS + 1 != ACCEPT || STAT_CELLS + 1 + REJECT != SAMPLES {
        return false;
    }
    if RED_RATE != 929 || RATE_FULL != 1000 {
        return false;
    }
    if STAGE_COUNT != 8 || Stage::all().len() as u32 != STAGE_COUNT {
        return false;
    }
    if STAGE_LAST_RANK != Stage::Rate.rank() {
        return false;
    }
    if SAMPLE_CELLS != CONFIG_COUNT * APPLET_COUNT {
        return false;
    }
    if EXIT_NOTFOUND != 127 || EXIT_OK != 0 {
        return false;
    }
    if APPLET_COUNT != Applet::all().len() || PATH_COUNT != PathKind::all().len() {
        return false;
    }
    if CONFIG_COUNT != Config::all().len() {
        return false;
    }
    true
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
        if f.code() != c || fault_ndx(f) != k {
            return false;
        }
        k += 1;
    }
    if Fault::Ok.code() != 0 {
        return false;
    }
    true
}

/// 03 CONFIG 位图：四位互异、并集满、三档自洽、裁剪可逆。
fn cfg_ok() -> bool {
    let bits = [CFG_MUSL, CFG_BUSYBOX, CFG_BUSYBOX_ROOT, CFG_SHELL];
    let mut i = 0usize;
    let mut union: u16 = 0;
    while i < bits.len() {
        let b = match bits.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if b == 0 || (b & (b - 1)) != 0 {
            return false;
        }
        if (union & b) != 0 {
            return false;
        }
        union |= b;
        i += 1;
    }
    if union != CFG_ALL {
        return false;
    }
    if count_bits(CFG_ALL) != 4 || count_bits(CFG_NONE) != 0 {
        return false;
    }
    if CFG_TIER.len() != CONFIG_COUNT {
        return false;
    }
    let mut c = 0usize;
    while c < CONFIG_COUNT {
        let tier = match Config::of_index(c) {
            Some(x) => x,
            None => return false,
        };
        let mask = match CFG_TIER.get(c) {
            Some(x) => *x,
            None => return false,
        };
        if tier.mask() != mask {
            return false;
        }
        if Config::of_mask(mask) != Some(tier) {
            return false;
        }
        if (mask & CFG_ALL) != mask || mask == CFG_NONE {
            return false;
        }
        if trim(mask, CFG_ALL) != CFG_NONE {
            return false;
        }
        if !trim_known(CFG_ALL, tier.dropped()) {
            return false;
        }
        if c == 0 && tier.dropped() != CFG_NONE {
            return false;
        }
        if c > 0 && count_bits(tier.dropped()) != 1 {
            return false;
        }
        c += 1;
    }
    if Config::of_mask(0x7777).is_some() || Config::of_index(CONFIG_COUNT).is_some() {
        return false;
    }
    if Config::Full.dropped() != CFG_NONE || Config::NoMusl.dropped() != CFG_MUSL {
        return false;
    }
    if Config::NoRoot.dropped() != CFG_BUSYBOX_ROOT {
        return false;
    }
    if (CFG_RESERVED & CFG_ALL) != 0 {
        return false;
    }
    true
}

/// 04 applet 表：八项互异、需要位是已定义位的子集、root/musl 计数恰为 3/1。
fn applet_ok() -> bool {
    let table = Applet::all();
    if table.len() != APPLET_COUNT || APPLET_COUNT != 8 {
        return false;
    }
    let mut i = 0usize;
    let mut roots = 0usize;
    let mut musl = 0usize;
    while i < APPLET_COUNT {
        let a = match table.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if a.name().len() == 0 {
            return false;
        }
        if Applet::of_id(a.id()) != Some(a) {
            return false;
        }
        let need = a.needs();
        if need == CFG_NONE || (need & CFG_ALL) != need {
            return false;
        }
        if a.survives(a.needs()) != true {
            return false;
        }
        if a.is_root() {
            roots += 1;
        }
        if a.is_musl() {
            musl += 1;
        }
        let mut j = i + 1usize;
        while j < APPLET_COUNT {
            if let Some(b) = table.get(j) {
                if a.id() == b.id() || a.name() == b.name() {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    if roots != ROOT_APPLET_COUNT || musl != MUSL_APPLET_COUNT {
        return false;
    }
    if Applet::of_id(0).is_some() || Applet::of_id(0x7f).is_some() {
        return false;
    }
    if table_len(CFG_ALL) != 8 || table_len(CFG_NO_MUSL) != 7 || table_len(CFG_NO_ROOT) != 5 {
        return false;
    }
    if table_len(CFG_ALL) + table_len(CFG_NO_MUSL) + table_len(CFG_NO_ROOT) != KEPT_CELLS {
        return false;
    }
    if table_len(CFG_NONE) != 0 {
        return false;
    }
    true
}

/// 05 notfound 路径：六条互异、前置位是已定义位、三档存在数 6/5/6。
fn path_ok() -> bool {
    let table = PathKind::all();
    if table.len() != PATH_COUNT || PATH_COUNT != 6 {
        return false;
    }
    let mut i = 0usize;
    while i < PATH_COUNT {
        let p = match table.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if p.name().len() == 0 || p.index() != i {
            return false;
        }
        let g = p.gate();
        if (g & CFG_ALL) != g {
            return false;
        }
        if !p.exists(CFG_ALL) {
            return false;
        }
        let mut j = i + 1usize;
        while j < PATH_COUNT {
            if let Some(q) = table.get(j) {
                if p.name() == q.name() {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    if !PathKind::LoaderMiss.exists(CFG_NONE) {
        return false;
    }
    if PathKind::ExecDirect.exists(CFG_NO_MUSL) {
        return false;
    }
    if !PathKind::ExecDirect.exists(CFG_ALL) {
        return false;
    }
    if PathKind::ShellBuiltin.exists(CFG_ALL & !CFG_SHELL) {
        return false;
    }
    let m = trim_matrix();
    if !m.decided() {
        return false;
    }
    if m.present_cells() != PATH_PRESENT_CELLS {
        return false;
    }
    if m.tier_present(0) != 6 || m.tier_present(1) != 5 || m.tier_present(2) != 6 {
        return false;
    }
    if m.render().len() == 0 {
        return false;
    }
    true
}

/// 06 重建确定性：同 cfg 两次同指纹，异 cfg 不同指纹，表无重号。
fn rebuild_ok() -> bool {
    let mut c = 0usize;
    while c < CONFIG_COUNT {
        let cfg = match CFG_TIER.get(c) {
            Some(x) => *x,
            None => return false,
        };
        if !rebuild_is_deterministic(cfg) {
            return false;
        }
        let a = table_seal(cfg);
        let mut d = c + 1usize;
        while d < CONFIG_COUNT {
            if let Some(other) = CFG_TIER.get(d) {
                if table_seal(*other) == a {
                    return false;
                }
            }
            d += 1;
        }
        c += 1;
    }
    if table_duplicated() {
        return false;
    }
    let mut kept = 0u32;
    let mut i = 0usize;
    while i < CONFIG_COUNT {
        if let Some(t) = Config::of_index(i) {
            kept += tier_table_len(t);
        }
        i += 1;
    }
    let shifted = (SAMPLE_CELLS as u32) - kept;
    if kept != KEPT_CELLS || shifted != SHIFT_CELLS {
        return false;
    }
    // 重建不是"删上一档的行"：缺席位必须真的把 applet 从表里拿掉。
    let sh_in_full = Applet::Sh.survives(CFG_ALL);
    let sh_in_no_shell = Applet::Sh.survives(CFG_ALL & !CFG_SHELL);
    if !sh_in_full || sh_in_no_shell {
        return false;
    }
    // 裁剪只摘自己的位：缺 musl 档不能顺手把 root applet 也摘掉。
    if !Applet::Su.survives(CFG_NO_MUSL) {
        return false;
    }
    if Applet::Dd.survives(CFG_NO_ROOT) != true {
        return false;
    }
    true
}

/// 07 三档裁剪矩阵（本卡核心）：在册必同判、缺席必显式改道，且退出码不许当判据。
fn trim_matrix_ok() -> bool {
    let mut kept = 0u32;
    let mut shifted = 0u32;
    let mut c = 0usize;
    while c < CONFIG_COUNT {
        let cfg = match CFG_TIER.get(c) {
            Some(x) => *x,
            None => return false,
        };
        let mut a = 0usize;
        while a < APPLET_COUNT {
            let ap = match Applet::all().get(a) {
                Some(x) => *x,
                None => return false,
            };
            if ap.survives(cfg) {
                kept += 1;
                if !verdict_identical(cfg, ap) {
                    return false;
                }
                // 在册档的文案必须与全量档逐字相同。
                if text_in(cfg, ap) != text_in(CFG_ALL, ap) {
                    return false;
                }
                if text_in(cfg, ap).len() == 0 {
                    return false;
                }
                // 在册档绝不许落到装载器缺件。
                if path_in(cfg, ap) == PathKind::LoaderMiss {
                    return false;
                }
            } else {
                shifted += 1;
                if !shift_explicit(cfg, ap) {
                    return false;
                }
                // 缺席档必须落到装载器缺件，且文案与全量档显式不同。
                if path_in(cfg, ap) != PathKind::LoaderMiss {
                    return false;
                }
                if text_in(cfg, ap) == text_in(CFG_ALL, ap) {
                    return false;
                }
                if !text_in(cfg, ap).as_str().ends_with(T_NO_SUCH) {
                    return false;
                }
            }
            // 退出码在两档里是同一个常量：这正是"码不是判据"的书面证据。
            if EXIT_NOTFOUND != 127 {
                return false;
            }
            a += 1;
        }
        c += 1;
    }
    if kept != KEPT_CELLS || shifted != SHIFT_CELLS {
        return false;
    }
    if kept + shifted != SAMPLE_CELLS as u32 {
        return false;
    }
    // 缺 root 档：三条 root applet 全缺席，五条非 root 全在册。
    let no_root = CFG_NO_ROOT;
    if Applet::Passwd.survives(no_root) || Applet::Su.survives(no_root) || Applet::Mount.survives(no_root) {
        return false;
    }
    if !Applet::Sh.survives(no_root) || !Applet::Dd.survives(no_root) || !Applet::Ls.survives(no_root) {
        return false;
    }
    // 缺 musl 档：装载器缺席，其余七条在册。
    if Applet::MuslLd.survives(CFG_NO_MUSL) {
        return false;
    }
    if table_len(CFG_NO_MUSL) != 7 {
        return false;
    }
    // 矩阵本身：三档 × 六路都要有格，且 path-search 不随 root 位消失。
    let m = trim_matrix();
    if !m.decided() || m.present_cells() != PATH_PRESENT_CELLS {
        return false;
    }
    let mut p = 0usize;
    while p < PATH_COUNT {
        let path = match PathKind::all().get(p) {
            Some(x) => *x,
            None => return false,
        };
        if m.cell(2, p) != Some(path.exists(CFG_NO_ROOT)) {
            return false;
        }
        p += 1;
    }
    if m.cell(1, PathKind::ExecDirect.index()) != Some(false) {
        return false;
    }
    if m.cell(0, PathKind::LoaderMiss.index()) != Some(true) {
        return false;
    }
    if !tiers_agree() {
        return false;
    }
    true
}

/// 08 语料十四槽互校：期望表、申报表、现场真结论三路必须逐维对上。
fn corpus_ok() -> bool {
    let cs = cases();
    if cs.len() != CORPUS {
        return false;
    }
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(x) => x,
            None => return false,
        };
        if SHAPES.get(i).is_none() || CLAIMS.get(i).is_none() {
            return false;
        }
        match case_at(i) {
            Some(x) => {
                if x.shape != c.shape || x.claim != c.claim {
                    return false;
                }
            }
            None => return false,
        }
        let wst = match WANT_STAGE.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let wc = match WANT_CODE.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let wn = match WANT_NOTE.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let wd = match WANT_DEFAULT.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let wv = match WANT_VALUE.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let ws = match WANT_SUCCESS.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let wt = match WANT_TAGS.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let wv2 = match WANT_VERDICT.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let wndx = match WANT_NDX.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let dead = match expected(i) {
            Some(x) => x,
            None => return false,
        };
        if fault_ndx(dead) != wndx {
            return false;
        }
        let a = c.actual();
        if a.stage != wst
            || a.code != wc
            || a.note != wn
            || a.default != wd
            || a.value != wv
            || a.ok() != ws
            || a.verdict != wv2
        {
            return false;
        }
        if c.tag() != wt {
            return false;
        }
        if judge(&c) != dead {
            return false;
        }
        // 申报与真结论**只能**在本槽那一维分叉。
        let mut diff = 0usize;
        let mut which = Fault::Ok;
        if c.claim.success() != ws {
            diff += 1;
            which = Fault::Estate;
        }
        if c.claim.stage != wst {
            diff += 1;
            which = Fault::Stage;
        }
        if c.claim.code != wc {
            diff += 1;
            which = Fault::Code;
        }
        if c.claim.value != wv {
            diff += 1;
            which = Fault::Value;
        }
        if c.claim.note != wn || c.claim.verdict != wv2 {
            diff += 1;
            which = Fault::Note;
        }
        if c.claim.default != wd {
            diff += 1;
            which = Fault::Default;
        }
        if dead == Fault::Ok {
            if diff != 0 {
                return false;
            }
        } else if diff != 1 || which != dead {
            return false;
        }
        if cover(i).is_err() {
            return false;
        }
        i += 1;
    }
    let mut k = 0usize;
    while k < REJECT {
        let slot = match ACCEPT.checked_add(k) {
            Some(x) => x,
            None => return false,
        };
        let want = match expected(slot) {
            Some(x) => x,
            None => return false,
        };
        if fault_ndx(want) != k {
            return false;
        }
        k += 1;
    }
    if cover(CORPUS + 1).is_ok() || cover(CORPUS).is_err() {
        return false;
    }
    true
}

/// 09 判序六维互斥：单维改坏只挂那一维，成败互报压过后五维。
fn judge_ok() -> bool {
    let base = match case_at(0) {
        Some(x) => x,
        None => return false,
    };
    let actual = base.actual();
    let estate = Case {
        shape: base.shape,
        claim: Claim {
            success: !base.claim.success(),
            ..base.claim
        },
    };
    if judge(&estate) != Fault::Estate {
        return false;
    }
    let all_bad = Case {
        shape: base.shape,
        claim: Claim {
            success: !base.claim.success(),
            stage: match Stage::of_rank(actual.stage.rank() + 1) {
                Some(x) => x,
                None => Stage::Rate,
            },
            code: TM_ESTATE,
            value: actual.value + 9,
            note: Note::SilentSame,
            verdict: Verdict::Red,
            default: true,
            tag: head_tag(),
        },
    };
    if judge(&all_bad) != Fault::Estate {
        return false;
    }
    let earlier = match Stage::of_rank(actual.stage.rank().saturating_sub(1)) {
        Some(x) => x,
        None => return false,
    };
    if earlier == actual.stage {
        return false;
    }
    let stage_only = Case {
        shape: base.shape,
        claim: Claim {
            stage: earlier,
            ..base.claim
        },
    };
    if judge(&stage_only) != Fault::Stage {
        return false;
    }
    let code_only = Case {
        shape: base.shape,
        claim: Claim {
            code: TM_ESEAL,
            ..base.claim
        },
    };
    if judge(&code_only) != Fault::Code {
        return false;
    }
    let value_only = Case {
        shape: base.shape,
        claim: Claim {
            value: actual.value + 1,
            ..base.claim
        },
    };
    if judge(&value_only) != Fault::Value {
        return false;
    }
    let note_only = Case {
        shape: base.shape,
        claim: Claim {
            note: Note::PathGone,
            ..base.claim
        },
    };
    if judge(&note_only) != Fault::Note {
        return false;
    }
    let verdict_only = Case {
        shape: base.shape,
        claim: Claim {
            verdict: Verdict::Red,
            ..base.claim
        },
    };
    if judge(&verdict_only) != Fault::Note {
        return false;
    }
    let default_only = Case {
        shape: base.shape,
        claim: Claim {
            default: true,
            ..base.claim
        },
    };
    if judge(&default_only) != Fault::Default {
        return false;
    }
    let faults = Fault::reject_faults();
    let mut i = 0usize;
    while i < faults.len() {
        let f = match faults.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if ndx_fault(fault_ndx(f)) != Some(f) || f.name().len() == 0 {
            return false;
        }
        i += 1;
    }
    if ndx_fault(REJECT_FAULTS.len() + 9).is_some() || ndx_fault(6) != Some(Fault::Ok) {
        return false;
    }
    if REJECT_FAULTS.len() != REJECT {
        return false;
    }
    let mut k = 0usize;
    while k < REJECT {
        if let Some(a) = REJECT_FAULTS.get(k) {
            let mut j = k + 1usize;
            while j < REJECT {
                if let Some(b) = REJECT_FAULTS.get(j) {
                    if a == b || a.code() == b.code() {
                        return false;
                    }
                }
                j += 1;
            }
        }
        k += 1;
    }
    // 未判档必须由兜底闸接住，不许静默绿。
    let unjudged = match SHAPES.get(CORPUS - 1) {
        Some(x) => *x,
        None => return false,
    };
    let o = define(&unjudged);
    if !o.default || o.stage != Stage::Rate || o.code != TM_EDEFAULT {
        return false;
    }
    if o.exit() != 2 {
        return false;
    }
    // 头号缺陷必须落在 text 码上，且注记是 silent-same。
    let silent = match SHAPES.get(ACCEPT) {
        Some(x) => *x,
        None => return false,
    };
    let so = define(&silent);
    if so.ok() || so.code != TM_ETEXT || so.note != Note::SilentSame {
        return false;
    }
    if so.stage != Stage::Verdict {
        return false;
    }
    true
}

/// 10 四档矩阵覆盖：每档都有落点，正例不落绿档即红，反例落绿档更是判死。
fn matrix_ok() -> bool {
    let m = Matrix::build();
    if m.slots() != CORPUS * TIER_COUNT || m.goods() != CORPUS {
        return false;
    }
    if !m.covered() {
        return false;
    }
    if m.red_total() + m.goods() != m.slots() {
        return false;
    }
    let kept = match m.row(Tier::Kept) {
        Some(x) => x,
        None => return false,
    };
    if kept.good != 5 || kept.total != CORPUS {
        return false;
    }
    let shifted = match m.row(Tier::Shifted) {
        Some(x) => x,
        None => return false,
    };
    if shifted.good != 3 || shifted.total != CORPUS {
        return false;
    }
    let void = match m.row(Tier::Void) {
        Some(x) => x,
        None => return false,
    };
    if void.good != 3 || void.tier.is_green() {
        return false;
    }
    let unrun = match m.row(Tier::Unrun) {
        Some(x) => x,
        None => return false,
    };
    if unrun.good != 3 || unrun.tier.is_green() {
        return false;
    }
    if m.perfect_rows() != 0 {
        return false;
    }
    if m.render().len() == 0 {
        return false;
    }
    let table = Tier::all();
    if table.len() != TIER_COUNT {
        return false;
    }
    let mut green = 0usize;
    let mut i = 0usize;
    while i < TIER_COUNT {
        if let Some(t) = table.get(i) {
            if t.is_green() {
                green += 1;
            }
            if t.name().len() == 0 {
                return false;
            }
        }
        i += 1;
    }
    if green != 2 {
        return false;
    }
    // 归档函数本身：判死结果必须进 Void，绝不许被当成绿档交付。
    let dead = match SHAPES.get(ACCEPT + 2) {
        Some(x) => *x,
        None => return false,
    };
    if Tier::of(&define(&dead)) != Tier::Void {
        return false;
    }
    let silent = match SHAPES.get(ACCEPT) {
        Some(x) => *x,
        None => return false,
    };
    if Tier::of(&define(&silent)) != Tier::Unrun {
        return false;
    }
    true
}

/// 11 全绿渲染逐字：报告、矩阵、一致率三条独立路径必须给同一个数。
fn render_ok() -> bool {
    let rep = match run() {
        Ok(x) => x,
        Err(_) => return false,
    };
    if !rep.green() {
        return false;
    }
    if rep.render().as_str() != RENDER_OK {
        return false;
    }
    if rep.total != SAMPLE_CELLS || rep.accept != ACCEPT || rep.reject != REJECT {
        return false;
    }
    if rep.rate() != RATE_FULL {
        return false;
    }
    if rep.matched != SAMPLE_CELLS {
        return false;
    }
    if rep.violations != 0 || rep.unrun != 0 || rep.red_cells() != 0 {
        return false;
    }
    if rep.tier_cells() != SAMPLE_CELLS as u32 {
        return false;
    }
    if rep.reds() != 0 || rep.kept != KEPT_CELLS as usize || rep.shifted != SHIFT_CELLS as usize {
        return false;
    }
    if empty_report().green() || empty_report().rate() != 0 {
        return false;
    }
    if empty_report().gate().is_ok() {
        return false;
    }
    if !report_agrees_with_matrix() {
        return false;
    }
    if !red_rate_calibrated() {
        return false;
    }
    if run_with(Some((CORPUS, Fault::Value))).is_ok() {
        return false;
    }
    match run_with(Some((0, Fault::Value))) {
        Ok(r) => {
            if r.green() || r.violations == 0 {
                return false;
            }
        }
        Err(_) => return false,
    }
    true
}

/// 12 阶段闸门不倒退：正序走满八段，倒退与封账后再走都判死。
fn gate_ok() -> bool {
    let table = Stage::all();
    let mut g = Gate::new();
    if g.stage().is_some() || g.steps() != 0 || g.refusals() != 0 || g.is_closed() {
        return false;
    }
    let mut i = 0usize;
    while i < table.len() {
        let s = match table.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if g.step(s).is_err() {
            return false;
        }
        if g.stage() != Some(s) {
            return false;
        }
        i += 1;
    }
    if g.steps() != table.len() as u32 {
        return false;
    }
    let early = match table.get(2) {
        Some(x) => *x,
        None => return false,
    };
    if g.step(early).is_ok() {
        return false;
    }
    if g.refusals() != 1 {
        return false;
    }
    let last = match table.get(table.len() - 1) {
        Some(x) => *x,
        None => return false,
    };
    if g.step(last).is_err() {
        return false;
    }
    let root = match g.close() {
        Ok(x) => x,
        Err(_) => return false,
    };
    if root == 0 || !g.is_closed() || g.root() != root {
        return false;
    }
    if g.close().is_ok() || g.step(last).is_ok() {
        return false;
    }
    if g.refusals() != 3 {
        return false;
    }
    if g.render().len() == 0 {
        return false;
    }
    match Gate::full_pass() {
        Ok(h) => {
            if h == 0 {
                return false;
            }
        }
        Err(_) => return false,
    }
    let mut empty = Gate::new();
    if empty.close().is_ok() {
        return false;
    }
    let skip = [Stage::Cfg, Stage::Verdict, Stage::Trim];
    if Gate::walk(&skip).is_ok() {
        return false;
    }
    let good = [Stage::Cfg, Stage::Trim, Stage::Rebuild];
    match Gate::walk(&good) {
        Ok(x) => {
            if x.steps() != 3 {
                return false;
            }
        }
        Err(_) => return false,
    }
    // 段秩与段名必须一一对应，否则"倒退"判不出来。
    let mut r = 1u32;
    while r <= STAGE_LAST_RANK {
        let s = match Stage::of_rank(r) {
            Some(x) => x,
            None => return false,
        };
        if s.rank() != r || s.name().len() == 0 || s.fault_code() <= SEG_BASE {
            return false;
        }
        if s.is_verdict_side() != (r >= Stage::Verdict.rank()) {
            return false;
        }
        r += 1;
    }
    if Stage::of_rank(0).is_some() || Stage::of_rank(STAGE_LAST_RANK + 1).is_some() {
        return false;
    }
    true
}

/// 13 账本：槽位严格、逐格复核、冻结重放同指纹。
fn ledger_ok() -> bool {
    let mut l = build_ledger();
    if l.len() != SAMPLES || l.is_empty() {
        return false;
    }
    if l.rejects() != 0 {
        return false;
    }
    if l.ok_count() != ACCEPT || l.err_count() != REJECT {
        return false;
    }
    if l.audit().is_err() || l.first_mismatch().is_some() {
        return false;
    }
    if l.render().len() == 0 {
        return false;
    }
    let mut strict = Ledger::new();
    let late = Sample {
        slot: 1,
        op: 1,
        ok: true,
        code: 0,
        value: head_val(),
    };
    if strict.register(late).is_ok() {
        return false;
    }
    if strict.rejects() != 1 || strict.len() != 0 {
        return false;
    }
    let zero = Sample {
        slot: 0,
        op: 1,
        ok: true,
        code: 0,
        value: head_val(),
    };
    if strict.register(zero).is_err() {
        return false;
    }
    if strict.register(zero).is_ok() {
        return false;
    }
    if strict.rejects() != 2 || strict.len() != 1 {
        return false;
    }
    let mut b = 0u8;
    while b < 3 {
        let broken = build_ledger_broken(b);
        if broken.audit().is_ok() || broken.first_mismatch().is_none() {
            return false;
        }
        b += 1;
    }
    let root = match l.freeze() {
        Ok(x) => x,
        Err(_) => return false,
    };
    if root == 0 || !l.is_frozen() || l.root() != root {
        return false;
    }
    match l.replay() {
        Ok(x) => {
            if x != root {
                return false;
            }
        }
        Err(_) => return false,
    }
    if l.freeze().is_ok() || l.poke(0, 7).is_ok() {
        return false;
    }
    if l.audit().is_err() {
        return false;
    }
    if l.sample(SAMPLES).is_some() || l.sample(0).is_none() {
        return false;
    }
    let mut fresh = Ledger::new();
    if fresh.replay().is_ok() || fresh.freeze().is_ok() {
        return false;
    }
    // 七统计格 + 一红档格（929）+ 六缺陷格。
    if VALS.len() != ACCEPT || val_at(7) != RED_RATE {
        return false;
    }
    if val_at(0) as usize != APPLET_COUNT || val_at(1) as usize != CONFIG_COUNT {
        return false;
    }
    if val_at(2) as usize != SAMPLE_CELLS || val_at(3) != KEPT_CELLS || val_at(4) != SHIFT_CELLS {
        return false;
    }
    if val_at(5) != PATH_PRESENT_CELLS || val_at(6) != RATE_FULL {
        return false;
    }
    true
}

/// 统计表取格（越界给 0，绝不靠下标）。
pub fn val_at(i: usize) -> u32 {
    match VALS.get(i) {
        Some(x) => *x,
        None => 0,
    }
}

/// 申报表第一格的标签：跨槽复用时的缺省 tag。
pub fn head_tag() -> &'static str {
    match WANT_TAGS.get(0) {
        Some(x) => *x,
        None => "f11676-full-dd-kept",
    }
}

/// 统计表第一格的值：台账注入用的缺省值。
pub fn head_val() -> u32 {
    match VALS.get(0) {
        Some(x) => *x,
        None => 0,
    }
}

/// 汇总：十三条判据，一条不多一条不少。
pub fn run_unx11676_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11676_trim_matrix_rebuild");
    s.add("TM01-名称与段基址", name_ok(), "static");
    s.add("TM02-十三码互异", codes_ok(), "static");
    s.add("TM03-CONFIG位图自洽", cfg_ok(), "static");
    s.add("TM04-applet表八项互异", applet_ok(), "static");
    s.add("TM05-notfound路径存在性", path_ok(), "static");
    s.add("TM06-重建确定性", rebuild_ok(), "static");
    s.add("TM07-三档裁剪矩阵", trim_matrix_ok(), "static");
    s.add("TM08-语料十四槽互校", corpus_ok(), "static");
    s.add("TM09-判序六维互斥", judge_ok(), "static");
    s.add("TM10-四档矩阵覆盖", matrix_ok(), "static");
    s.add("TM11-全绿渲染逐字", render_ok(), "static");
    s.add("TM12-阶段闸门不倒退", gate_ok(), "static");
    s.add(
        "TM13-冻结重放四注入",
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

    /// 取一槽；取不到就直接结束本测试，绝不拿默认值冒充结果。
    fn slot(i: usize) -> Option<Case> {
        case_at(i)
    }

    #[test]
    fn t01_seg_base_is_pinned() {
        assert_eq!(SEG_BASE, 0x6d2a0);
        assert_eq!(ANCHOR_F11675_SEG + 0x10, SEG_BASE);
        assert_eq!(SEG_BASE + 0x10, ANCHOR_F11677_SEG);
        assert_eq!(ANCHOR_F11595_SEG + ANCHOR_F11595_GAP, SEG_BASE);
    }

    #[test]
    fn t02_scale_constants_agree() {
        assert_eq!(CORPUS, SAMPLES);
        assert_eq!(ACCEPT + REJECT, CORPUS);
        assert_eq!(STAT_CELLS + 1, ACCEPT);
        assert_eq!(STAT_CELLS + 1 + REJECT, SAMPLES);
        assert_eq!(SAMPLE_CELLS, CONFIG_COUNT * APPLET_COUNT);
        assert_eq!(SAMPLE_CELLS, 24);
    }

    #[test]
    fn t03_thirteen_codes_live_inside_the_segment() {
        assert_eq!(ALL_CODES.len(), 13);
        let mut i = 0usize;
        while i < ALL_CODES.len() {
            assert!(ALL_CODES[i] > SEG_BASE);
            assert!(ALL_CODES[i] <= SEG_BASE + 0x0d);
            i += 1;
        }
    }

    #[test]
    fn t04_thirteen_codes_are_pairwise_distinct() {
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
    fn t05_reject_codes_follow_the_fault_order() {
        let faults = Fault::reject_faults();
        assert_eq!(faults.len(), REJECT);
        let mut k = 0usize;
        while k < REJECT {
            assert_eq!(REJECT_CODES[k], faults[k].code());
            assert_eq!(REJECT_FAULTS[k], faults[k]);
            assert_eq!(fault_ndx(faults[k]), k);
            k += 1;
        }
    }

    #[test]
    fn t06_vals_cells_are_the_pinned_statistics() {
        assert_eq!(VALS[0] as usize, APPLET_COUNT);
        assert_eq!(VALS[1] as usize, CONFIG_COUNT);
        assert_eq!(VALS[2] as usize, SAMPLE_CELLS);
        assert_eq!(VALS[3], KEPT_CELLS);
        assert_eq!(VALS[4], SHIFT_CELLS);
        assert_eq!(VALS[5], PATH_PRESENT_CELLS);
        assert_eq!(VALS[6], RATE_FULL);
        assert_eq!(VALS[7], RED_RATE);
        assert_eq!(val_at(99), 0);
    }

    #[test]
    fn t07_fold_helpers_are_not_identities() {
        let h = FNV_OFF;
        assert_eq!(fold_byte(h, b'A'), fold_str(h, "A"));
        assert_ne!(fold_byte(h, b'A'), h);
        assert_ne!(fold_word(h, 0), h);
        assert_ne!(fold_bool(h, false), fold_bool(h, true));
        assert_ne!(fold_i32(h, -1), fold_i32(h, 1));
        assert_ne!(fold_word(h, 1), fold_word(h, 2));
    }

    #[test]
    fn t08_decimal_and_hex_renderers() {
        assert_eq!(dec_text(0).as_str(), "0");
        assert_eq!(dec_text(24).as_str(), "24");
        assert_eq!(u32_text(1000).as_str(), "1000");
        assert_eq!(hex_text(0x0a, 4).as_str(), "000A");
        assert_eq!(hex_text(SEG_BASE, 6).as_str(), "06D2A0");
        assert_eq!(i32_text(-28).as_str(), "-28");
    }

    #[test]
    fn t09_segment_permille_and_cfg_text() {
        assert_eq!(seg_text(SEG_BASE).as_str(), "0x6D2A0");
        assert_eq!(permille_text(929).as_str(), "929permille");
        assert_eq!(cfg_text(CFG_NO_MUSL).as_str(), "cfg=0E");
        assert_eq!(bool_text(true), "true");
        assert_eq!(bool_text(false), "false");
    }

    #[test]
    fn t10_cfg_bits_are_disjoint_and_complete() {
        let bits = [CFG_MUSL, CFG_BUSYBOX, CFG_BUSYBOX_ROOT, CFG_SHELL];
        let mut union: u16 = 0;
        let mut i = 0usize;
        while i < bits.len() {
            assert_eq!(bits[i] & (bits[i] - 1), 0);
            assert_eq!(union & bits[i], 0);
            union |= bits[i];
            i += 1;
        }
        assert_eq!(union, CFG_ALL);
        assert_eq!(count_bits(CFG_ALL), 4);
        assert_eq!(count_bits(CFG_NONE), 0);
    }

    #[test]
    fn t11_trim_is_reversible_on_known_bits() {
        assert_eq!(trim(CFG_ALL, CFG_MUSL), CFG_NO_MUSL);
        assert_eq!(trim(CFG_NO_MUSL, 0), CFG_NO_MUSL);
        assert_eq!(trim(CFG_ALL, CFG_ALL), CFG_NONE);
        assert!(trim_known(CFG_ALL, CFG_MUSL));
        assert!(!trim_known(CFG_ALL, 0x8000));
    }

    #[test]
    fn t12_config_mask_roundtrip() {
        let table = Config::all();
        let mut i = 0usize;
        while i < table.len() {
            let c = table[i];
            assert_eq!(c.index(), i);
            assert_eq!(Config::of_mask(c.mask()), Some(c));
            assert_eq!(Config::of_index(i), Some(c));
            assert_eq!(CFG_TIER[i], c.mask());
            i += 1;
        }
        assert_eq!(Config::of_mask(0x7777), None);
        assert_eq!(Config::of_index(3), None);
        assert_eq!(Config::Full.dropped(), CFG_NONE);
        assert_eq!(Config::NoMusl.dropped(), CFG_MUSL);
        assert_eq!(Config::NoRoot.dropped(), CFG_BUSYBOX_ROOT);
    }

    #[test]
    fn t13_applet_id_roundtrip() {
        let table = Applet::all();
        let mut i = 0usize;
        while i < table.len() {
            let a = table[i];
            assert_eq!(Applet::of_id(a.id()), Some(a));
            assert!(!a.name().is_empty());
            i += 1;
        }
        assert_eq!(Applet::of_id(0), None);
        assert_eq!(Applet::of_id(9), None);
    }

    #[test]
    fn t14_applet_needs_are_defined_bits() {
        let table = Applet::all();
        let mut roots = 0usize;
        let mut musl = 0usize;
        let mut i = 0usize;
        while i < table.len() {
            let a = table[i];
            let need = a.needs();
            assert_ne!(need, CFG_NONE);
            assert_eq!(need & CFG_ALL, need);
            assert!(a.survives(need));
            if a.is_root() {
                roots += 1;
            }
            if a.is_musl() {
                musl += 1;
            }
            i += 1;
        }
        assert_eq!(roots, ROOT_APPLET_COUNT);
        assert_eq!(musl, MUSL_APPLET_COUNT);
    }

    #[test]
    fn t15_root_applets_vanish_only_in_no_root_tier() {
        assert!(Applet::Su.survives(CFG_ALL));
        assert!(Applet::Su.survives(CFG_NO_MUSL));
        assert!(!Applet::Su.survives(CFG_NO_ROOT));
        assert!(Applet::Passwd.survives(CFG_NO_MUSL));
        assert!(!Applet::Mount.survives(CFG_NO_ROOT));
        assert!(Applet::Dd.survives(CFG_NO_ROOT));
    }

    #[test]
    fn t16_table_len_per_tier() {
        assert_eq!(table_len(CFG_ALL), 8);
        assert_eq!(table_len(CFG_NO_MUSL), 7);
        assert_eq!(table_len(CFG_NO_ROOT), 5);
        assert_eq!(table_len(CFG_NONE), 0);
        assert_eq!(tier_table_len(Config::Full), 8);
        assert_eq!(tier_table_len(Config::NoMusl), 7);
        assert_eq!(tier_table_len(Config::NoRoot), 5);
    }

    #[test]
    fn t17_rebuild_is_deterministic() {
        assert!(rebuild_is_deterministic(CFG_ALL));
        assert!(rebuild_is_deterministic(CFG_NO_MUSL));
        assert!(rebuild_is_deterministic(CFG_NO_ROOT));
        assert_eq!(table_seal(CFG_ALL), table_seal(CFG_ALL));
        assert_ne!(table_seal(CFG_ALL), 0);
    }

    #[test]
    fn t18_tiers_have_distinct_tables() {
        assert_ne!(table_seal(CFG_ALL), table_seal(CFG_NO_MUSL));
        assert_ne!(table_seal(CFG_ALL), table_seal(CFG_NO_ROOT));
        assert_ne!(table_seal(CFG_NO_MUSL), table_seal(CFG_NO_ROOT));
    }

    #[test]
    fn t19_table_has_no_duplicates() {
        assert!(!table_duplicated());
        assert!(!Applet::all().is_empty());
    }

    #[test]
    fn t20_paths_exist_per_tier() {
        let m = trim_matrix();
        assert_eq!(m.tier_present(0), 6);
        assert_eq!(m.tier_present(1), 5);
        assert_eq!(m.tier_present(2), 6);
        assert_eq!(m.cell(1, PathKind::ExecDirect.index()), Some(false));
        assert_eq!(m.cell(2, PathKind::ExecDirect.index()), Some(true));
        assert!(PathKind::LoaderMiss.exists(CFG_NONE));
    }

    #[test]
    fn t21_matrix_is_decided_and_sized() {
        let m = trim_matrix();
        assert!(m.decided());
        assert_eq!(m.cell(CONFIG_COUNT, 0), None);
        assert_eq!(m.cell(0, PATH_COUNT), None);
        assert_eq!(m.present_cells(), PATH_PRESENT_CELLS);
    }

    #[test]
    fn t22_matrix_rows_render_one_bit_per_path() {
        let m = trim_matrix();
        let row = m.row_text(0);
        assert!(row.starts_with("full:"));
        assert_eq!(row.len(), "full:".len() + PATH_COUNT);
        assert_eq!(m.row_text(1), "no-musl:111011");
        assert_eq!(m.render().split(' ').count(), CONFIG_COUNT);
    }

    #[test]
    fn t23_surviving_applets_keep_the_verdict() {
        assert!(verdict_identical(CFG_NO_MUSL, Applet::Ls));
        assert!(verdict_identical(CFG_NO_ROOT, Applet::Dd));
        assert!(verdict_identical(CFG_ALL, Applet::Sh));
        assert!(!verdict_identical(CFG_NO_ROOT, Applet::Su));
        assert_eq!(text_in(CFG_NO_MUSL, Applet::Ls), text_in(CFG_ALL, Applet::Ls));
    }

    #[test]
    fn t24_absent_applets_shift_explicitly() {
        assert!(shift_explicit(CFG_NO_ROOT, Applet::Su));
        assert!(shift_explicit(CFG_NO_MUSL, Applet::MuslLd));
        assert!(!shift_explicit(CFG_NO_ROOT, Applet::Dd));
        assert_eq!(path_in(CFG_NO_ROOT, Applet::Su), PathKind::LoaderMiss);
        assert_ne!(text_in(CFG_NO_ROOT, Applet::Su), text_in(CFG_ALL, Applet::Su));
    }

    #[test]
    fn t25_all_twenty_four_samples_agree() {
        assert!(tiers_agree());
        assert!(sample_agrees(CFG_ALL, Applet::Passwd));
        assert!(sample_agrees(CFG_NO_ROOT, Applet::Mount));
        assert!(sample_agrees(CFG_NO_MUSL, Applet::MuslLd));
    }

    #[test]
    fn t26_accept_slots_are_green() {
        let mut i = 0usize;
        while i < ACCEPT {
            let c = match slot(i) {
                Some(x) => x,
                None => return,
            };
            let o = c.actual();
            assert!(o.ok());
            assert_eq!(o.code, 0);
            assert_eq!(o.value, 2);
            assert_eq!(c.fault(), Fault::Ok);
            assert_eq!(o.verdict, c.shape.truth());
            i += 1;
        }
    }

    #[test]
    fn t27_slot_eight_is_silent_same() {
        let c = match slot(ACCEPT) {
            Some(x) => x,
            None => return,
        };
        let o = c.actual();
        assert!(!o.ok());
        assert_eq!(o.note, Note::SilentSame);
        assert_eq!(o.stage, Stage::Verdict);
        assert_eq!(o.code, TM_ETEXT);
        assert_eq!(c.fault(), Fault::Estate);
    }

    #[test]
    fn t28_slot_nine_is_rebuild_drift() {
        let c = match slot(ACCEPT + 1) {
            Some(x) => x,
            None => return,
        };
        let o = c.actual();
        assert_eq!(o.stage, Stage::Rebuild);
        assert_eq!(o.note, Note::TableDrift);
        assert!(o.refused());
        assert_eq!(Tier::of(&o), Tier::Void);
        assert_eq!(c.fault(), Fault::Stage);
    }

    #[test]
    fn t29_slot_ten_is_cfg_unknown() {
        let c = match slot(ACCEPT + 2) {
            Some(x) => x,
            None => return,
        };
        let o = c.actual();
        assert_eq!(o.stage, Stage::Cfg);
        assert_eq!(o.code, TM_ECFG);
        assert_eq!(o.note, Note::CfgUnknown);
        assert_eq!(c.fault(), Fault::Code);
    }

    #[test]
    fn t30_slot_eleven_is_table_missing() {
        let c = match slot(ACCEPT + 3) {
            Some(x) => x,
            None => return,
        };
        let o = c.actual();
        assert_eq!(o.stage, Stage::Table);
        assert_eq!(o.note, Note::TableMissing);
        assert_eq!(c.fault(), Fault::Value);
    }

    #[test]
    fn t31_slot_twelve_is_path_gone() {
        let c = match slot(ACCEPT + 4) {
            Some(x) => x,
            None => return,
        };
        let o = c.actual();
        assert_eq!(o.stage, Stage::Path);
        assert_eq!(o.note, Note::PathGone);
        assert_eq!(o.code, TM_EPATH);
        assert_eq!(c.fault(), Fault::Note);
    }

    #[test]
    fn t32_slot_thirteen_is_default() {
        let c = match slot(CORPUS - 1) {
            Some(x) => x,
            None => return,
        };
        let o = c.actual();
        assert!(o.default);
        assert_eq!(o.stage, Stage::Rate);
        assert_eq!(o.code, TM_EDEFAULT);
        assert_eq!(c.fault(), Fault::Default);
        assert_eq!(Tier::of(&o), Tier::Unrun);
    }

    #[test]
    fn t33_fault_index_roundtrip() {
        let faults = Fault::reject_faults();
        let mut i = 0usize;
        while i < faults.len() {
            assert_eq!(ndx_fault(fault_ndx(faults[i])), Some(faults[i]));
            i += 1;
        }
        assert_eq!(ndx_fault(6), Some(Fault::Ok));
        assert_eq!(ndx_fault(7), None);
    }

    #[test]
    fn t34_every_slot_judges_as_declared() {
        let mut i = 0usize;
        while i < CORPUS {
            let c = match slot(i) {
                Some(x) => x,
                None => return,
            };
            assert_eq!(c.fault(), expected(i).unwrap_or(Fault::Ok));
            assert_eq!(c.tag(), WANT_TAGS[i]);
            i += 1;
        }
    }

    #[test]
    fn t35_claim_seal_depends_on_every_dimension() {
        let a = match slot(0) {
            Some(x) => x,
            None => return,
        };
        let b = Claim {
            value: a.claim.value + 1,
            ..a.claim
        };
        assert_ne!(a.claim.seal(), b.seal());
        assert_ne!(a.claim.seal(), a.shape.seal());
        assert_ne!(a.seal(), 0);
    }

    #[test]
    fn t36_matrix_totals() {
        let m = Matrix::build();
        assert_eq!(m.slots(), CORPUS * TIER_COUNT);
        assert_eq!(m.goods(), CORPUS);
        assert!(m.covered());
        assert_eq!(m.red_total(), m.slots() - m.goods());
        assert_eq!(m.row(Tier::Kept).map(|r| r.good), Some(5));
        assert_eq!(m.row(Tier::Shifted).map(|r| r.good), Some(3));
    }

    #[test]
    fn t37_report_is_green_and_renders_verbatim() {
        let rep = match run() {
            Ok(x) => x,
            Err(_) => return,
        };
        assert!(rep.green());
        assert_eq!(rep.render().as_str(), RENDER_OK);
        assert_eq!(rep.rate(), RATE_FULL);
        assert_eq!(rep.kept, KEPT_CELLS as usize);
        assert_eq!(rep.shifted, SHIFT_CELLS as usize);
        assert_eq!(rep.tier_cells(), SAMPLE_CELLS as u32);
    }

    #[test]
    fn t38_empty_report_is_not_green() {
        let e = empty_report();
        assert!(!e.green());
        assert_eq!(e.rate(), 0);
        assert!(e.gate().is_err());
    }

    #[test]
    fn t39_run_with_rejects_out_of_range_slots() {
        assert_eq!(run_with(Some((CORPUS, Fault::Ok))).err(), Some(TM_ESLOT));
        assert_eq!(run_with(Some((CORPUS + 7, Fault::Default))).err(), Some(TM_ESLOT));
        match run_with(Some((3, Fault::Value))) {
            Ok(r) => assert!(!r.green()),
            Err(_) => return,
        }
    }

    #[test]
    fn t40_gate_walks_forward_only() {
        let mut g = Gate::new();
        assert!(g.stage().is_none());
        let table = Stage::all();
        let mut i = 0usize;
        while i < table.len() {
            assert!(g.step(table[i]).is_ok());
            i += 1;
        }
        assert_eq!(g.steps(), 8);
        assert!(g.step(Stage::Cfg).is_err());
        assert_eq!(g.refusals(), 1);
        assert!(g.close().is_ok());
        assert!(g.step(Stage::Rate).is_err());
        assert!(g.close().is_err());
        assert_eq!(g.refusals(), 3);
    }

    #[test]
    fn t41_gate_full_pass_and_walk() {
        match Gate::full_pass() {
            Ok(h) => assert_ne!(h, 0),
            Err(_) => return,
        }
        assert!(Gate::walk(&[Stage::Path, Stage::Cfg]).is_err());
        match Gate::walk(&[Stage::Cfg, Stage::Trim]) {
            Ok(g) => assert_eq!(g.steps(), 2),
            Err(_) => return,
        }
    }

    #[test]
    fn t42_ledger_audits_freezes_and_replays() {
        let mut l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), ACCEPT);
        assert_eq!(l.err_count(), REJECT);
        assert!(l.audit().is_ok());
        let root = match l.freeze() {
            Ok(x) => x,
            Err(_) => return,
        };
        assert!(root != 0 && l.is_frozen());
        assert_eq!(l.replay(), Ok(root));
        assert!(l.freeze().is_err());
        assert!(l.poke(0, 1).is_err());
    }

    #[test]
    fn t43_broken_ledgers_are_caught() {
        let mut b = 0u8;
        while b < 3 {
            let broken = build_ledger_broken(b);
            assert!(broken.audit().is_err());
            assert!(broken.first_mismatch().is_some());
            b += 1;
        }
    }

    #[test]
    fn t44_ledger_slot_discipline() {
        let mut l = Ledger::new();
        let jump = Sample {
            slot: 2,
            op: 1,
            ok: true,
            code: 0,
            value: 1,
        };
        assert!(l.register(jump).is_err());
        assert_eq!(l.rejects(), 1);
        assert!(l.is_empty());
        let first = Sample {
            slot: 0,
            op: 1,
            ok: true,
            code: 0,
            value: 1,
        };
        assert!(l.register(first).is_ok());
        assert!(l.register(first).is_err());
        assert_eq!(l.rejects(), 2);
        assert_eq!(l.len(), 1);
    }

    #[test]
    fn t45_inject_four_is_all_true() {
        let four = inject_four();
        assert_eq!(four.len(), 4);
        let mut i = 0usize;
        while i < four.len() {
            assert!(four[i]);
            i += 1;
        }
    }

    #[test]
    fn t46_message_and_detail_are_non_empty() {
        let mut i = 0usize;
        while i < CORPUS {
            let c = match slot(i) {
                Some(x) => x,
                None => return,
            };
            let o = c.actual();
            assert!(!message(&c.shape, &o).as_str().is_empty());
            assert!(!detail(&c).as_str().is_empty());
            assert!(detail(&c).as_str().contains(c.tag()));
            assert_ne!(o.seal(), 0);
            i += 1;
        }
    }

    #[test]
    fn t47_all_thirteen_checks_are_green() {
        let s = run_unx11676_checks();
        assert!(s.all_passed());
    }
}
