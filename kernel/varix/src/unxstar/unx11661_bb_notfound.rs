//! UNX-F11661 busybox notfound 直接调用报错判据
//!
//! busybox 是**单文件多调用**（multi-call）二进制：`applet_name[]` /
//! `applet_main[]` 两张平行表把"名字"和"入口"绑在一起，`busybox <name>
//! [args...]` 这种**直接调用**形态里，`<name>` 就是查表的键。查不到时
//! busybox 走 `bb_error_msg_and_die("'%s' applet not found", name)`：程序名
//! 取 `argv[0]` 的 basename，冒号、空格、单引号、句尾句点全部是文案的一部分，
//! 输出走 stderr，进程以 127 结束。127 不是随手挑的数——它是 shell 对
//! "command not found" 的约定档位，本卡把它钉成码档。
//!
//! 本卡校验**直接调用**这条面：名字怎么进表、表怎么答复、答复文案怎么逐字
//! 成形、文案落哪条流、errno 与退出码各归哪一档。少一维就会漏一类真实故障：
//! 少了"流"这一维，把报错打到 stdout 的构建照样能过；少了"码档"这一维，
//! 退 1 而非退 127 的构建在文案上完全看不出来；少了"调用路径"这一维，
//! 直接 exec 能过、多调用符号链接路径却挂掉的情形就无人认领。
//!
//! 主锚（B20 联签锚点）逐字为：
//!
//! ```text
//! busybox: 'foo': applet not found
//! ```
//!
//! 变体档（同族、非逐字）：
//!
//! ```text
//! busybox: applet not found                 （旧版无名字的形态）
//! busybox: applet 'foo' not found           （noapplet 桩的形态）
//! ```
//!
//! # 闸门次序
//!
//! `lex → dispatch → lookup → text → stream → code → ready`，七段单调，
//! 后段不得在前段短路后被"事后补记"。次序不可倒的理由：
//!
//! 1. `lex` 最先——空名、含 `/` 的名字、超过表名上限的名字、带控制字符的名字
//!    根本不该进表；先进表会得到"unknown name"的错误归类，那是把形态问题
//!    误判成注册问题，归因直接错一层。
//! 2. `dispatch` 次之——`--list`/`--list-full`/`--help`/-h/-V 这些特殊形态
//!    在查表**之前**就被 busybox 拦下，退 0；把它们也送去查表会得到
//!    "'--list': applet not found"，与真机不符。
//! 3. `lookup` 第三——表答复分五档（注册名 / 别名 / 裁剪件 / 隐藏件 / 未知名），
//!    其中后两档在此卡一律折叠成 notfound，这正是本卡与 F11663 的边界。
//! 4. `text` 第四——文案档由 flavor（现代/旧版/桩）与调用路径共同决定，
//!    只有主锚档算"逐字命中"。
//! 5. `stream` 第五——notfound 必须走 stderr；走 stdout 即判死。
//! 6. `code` 第六——errno 与退出码一起归档；notfound 恒 ENOENT + 127。
//! 7. `ready` 收尾——只有走到 ready 才是"观测完整"。
//!
//! # 关键差异（本卡深度所在）
//!
//! - **本卡 vs UNX-F11662**：F11662 判的是 `argv[0]` 本身是未注册名时的
//!   **降级主行为**（打 usage 横幅 + notfound 行、退 127），键取自 `argv[0]`
//!   并要过 basename 提取；本卡判的是 `argv[1]` 直接给出名字时的**报错面**，
//!   键就是那个字串、不做降级。同一条 notfound 文案在两卡里位置不同：
//!   本卡里它是唯一现场，F11662 里它只是 usage 横幅的尾行。本卡判死、
//!   F11662 放行的例子：`busybox foo`（argv[0] 就是 busybox，无需降级）。
//! - **本卡 vs UNX-F11663**：F11663 判"表里本来有这个名字、但被 config 裁掉"
//!   这一内部维度，并主张其**用户面与未知名完全等同**；本卡只按表答复的
//!   表面归档，`Trimmed` 与 `Unknown` 在本卡的 Outcome 上必须**不可分辨**
//!   （同 stage、同码、同档、同主锚），差别只留给 F11663 展开。
//!   换句话说：本卡判"文案与码档逐字对不对"，F11663 判"裁剪这一维是否
//!   被偷偷做进了用户面"。
//!
//! 判序：状态 → 阶段 → errno → 退出码 → 注解 → 主锚逐字位。
//!
//! 语料 14 槽：8 槽申报正确（运行 3、主锚 2、变体 2、畸形 1），6 槽各挂
//! 一处错误申报（状态/阶段/码/值/注解/主锚）。账本 14 格 8 成 6 败，
//! 冻结后重放同指纹。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，`audit` 逐格
//! 对账，冻结后 `replay` 重建同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址（F11661）。
pub const SEG_BASE: u32 = 0x6d100;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 语料槽数。
pub const CORPUS: usize = 14;

/// 申报正确的槽数。
pub const ACCEPT: usize = 8;

/// 申报错误的槽数。
pub const REJECT: usize = 6;

/// 档位数。
pub const TIER_COUNT: usize = 4;

// ---------------------------------------------------------------------------
// busybox 现场常量
// ---------------------------------------------------------------------------

/// applet 名在表中的最大长度（含末位 NUL 前的字符数）。
pub const APPLET_NAME_MAX: usize = 30;

/// 单个 applet 的别名上限（模型值）。
pub const ALIAS_MAX: usize = 3;

/// busybox 自身可执行文件的名字集合大小。
pub const SELF_NAME_COUNT: usize = 4;

/// 直接调用形态的退出码：applet 正常返回 0。
pub const EXIT_RUN_OK: u16 = 0;

/// applet 自身判定失败（`return EXIT_FAILURE`）。
pub const EXIT_RUN_FAIL: u16 = 1;

/// applet 需要 root 却拿到非 root。
pub const EXIT_NEED_ROOT: u16 = 1;

/// 空名直接调用（等价 `busybox` 无参），打 usage 后退出。
pub const EXIT_USAGE: u16 = 1;

/// notfound 档位——shell 对 "command not found" 的约定码。
pub const EXIT_NOT_FOUND: u16 = 127;

/// 找到名字但入口不可执行（stub 未接线）。
pub const EXIT_NO_EXEC: u16 = 126;

// ---------------------------------------------------------------------------
// errno
// ---------------------------------------------------------------------------

/// 权限不足（`applet needs root`）。
pub const ERR_EACCES: i32 = 13;

/// 名字在表里不存在——notfound 的正牌 errno。
pub const ERR_ENOENT: i32 = 2;

/// 功能未编译进来（`-ENOSYS` 家族）。
pub const ERR_ENOSYS: i32 = 38;

/// 名字形态不合法。
pub const ERR_EINVAL: i32 = 22;

/// 名字过长。
pub const ERR_ENAMETOOLONG: i32 = 36;

/// 成功——errno 恒零。
pub const ERR_NONE: i32 = 0;

// ---------------------------------------------------------------------------
// 错误码（13 个）
// ---------------------------------------------------------------------------

/// 阶段申报不符。
pub const NF_ESTAGE: u32 = SEG_BASE + 0x01;
/// errno 申报不符。
pub const NF_ECODE: u32 = SEG_BASE + 0x02;
/// 退出码申报不符。
pub const NF_EVALUE: u32 = SEG_BASE + 0x03;
/// 注解申报不符。
pub const NF_ENOTE: u32 = SEG_BASE + 0x04;
/// 主锚逐字位申报不符。
pub const NF_EDEFAULT: u32 = SEG_BASE + 0x05;
/// 调用路径档申报不符。
pub const NF_EVIA: u32 = SEG_BASE + 0x06;
/// 状态矛盾（成败互报）。
pub const NF_ESTATE: u32 = SEG_BASE + 0x07;
/// 计数不符。
pub const NF_ECOUNT: u32 = SEG_BASE + 0x08;
/// 槽位非法。
pub const NF_ESLOT: u32 = SEG_BASE + 0x09;
/// 指纹不符。
pub const NF_ESEAL: u32 = SEG_BASE + 0x0a;
/// 无观测。
pub const NF_ENOBS: u32 = SEG_BASE + 0x0b;
/// 矩阵不平。
pub const NF_EMATRIX: u32 = SEG_BASE + 0x0c;
/// 账本不平。
pub const NF_EBAL: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 名字形态判定。
pub const OP_LEX: u8 = 0x01;
/// 分发决策。
pub const OP_DISPATCH: u8 = 0x02;
/// 注册表查找。
pub const OP_LOOKUP: u8 = 0x03;
/// 文案成形。
pub const OP_TEXT: u8 = 0x04;
/// 输出流归属。
pub const OP_STREAM: u8 = 0x05;
/// 码档归档。
pub const OP_CODE: u8 = 0x06;
/// 语料全跑。
pub const OP_CORPUS: u8 = 0x07;
/// 重放。
pub const OP_REPLAY: u8 = 0x08;

// ---------------------------------------------------------------------------
// 哈希
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

/// 折入一个字（大端序逐字节）。
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

/// djb2 名字散列——表冲突体检用（不是 busybox 的真散列，只用于本卡的
/// "同桶不同名"判别）。
pub fn name_hash(name: &str) -> u32 {
    let bytes = name.as_bytes();
    let mut h = 5381u32;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h = h.wrapping_mul(33).wrapping_add(*b as u32);
        }
        i += 1;
    }
    h
}

// ---------------------------------------------------------------------------
// 文本
// ---------------------------------------------------------------------------

/// 十进制文本。
fn dec_text(v: u32) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 10];
    let mut n = v;
    let mut i = 10usize;
    while n > 0 && i > 0 {
        i -= 1;
        buf[i] = b'0' + ((n % 10) as u8);
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

/// 有符号十进制文本。
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

/// u32 文本。
pub fn u32_text(v: u32) -> String {
    dec_text(v)
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
// 字节串基元（no_std 下没有 str::find 的稳定替代，全部手写）
// ---------------------------------------------------------------------------

/// 两串是否逐字相等（显式写出来，是为了让"逐字"这件事在代码里可见）。
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

/// 串长度。
pub fn text_len(s: &str) -> usize {
    s.as_bytes().len()
}

/// 串里是否含 `needle`（单字节查询，够用且无依赖）。
pub fn has_byte(s: &str, needle: u8) -> bool {
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if let Some(c) = b.get(i) {
            if *c == needle {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 从 `from` 起在 `hay` 里找 `needle`（手写窗口匹配）。
///
/// 不用切片语法：判据代码里一次越界 panic 就是把整卡判红，比误绿贵得多，
/// 所以子串查询也必须逐字节走 `get`。
pub fn find_from(hay: &str, needle: &str, from: usize) -> Option<usize> {
    let h = hay.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() || from > h.len() {
        return None;
    }
    let mut i = from;
    while i + n.len() <= h.len() {
        let mut k = 0usize;
        let mut hit = true;
        while k < n.len() {
            match (h.get(i + k), n.get(k)) {
                (Some(a), Some(b)) => {
                    if a != b {
                        hit = false;
                    }
                }
                _ => hit = false,
            }
            if !hit {
                break;
            }
            k += 1;
        }
        if hit {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// busybox 的 `last_component(argv[0])` 语义：取最后一个 `/` 之后的子串。
/// 不含 `/` 时原样返回；`"/"` 与 `"foo/"` 返回空串——空 basename 正是
/// 伪链形态里最阴的一类，必须在 lex 闸门前就能被拿到。
pub fn last_component(s: &'static str) -> &'static str {
    match s.rsplit_once('/') {
        Some((_, tail)) => tail,
        None => s,
    }
}

/// 取最后一个 `/` **之前**（含 `/`）的前缀；没有 `/` 时返回 `None`。
pub fn dir_prefix(s: &'static str) -> Option<&'static str> {
    match s.rsplit_once('/') {
        Some((head, _)) => Some(head),
        None => None,
    }
}

/// 是否含 `/`，以及是否以 `/` 结尾（尾斜杠会让 basename 变空）。
pub fn slash_shape(s: &str) -> (bool, bool) {
    let b = s.as_bytes();
    let mut any = false;
    let mut trailing = false;
    let mut i = 0usize;
    while i < b.len() {
        if let Some(c) = b.get(i) {
            if *c == b'/' {
                any = true;
                trailing = i + 1 == b.len();
            }
        }
        i += 1;
    }
    (any, trailing)
}

/// 是否含控制字符（< 0x20 或 0x7f）。
pub fn has_control(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if let Some(c) = b.get(i) {
            if *c < 0x20 || *c == 0x7f {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 是否全部由 `.` 组成（"." 与 ".." 都是合法路径片段，却绝不是 applet 名）。
pub fn is_dots(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if let Some(c) = b.get(i) {
            if *c != b'.' {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// busybox 的 argv[0] 伪装规则：首字符是 `@` 表示"用后面的字串当程序名"
/// （`busybox @ls -l` 这类形态：argv[0] 带 `@` 前缀时，程序名取去掉 `@`
/// 之后的部分，分发用的名字取 argv[1]）。
pub fn has_at_prefix(s: &str) -> bool {
    match s.as_bytes().get(0) {
        Some(c) => *c == b'@',
        None => false,
    }
}

/// 剥掉一层 `@` 前缀；没有前缀时原样返回。
pub fn strip_at(s: &'static str) -> &'static str {
    match s.strip_prefix('@') {
        Some(rest) => rest,
        None => s,
    }
}

// ---------------------------------------------------------------------------
// applet 注册表
// ---------------------------------------------------------------------------

/// 表项个数（本构建的 `applet_name[]` / `applet_main[]` 平行表）。
pub const APLET_COUNT: usize = 30;

/// 一条 applet 表项。
///
/// busybox 的表由 `applets/applet_tables` 生成，名字、入口、Kconfig 符号、
/// 分区字符串四者一一对应。本结构把"是否被编进这个二进制"（`built`）与
/// "是否可按名分发"（`hidden`）拆成两维——`F11663` 判的就是这两维在用户面
/// 的**不可分辨性**，本卡只消费其结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Applet {
    /// 表里的名字（查表键，逐字 strcmp）。
    pub name: &'static str,
    /// 别名指向的主名；空串表示本身就是主名。
    pub alias_of: &'static str,
    /// Kconfig 符号。
    pub kconfig: &'static str,
    /// 分区（coreutils / editors / net-utils …）。
    pub section: &'static str,
    /// 入口函数名；空串表示表已生成但入口未接线（半成品构建）。
    pub main: &'static str,
    /// 注册序（表下标），用于验证"查找不得依赖遍历顺序之外的隐式状态"。
    pub prio: u8,
    /// 是否被编入本二进制。
    pub built: bool,
    /// 是否被 FEATURE 开关挡住（在表里但不可按名分发）。
    pub hidden: bool,
}

impl Applet {
    /// 空白表项。
    pub const fn blank() -> Applet {
        Applet {
            name: "",
            alias_of: "",
            kconfig: "",
            section: "",
            main: "",
            prio: 0,
            built: false,
            hidden: false,
        }
    }

    /// 是否别名项。
    pub fn is_alias(&self) -> bool {
        !self.alias_of.is_empty()
    }

    /// 是否主名项。
    pub fn is_primary(&self) -> bool {
        self.alias_of.is_empty()
    }

    /// 入口是否已接线。
    pub fn wired(&self) -> bool {
        !self.main.is_empty()
    }

    /// 是否可按名分发：既被编入、又未被 FEATURE 挡住、且入口存在
    /// （别名项共用主名入口，故 `main` 为空不算未接线）。
    pub fn dispatchable(&self) -> bool {
        self.built && !self.hidden && (self.wired() || self.is_alias())
    }

    /// 表项指纹（对全部字段敏感）。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_str(h, self.name);
        h = fold_str(h, self.alias_of);
        h = fold_str(h, self.kconfig);
        h = fold_str(h, self.section);
        h = fold_str(h, self.main);
        h = fold_word(h, self.prio as u64);
        h = fold_word(h, self.built as u64);
        h = fold_word(h, self.hidden as u64);
        h
    }

    /// 渲染成 `applet_tables` 风格一行。
    pub fn render(&self) -> String {
        let mut s = String::from(self.name);
        s.push_str(" [");
        s.push_str(self.section);
        s.push_str("] ");
        s.push_str(self.kconfig);
        s.push('+');
        if !self.is_alias() {
            s.push_str(" main=");
            s.push_str(self.main);
        } else {
            s.push_str(" alias-of=");
            s.push_str(self.alias_of);
        }
        if !self.built {
            s.push_str(" trimmed");
        }
        if self.hidden {
            s.push_str(" hidden");
        }
        s.push('\n');
        s
    }
}

/// 本构建的 applet 表。手写 30 项：主名 24、别名 6，其中 6 项被裁掉
/// （`built = false`）、2 项被 FEATURE 挡住（`hidden = true`）。
///
/// 名字全部取自真 busybox 的 applet 表；`vi`/`ifconfig`/`ip`/`wget`/
/// `udhcpc`/`crond` 标成裁剪件，是给 F11663 留的同源样本；`less`/`httpd`
/// 标成隐藏件，是真机里 `CONFIG_FEATURE_…` 门控的对应物。
pub fn registry() -> [Applet; APLET_COUNT] {
    [
        Applet {
            name: "sh",
            alias_of: "",
            kconfig: "CONFIG_SH",
            section: "busybox",
            main: "sh_main",
            prio: 0,
            built: true,
            hidden: false,
        },
        Applet {
            name: "ash",
            alias_of: "sh",
            kconfig: "CONFIG_ASH",
            section: "busybox",
            main: "",
            prio: 1,
            built: true,
            hidden: false,
        },
        Applet {
            name: "hush",
            alias_of: "sh",
            kconfig: "CONFIG_HUSH",
            section: "busybox",
            main: "",
            prio: 2,
            built: true,
            hidden: false,
        },
        Applet {
            name: "busybox",
            alias_of: "",
            kconfig: "CONFIG_BUSYBOX",
            section: "busybox",
            main: "busybox_main",
            prio: 3,
            built: true,
            hidden: false,
        },
        Applet {
            name: "ls",
            alias_of: "",
            kconfig: "CONFIG_LS",
            section: "coreutils",
            main: "ls_main",
            prio: 4,
            built: true,
            hidden: false,
        },
        Applet {
            name: "cp",
            alias_of: "",
            kconfig: "CONFIG_CP",
            section: "coreutils",
            main: "cp_main",
            prio: 5,
            built: true,
            hidden: false,
        },
        Applet {
            name: "mv",
            alias_of: "",
            kconfig: "CONFIG_MV",
            section: "coreutils",
            main: "mv_main",
            prio: 6,
            built: true,
            hidden: false,
        },
        Applet {
            name: "rm",
            alias_of: "",
            kconfig: "CONFIG_RM",
            section: "coreutils",
            main: "rm_main",
            prio: 7,
            built: true,
            hidden: false,
        },
        Applet {
            name: "cat",
            alias_of: "",
            kconfig: "CONFIG_CAT",
            section: "coreutils",
            main: "cat_main",
            prio: 8,
            built: true,
            hidden: false,
        },
        Applet {
            name: "echo",
            alias_of: "",
            kconfig: "CONFIG_ECHO",
            section: "coreutils",
            main: "echo_main",
            prio: 9,
            built: true,
            hidden: false,
        },
        Applet {
            name: "dd",
            alias_of: "",
            kconfig: "CONFIG_DD",
            section: "coreutils",
            main: "dd_main",
            prio: 10,
            built: true,
            hidden: false,
        },
        Applet {
            name: "grep",
            alias_of: "",
            kconfig: "CONFIG_GREP",
            section: "grep",
            main: "grep_main",
            prio: 11,
            built: true,
            hidden: false,
        },
        Applet {
            name: "sed",
            alias_of: "",
            kconfig: "CONFIG_SED",
            section: "editors",
            main: "sed_main",
            prio: 12,
            built: true,
            hidden: false,
        },
        Applet {
            name: "awk",
            alias_of: "",
            kconfig: "CONFIG_AWK",
            section: "editors",
            main: "awk_main",
            prio: 13,
            built: true,
            hidden: false,
        },
        Applet {
            name: "find",
            alias_of: "",
            kconfig: "CONFIG_FIND",
            section: "findutils",
            main: "find_main",
            prio: 14,
            built: true,
            hidden: false,
        },
        Applet {
            name: "tar",
            alias_of: "",
            kconfig: "CONFIG_TAR",
            section: "archivers",
            main: "tar_main",
            prio: 15,
            built: true,
            hidden: false,
        },
        Applet {
            name: "gzip",
            alias_of: "",
            kconfig: "CONFIG_GZIP",
            section: "archivers",
            main: "gzip_main",
            prio: 16,
            built: true,
            hidden: false,
        },
        Applet {
            name: "vi",
            alias_of: "",
            kconfig: "CONFIG_VI",
            section: "editors",
            main: "vi_main",
            prio: 17,
            built: false,
            hidden: false,
        },
        Applet {
            name: "editor",
            alias_of: "vi",
            kconfig: "CONFIG_VI",
            section: "editors",
            main: "",
            prio: 18,
            built: false,
            hidden: false,
        },
        Applet {
            name: "less",
            alias_of: "",
            kconfig: "CONFIG_LESS",
            section: "utils",
            main: "less_main",
            prio: 19,
            built: true,
            hidden: true,
        },
        Applet {
            name: "ping",
            alias_of: "",
            kconfig: "CONFIG_PING",
            section: "net-ping",
            main: "ping_main",
            prio: 20,
            built: true,
            hidden: false,
        },
        Applet {
            name: "ping6",
            alias_of: "ping",
            kconfig: "CONFIG_PING6",
            section: "net-ping",
            main: "",
            prio: 21,
            built: true,
            hidden: false,
        },
        Applet {
            name: "ifconfig",
            alias_of: "",
            kconfig: "CONFIG_IFCONFIG",
            section: "net-utils",
            main: "ifconfig_main",
            prio: 22,
            built: false,
            hidden: false,
        },
        Applet {
            name: "ip",
            alias_of: "",
            kconfig: "CONFIG_IP",
            section: "net-utils",
            main: "ip_main",
            prio: 23,
            built: false,
            hidden: false,
        },
        Applet {
            name: "route",
            alias_of: "",
            kconfig: "CONFIG_ROUTE",
            section: "net-utils",
            main: "route_main",
            prio: 24,
            built: true,
            hidden: false,
        },
        Applet {
            name: "wget",
            alias_of: "",
            kconfig: "CONFIG_WGET",
            section: "net-utils",
            main: "wget_main",
            prio: 25,
            built: false,
            hidden: false,
        },
        Applet {
            name: "httpd",
            alias_of: "",
            kconfig: "CONFIG_HTTPD",
            section: "www-programs",
            main: "httpd_main",
            prio: 26,
            built: true,
            hidden: true,
        },
        Applet {
            name: "udhcpc",
            alias_of: "",
            kconfig: "CONFIG_UDHCPC",
            section: "udhcp",
            main: "udhcpc_main",
            prio: 27,
            built: false,
            hidden: false,
        },
        Applet {
            name: "crond",
            alias_of: "",
            kconfig: "CONFIG_CROND",
            section: "sorting",
            main: "crond_main",
            prio: 28,
            built: false,
            hidden: false,
        },
        Applet {
            name: "noapplet",
            alias_of: "",
            kconfig: "CONFIG_NOAPPLET",
            section: "misc_full",
            main: "noapplet_main",
            prio: 29,
            built: true,
            hidden: false,
        },
    ]
}

/// 查表结果类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Known {
    /// 主名、已编入、可按名分发。
    Registered,
    /// 别名，且指向的主名可分发。
    Alias,
    /// 表里有这个名字，但被 config 裁掉了。
    Trimmed,
    /// 表里有这个名字，但被 FEATURE 开关挡住。
    Hidden,
    /// 表里根本没有这个名字（从未注册）。
    Unknown,
}

impl Known {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Known::Registered => "registered",
            Known::Alias => "alias",
            Known::Trimmed => "trimmed",
            Known::Hidden => "hidden",
            Known::Unknown => "unknown",
        }
    }

    /// 是否命中表（在表里存在，无论能否分发）。
    pub fn in_table(&self) -> bool {
        match self {
            Known::Unknown => false,
            _ => true,
        }
    }

    /// 是否答复"可以跑"。
    pub fn runnable(&self) -> bool {
        match self {
            Known::Registered | Known::Alias => true,
            _ => false,
        }
    }

    /// 用户面是否应报 notfound。本卡把四种不可分发一律折叠成同一档。
    pub fn not_found(&self) -> bool {
        match self {
            Known::Trimmed | Known::Hidden | Known::Unknown => true,
            _ => false,
        }
    }
}

/// 一次查表的完整答复。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Found {
    /// 类别。
    pub class: Known,
    /// 有效名（别名解析后的主名；未命中时等于查询名）。
    pub effective: &'static str,
    /// 命中的表项序号（未命中为 0，故取值一律配合 `class` 判活）。
    pub index: u8,
}

impl Found {
    /// 未命中。
    pub const fn none(name: &'static str) -> Found {
        Found { class: Known::Unknown, effective: name, index: 0 }
    }

    /// 别名深度上限（防止 alias 环）。
    pub const ALIAS_DEPTH: u8 = 3;

    /// 是否可分发。
    pub fn runnable(&self) -> bool {
        self.class.runnable()
    }
}

/// 精确名查表（busybox 的 `find_applet_by_name` 就是逐个 strcmp，
/// 不做 basename、不做大小写折叠）。
pub fn applet_at(name: &str) -> Option<Applet> {
    let reg = registry();
    let mut i = 0usize;
    while i < reg.len() {
        if let Some(a) = reg.get(i) {
            if same_text(a.name, name) {
                return Some(*a);
            }
        }
        i += 1;
    }
    None
}

/// 取表项序号（未命中返回 `None`，绝不返回 0 冒充命中）。
pub fn applet_index(name: &str) -> Option<usize> {
    let reg = registry();
    let mut i = 0usize;
    while i < reg.len() {
        if let Some(a) = reg.get(i) {
            if same_text(a.name, name) {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// 解析一次查询：主名直接答复；别名再走一跳，跳数有上限。
///
/// 别名指向被裁的主名时，类别落 `Trimmed`——这是"别名不得成为第二条通路"
/// 的硬判据：`editor`（`vi` 的别名）在 `CONFIG_VI=n` 的构建里必须与直接
/// 调 `vi` 得到完全相同的答复。
pub fn lookup(name: &'static str) -> Found {
    match applet_at(name) {
        None => Found::none(name),
        Some(a) => {
            if a.is_primary() {
                Found {
                    class: classify(&a),
                    effective: a.name,
                    index: match applet_index(name) {
                        Some(v) => v as u8,
                        None => 0,
                    },
                }
            } else {
                // 别名：解析到主名，主名的可分发性说了算；能跑才叫 Alias，
                // 跑不了就退回主名的失败类别（被裁/被挡/从未注册）。
                match applet_at(a.alias_of) {
                    None => Found {
                        class: Known::Unknown,
                        effective: a.alias_of,
                        index: 0,
                    },
                    Some(t) => {
                        let cls = classify(&t);
                        Found {
                            class: if cls == Known::Registered { Known::Alias } else { cls },
                            effective: t.name,
                            index: match applet_index(a.alias_of) {
                                Some(v) => v as u8,
                                None => 0,
                            },
                        }
                    }
                }
            }
        }
    }
}

/// 表项 → 类别（主名视角）。
fn classify(a: &Applet) -> Known {
    if !a.built {
        return Known::Trimmed;
    }
    if a.hidden {
        return Known::Hidden;
    }
    if !a.wired() {
        // 入口未接线：仍在表里，但分发不了。
        return Known::Hidden;
    }
    Known::Registered
}

// ---------------------------------------------------------------------------
// 枚举
// ---------------------------------------------------------------------------

/// 调用路径形态——同一份表答复，能被六条不同路径送出来。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Via {
    /// `busybox foo`：argv[0] 就是 busybox，argv[1] 是查询键。
    Direct,
    /// `/bin/busybox foo`：同上，只是 argv[0] 带目录。
    DirectPath,
    /// 符号链接 `foo -> busybox`：argv[0]="foo"，键取 basename。
    LinkRel,
    /// 符号链接 `/bin/foo -> /bin/busybox`：argv[0] 带目录。
    LinkAbs,
    /// argv[0] 原样进表（不剥 basename 的伪装机型）。
    RawArgv0,
    /// `argv[0]="@busybox"`：伪装程序名形态。
    Retitle,
}

impl Via {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Via::Direct => "direct",
            Via::DirectPath => "direct-path",
            Via::LinkRel => "link-rel",
            Via::LinkAbs => "link-abs",
            Via::RawArgv0 => "raw-argv0",
            Via::Retitle => "retitle",
        }
    }

    /// 键是否来自 argv[0]（多调用链接路径）。
    pub fn from_argv0(&self) -> bool {
        match self {
            Via::LinkRel | Via::LinkAbs | Via::RawArgv0 => true,
            _ => false,
        }
    }

    /// 名字是否要过提取（链接路径取 basename，伪装路径剥首 `@`）。
    pub fn stripped(&self) -> bool {
        match self {
            Via::Direct | Via::DirectPath | Via::RawArgv0 => false,
            Via::LinkRel | Via::LinkAbs | Via::Retitle => true,
        }
    }

    /// 程序名前缀是否为 busybox 自身。
    pub fn self_named(&self) -> bool {
        match self {
            Via::Direct | Via::DirectPath | Via::Retitle => true,
            _ => false,
        }
    }
}

/// 分发前的开关形态（这些一律不进表）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Special {
    /// 无开关，正常按名分发。
    None,
    /// `busybox --list`
    List,
    /// `busybox --list-full`
    ListFull,
    /// `busybox --help` / `-h`
    Help,
    /// `busybox -V` / `--version`
    Version,
    /// `busybox --install -s /bin`（不认识的二长 argv，判死在分发段）
    UnknownSwitch,
}

impl Special {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Special::None => "dispatch",
            Special::List => "--list",
            Special::ListFull => "--list-full",
            Special::Help => "--help",
            Special::Version => "-V",
            Special::UnknownSwitch => "--unknown",
        }
    }

    /// 是否本卡的合法档（未知开关不进本卡主锚，判死在分发段）。
    pub fn informational(&self) -> bool {
        match self {
            Special::List | Special::ListFull | Special::Help | Special::Version => true,
            _ => false,
        }
    }

    /// 是否非分发形态（不需要查询键）。
    pub fn skips_lookup(&self) -> bool {
        match self {
            Special::None => false,
            _ => true,
        }
    }
}

/// 查询键的名字形态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameKind {
    /// 可用名。
    Ok,
    /// 空串（`busybox ""`）。
    Empty,
    /// 含 `/`——busybox 查表是精确 strcmp，带路径的名永远不中。
    HasSlash,
    /// 纯点（`.` / `..`）。
    Dots,
    /// 含控制字符。
    Control,
    /// 超过表名上限。
    Overlong,
}

impl NameKind {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            NameKind::Ok => "ok",
            NameKind::Empty => "empty",
            NameKind::HasSlash => "has-slash",
            NameKind::Dots => "dots",
            NameKind::Control => "control",
            NameKind::Overlong => "overlong",
        }
    }

    /// 是否可用。
    pub fn usable(&self) -> bool {
        match self {
            NameKind::Ok => true,
            _ => false,
        }
    }

    /// 对应 errno。
    pub fn errno(&self) -> i32 {
        match self {
            NameKind::Overlong => ERR_ENAMETOOLONG,
            NameKind::Ok => ERR_NONE,
            _ => ERR_EINVAL,
        }
    }
}

/// 文案形态（对应 busybox 的版本谱系与 `noapplet` 桩）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flavor {
    /// 现代主锚：`busybox: 'foo': applet not found`
    Modern,
    /// 旧版：`busybox: applet not found`（无名字）
    Legacy,
    /// `noapplet` 桩：`busybox: applet 'foo' not found`
    Stub,
}

impl Flavor {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Flavor::Modern => "modern",
            Flavor::Legacy => "legacy",
            Flavor::Stub => "noapplet-stub",
        }
    }

    /// 是否主锚逐字档。
    pub fn is_main(&self) -> bool {
        match self {
            Flavor::Modern => true,
            _ => false,
        }
    }

    /// 文案是否带名字。
    pub fn quotes_name(&self) -> bool {
        match self {
            Flavor::Modern | Flavor::Stub => true,
            Flavor::Legacy => false,
        }
    }
}

/// 文案归属家族。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    /// 主锚家族（逐字可比）。
    Main,
    /// 旧版家族。
    Legacy,
    /// 桩家族。
    Stub,
    /// 不属于 notfound 家族（需 root / 入口未接线 / 成功）。
    Silent,
}

impl Anchor {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Anchor::Main => "main-anchor",
            Anchor::Legacy => "legacy-anchor",
            Anchor::Stub => "stub-anchor",
            Anchor::Silent => "silent",
        }
    }
}

/// 输出去向。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stream {
    /// stderr（notfound 的正牌去向）。
    Stderr,
    /// stdout（--list / usage 的去向）。
    Stdout,
    /// 串台：报错打到 stdout。
    Split,
    /// 丢弃。
    Devnull,
}

impl Stream {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Stream::Stderr => "stderr",
            Stream::Stdout => "stdout",
            Stream::Split => "leaked-stdout",
            Stream::Devnull => "/dev/null",
        }
    }

    /// 是否是 notfound 可接受的流向。
    pub fn valid_for_error(&self) -> bool {
        match self {
            Stream::Stderr => true,
            _ => false,
        }
    }
}

/// notfound 判定纵线的七个闸门。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// 名字形态。
    Lex,
    /// 分发决策（开关 / 按名）。
    Dispatch,
    /// 注册表答复。
    Lookup,
    /// 文案成形。
    Text,
    /// 输出去向。
    Stream,
    /// errno 与退出码归档。
    Code,
    /// 走通。
    Ready,
}

impl Stage {
    /// 次序。
    pub fn order(&self) -> u8 {
        match self {
            Stage::Lex => 0,
            Stage::Dispatch => 1,
            Stage::Lookup => 2,
            Stage::Text => 3,
            Stage::Stream => 4,
            Stage::Code => 5,
            Stage::Ready => 6,
        }
    }

    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Stage::Lex => "lex",
            Stage::Dispatch => "dispatch",
            Stage::Lookup => "lookup",
            Stage::Text => "text",
            Stage::Stream => "stream",
            Stage::Code => "code",
            Stage::Ready => "ready",
        }
    }

    /// 是否失败点。
    pub fn failure(&self) -> bool {
        match self {
            Stage::Ready => false,
            _ => true,
        }
    }
}

/// 现场注解。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Note {
    /// 主名命中，已分发。
    Ran,
    /// 别名解析后命中，已分发。
    AliasRan,
    /// `--list` 口袋清单。
    ListDump,
    /// `--help` / `-V` 信息形态。
    UsageDump,
    /// 主锚 notfound。
    Anchor,
    /// 旧版 notfound（无名字）。
    LegacyText,
    /// `noapplet` 桩 notfound。
    StubText,
    /// 命中但需要 root。
    NeedRoot,
    /// 命中但入口未接线。
    NoExec,
    /// 名字形态不合法。
    BadName,
}

impl Note {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Note::Ran => "ran",
            Note::AliasRan => "alias-ran",
            Note::ListDump => "list-dump",
            Note::UsageDump => "usage-dump",
            Note::Anchor => "main-anchor",
            Note::LegacyText => "legacy-anchor",
            Note::StubText => "stub-anchor",
            Note::NeedRoot => "needs-root",
            Note::NoExec => "unwired-main",
            Note::BadName => "bad-name",
        }
    }

    /// 是否成功档。
    pub fn success(&self) -> bool {
        match self {
            Note::Ran
            | Note::AliasRan
            | Note::ListDump
            | Note::UsageDump => true,
            _ => false,
        }
    }

    /// 是否 notfound 家族。
    pub fn not_found(&self) -> bool {
        match self {
            Note::Anchor | Note::LegacyText | Note::StubText | Note::BadName => true,
            _ => false,
        }
    }

    /// 是否文案家族问题（而非码档问题）。
    pub fn text_variant(&self) -> bool {
        match self {
            Note::LegacyText | Note::StubText => true,
            _ => false,
        }
    }

    /// 归属家族。
    pub fn family(&self) -> Anchor {
        match self {
            Note::Anchor | Note::BadName => Anchor::Main,
            Note::LegacyText => Anchor::Legacy,
            Note::StubText => Anchor::Stub,
            _ => Anchor::Silent,
        }
    }
}

/// 结果档位——本卡的四档全部由**实际结果**投影，不看申报。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// 走通：分发成功或信息形态打印完毕。
    Run,
    /// 主锚逐字命中（notfound 的正牌现场）。
    Anchor,
    /// 变体档：同为失败现场，但文案家族或输出去向不合主锚。
    Variant,
    /// 畸形档：查询键本身不可用，判死在形态闸门。
    Malformed,
}

impl Tier {
    /// 由实际结果投影。
    ///
    /// 只看 `Outcome`，不看 `Shape`——这是矩阵能与申报解耦的前提：同一个
    /// 现场（同一条 notfound）被六种方式误报时，它仍留在自己那一档里，
    /// 覆盖面不会因为申报错乱而虚高。
    pub fn of(o: &Outcome) -> Tier {
        if o.stage == Stage::Ready {
            return Tier::Run;
        }
        if o.stage == Stage::Lex {
            return Tier::Malformed;
        }
        if o.default {
            Tier::Anchor
        } else {
            Tier::Variant
        }
    }

    /// 次序。
    pub fn index(&self) -> usize {
        match self {
            Tier::Run => 0,
            Tier::Anchor => 1,
            Tier::Variant => 2,
            Tier::Malformed => 3,
        }
    }

    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Tier::Run => "run",
            Tier::Anchor => "anchor",
            Tier::Variant => "variant",
            Tier::Malformed => "malformed",
        }
    }

    /// 是否成功档。
    pub fn success(&self) -> bool {
        match self {
            Tier::Run => true,
            _ => false,
        }
    }

    /// 是否 notfound 面（本卡 4 档里 3 档都是 notfound 面）。
    pub fn not_found_face(&self) -> bool {
        match self {
            Tier::Run => false,
            _ => true,
        }
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
    /// 退出码错。
    Value,
    /// 注解错。
    Note,
    /// 主锚逐字位错。
    Default,
    /// 调用路径错。
    Via,
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
            Fault::Stage => NF_ESTAGE,
            Fault::Code => NF_ECODE,
            Fault::Value => NF_EVALUE,
            Fault::Note => NF_ENOTE,
            Fault::Default => NF_EDEFAULT,
            Fault::Via => NF_EVIA,
            Fault::Estate => NF_ESTATE,
            Fault::Count => NF_ECOUNT,
            Fault::Slot => NF_ESLOT,
            Fault::Seal => NF_ESEAL,
            Fault::Nobs => NF_ENOBS,
            Fault::Matrix => NF_EMATRIX,
            Fault::Bal => NF_EBAL,
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
        Fault::Via => "via",
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
// 形状与结果
// ---------------------------------------------------------------------------

/// 一次直接调用现场的全部维度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shape {
    /// 槽标签（唯一）。
    pub tag: &'static str,
    /// argv[0]（原样）。
    pub argv0: &'static str,
    /// argv[1]（直接调用给出的名字；链接形态下仅作对照）。
    pub req: &'static str,
    /// 调用路径。
    pub via: Via,
    /// 开关形态。
    pub special: Special,
    /// 文案家族。
    pub flavor: Flavor,
    /// 报错是否串台到 stdout。
    pub leak: bool,
    /// 命中的 applet 是否声明需要 root。
    pub root_only: bool,
    /// 当前进程是否有 root。
    pub am_root: bool,
    /// 命中项入口是否未接线（半成品构建表）。
    pub stub_unwired: bool,
    /// 命中后 applet 自己的退出码。
    pub applet_rc: u16,
}

impl Shape {
    /// 空白形状。
    pub const fn blank() -> Shape {
        Shape {
            tag: "",
            argv0: "busybox",
            req: "",
            via: Via::Direct,
            special: Special::None,
            flavor: Flavor::Modern,
            leak: false,
            root_only: false,
            am_root: true,
            stub_unwired: false,
            applet_rc: 0,
        }
    }

    /// 换查询键的副本（同一路径、同一文案家族复算用）。
    pub const fn clone_req(&self, name: &'static str) -> Shape {
        Shape {
            tag: self.tag,
            argv0: self.argv0,
            req: name,
            via: self.via,
            special: self.special,
            flavor: self.flavor,
            leak: self.leak,
            root_only: self.root_only,
            am_root: self.am_root,
            stub_unwired: self.stub_unwired,
            applet_rc: self.applet_rc,
        }
    }

    /// 换调用路径的副本（同一个名字走第二条路）。
    pub const fn clone_via(&self, v: Via) -> Shape {
        Shape {
            tag: self.tag,
            argv0: self.argv0,
            req: self.req,
            via: v,
            special: self.special,
            flavor: self.flavor,
            leak: self.leak,
            root_only: self.root_only,
            am_root: self.am_root,
            stub_unwired: self.stub_unwired,
            applet_rc: self.applet_rc,
        }
    }

    /// 实际进表的查询键。
    ///
    /// 六条路径的键来源不同：直接调用取 argv[1]，链接形态取 argv[0] 的
    /// basename，raw 形态取 argv[0] 原样，伪装形态（argv[0]="@busybox"）
    /// 仍然取 argv[1]——`@` 只改程序名，不改分发键。
    pub fn key(&self) -> &'static str {
        match self.via {
            Via::Direct | Via::DirectPath | Via::Retitle => self.req,
            Via::LinkRel | Via::LinkAbs => last_component(self.argv0),
            Via::RawArgv0 => self.argv0,
        }
    }

    /// 报错前缀的程序名。
    pub fn prog(&self) -> &'static str {
        match self.via {
            Via::Direct | Via::DirectPath | Via::Retitle => strip_at(last_component(self.argv0)),
            Via::LinkRel | Via::LinkAbs => last_component(self.argv0),
            Via::RawArgv0 => self.argv0,
        }
    }

    /// 查询键的名字形态。
    pub fn name_kind(&self) -> NameKind {
        if self.special.skips_lookup() {
            return NameKind::Ok;
        }
        lex_name(self.key())
    }

    /// 输出去向（成功走 applet 自己的流，notfound 走 stderr；串台例外）。
    pub fn stream(&self) -> Stream {
        if self.leak {
            return Stream::Split;
        }
        match self.special {
            Special::List | Special::ListFull | Special::Help | Special::Version => Stream::Stdout,
            _ => Stream::Stderr,
        }
    }

    /// 形状指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_str(h, self.tag);
        h = fold_str(h, self.argv0);
        h = fold_str(h, self.req);
        h = fold_str(h, self.via.name());
        h = fold_str(h, self.special.name());
        h = fold_str(h, self.flavor.name());
        h = fold_word(h, self.leak as u64);
        h = fold_word(h, self.root_only as u64);
        h = fold_word(h, self.am_root as u64);
        h = fold_word(h, self.stub_unwired as u64);
        h = fold_word(h, self.applet_rc as u64);
        h
    }
}

/// 判定结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// 结束阶段。
    pub stage: Stage,
    /// errno（0 表示成功）。
    pub code: i32,
    /// 退出码。
    pub value: u16,
    /// 注解。
    pub note: Note,
    /// 是否逐字命中主锚（文案 + 流向 + 码档三者同真）。
    pub default: bool,
}

impl Outcome {
    /// 空白结果。
    pub const fn blank() -> Outcome {
        Outcome {
            stage: Stage::Lex,
            code: ERR_EINVAL,
            value: EXIT_NOT_FOUND,
            note: Note::BadName,
            default: false,
        }
    }

    /// 是否成功。
    pub fn ok(&self) -> bool {
        self.code == 0 && self.stage == Stage::Ready && self.note.success()
    }

    /// 是否失败。
    pub fn failed(&self) -> bool {
        !self.ok()
    }

    /// 是否 notfound 面。
    pub fn not_found(&self) -> bool {
        self.failed() && self.note.not_found()
    }

    /// 结果指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.stage.order() as u64);
        h = fold_word(h, self.code as i64 as u64);
        h = fold_word(h, self.value as u64);
        h = fold_str(h, self.note.name());
        h = fold_word(h, self.default as u64);
        h
    }
}

/// 名字形态判定（闸门一的真身）。
///
/// 判序本身有讲究：空名最先，控制字符其次，带路径第三，纯点第四，超长最后。
/// 超长放最后是刻意的——一个 200 字节、内含 `/` 的名字应该先报"带路径"，
/// 因为真 busybox 的 strcmp 在第一个字符上就已经不中了，长度只是附带事实。
pub fn lex_name(name: &str) -> NameKind {
    if name.is_empty() {
        return NameKind::Empty;
    }
    if has_control(name) {
        return NameKind::Control;
    }
    if slash_shape(name).0 {
        return NameKind::HasSlash;
    }
    if is_dots(name) {
        return NameKind::Dots;
    }
    if text_len(name) > APPLET_NAME_MAX {
        return NameKind::Overlong;
    }
    NameKind::Ok
}

/// 校验一条直接调用现场。
///
/// 闸门次序：`lex → dispatch → lookup → text → stream → code → ready`。
/// 每一段只在上一段放行后执行；失败立即返回该段的 errno、退出码与注解。
pub fn define(s: &Shape) -> Outcome {
    // ---- 闸一：形态 ----
    let kind = s.name_kind();
    if !kind.usable() {
        return Outcome {
            stage: Stage::Lex,
            code: kind.errno(),
            value: EXIT_NOT_FOUND,
            note: Note::BadName,
            default: s.flavor.is_main() && !s.leak,
        };
    }

    // ---- 闸二：分发决策 ----
    match s.special {
        Special::None => {}
        Special::UnknownSwitch => {
            // 二长开关既不进表也不算信息形态：判死在分发段。
            return Outcome {
                stage: Stage::Dispatch,
                code: ERR_EINVAL,
                value: EXIT_NOT_FOUND,
                note: Note::BadName,
                default: false,
            };
        }
        Special::List | Special::ListFull => {
            return Outcome {
                stage: Stage::Ready,
                code: ERR_NONE,
                value: EXIT_RUN_OK,
                note: Note::ListDump,
                default: false,
            };
        }
        Special::Help | Special::Version => {
            return Outcome {
                stage: Stage::Ready,
                code: ERR_NONE,
                value: EXIT_RUN_OK,
                note: Note::UsageDump,
                default: false,
            };
        }
    }

    // ---- 闸三：注册表答复 ----
    let key = s.key();
    let found = lookup(key);
    if found.runnable() {
        if s.stub_unwired {
            return Outcome {
                stage: Stage::Lookup,
                code: ERR_ENOSYS,
                value: EXIT_NO_EXEC,
                note: Note::NoExec,
                default: false,
            };
        }
        if s.root_only && !s.am_root {
            return Outcome {
                stage: Stage::Code,
                code: ERR_EACCES,
                value: EXIT_NEED_ROOT,
                note: Note::NeedRoot,
                default: false,
            };
        }
        let note = match found.class {
            Known::Alias => Note::AliasRan,
            _ => Note::Ran,
        };
        return Outcome {
            stage: Stage::Ready,
            code: ERR_NONE,
            value: s.applet_rc,
            note,
            default: false,
        };
    }

    // 四类不可分发（别名指向被裁 / 被挡 / 从未注册 / 被裁）一律折叠成
    // notfound——折叠本身就是本卡与 F11663 的边界判据。
    // ---- 闸四：文案成形 ----
    let note = match s.flavor {
        Flavor::Modern => Note::Anchor,
        Flavor::Legacy => Note::LegacyText,
        Flavor::Stub => Note::StubText,
    };
    if !s.flavor.is_main() {
        return Outcome {
            stage: Stage::Text,
            code: ERR_ENOENT,
            value: EXIT_NOT_FOUND,
            note,
            default: false,
        };
    }

    // ---- 闸五：输出去向 ----
    if s.leak {
        return Outcome {
            stage: Stage::Stream,
            code: ERR_ENOENT,
            value: EXIT_NOT_FOUND,
            note,
            default: false,
        };
    }

    // ---- 闸六：码档归档 ----
    Outcome {
        stage: Stage::Code,
        code: ERR_ENOENT,
        value: EXIT_NOT_FOUND,
        note,
        default: true,
    }
}

// ---------------------------------------------------------------------------
// 文案（逐字复刻真 busybox 报错）
// ---------------------------------------------------------------------------

/// 主锚：`busybox: 'foo': applet not found`
pub fn msg_main_anchor(prog: &str, name: &str) -> String {
    let mut s = String::from(prog);
    s.push_str(": '");
    s.push_str(name);
    s.push_str("': applet not found");
    s
}

/// 旧版档：`busybox: applet not found`（没有名字）。
pub fn msg_legacy_anchor(prog: &str) -> String {
    let mut s = String::from(prog);
    s.push_str(": applet not found");
    s
}

/// `noapplet` 桩档：`busybox: applet 'foo' not found`
pub fn msg_stub_anchor(prog: &str, name: &str) -> String {
    let mut s = String::from(prog);
    s.push_str(": applet '");
    s.push_str(name);
    s.push_str("' not found");
    s
}

/// 需要 root 档。
pub fn msg_need_root(prog: &str, name: &str) -> String {
    let mut s = String::from(prog);
    s.push_str(": applet '");
    s.push_str(name);
    s.push_str("' requires root");
    s
}

/// 入口未接线档。
pub fn msg_no_exec(prog: &str, name: &str) -> String {
    let mut s = String::from(prog);
    s.push_str(": applet '");
    s.push_str(name);
    s.push_str("' is not executable");
    s
}

/// 主锚常量（B20 联签锚点，逐字）。
pub const MAIN_ANCHOR_LIT: &str = "busybox: 'foo': applet not found";

/// 把现场文案按注解成形；成功档一律空串（成功不报错，这是文案面的
/// 一票否决：任何成功现场带出错行都算红）。
pub fn message(s: &Shape, o: &Outcome) -> String {
    let prog = s.prog();
    let key = s.key();
    match o.note {
        Note::Anchor | Note::BadName => msg_main_anchor(prog, key),
        Note::LegacyText => msg_legacy_anchor(prog),
        Note::StubText => msg_stub_anchor(prog, key),
        Note::NeedRoot => msg_need_root(prog, key),
        Note::NoExec => msg_no_exec(prog, key),
        Note::Ran | Note::AliasRan | Note::ListDump | Note::UsageDump => String::new(),
    }
}

/// `--list` 的口袋清单：把表里可按名分发的**主名**按注册序拼成一行。
///
/// 这条函数是注册表的第二份陈述：清单里少了 `ls`、或多出被裁的 `vi`，
/// 都说明 `built`/`hidden` 两维在某处失真。
pub fn applet_list(sep: &str) -> String {
    let reg = registry();
    let mut out = String::new();
    let mut i = 0usize;
    while i < reg.len() {
        if let Some(a) = reg.get(i) {
            if a.is_primary() && a.dispatchable() {
                if !out.is_empty() {
                    out.push_str(sep);
                }
                out.push_str(a.name);
            }
        }
        i += 1;
    }
    out
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
    /// 退出码。
    pub value: u16,
    /// 注解。
    pub note: Note,
    /// 主锚逐字位。
    pub default: bool,
}

impl Claim {
    /// 空白申报。
    pub const fn blank() -> Claim {
        Claim {
            stage: Stage::Ready,
            code: 0,
            value: 0,
            note: Note::Ran,
            default: false,
        }
    }

    /// 是否申报成功。
    pub fn success(&self) -> bool {
        self.stage == Stage::Ready && self.code == 0
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
// 语料（14 槽）
// ---------------------------------------------------------------------------

/// 8 个可正确申报的现场 + 6 个"同一档答复、六种误报"的现场。
///
/// 后六槽的形状**互不相同**（路径不同、名字不同、内部类别不同），但实际
/// Outcome 必须逐位相同——这正是本卡要的"红档显式"：一档答复能被六条
/// 不同的路走歪，而现场本身没有区别。
pub const SHAPES: [Shape; CORPUS] = [
    Shape {
        tag: "direct-run-sh",
        argv0: "busybox",
        req: "sh",
        via: Via::Direct,
        special: Special::None,
        flavor: Flavor::Modern,
        leak: false,
        root_only: false,
        am_root: true,
        stub_unwired: false,
        applet_rc: 0,
    },
    Shape {
        tag: "alias-run-ash",
        argv0: "busybox",
        req: "ash",
        via: Via::Direct,
        special: Special::None,
        flavor: Flavor::Modern,
        leak: false,
        root_only: false,
        am_root: true,
        stub_unwired: false,
        applet_rc: 0,
    },
    Shape {
        tag: "list-dump",
        argv0: "busybox",
        req: "--list",
        via: Via::Direct,
        special: Special::List,
        flavor: Flavor::Modern,
        leak: false,
        root_only: false,
        am_root: true,
        stub_unwired: false,
        applet_rc: 0,
    },
    Shape {
        tag: "direct-unknown-foo",
        argv0: "busybox",
        req: "foo",
        via: Via::Direct,
        special: Special::None,
        flavor: Flavor::Modern,
        leak: false,
        root_only: false,
        am_root: true,
        stub_unwired: false,
        applet_rc: 0,
    },
    Shape {
        tag: "link-abs-trimmed",
        argv0: "/bin/wget",
        req: "wget",
        via: Via::LinkAbs,
        special: Special::None,
        flavor: Flavor::Modern,
        leak: false,
        root_only: false,
        am_root: true,
        stub_unwired: false,
        applet_rc: 0,
    },
    Shape {
        tag: "legacy-flavor",
        argv0: "busybox",
        req: "bash",
        via: Via::Direct,
        special: Special::None,
        flavor: Flavor::Legacy,
        leak: false,
        root_only: false,
        am_root: true,
        stub_unwired: false,
        applet_rc: 0,
    },
    Shape {
        tag: "leaked-stream",
        argv0: "busybox",
        req: "foobar",
        via: Via::Direct,
        special: Special::None,
        flavor: Flavor::Modern,
        leak: true,
        root_only: false,
        am_root: true,
        stub_unwired: false,
        applet_rc: 0,
    },
    Shape {
        tag: "slash-name",
        argv0: "busybox",
        req: "bin/ls",
        via: Via::Direct,
        special: Special::None,
        flavor: Flavor::Modern,
        leak: false,
        root_only: false,
        am_root: true,
        stub_unwired: false,
        applet_rc: 0,
    },
    Shape {
        tag: "estate-swap",
        argv0: "xyzzy",
        req: "xyzzy",
        via: Via::LinkRel,
        special: Special::None,
        flavor: Flavor::Modern,
        leak: false,
        root_only: false,
        am_root: true,
        stub_unwired: false,
        applet_rc: 0,
    },
    Shape {
        tag: "stage-swap",
        argv0: "/sbin/ifconfig",
        req: "ifconfig",
        via: Via::LinkAbs,
        special: Special::None,
        flavor: Flavor::Modern,
        leak: false,
        root_only: false,
        am_root: true,
        stub_unwired: false,
        applet_rc: 0,
    },
    Shape {
        tag: "code-swap",
        argv0: "busybox",
        req: "ip",
        via: Via::Direct,
        special: Special::None,
        flavor: Flavor::Modern,
        leak: false,
        root_only: false,
        am_root: true,
        stub_unwired: false,
        applet_rc: 0,
    },
    Shape {
        tag: "value-swap",
        argv0: "udhcpc",
        req: "udhcpc",
        via: Via::RawArgv0,
        special: Special::None,
        flavor: Flavor::Modern,
        leak: false,
        root_only: false,
        am_root: true,
        stub_unwired: false,
        applet_rc: 0,
    },
    Shape {
        tag: "note-swap",
        argv0: "editor",
        req: "editor",
        via: Via::LinkRel,
        special: Special::None,
        flavor: Flavor::Modern,
        leak: false,
        root_only: false,
        am_root: true,
        stub_unwired: false,
        applet_rc: 0,
    },
    Shape {
        tag: "default-swap",
        argv0: "@busybox",
        req: "httpd",
        via: Via::Retitle,
        special: Special::None,
        flavor: Flavor::Modern,
        leak: false,
        root_only: false,
        am_root: true,
        stub_unwired: false,
        applet_rc: 0,
    },
];

/// 与 `SHAPES` 一一对应的申报。
pub const CLAIMS: [Claim; CORPUS] = [
    Claim { stage: Stage::Ready, code: 0, value: 0, note: Note::Ran, default: false },
    Claim { stage: Stage::Ready, code: 0, value: 0, note: Note::AliasRan, default: false },
    Claim { stage: Stage::Ready, code: 0, value: 0, note: Note::ListDump, default: false },
    Claim { stage: Stage::Code, code: ERR_ENOENT, value: 127, note: Note::Anchor, default: true },
    Claim { stage: Stage::Code, code: ERR_ENOENT, value: 127, note: Note::Anchor, default: true },
    Claim { stage: Stage::Text, code: ERR_ENOENT, value: 127, note: Note::LegacyText, default: false },
    Claim { stage: Stage::Stream, code: ERR_ENOENT, value: 127, note: Note::Anchor, default: false },
    Claim { stage: Stage::Lex, code: ERR_EINVAL, value: 127, note: Note::BadName, default: true },
    Claim { stage: Stage::Ready, code: 0, value: 0, note: Note::Ran, default: false },
    Claim { stage: Stage::Lookup, code: ERR_ENOENT, value: 127, note: Note::Anchor, default: true },
    Claim { stage: Stage::Code, code: ERR_ENOSYS, value: 127, note: Note::Anchor, default: true },
    Claim { stage: Stage::Code, code: ERR_ENOENT, value: 1, note: Note::Anchor, default: true },
    Claim { stage: Stage::Code, code: ERR_ENOENT, value: 127, note: Note::StubText, default: true },
    Claim { stage: Stage::Code, code: ERR_ENOENT, value: 127, note: Note::Anchor, default: false },
];

/// 后 6 槽的期望缺陷。
pub const REJECT_FAULTS: [Fault; REJECT] =
    [Fault::Estate, Fault::Stage, Fault::Code, Fault::Value, Fault::Note, Fault::Default];

/// 后 6 槽的缺陷码。
pub const REJECT_CODES: [u32; REJECT] = [
    NF_ESTATE,
    NF_ESTAGE,
    NF_ECODE,
    NF_EVALUE,
    NF_ENOTE,
    NF_EDEFAULT,
];

/// 账本 8 个成功格的值（总数、成、败、运行、主锚、变体、畸形、红档率‰）。
pub const VALS: [u32; ACCEPT] = [14, 8, 6, 3, 2, 2, 1, 929];

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
        return Err(NF_ENOBS);
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

/// 对判。
///
/// 判序固定为 Estate → Stage → Code → Value → Note → Default。
/// Estate 最先的理由：成败互报是最贵的一类误报（把 127 现场说成跑通了），
/// 它必须先于"哪个段失败"被抓住；否则一条既报成功又报错阶段的申报会被
/// 归因成阶段笔误，把造假降级成手滑。
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
    let mut s = String::from(c.shape.prog());
    s.push_str("/");
    s.push_str(c.shape.key());
    s.push_str(" stage=");
    s.push_str(o.stage.name());
    s.push_str(" code=");
    s.push_str(i32_text(o.code).as_str());
    s.push_str(" exit=");
    s.push_str(u32_text(o.value as u32).as_str());
    s.push_str(" anchor=");
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
/// 这份表把 `SHAPES`/`CLAIMS` 用**另一种形式**再写一遍：申报表说
/// "我声明什么"，期望表说"七闸实际走到哪一步、答复落在哪一档"。任一处
/// 笔误都会在这里露出来。
const WANT_STAGE: [Stage; CORPUS] = [
    Stage::Ready,
    Stage::Ready,
    Stage::Ready,
    Stage::Code,
    Stage::Code,
    Stage::Text,
    Stage::Stream,
    Stage::Lex,
    Stage::Code,
    Stage::Code,
    Stage::Code,
    Stage::Code,
    Stage::Code,
    Stage::Code,
];

/// 14 槽的期望 errno。
const WANT_CODE: [i32; CORPUS] = [
    0,
    0,
    0,
    ERR_ENOENT,
    ERR_ENOENT,
    ERR_ENOENT,
    ERR_ENOENT,
    ERR_EINVAL,
    ERR_ENOENT,
    ERR_ENOENT,
    ERR_ENOENT,
    ERR_ENOENT,
    ERR_ENOENT,
    ERR_ENOENT,
];

/// 14 槽的期望退出码。
const WANT_VALUE: [u16; CORPUS] = [0, 0, 0, 127, 127, 127, 127, 127, 127, 127, 127, 127, 127, 127];

/// 14 槽的期望注解。
const WANT_NOTE: [Note; CORPUS] = [
    Note::Ran,
    Note::AliasRan,
    Note::ListDump,
    Note::Anchor,
    Note::Anchor,
    Note::LegacyText,
    Note::Anchor,
    Note::BadName,
    Note::Anchor,
    Note::Anchor,
    Note::Anchor,
    Note::Anchor,
    Note::Anchor,
    Note::Anchor,
];

/// 14 槽的期望主锚逐字位。
const WANT_DEFAULT: [bool; CORPUS] = [
    false, false, false, true, true, false, false, true, true, true, true, true, true, true,
];

/// 14 槽标签。
const WANT_TAGS: [&str; CORPUS] = [
    "direct-run-sh",
    "alias-run-ash",
    "list-dump",
    "direct-unknown-foo",
    "link-abs-trimmed",
    "legacy-flavor",
    "leaked-stream",
    "slash-name",
    "estate-swap",
    "stage-swap",
    "code-swap",
    "value-swap",
    "note-swap",
    "default-swap",
];

/// 14 槽的期望档位（覆盖四档，且后六槽全部落主锚档）。
const WANT_TIER: [Tier; CORPUS] = [
    Tier::Run,
    Tier::Run,
    Tier::Run,
    Tier::Anchor,
    Tier::Anchor,
    Tier::Variant,
    Tier::Variant,
    Tier::Malformed,
    Tier::Anchor,
    Tier::Anchor,
    Tier::Anchor,
    Tier::Anchor,
    Tier::Anchor,
    Tier::Anchor,
];

/// 八格申报正确的下标。
const ACCEPT_SLOTS: [usize; ACCEPT] = [0usize, 1, 2, 3, 4, 5, 6, 7];

/// 六格申报错误的下标。
const REJECT_SLOTS: [usize; REJECT] = [8usize, 9, 10, 11, 12, 13];

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
/// 分档只看**实际结果**落在哪一档，申报正确与否单独计。本卡的六格复刻现场
/// 全部落 anchor 档，于是 anchor 行必然出现 `total=8 good=2 red=6`：覆盖面
/// 看着满，红档却挤在同一行——这正是把 total 与 good 拆成两维的理由。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Matrix {
    /// 行（run/anchor/variant/malformed）。
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

    /// 渲染全矩阵。
    pub fn render(&self) -> String {
        let tiers = [Tier::Run, Tier::Anchor, Tier::Variant, Tier::Malformed];
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
    /// 运行档正确数。
    pub run: u32,
    /// 主锚档正确数。
    pub anchor: u32,
    /// 变体档正确数。
    pub variant: u32,
    /// 畸形档正确数。
    pub malformed: u32,
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
            Err(NF_ECOUNT)
        }
    }

    /// 成功档合计（申报正确且结果成功）。
    pub fn success_cells(&self) -> u32 {
        self.run
    }

    /// 失败档合计（申报正确且结果失败）。
    pub fn failure_cells(&self) -> u32 {
        self.anchor + self.variant + self.malformed
    }

    /// notfound 面占申报正确格的比例（百分数，整型）。
    pub fn not_found_share(&self) -> u32 {
        if self.accept == 0 {
            return 0;
        }
        (self.failure_cells() * 100) / self.accept
    }

    /// 渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("notfound corpus=");
        s.push_str(u32_text(self.total).as_str());
        s.push_str(" accept=");
        s.push_str(u32_text(self.accept).as_str());
        s.push_str(" reject=");
        s.push_str(u32_text(self.reject).as_str());
        s.push_str(" run=");
        s.push_str(u32_text(self.run).as_str());
        s.push_str(" anchor=");
        s.push_str(u32_text(self.anchor).as_str());
        s.push_str(" variant=");
        s.push_str(u32_text(self.variant).as_str());
        s.push_str(" malformed=");
        s.push_str(u32_text(self.malformed).as_str());
        s.push_str(" match=");
        s.push_str(u32_text(self.matched).as_str());
        s.push_str(" rate=");
        s.push_str(u32_text(self.rate()).as_str());
        s.push_str("ppm\n");
        s
    }
}

/// 全绿渲染文本（逐字钉住）。
const RENDER_OK: &str =
    "notfound corpus=14 accept=8 reject=6 run=3 anchor=2 variant=2 malformed=1 match=14 rate=1000ppm\n";

/// 全跑。
pub fn run() -> Result<Report, u32> {
    run_with(None)
}

/// 可注入一槽缺陷的全跑。
///
/// 注入把"期望表"本身也放进被测面：强行宣称某槽犯了另一种缺陷，matched
/// 必须掉、rate 必须离开 1000。若注入后仍报全绿，说明 matched 是硬编码的。
pub fn run_with(over: Option<(usize, Fault)>) -> Result<Report, u32> {
    if let Some((i, _)) = over {
        if i >= CORPUS {
            return Err(NF_ESLOT);
        }
    }
    let cs = cases();
    let mut total = 0u32;
    let mut accept = 0u32;
    let mut reject = 0u32;
    let mut run = 0u32;
    let mut anchor = 0u32;
    let mut variant = 0u32;
    let mut malformed = 0u32;
    let mut matched = 0u32;
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return Err(NF_ENOBS),
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
            None => return Err(NF_ENOBS),
        };
        total += 1;
        if f == want {
            matched += 1;
        }
        if f == Fault::Ok {
            accept += 1;
            match Tier::of(&a) {
                Tier::Run => run += 1,
                Tier::Anchor => anchor += 1,
                Tier::Variant => variant += 1,
                Tier::Malformed => malformed += 1,
            }
        } else {
            reject += 1;
        }
        i += 1;
    }
    Ok(Report { total, accept, reject, run, anchor, variant, malformed, matched })
}

// ---------------------------------------------------------------------------
// 阶段闸门
// ---------------------------------------------------------------------------

/// 单调阶段闸门。
///
/// 判定纵线 `lex → dispatch → lookup → text → stream → code → ready` 七段
/// 只许向前。回退（例如已经判死在 code 又声称回到 lookup 复核）说明申报者
/// 没有按闸门次序观测，而是事后倒推——这类申报在双机对照里最危险，因为
/// 文案完全合理，只有归因链是假的。
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
            return Err(NF_ESTATE);
        }
        let o = s.order() as i32;
        if o < self.last {
            return Err(NF_ESTAGE);
        }
        self.last = o;
        self.steps = self.steps.saturating_add(1);
        Ok(())
    }

    /// 关闭，返回段数。
    pub fn close(&mut self) -> Result<u32, u32> {
        if self.closed {
            return Err(NF_ESTATE);
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
    /// 槽位（必须严格等于当前长度才可登记）。
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

    /// 格指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.slot as u64);
        h = fold_word(h, self.op as u64);
        h = fold_word(h, self.ok as u64);
        h = fold_word(h, self.code as u64);
        h = fold_word(h, self.value as u64);
        h
    }
}

/// 账本容量下限（与 `SAMPLES` 同值，留作显式声明）。
pub const CAP: usize = 6;

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
            return Err(NF_ESLOT);
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

    /// 逐格对账：槽位连续、成格值逐格等于 `VALS`、败格码逐格等于
    /// `REJECT_CODES` 且败格值必须为 0。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != ACCEPT || self.err_count() != REJECT {
            return Err(NF_EBAL);
        }
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(NF_EBAL),
            };
            if s.slot as usize != i {
                return Err(NF_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(NF_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(NF_EVALUE);
                        }
                    }
                    None => return Err(NF_EBAL),
                }
                vi += 1;
            } else {
                if s.value != 0 {
                    return Err(NF_EVALUE);
                }
                match REJECT_CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(NF_EVALUE);
                        }
                    }
                    None => return Err(NF_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != ACCEPT || ci != REJECT {
            return Err(NF_EBAL);
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
            return Err(NF_EBAL);
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
            Err(NF_ESEAL)
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

/// 建账本：7 格统计 + 1 格红档率 + 6 格缺陷码。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let r = run();
    let _ = l.register(count_cell(0, OP_CORPUS, r.map(|x| x.total)));
    let _ = l.register(count_cell(1, OP_CORPUS, r.map(|x| x.accept)));
    let _ = l.register(count_cell(2, OP_CORPUS, r.map(|x| x.reject)));
    let _ = l.register(count_cell(3, OP_DISPATCH, r.map(|x| x.run)));
    let _ = l.register(count_cell(4, OP_LOOKUP, r.map(|x| x.anchor)));
    let _ = l.register(count_cell(5, OP_TEXT, r.map(|x| x.variant)));
    let _ = l.register(count_cell(6, OP_LEX, r.map(|x| x.malformed)));

    let red = run_with(Some((0, Fault::Code)));
    let _ = l.register(count_cell(7, OP_REPLAY, red.map(|x| x.rate())));

    let cs = cases();
    let mut i = 0usize;
    while i < REJECT {
        if let Some(c) = cs.get(i + ACCEPT) {
            let f = judge(c);
            let _ = l.register(Sample {
                slot: (ACCEPT + i) as u8,
                op: OP_CODE,
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

/// 四路注入：表路 / 判路 / 渲染路 / 账本路。
///
/// 1. 表路——段基址、枚举、注册表、调用路径形态；
/// 2. 判路——七闸纵线、码档、对判、矩阵；
/// 3. 渲染路——语料、期望表、红档、文案、释义；
/// 4. 账本路——阶段闸门、逐格对账、冻结重放。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = name_ok() && enums_ok() && registry_ok() && via_ok();
    out[1] = define_ok() && anchor_ok() && judge_ok() && matrix_ok();
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

/// 名称与段基址：常量链、尺寸、errno/码档取值。
fn name_ok() -> bool {
    if SEG_BASE != 0x6d100 {
        return false;
    }
    if NF_ESTAGE != SEG_BASE + 0x01
        || NF_ECODE != SEG_BASE + 0x02
        || NF_EVALUE != SEG_BASE + 0x03
        || NF_ENOTE != SEG_BASE + 0x04
        || NF_EDEFAULT != SEG_BASE + 0x05
        || NF_EVIA != SEG_BASE + 0x06
    {
        return false;
    }
    if NF_ESTATE != SEG_BASE + 0x07
        || NF_ECOUNT != SEG_BASE + 0x08
        || NF_ESLOT != SEG_BASE + 0x09
        || NF_ESEAL != SEG_BASE + 0x0a
        || NF_ENOBS != SEG_BASE + 0x0b
        || NF_EMATRIX != SEG_BASE + 0x0c
        || NF_EBAL != SEG_BASE + 0x0d
    {
        return false;
    }
    if CORPUS != 14 || SAMPLES != 14 || ACCEPT != 8 || REJECT != 6 || TIER_COUNT != 4 {
        return false;
    }
    if APLET_COUNT != 30 || ALIAS_MAX != 3 || SELF_NAME_COUNT != 4 {
        return false;
    }
    if APPLET_NAME_MAX != 30 {
        return false;
    }
    // 码档：127 是 notfound 的约定档，126 是"找到但不可执行"，1 是 applet 自败。
    if EXIT_NOT_FOUND != 127 || EXIT_NO_EXEC != 126 || EXIT_NEED_ROOT != 1 {
        return false;
    }
    if EXIT_RUN_OK != 0 || EXIT_RUN_FAIL != 1 || EXIT_USAGE != 1 {
        return false;
    }
    if ERR_ENOENT != 2 || ERR_EINVAL != 22 || ERR_EACCES != 13 {
        return false;
    }
    if ERR_ENOSYS != 38 || ERR_ENAMETOOLONG != 36 || ERR_NONE != 0 {
        return false;
    }
    if OP_LEX != 1 || OP_DISPATCH != 2 || OP_LOOKUP != 3 || OP_TEXT != 4 {
        return false;
    }
    if OP_STREAM != 5 || OP_CODE != 6 || OP_CORPUS != 7 || OP_REPLAY != 8 {
        return false;
    }
    if CAP != 6 {
        return false;
    }
    // 主锚字面量必须与生成器一致（这条是"联签"的物理形式）。
    if msg_main_anchor("busybox", "foo") != MAIN_ANCHOR_LIT {
        return false;
    }
    let mut v = String::new();
    v.push_str("applet not found");
    v.len() == 16 && !has_byte(v.as_str(), b'\n')
}

/// 枚举：次序、文本唯一性、分类不重不漏。
fn enums_ok() -> bool {
    let stages = [
        Stage::Lex,
        Stage::Dispatch,
        Stage::Lookup,
        Stage::Text,
        Stage::Stream,
        Stage::Code,
        Stage::Ready,
    ];
    let mut i = 0usize;
    while i < stages.len() {
        let s = match stages.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if s.order() as usize != i {
            return false;
        }
        // 只有 ready 不是失败点。
        if i + 1 == stages.len() {
            if s.failure() {
                return false;
            }
        } else if !s.failure() {
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
        Note::Ran,
        Note::AliasRan,
        Note::ListDump,
        Note::UsageDump,
        Note::Anchor,
        Note::LegacyText,
        Note::StubText,
        Note::NeedRoot,
        Note::NoExec,
        Note::BadName,
    ];
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
        // 三分类互斥：成功档不得同时是 notfound 面或文案变体档。
        if a.success() && (a.not_found() || a.text_variant()) {
            return false;
        }
        // notfound 面与文案变体档都必须有可比的文案家族，不得落进静默档。
        if (a.not_found() || a.text_variant()) && a.family() == Anchor::Silent {
            return false;
        }
        n += 1;
    }
    if !Note::Anchor.not_found() || !Note::BadName.not_found() {
        return false;
    }
    if Note::Anchor.family() != Anchor::Main || Note::BadName.family() != Anchor::Main {
        return false;
    }
    if Note::LegacyText.family() != Anchor::Legacy || Note::StubText.family() != Anchor::Stub {
        return false;
    }
    if !Note::Ran.success() || Note::NeedRoot.success() || Note::NoExec.success() {
        return false;
    }
    if Note::NeedRoot.family() != Anchor::Silent || Note::NoExec.family() != Anchor::Silent {
        return false;
    }

    let tiers = [Tier::Run, Tier::Anchor, Tier::Variant, Tier::Malformed];
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
        // 除 run 外全部是 notfound 面。
        if x.not_found_face() != (t != 0) {
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
        Fault::Via,
        Fault::Estate,
        Fault::Count,
        Fault::Slot,
        Fault::Seal,
        Fault::Nobs,
        Fault::Matrix,
        Fault::Bal,
    ];
    let mut f = 0usize;
    let mut seen = [0u32; 14];
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

    let kinds = [
        NameKind::Ok,
        NameKind::Empty,
        NameKind::HasSlash,
        NameKind::Dots,
        NameKind::Control,
        NameKind::Overlong,
    ];
    let mut p = 0usize;
    while p < kinds.len() {
        let k = match kinds.get(p) {
            Some(v) => *v,
            None => return false,
        };
        // 只有 Ok 可用，且 Ok 的 errno 必须为零。
        if k.usable() != (p == 0) || k.errno() == 0 && !k.usable() {
            return false;
        }
        if k.usable() && k.errno() != ERR_NONE {
            return false;
        }
        p += 1;
    }
    if NameKind::Overlong.errno() != ERR_ENAMETOOLONG {
        return false;
    }
    if NameKind::Empty.errno() != ERR_EINVAL || NameKind::Dots.errno() != ERR_EINVAL {
        return false;
    }

    let known = [Known::Registered, Known::Alias, Known::Trimmed, Known::Hidden, Known::Unknown];
    let mut q = 0usize;
    while q < known.len() {
        let z = match known.get(q) {
            Some(v) => *v,
            None => return false,
        };
        // 可跑的两档一定在表里；notfound 的三档一定答复"跑不了"。
        if z.runnable() && !z.in_table() {
            return false;
        }
        if z.not_found() == z.runnable() {
            return false;
        }
        if z == Known::Unknown && z.in_table() {
            return false;
        }
        q += 1;
    }

    let flavors = [Flavor::Modern, Flavor::Legacy, Flavor::Stub];
    let mut w = 0usize;
    while w < flavors.len() {
        let x = match flavors.get(w) {
            Some(v) => *v,
            None => return false,
        };
        if x.is_main() != (w == 0) {
            return false;
        }
        if !x.quotes_name() && x.is_main() {
            return false;
        }
        w += 1;
    }
    if !Flavor::Stub.quotes_name() || Flavor::Legacy.quotes_name() {
        return false;
    }

    let streams = [Stream::Stderr, Stream::Stdout, Stream::Split, Stream::Devnull];
    let mut e = 0usize;
    while e < streams.len() {
        let x = match streams.get(e) {
            Some(v) => *v,
            None => return false,
        };
        // 报错只认 stderr。
        if x.valid_for_error() != (e == 0) {
            return false;
        }
        e += 1;
    }

    Fault::Ok.code() == 0
        && Fault::Stage.code() == NF_ESTAGE
        && Fault::Estate.code() == NF_ESTATE
        && Fault::Via.code() == NF_EVIA
        && Fault::Bal.code() == NF_EBAL
}

/// 注册表闸：名字唯一、注册序连续、别名必指主名、清单与两维一致。
fn registry_ok() -> bool {
    let reg = registry();
    if reg.len() != APLET_COUNT {
        return false;
    }
    let mut i = 0usize;
    let mut primaries = 0usize;
    let mut aliases = 0usize;
    let mut trimmed = 0usize;
    let mut hidden = 0usize;
    let mut runnable = 0usize;
    while i < reg.len() {
        let a = match reg.get(i) {
            Some(v) => *v,
            None => return false,
        };
        // 注册序必须等于下标（表序失真意味着分发序与观测序脱钩）。
        if a.prio as usize != i {
            return false;
        }
        if a.name.is_empty() || a.kconfig.is_empty() || a.section.is_empty() {
            return false;
        }
        if text_len(a.name) > APPLET_NAME_MAX {
            return false;
        }
        // 名字绝不该带路径或控制字符。
        if slash_shape(a.name).0 || has_control(a.name) || has_byte(a.name, b' ') {
            return false;
        }
        // 主名与别名二选一。
        if a.is_primary() == a.is_alias() {
            return false;
        }
        if a.is_primary() {
            primaries += 1;
            if !a.wired() {
                return false;
            }
        } else {
            aliases += 1;
            // 别名项共用主名入口，main 必须为空。
            if !a.main.is_empty() {
                return false;
            }
            match applet_at(a.alias_of) {
                Some(t) => {
                    if !t.is_primary() {
                        return false;
                    }
                    if same_text(t.name, a.name) {
                        return false;
                    }
                }
                None => return false,
            }
        }
        if !a.built {
            trimmed += 1;
        }
        if a.hidden {
            hidden += 1;
        }
        // 只数主名：别名共用主名入口，不进口袋清单。
        if a.is_primary() && a.dispatchable() {
            runnable += 1;
        }
        // 被挡的项不得同时可分发。
        if a.hidden && a.dispatchable() {
            return false;
        }
        if !a.built && a.dispatchable() {
            return false;
        }
        // 名字必须能被查回来。
        if let Some(back) = applet_at(a.name) {
            if back != a {
                return false;
            }
        } else {
            return false;
        }
        if let Some(idx) = applet_index(a.name) {
            if idx != i {
                return false;
            }
        } else {
            return false;
        }
        let mut k = i + 1;
        while k < reg.len() {
            if let Some(o) = reg.get(k) {
                if same_text(o.name, a.name) {
                    return false;
                }
            }
            k += 1;
        }
        i += 1;
    }
    if primaries != 26 || aliases != 4 {
        return false;
    }
    // 6 个被裁主名 + editor（别名也标了 built=false）= 7；2 个被挡主名。
    if trimmed != 7 || hidden != 2 {
        return false;
    }
    // 可分发的只有 18 个主名（别名不重复计入清单）。
    if runnable != 18 {
        return false;
    }
    // 查表答复逐档校一遍真值。
    if lookup("sh").class != Known::Registered || lookup("sh").effective != "sh" {
        return false;
    }
    if lookup("ash").class != Known::Alias || lookup("ash").effective != "sh" {
        return false;
    }
    if lookup("hush").class != Known::Alias || lookup("hush").effective != "sh" {
        return false;
    }
    if lookup("ping6").class != Known::Alias || lookup("ping6").effective != "ping" {
        return false;
    }
    if lookup("editor").class != Known::Trimmed || lookup("editor").effective != "vi" {
        return false;
    }
    if lookup("less").class != Known::Hidden || lookup("vi").class != Known::Trimmed {
        return false;
    }
    if lookup("zzz-not-an-applet").class != Known::Unknown {
        return false;
    }
    // 空名查不中（表里没有空字符串项）。
    if lookup("").class != Known::Unknown || applet_at("").is_some() {
        return false;
    }
    // 口袋清单：含 ls/route/noapplet，不含被裁与被挡的四类名字。
    let list = applet_list(",");
    if !list.contains("ls") || !list.contains("route") || !list.contains("noapplet") {
        return false;
    }
    if list.contains("vi,") || list.contains("ifconfig") || list.contains("less") {
        return false;
    }
    if list.contains("httpd") || list.contains("editor") || list.contains("udhcpc") {
        return false;
    }
    // 清单条数必须等于可分发主名数。
    let mut items = 1usize;
    let lb = list.as_bytes();
    let mut j = 0usize;
    while j < lb.len() {
        if let Some(c) = lb.get(j) {
            if *c == b',' {
                items += 1;
            }
        }
        j += 1;
    }
    let mut prim_runnable = 0usize;
    let mut k = 0usize;
    while k < reg.len() {
        if let Some(a) = reg.get(k) {
            if a.is_primary() && a.dispatchable() {
                prim_runnable += 1;
            }
        }
        k += 1;
    }
    items == prim_runnable && !list.is_empty() && !list.ends_with(',')
}

/// 调用路径形态：六条路径的键来源与程序名来源必须各自成立。
fn via_ok() -> bool {
    // 分类互斥性。
    if Via::Direct.from_argv0()
        || Via::DirectPath.from_argv0()
        || Via::Retitle.from_argv0()
        || !Via::LinkRel.from_argv0()
        || !Via::LinkAbs.from_argv0()
        || !Via::RawArgv0.from_argv0()
    {
        return false;
    }
    if Via::Direct.stripped() || !Via::LinkRel.stripped() || !Via::LinkAbs.stripped() {
        return false;
    }
    if Via::DirectPath.stripped() || !Via::Retitle.stripped() || Via::RawArgv0.stripped() {
        return false;
    }
    if !Via::Direct.self_named() || Via::LinkAbs.self_named() || Via::RawArgv0.self_named() {
        return false;
    }
    // 开关形态：只有 UnknownSwitch 不是信息形态，只有 None 进表。
    if Special::None.skips_lookup()
        || !Special::List.skips_lookup()
        || !Special::UnknownSwitch.skips_lookup()
    {
        return false;
    }
    if Special::None.informational()
        || !Special::List.informational()
        || Special::UnknownSwitch.informational()
    {
        return false;
    }
    if Special::List.name() != "--list" || Special::UnknownSwitch.name() != "--unknown" {
        return false;
    }
    // basename 提取：六条路径各自的键与程序名。
    let mut d = Shape::blank();
    d.argv0 = "busybox";
    d.req = "foo";
    d.via = Via::Direct;
    if d.key() != "foo" || d.prog() != "busybox" {
        return false;
    }
    let mut dp = d;
    dp.argv0 = "/bin/busybox";
    dp.via = Via::DirectPath;
    if dp.key() != "foo" || dp.prog() != "busybox" {
        return false;
    }
    let mut lr = d;
    lr.argv0 = "foo";
    lr.via = Via::LinkRel;
    if lr.key() != "foo" || lr.prog() != "foo" || !lr.via.from_argv0() {
        return false;
    }
    let mut la = d;
    la.argv0 = "/usr/sbin/foo";
    la.via = Via::LinkAbs;
    if la.key() != "foo" || la.prog() != "foo" {
        return false;
    }
    let mut rw = d;
    rw.argv0 = "bin/foo";
    rw.via = Via::RawArgv0;
    if rw.key() != "bin/foo" || rw.prog() != "bin/foo" {
        return false;
    }
    let mut rt = d;
    rt.argv0 = "@busybox";
    rt.via = Via::Retitle;
    if rt.key() != "foo" || rt.prog() != "busybox" {
        return false;
    }
    // `@` 伪装：只有首字符是 @ 才剥，且只剥一层。
    if !has_at_prefix("@busybox") || has_at_prefix("busybox") || has_at_prefix("") {
        return false;
    }
    if strip_at("@busybox") != "busybox" || strip_at("busybox") != "busybox" {
        return false;
    }
    if strip_at("@@busybox") != "@busybox" {
        return false;
    }
    // last_component：无斜杠原样、有斜杠取尾、尾斜杠得空串。
    if last_component("foo") != "foo" || last_component("/bin/foo") != "foo" {
        return false;
    }
    if last_component("a/b/c") != "c" || last_component("/bin/") != "" {
        return false;
    }
    if dir_prefix("foo").is_some() || dir_prefix("/bin/foo") != Some("/bin") {
        return false;
    }
    // 斜杠形态探测：任一/尾一。
    if slash_shape("foo") != (false, false) || slash_shape("/bin/foo") != (true, false) {
        return false;
    }
    if slash_shape("foo/") != (true, true) || slash_shape("/") != (true, true) {
        return false;
    }
    // 输出去向：串台优先于形态判定。
    let mut leak = d;
    leak.leak = true;
    if leak.stream() != Stream::Split || d.stream() != Stream::Stderr {
        return false;
    }
    let mut ls = d;
    ls.special = Special::List;
    if ls.stream() != Stream::Stdout {
        return false;
    }
    let mut bad = ls;
    bad.leak = true;
    if bad.stream() != Stream::Split || !Stream::Stderr.valid_for_error() {
        return false;
    }
    // 名字形态判序：空、控制字符、带斜杠、纯点、超长。
    if lex_name("") != NameKind::Empty || lex_name("a\u{7}b") != NameKind::Control {
        return false;
    }
    if lex_name("bin/ls") != NameKind::HasSlash || lex_name("..") != NameKind::Dots {
        return false;
    }
    if lex_name("ls") != NameKind::Ok {
        return false;
    }
    let over = "applet_name_that_is_definitely_longer_than_thirty_bytes";
    if lex_name(over) != NameKind::Overlong {
        return false;
    }
    // 边界：正好 30 字节可用，31 字节超长。
    let exact = "abcdefghijklmnopqrstuvwxyzabcd";
    if text_len(exact) != APPLET_NAME_MAX || lex_name(exact) != NameKind::Ok {
        return false;
    }
    let mut one_more = String::from(exact);
    one_more.push('p');
    if lex_name(one_more.as_str()) != NameKind::Overlong {
        return false;
    }
    // 带斜杠又超长：斜杠优先（strcmp 在第一字符就输了）。
    let slashy = "aaaaaaaaaaaaaaaaaaaaaaaaaaaa/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    if lex_name(slashy) != NameKind::HasSlash {
        return false;
    }
    // 形状指纹对维度敏感。
    let s1 = d.seal();
    let s2 = lr.seal();
    let s3 = la.seal();
    s1 != s2 && s1 != s3 && s2 != s3 && d.clone_req("bar").seal() != s1
}

/// 定义纵线：七闸逐段推进，先短路的段不得被后段覆盖。
fn define_ok() -> bool {
    // 闸一：空名。直接调用给出空串时判死在形态段，绝不进表。
    let mut empty = Shape::blank();
    empty.req = "";
    let o = define(&empty);
    if o.stage != Stage::Lex || o.code != ERR_EINVAL || o.value != EXIT_NOT_FOUND {
        return false;
    }
    if o.note != Note::BadName || o.default != true || o.ok() {
        return false;
    }
    // 控制字符、纯点同样判死，但文案仍是主锚形态（真机只有一条 notfound）。
    let mut dots = empty;
    dots.req = "..";
    let d = define(&dots);
    if d.stage != Stage::Lex || d.note != Note::BadName || d.code != ERR_EINVAL {
        return false;
    }
    let mut ctrl = empty;
    ctrl.req = "a\u{1}b";
    if define(&ctrl).code != ERR_EINVAL || define(&ctrl).stage != Stage::Lex {
        return false;
    }
    // 超长名单独一档 errno，与"不可用"区分开。
    let mut long = empty;
    long.req = "applet_name_that_is_definitely_longer_than_thirty_bytes";
    let l = define(&long);
    if l.stage != Stage::Lex || l.code != ERR_ENAMETOOLONG || l.value != EXIT_NOT_FOUND {
        return false;
    }
    // 形态段的短路必须先于分发段：开关形态带着坏名也不查表。
    let mut special_bad = empty;
    special_bad.req = "a/b";
    special_bad.special = Special::List;
    if define(&special_bad).stage != Stage::Ready {
        return false;
    }

    // 闸二：分发决策。
    let mut list = Shape::blank();
    list.req = "--list";
    list.special = Special::List;
    let o = define(&list);
    if o.stage != Stage::Ready || o.code != 0 || o.value != 0 || o.note != Note::ListDump {
        return false;
    }
    if !o.ok() || Tier::of(&o) != Tier::Run || o.default {
        return false;
    }
    let mut help = list;
    help.special = Special::Help;
    if define(&help).note != Note::UsageDump || define(&help).value != 0 {
        return false;
    }
    let mut unk = list;
    unk.special = Special::UnknownSwitch;
    let u = define(&unk);
    if u.stage != Stage::Dispatch || u.code != ERR_EINVAL || !u.failed() {
        return false;
    }

    // 闸三：注册表答复。
    let mut run = Shape::blank();
    run.req = "ls";
    let o = define(&run);
    if o.stage != Stage::Ready || o.note != Note::Ran || o.code != 0 || o.default {
        return false;
    }
    // applet 自己的退出码不得污染 errno。
    run.applet_rc = EXIT_RUN_FAIL;
    let o = define(&run);
    if o.value != 1 || o.code != 0 || !o.ok() {
        return false;
    }
    // 别名走同一条分发，但注解必须记成别名。
    let mut al = Shape::blank();
    al.req = "ash";
    let o = define(&al);
    if o.note != Note::AliasRan || !o.ok() || Tier::of(&o) != Tier::Run {
        return false;
    }
    // 需要 root 却没 root：判死在码档段，errno 用 EACCES、退 1。
    let mut root = Shape::blank();
    root.req = "ls";
    root.root_only = true;
    root.am_root = false;
    let r = define(&root);
    if r.stage != Stage::Code || r.code != ERR_EACCES || r.value != EXIT_NEED_ROOT {
        return false;
    }
    if r.note != Note::NeedRoot || r.default || Tier::of(&r) != Tier::Variant {
        return false;
    }
    root.am_root = true;
    if define(&root).note != Note::Ran {
        return false;
    }
    // 表生成了但入口未接线：判死在查表段，码档 126。
    let mut unwired = Shape::blank();
    unwired.req = "ls";
    unwired.stub_unwired = true;
    let w = define(&unwired);
    if w.stage != Stage::Lookup || w.code != ERR_ENOSYS || w.value != EXIT_NO_EXEC {
        return false;
    }
    if w.note != Note::NoExec || w.default || w.ok() {
        return false;
    }

    // 闸三四五六：notfound 折叠。四类不可分发必须给出逐位相同的结果。
    let mut unknown = Shape::blank();
    unknown.req = "zzz-never-a-name";
    let a1 = define(&unknown);
    let mut trimmed = Shape::blank();
    trimmed.req = "vi";
    let a2 = define(&trimmed);
    let mut hidden = Shape::blank();
    hidden.req = "less";
    let a3 = define(&hidden);
    let mut alias_dead = Shape::blank();
    alias_dead.req = "editor";
    let a4 = define(&alias_dead);
    if a1 != a2 || a2 != a3 || a3 != a4 {
        return false;
    }
    if a1.stage != Stage::Code || a1.code != ERR_ENOENT || a1.value != EXIT_NOT_FOUND {
        return false;
    }
    if a1.note != Note::Anchor || !a1.default || Tier::of(&a1) != Tier::Anchor {
        return false;
    }
    // 折叠只许发生在 Outcome 上，表答复本身仍要能分辨（F11663 的入口）。
    if lookup("vi").class == lookup("zzz-never-a-name").class {
        return false;
    }
    if lookup("less").class != Known::Hidden || lookup("editor").class != Known::Trimmed {
        return false;
    }

    // 闸四：文案家族。非主锚档判死在文案段。
    let mut legacy = unknown;
    legacy.flavor = Flavor::Legacy;
    let o = define(&legacy);
    if o.stage != Stage::Text || o.note != Note::LegacyText || o.default {
        return false;
    }
    if o.code != ERR_ENOENT || o.value != EXIT_NOT_FOUND || Tier::of(&o) != Tier::Variant {
        return false;
    }
    let mut stub = unknown;
    stub.flavor = Flavor::Stub;
    let s = define(&stub);
    if s.stage != Stage::Text || s.note != Note::StubText || s.default {
        return false;
    }
    if message(&stub, &s) == message(&legacy, &o) {
        return false;
    }

    // 闸五：输出去向。串台判死在流段，文案本身仍然合主锚。
    let mut leak = unknown;
    leak.leak = true;
    let o = define(&leak);
    if o.stage != Stage::Stream || o.note != Note::Anchor || o.default {
        return false;
    }
    if o.code != ERR_ENOENT || o.value != EXIT_NOT_FOUND {
        return false;
    }
    if message(&leak, &o) != msg_main_anchor("busybox", "zzz-never-a-name") {
        return false;
    }
    if define(&unknown).default && !o.default {
        // 同文案、不同流：主锚逐字位必须为假——流维不是装饰。
    } else {
        return false;
    }

    // 结果指纹：同形同指纹、异形异指纹。
    let x = define(&unknown);
    let y = define(&unknown);
    x.seal() == y.seal() && x.seal() != o.seal() && o.seal() != define(&legacy).seal()
}

/// 主锚族：逐字文案、程序名前缀、六路同档。
fn anchor_ok() -> bool {
    // 主锚逐字。
    if msg_main_anchor("busybox", "foo") != "busybox: 'foo': applet not found" {
        return false;
    }
    // msg_* 逐字照抄传入的程序名，取 basename 是 Shape::prog 的职责：
    // 直接路径形态必须由 prog() 剥掉目录，再由 message() 拼成主锚。
    let mut pathed = Shape::blank();
    pathed.argv0 = "/bin/busybox";
    pathed.req = "foo";
    pathed.via = Via::DirectPath;
    let po = define(&pathed);
    if pathed.prog() != "busybox" || message(&pathed, &po) != MAIN_ANCHOR_LIT {
        return false;
    }
    if msg_main_anchor("ls", "foo") != "ls: 'foo': applet not found" {
        return false;
    }
    // 旧版档不带名字。
    if msg_legacy_anchor("busybox") != "busybox: applet not found" {
        return false;
    }
    if has_byte(msg_legacy_anchor("busybox").as_str(), b'q') {
        return false;
    }
    // 桩档把 applet 一词放进引号之前。
    if msg_stub_anchor("busybox", "foo") != "busybox: applet 'foo' not found" {
        return false;
    }
    if msg_need_root("busybox", "ip") != "busybox: applet 'ip' requires root" {
        return false;
    }
    if msg_no_exec("busybox", "ls") != "busybox: applet 'ls' is not executable" {
        return false;
    }
    // 主锚与两个变体档必须两两不同（否则"变体"这一维不存在）。
    let main = msg_main_anchor("busybox", "foo");
    let legacy = msg_legacy_anchor("busybox");
    let stub = msg_stub_anchor("busybox", "foo");
    if main == legacy || main == stub || legacy == stub {
        return false;
    }
    // 三族共享尾串 "applet not found" / "not found"——联签面。
    if !main.ends_with("applet not found") {
        return false;
    }
    if !legacy.ends_with("applet not found") || !stub.ends_with("not found") {
        return false;
    }
    // 空名字也要成句（真机把空串照抄进引号里）。
    if msg_main_anchor("busybox", "") != "busybox: '': applet not found" {
        return false;
    }
    // 六条调用路径下同一名字得到同一结果，只有程序名前缀不同。
    let vias = [
        Via::Direct,
        Via::DirectPath,
        Via::LinkRel,
        Via::LinkAbs,
        Via::RawArgv0,
        Via::Retitle,
    ];
    let mut base: Option<Outcome> = None;
    let mut i = 0usize;
    while i < vias.len() {
        let v = match vias.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let mut s = Shape::blank();
        s.req = "foo";
        s.via = v;
        s.argv0 = match v {
            Via::Direct => "busybox",
            Via::DirectPath => "/bin/busybox",
            Via::LinkRel => "foo",
            Via::LinkAbs => "/bin/foo",
            Via::RawArgv0 => "foo",
            Via::Retitle => "@busybox",
        };
        let o = define(&s);
        if !o.default || o.stage != Stage::Code || o.value != EXIT_NOT_FOUND {
            return false;
        }
        match base {
            None => base = Some(o),
            Some(b) => {
                if b != o {
                    return false;
                }
            }
        }
        // 程序名前缀必须逐路成形。
        let want_prog = match v {
            Via::Direct | Via::DirectPath | Via::Retitle => "busybox",
            _ => "foo",
        };
        if s.prog() != want_prog {
            return false;
        }
        if message(&s, &o) != msg_main_anchor(want_prog, "foo") {
            return false;
        }
        i += 1;
    }
    // 六路必须真的都跑过，且共用同一个非零结果指纹。
    let b = match base {
        Some(o) => o,
        None => return false,
    };
    if vias.len() != 6 {
        return false;
    }
    b.seal() != 0 && b.note == Note::Anchor
}

/// 申报对判：判序与六维逐维独立可检。
fn judge_ok() -> bool {
    let cs = cases();
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let a = c.actual();
        let k = c.claim;
        let f = judge(&c);
        match expected(i) {
            Some(w) => {
                if f != w {
                    return false;
                }
            }
            None => return false,
        }
        if i < ACCEPT {
            if f != Fault::Ok {
                return false;
            }
            if !(a.stage == k.stage
                && a.code == k.code
                && a.value == k.value
                && a.note == k.note
                && a.default == k.default)
            {
                return false;
            }
        } else if f == Fault::Ok {
            return false;
        }
        // 逐维单点注入：只改一维必须落到该维对应缺陷。
        if i < ACCEPT {
            // 成功现场只能隔离 Value/Note/Default 三维：Stage 与 Code 任一处
            // 改动都会翻转申报的成败，先撞 Estate，归因就错了。
            let mut c2 = c;
            c2.claim.value = a.value.saturating_add(7);
            if judge(&c2) != Fault::Value {
                return false;
            }
            let mut c3 = c;
            c3.claim.default = !a.default;
            if judge(&c3) != Fault::Default {
                return false;
            }
            let mut c5 = c;
            c5.claim.note = if a.note == Note::Anchor {
                Note::StubText
            } else {
                Note::Anchor
            };
            if judge(&c5) != Fault::Note {
                return false;
            }
        }
        i += 1;
    }
    // Stage / Code 两维在失败现场（第 3 槽）做隔离注入。
    let fail = match case_at(3) {
        Some(c) => c,
        None => return false,
    };
    let fa = fail.actual();
    if fa.ok() || fa.stage == Stage::Ready || fa.code == 0 {
        return false;
    }
    let mut f1 = fail;
    f1.claim.stage = Stage::Lookup;
    if judge(&f1) != Fault::Stage {
        return false;
    }
    let mut f2 = fail;
    f2.claim.stage = fa.stage;
    f2.claim.code = ERR_EACCES;
    if judge(&f2) != Fault::Code {
        return false;
    }
    let mut f3 = fail;
    f3.claim.stage = fa.stage;
    f3.claim.value = fa.value.saturating_add(1);
    if judge(&f3) != Fault::Value {
        return false;
    }
    let mut f4 = fail;
    f4.claim.stage = fa.stage;
    f4.claim.default = !fa.default;
    if judge(&f4) != Fault::Default {
        return false;
    }
    // 状态互报必须优先：成功实际 + 失败申报 → Estate（不是 Stage/Note）。
    let mut good = match case_at(0) {
        Some(c) => c,
        None => return false,
    };
    if !good.actual().ok() {
        return false;
    }
    good.claim = Claim {
        stage: Stage::Code,
        code: ERR_ENOENT,
        value: 127,
        note: Note::Anchor,
        default: true,
    };
    if judge(&good) != Fault::Estate || good.claim.success() {
        return false;
    }
    // 失败实际 + 成功申报同样落 Estate，且必须盖住随后四个维的错。
    let mut rev = match case_at(3) {
        Some(c) => c,
        None => return false,
    };
    rev.claim = Claim {
        stage: Stage::Ready,
        code: 0,
        value: 3,
        note: Note::NoExec,
        default: true,
    };
    judge(&rev) == Fault::Estate && !rev.actual().ok()
}

/// 矩阵统计：档位投影、逐档计数、渲染文本三面同真。
fn matrix_ok() -> bool {
    const WANT_RENDER: &str = "run total=3 good=3 red=0\nanchor total=8 good=2 red=6\n\
        variant total=2 good=2 red=0\nmalformed total=1 good=1 red=0\n";
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
    // 只有 run / variant / malformed 三档全对；主锚档必然留六红。
    if m.perfect_rows() != 3 {
        return false;
    }
    // 统计可重放：两次建矩阵必须逐位相同（不可依赖任何隐式状态）。
    if Matrix::of() != m {
        return false;
    }
    let tiers = [Tier::Run, Tier::Anchor, Tier::Variant, Tier::Malformed];
    let mut i = 0usize;
    while i < tiers.len() {
        let t = match tiers.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if t.index() != i {
            return false;
        }
        let mut total = 0usize;
        let mut good = 0usize;
        let mut j = 0usize;
        while j < CORPUS {
            let w = match WANT_TIER.get(j) {
                Some(v) => *v,
                None => return false,
            };
            if w == t {
                total += 1;
                if j < ACCEPT {
                    good += 1;
                }
            }
            j += 1;
        }
        let r = match m.row(t) {
            Some(v) => v,
            None => return false,
        };
        if r.total != total || r.good != good {
            return false;
        }
        if good > total || r.red() != total - good {
            return false;
        }
        if r.perfect() != (total > 0 && total == good) {
            return false;
        }
        if t.success() == t.not_found_face() {
            return false;
        }
        i += 1;
    }
    // 主锚行是本卡的"红档聚集地"：八槽两对六红。
    let ar = match m.row(Tier::Anchor) {
        Some(v) => v,
        None => return false,
    };
    if ar.total != 8 || ar.good != 2 || ar.red() != REJECT {
        return false;
    }
    if m.render() != WANT_RENDER {
        return false;
    }
    // 四档文本与投影规则逐字钉住。
    if Tier::Run.name() != "run" || Tier::Anchor.name() != "anchor" {
        return false;
    }
    if Tier::Variant.name() != "variant" || Tier::Malformed.name() != "malformed" {
        return false;
    }
    let mut o = Outcome::blank();
    o.stage = Stage::Ready;
    o.default = true;
    if Tier::of(&o) != Tier::Run {
        return false;
    }
    o.stage = Stage::Lex;
    if Tier::of(&o) != Tier::Malformed {
        return false;
    }
    o.stage = Stage::Code;
    if Tier::of(&o) != Tier::Anchor {
        return false;
    }
    o.default = false;
    if Tier::of(&o) != Tier::Variant {
        return false;
    }
    // 空矩阵/空行不谎报覆盖，红档计数绝不下溢。
    let bm = Matrix::blank();
    if bm.slots() != 0 || bm.goods() != 0 || bm.covered() || bm.perfect_rows() != 0 {
        return false;
    }
    let br = Row::blank();
    if br.perfect() || br.total != 0 || br.red() != 0 {
        return false;
    }
    let over = Row { total: 2, good: 5 };
    if over.perfect() || over.red() != 2 {
        return false;
    }
    true
}

/// 语料与渲染：两张表互为第二陈述，期望表与实测算得逐位一致。
fn corpus_ok() -> bool {
    if SHAPES.len() != CORPUS || CLAIMS.len() != CORPUS {
        return false;
    }
    if WANT_STAGE.len() != CORPUS || WANT_CODE.len() != CORPUS || WANT_VALUE.len() != CORPUS {
        return false;
    }
    if WANT_NOTE.len() != CORPUS || WANT_DEFAULT.len() != CORPUS || WANT_TAGS.len() != CORPUS {
        return false;
    }
    let cs = cases();
    if cs.len() != CORPUS {
        return false;
    }
    let mut i = 0usize;
    while i < CORPUS {
        let s = match SHAPES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let k = match CLAIMS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if c.shape != s || c.claim != k {
            return false;
        }
        if c.actual() != define(&s) {
            return false;
        }
        if c.tag() != s.tag {
            return false;
        }
        let wtag = match WANT_TAGS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if c.tag() != wtag {
            return false;
        }
        // 标签唯一：两格同名就没法把账本指回语料。
        let mut j = i + 1;
        while j < CORPUS {
            if let Some(o) = WANT_TAGS.get(j) {
                if same_text(o, wtag) {
                    return false;
                }
            }
            j += 1;
        }
        let a = c.actual();
        if a.stage != *match WANT_STAGE.get(i) { Some(v) => v, None => return false } {
            return false;
        }
        if a.code != *match WANT_CODE.get(i) { Some(v) => v, None => return false } {
            return false;
        }
        if a.value != *match WANT_VALUE.get(i) { Some(v) => v, None => return false } {
            return false;
        }
        if a.note != *match WANT_NOTE.get(i) { Some(v) => v, None => return false } {
            return false;
        }
        if a.default != *match WANT_DEFAULT.get(i) { Some(v) => v, None => return false } {
            return false;
        }
        if Tier::of(&a) != *match WANT_TIER.get(i) { Some(v) => v, None => return false } {
            return false;
        }
        // 期望缺陷表与 CLAIMS 的后六段申报互相对应。
        let want = match expected(i) {
            Some(v) => v,
            None => return false,
        };
        if i < ACCEPT {
            if want != Fault::Ok {
                return false;
            }
        } else {
            match REJECT_FAULTS.get(i - ACCEPT) {
                Some(v) => {
                    if *v != want {
                        return false;
                    }
                }
                None => return false,
            }
        }
        if judge(&c) != want {
            return false;
        }
        i += 1;
    }
    // 越界访问一律 None / Err。
    if expected(CORPUS).is_some() || case_at(CORPUS).is_some() {
        return false;
    }
    if case_at(0).is_none() || case_at(CORPUS - 1).is_none() {
        return false;
    }
    if cover(CORPUS).is_err() {
        return false;
    }
    if cover(CORPUS - 1) != Err(NF_ENOBS) || cover(0) != Err(NF_ENOBS) {
        return false;
    }
    // 汇总报表：计数、比例、渲染三面对齐。
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if !r.green() || r.gate() != Ok(()) {
        return false;
    }
    if r.total != CORPUS as u32 || r.accept != ACCEPT as u32 || r.reject != REJECT as u32 {
        return false;
    }
    if r.run != 3 || r.anchor != 2 || r.variant != 2 || r.malformed != 1 {
        return false;
    }
    if r.matched != CORPUS as u32 || r.rate() != 1000 {
        return false;
    }
    if r.success_cells() != 3 || r.failure_cells() != 5 {
        return false;
    }
    if r.not_found_share() != 62 {
        return false;
    }
    if r.render() != RENDER_OK {
        return false;
    }
    // 注入一槽缺陷：matched 必掉、rate 必离开 1000、gate 必红。
    let inj = match run_with(Some((0, Fault::Code))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if inj.matched != 13 || inj.rate() != 929 || inj.green() || inj.gate() == Ok(()) {
        return false;
    }
    if inj.gate() != Err(NF_ECOUNT) {
        return false;
    }
    // 注入成"申报全对"也要掉分（后六槽的期望不是 Ok）：accept 虚增到 9、
    // reject 掉到 5，而 matched 必须离开 14——虚增的申报数救不回对判分。
    let inj2 = match run_with(Some((8, Fault::Ok))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if inj2.matched != 13 || inj2.accept != 9 || inj2.reject != 5 {
        return false;
    }
    if inj2.rate() != 929 || inj2.green() {
        return false;
    }
    if run_with(Some((CORPUS, Fault::Ok))).is_ok() {
        return false;
    }
    // 零总槽不炸，且必然判红。
    let z = Report {
        total: 0,
        accept: 0,
        reject: 0,
        run: 0,
        anchor: 0,
        variant: 0,
        malformed: 0,
        matched: 0,
    };
    if z.rate() != 0 || z.green() || z.not_found_share() != 0 {
        return false;
    }
    true
}

/// 槽位划分与缺陷码：八成的八格、六败的六格，互不重叠、码各不同。
fn slots_ok() -> bool {
    if ACCEPT + REJECT != CORPUS {
        return false;
    }
    if ACCEPT_SLOTS.len() != ACCEPT || REJECT_SLOTS.len() != REJECT {
        return false;
    }
    let mut seen = [false; CORPUS];
    let mut i = 0usize;
    while i < ACCEPT_SLOTS.len() {
        let s = match ACCEPT_SLOTS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if s >= CORPUS {
            return false;
        }
        match seen.get(s) {
            Some(v) => {
                if *v {
                    return false;
                }
            }
            None => return false,
        }
        seen[s] = true;
        if s != i {
            return false;
        }
        i += 1;
    }
    i = 0;
    while i < REJECT_SLOTS.len() {
        let s = match REJECT_SLOTS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if s >= CORPUS || s < ACCEPT {
            return false;
        }
        match seen.get(s) {
            Some(v) => {
                if *v {
                    return false;
                }
            }
            None => return false,
        }
        seen[s] = true;
        if s != ACCEPT + i {
            return false;
        }
        i += 1;
    }
    i = 0;
    while i < CORPUS {
        match seen.get(i) {
            Some(v) => {
                if !*v {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    // 六个缺陷码互不相同、均非零、且与缺陷枚举一一对应。
    i = 0;
    while i < REJECT {
        let f = match REJECT_FAULTS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let c = match REJECT_CODES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if c == 0 || f.code() != c || !f.alive() {
            return false;
        }
        if c < SEG_BASE + 1 || c > SEG_BASE + 0x0d {
            return false;
        }
        let mut j = 0usize;
        while j < i {
            if let Some(o) = REJECT_CODES.get(j) {
                if *o == c {
                    return false;
                }
            }
            if let Some(g) = REJECT_FAULTS.get(j) {
                if *g == f || same_text(fault_short(*g), fault_short(f)) {
                    return false;
                }
            }
            j += 1;
        }
        let cc = match case_at(ACCEPT + i) {
            Some(v) => v,
            None => return false,
        };
        if judge(&cc) != f {
            return false;
        }
        // 除 Estate 格外全走单维误报；Estate 格必须多维同失（整档互报）。
        if i == 0 {
            if diff_dims(&cc) < 2 {
                return false;
            }
        } else if diff_dims(&cc) != 1 {
            return false;
        }
        i += 1;
    }
    // 全谱缺陷枚举：码唯一、短名唯一、Ok 是唯一"非活"档。
    let all = [
        Fault::Ok,
        Fault::Stage,
        Fault::Code,
        Fault::Value,
        Fault::Note,
        Fault::Default,
        Fault::Via,
        Fault::Estate,
        Fault::Count,
        Fault::Slot,
        Fault::Seal,
        Fault::Nobs,
        Fault::Matrix,
        Fault::Bal,
    ];
    i = 0;
    while i < all.len() {
        let f = match all.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if f.alive() != (f != Fault::Ok) {
            return false;
        }
        if fault_short(f).is_empty() {
            return false;
        }
        let mut j = 0usize;
        while j < all.len() {
            if i != j {
                if let Some(o) = all.get(j) {
                    // 同码只允许同档：不同档撞码即失格；短名同理。
                    if (f.code() == o.code()) != (f == *o) {
                        return false;
                    }
                    if same_text(fault_short(*o), fault_short(f)) != (f == *o) {
                        return false;
                    }
                }
            }
            j += 1;
        }
        i += 1;
    }
    if Fault::Ok.code() != 0 {
        return false;
    }
    true
}

/// 红档显式：六格误报现场同档、单维可分，Estate 优先且只归整档互报。
fn reds_ok() -> bool {
    let mut base: Option<Outcome> = None;
    let mut i = 0usize;
    while i < REJECT {
        let c = match case_at(ACCEPT + i) {
            Some(v) => v,
            None => return false,
        };
        let a = c.actual();
        // 六条路走歪，现场本身一模一样：Code / ENOENT / 127 / 主锚 / 逐字。
        if a.stage != Stage::Code
            || a.code != ERR_ENOENT
            || a.value != EXIT_NOT_FOUND
            || a.note != Note::Anchor
            || !a.default
        {
            return false;
        }
        match base {
            None => base = Some(a),
            Some(x) => {
                if x != a {
                    return false;
                }
            }
        }
        if Tier::of(&a) != Tier::Anchor {
            return false;
        }
        let f = judge(&c);
        let want = match expected(ACCEPT + i) {
            Some(v) => v,
            None => return false,
        };
        if f == Fault::Ok || f != want || !f.alive() {
            return false;
        }
        i += 1;
    }
    let b = match base {
        Some(v) => v,
        None => return false,
    };
    if b.seal() == 0 || !b.not_found() || b.ok() || !b.failed() {
        return false;
    }
    // 后五格是单维误报；第一格（Estate）是整档互报，多维同时不同。
    i = 1;
    while i < REJECT {
        let c = match case_at(ACCEPT + i) {
            Some(v) => v,
            None => return false,
        };
        if diff_dims(&c) != 1 {
            return false;
        }
        if judge(&c) == Fault::Estate {
            return false;
        }
        i += 1;
    }
    let e = match case_at(ACCEPT) {
        Some(v) => v,
        None => return false,
    };
    if diff_dims(&e) < 2 || judge(&e) != Fault::Estate {
        return false;
    }
    if !e.claim.success() || e.actual().ok() {
        return false;
    }
    // 矩阵里六红全落主锚行；报表 reject 恰为六。
    let m = Matrix::of();
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    match m.row(Tier::Anchor) {
        Some(row) => {
            if row.red() != REJECT || row.good != ACCEPT - REJECT {
                return false;
            }
        }
        None => return false,
    }
    if r.reject as usize != REJECT || r.matched != CORPUS as u32 {
        return false;
    }
    // 逐格注入成 Ok：每格都要掉一分，证明 matched 不是硬编码。
    i = 0;
    while i < REJECT {
        let slot = match REJECT_SLOTS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let inj = match run_with(Some((slot, Fault::Ok))) {
            Ok(v) => v,
            Err(_) => return false,
        };
        if inj.matched != CORPUS as u32 - 1 || inj.green() {
            return false;
        }
        i += 1;
    }
    true
}

/// 一槽申报与实际结果相差几维。
fn diff_dims(c: &Case) -> usize {
    let a = c.actual();
    let k = c.claim;
    let mut n = 0usize;
    if a.stage != k.stage {
        n += 1;
    }
    if a.code != k.code {
        n += 1;
    }
    if a.value != k.value {
        n += 1;
    }
    if a.note != k.note {
        n += 1;
    }
    if a.default != k.default {
        n += 1;
    }
    n
}

/// 文案面：三族逐字、成功档零错行、流向与家族互证。
fn message_ok() -> bool {
    if MAIN_ANCHOR_LIT != "busybox: 'foo': applet not found" {
        return false;
    }
    if msg_main_anchor("busybox", "foo") != MAIN_ANCHOR_LIT {
        return false;
    }
    // 只有现代主锚以句点收尾——旧版与桩档都没有句点，真机如此。
    // 真机的三条 notfound 行都不带句号：主锚以引号+冒号起名，旧版档不带
    // 名字（也就没有任何引号），桩档把 applet 一词提到名字之前。
    if msg_main_anchor("busybox", "foo") != "busybox: 'foo': applet not found" {
        return false;
    }
    if has_byte(msg_legacy_anchor("busybox").as_str(), b'\'') {
        return false;
    }
    if msg_stub_anchor("busybox", "foo") != "busybox: applet 'foo' not found" {
        return false;
    }
    if msg_main_anchor("busybox", "foo").ends_with('.') || msg_stub_anchor("busybox", "foo").ends_with('.')
    {
        return false;
    }
    if !msg_need_root("busybox", "ip").contains("requires root")
        || !msg_no_exec("busybox", "ls").contains("not executable")
    {
        return false;
    }
    // 三族共享 "applet" 一词，差别只在引号位置与是否带名字。
    let t0 = msg_main_anchor("busybox", "foo");
    let t1 = msg_legacy_anchor("busybox");
    let t2 = msg_stub_anchor("busybox", "foo");
    let trio = [t0.as_str(), t1.as_str(), t2.as_str()];
    let mut i = 0usize;
    while i < trio.len() {
        let m = match trio.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if !m.contains("applet") || !m.contains("not found") {
            return false;
        }
        if !m.starts_with("busybox:") || !has_byte(m, b' ') {
            return false;
        }
        i += 1;
    }
    // 旧版不带名字：主锚的名字绝不能出现在旧版文案里。
    if msg_legacy_anchor("busybox").contains("foo") {
        return false;
    }
    // 名字原样照抄：真机不转义引号与控制字符，也不做 basename。
    if msg_main_anchor("busybox", "a'b") != "busybox: 'a'b': applet not found" {
        return false;
    }
    if msg_main_anchor("busybox", "") != "busybox: '': applet not found" {
        return false;
    }
    // 逐槽：文案与注解家族必须互证，成功档一律零错行。
    let cs = cases();
    i = 0;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let a = c.actual();
        let prog = c.shape.prog();
        let key = c.shape.key();
        let m = message(&c.shape, &a);
        if m.is_empty() != a.note.success() {
            return false;
        }
        if !m.is_empty() && !m.starts_with(prog) {
            return false;
        }
        // 伪装形态的 '@' 只改 argv[0]，绝不进文案。
        if has_at_prefix(m.as_str()) {
            return false;
        }
        let want = match a.note {
            Note::Ran | Note::AliasRan | Note::ListDump | Note::UsageDump => String::new(),
            Note::Anchor | Note::BadName => msg_main_anchor(prog, key),
            Note::LegacyText => msg_legacy_anchor(prog),
            Note::StubText => msg_stub_anchor(prog, key),
            Note::NeedRoot => msg_need_root(prog, key),
            Note::NoExec => msg_no_exec(prog, key),
        };
        if m != want {
            return false;
        }
        if a.note.text_variant() && a.default {
            return false;
        }
        if a.default && (a.note.family() != Anchor::Main || c.shape.stream() != Stream::Stderr) {
            return false;
        }
        if a.note.family() == Anchor::Main && !m.contains("applet not found") {
            return false;
        }
        if a.failed() && !a.note.success() && !has_byte(m.as_str(), b':') {
            return false;
        }
        i += 1;
    }
    // 四种信息形态：走 stdout、退 0、无错行。
    let infos = [Special::List, Special::ListFull, Special::Help, Special::Version];
    i = 0;
    while i < infos.len() {
        let sp = match infos.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let mut s = Shape::blank();
        s.tag = "info-shape";
        s.req = "--x";
        s.special = sp;
        let o = define(&s);
        if o.stage != Stage::Ready || o.value != EXIT_RUN_OK || o.code != ERR_NONE {
            return false;
        }
        if !sp.informational() || !sp.skips_lookup() {
            return false;
        }
        if !message(&s, &o).is_empty() || s.stream() != Stream::Stdout {
            return false;
        }
        if !o.ok() || o.default {
            return false;
        }
        if o.note != match sp {
            Special::Help | Special::Version => Note::UsageDump,
            _ => Note::ListDump,
        } {
            return false;
        }
        i += 1;
    }
    // 未知二长开关：判死在分发段，文案仍主锚但逐字位为假。
    let mut sw = Shape::blank();
    sw.tag = "switch";
    sw.req = "--install";
    sw.special = Special::UnknownSwitch;
    let so = define(&sw);
    if so.stage != Stage::Dispatch
        || so.code != ERR_EINVAL
        || so.value != EXIT_NOT_FOUND
        || so.note != Note::BadName
        || so.default
    {
        return false;
    }
    if message(&sw, &so) != msg_main_anchor("busybox", "--install") {
        return false;
    }
    // 串台：文案仍是主锚，但流向判死在 stream 段。
    let mut lk = Shape::blank();
    lk.tag = "leak";
    lk.req = "foo";
    lk.leak = true;
    let lo = define(&lk);
    if lo.stage != Stage::Stream
        || lo.note != Note::Anchor
        || lo.default
        || lk.stream() != Stream::Split
    {
        return false;
    }
    if lk.stream().valid_for_error() {
        return false;
    }
    if message(&lk, &lo) != MAIN_ANCHOR_LIT {
        return false;
    }
    // 变体档走 text 段：文案成形即判死，不再进码档。
    let mut lg = Shape::blank();
    lg.tag = "legacy";
    lg.req = "bash";
    lg.flavor = Flavor::Legacy;
    let o = define(&lg);
    if o.stage != Stage::Text || o.note != Note::LegacyText || lg.stream() != Stream::Stderr {
        return false;
    }
    if lg.flavor.quotes_name() {
        return false;
    }
    if message(&lg, &o) != "busybox: applet not found" {
        return false;
    }
    // 口袋清单是注册表的第二陈述：条数、顺序、分隔符三面同真。
    let reg = registry();
    let mut names = 0usize;
    let mut joined = String::new();
    let mut k = 0usize;
    while k < reg.len() {
        if let Some(a) = reg.get(k) {
            if a.is_primary() && a.dispatchable() {
                if !joined.is_empty() {
                    joined.push(',');
                }
                joined.push_str(a.name);
                names += 1;
            }
        }
        k += 1;
    }
    let list = applet_list(",");
    if list != joined || names != 18 || list.is_empty() {
        return false;
    }
    if !list.starts_with("sh,busybox,ls,") {
        return false;
    }
    if applet_list("").contains(',') {
        return false;
    }
    if text_len(applet_list("").as_str()) != text_len(list.as_str()) - 17 {
        return false;
    }
    if applet_list(" ").contains(',') || !applet_list(" ").contains(" noapplet") {
        return false;
    }
    true
}

/// 释义面：逐槽文本成形、计数文本无依赖、字段顺序固定。
fn detail_ok() -> bool {
    let cs = cases();
    // 第 3 槽的释义逐字钉住（文案、码档、注解三者同时进一行）。
    let c3 = match cs.get(3) {
        Some(v) => *v,
        None => return false,
    };
    if detail(&c3)
        != "busybox/foo stage=code code=2 exit=127 anchor=true note=main-anchor msg=\"busybox: 'foo': applet not found\"\n"
    {
        return false;
    }
    let c0 = match cs.get(0) {
        Some(v) => *v,
        None => return false,
    };
    if detail(&c0) != "busybox/sh stage=ready code=0 exit=0 anchor=false note=ran\n" {
        return false;
    }
    let mut i = 0usize;
    let mut total = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let a = c.actual();
        let d = detail(&c);
        if d.is_empty() || !d.ends_with('\n') {
            return false;
        }
        let mut nl = 0usize;
        let mut j = 0usize;
        while j < d.len() {
            if let Some(b) = d.as_bytes().get(j) {
                if *b == b'\n' {
                    nl += 1;
                }
            }
            j += 1;
        }
        if nl != 1 {
            return false;
        }
        total += 1;
        let mut head = String::from(c.shape.prog());
        head.push('/');
        head.push_str(c.shape.key());
        if !d.starts_with(head.as_str()) {
            return false;
        }
        // 六个字段的次序固定：stage → code → exit → anchor → note →（msg）。
        let keys = [" stage=", " code=", " exit=", " anchor=", " note=", " msg=\""];
        let mut pos = 0usize;
        let mut g = 0usize;
        while g < keys.len() {
            let key = match keys.get(g) {
                Some(v) => *v,
                None => return false,
            };
            if g == 5 && message(&c.shape, &a).is_empty() {
                break;
            }
            match find_from(d.as_str(), key, pos) {
                Some(p) => pos = p + text_len(key),
                None => return false,
            }
            g += 1;
        }
        // 字段值本身也要逐字出现（次序之外的第二重约束）。
        if !d.contains(a.stage.name()) || !d.contains(a.note.name()) {
            return false;
        }
        if !d.contains(i32_text(a.code).as_str()) || !d.contains(bool_text(a.default)) {
            return false;
        }
        if !d.contains(u32_text(a.value as u32).as_str()) {
            return false;
        }
        if WANT_TAGS.get(i).is_none() {
            return false;
        }
        if d.contains(':') != !message(&c.shape, &a).is_empty() {
            return false;
        }
        i += 1;
    }
    if total != CORPUS {
        return false;
    }
    // 计数文本：零、个位、多位、u32 上界四态齐备，且无前导零。
    if u32_text(0) != "0" || u32_text(7) != "7" || u32_text(127) != "127" {
        return false;
    }
    if u32_text(1000) != "1000" || u32_text(929) != "929" {
        return false;
    }
    if u32_text(u32::MAX) != "4294967295" {
        return false;
    }
    if i32_text(0) != "0" || i32_text(2) != "2" || i32_text(-22) != "-22" {
        return false;
    }
    if i32_text(i32::MIN) != "-2147483648" {
        return false;
    }
    if hex_text(0x6d100, 5) != "6d100" || hex_text(0x0, 4) != "0000" {
        return false;
    }
    if hex_text(0xabcd, 8) != "0000abcd" {
        return false;
    }
    if bool_text(true) != "true" || bool_text(false) != "false" {
        return false;
    }
    true
}

/// 阶段闸门：七段只许向前，同段可重复，关闭后既不可再走也不可再关。
fn gate_ok() -> bool {
    let seq = [
        Stage::Lex,
        Stage::Dispatch,
        Stage::Lookup,
        Stage::Text,
        Stage::Stream,
        Stage::Code,
        Stage::Ready,
    ];
    if seq.len() != 7 {
        return false;
    }
    // 段序与段名逐字。
    let mut i = 0usize;
    while i < seq.len() {
        let s = match seq.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if s.order() as usize != i {
            return false;
        }
        if s.failure() != (i < 6) {
            return false;
        }
        let mut j = 0usize;
        while j < i {
            if let Some(o) = seq.get(j) {
                if o.order() >= s.order() || same_text(o.name(), s.name()) {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    if Stage::Lex.name() != "lex" || Stage::Dispatch.name() != "dispatch" {
        return false;
    }
    if Stage::Lookup.name() != "lookup" || Stage::Text.name() != "text" {
        return false;
    }
    if Stage::Stream.name() != "stream" || Stage::Code.name() != "code" {
        return false;
    }
    if Stage::Ready.name() != "ready" {
        return false;
    }
    // 顺走七段：段数递增、末段记忆、未关。
    let mut g = Gate::new();
    if g.steps() != 0 || g.last() != -1 || g.closed() || g.step(Stage::Lex).is_err() {
        return false;
    }
    if g.steps() != 1 || g.last() != 0 {
        return false;
    }
    i = 1;
    while i < seq.len() {
        let s = match seq.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if g.step(s).is_err() {
            return false;
        }
        if g.steps() != (i + 1) as u32 || g.last() != s.order() as i32 {
            return false;
        }
        i += 1;
    }
    if g.closed() {
        return false;
    }
    // 同段重复允许（非严格递增），但回退必判 ESTAGE。
    if g.step(Stage::Ready).is_err() {
        return false;
    }
    let mut back = Gate::new();
    if back.step(Stage::Ready).is_err() || back.step(Stage::Lookup) != Err(NF_ESTAGE) {
        return false;
    }
    if back.steps() != 1 || back.last() != 6 {
        return false;
    }
    if back.step(Stage::Lex) != Err(NF_ESTAGE) {
        return false;
    }
    // 关闭：一次成功返回段数，二次与关闭后走段一律 ESTATE。
    let mut cl = Gate::new();
    if cl.step(Stage::Lex).is_err() || cl.step(Stage::Code).is_err() {
        return false;
    }
    if cl.close() != Ok(2) || !cl.closed() {
        return false;
    }
    if cl.step(Stage::Ready).is_ok() || cl.close() != Err(NF_ESTATE) {
        return false;
    }
    if cl.step(Stage::Lex).is_ok() || cl.steps() != 2 {
        return false;
    }
    // 空闸门也能正常关闭（0 段）。
    let mut z = Gate::new();
    if z.close() != Ok(0) || !z.closed() {
        return false;
    }
    // 语料七闸的实际落段必须与 WANT_STAGE 一致（闸门不可倒）。
    let cs = cases();
    i = 0;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let a = c.actual();
        if a.stage != Stage::Ready && !a.stage.failure() {
            return false;
        }
        let mut walk = Gate::new();
        // 现场落段之前的每一段都必须能单调走过去。
        let mut k = 0u8;
        while k <= a.stage.order() {
            let s = match k {
                0 => Stage::Lex,
                1 => Stage::Dispatch,
                2 => Stage::Lookup,
                3 => Stage::Text,
                4 => Stage::Stream,
                5 => Stage::Code,
                _ => Stage::Ready,
            };
            if walk.step(s).is_err() {
                return false;
            }
            k += 1;
        }
        if walk.steps() != (a.stage.order() + 1) as u32 {
            return false;
        }
        i += 1;
    }
    true
}

/// 账本面：槽位严格递增、逐格对账、封印拒登记、重放同指纹。
fn ledger_ok() -> bool {
    if SAMPLES != CORPUS || CAP > SAMPLES {
        return false;
    }
    let l0 = Ledger::new();
    if l0.len() != 0 || !l0.is_empty() || l0.ok_count() != 0 || l0.err_count() != 0 {
        return false;
    }
    if l0.audit() != Err(NF_EBAL) || l0.sample(0).is_some() || l0.rejects() != 0 {
        return false;
    }
    if l0.is_frozen() || l0.root() != 0 || l0.seal() == 0 {
        // 空账也有指纹（FNV 初值），但绝不能被当成"已封印"。
        return false;
    }
    // 槽位不等于当前长度 → 拒登记并计数。
    let mut f = Ledger::new();
    let jump = Sample {
        slot: 1,
        op: OP_CORPUS,
        ok: true,
        code: 0,
        value: 14,
    };
    if f.register(jump).is_ok() || f.rejects() != 1 || f.len() != 0 {
        return false;
    }
    // 正格登记。
    let good = count_cell(0, OP_CORPUS, Ok(14));
    if !good.ok || good.value != 14 || good.code != 0 || good.seal() == 0 {
        return false;
    }
    if f.register(good).is_err() || f.len() != 1 || f.rejects() != 1 {
        return false;
    }
    // 同槽重复登记必须被拒。
    if f.register(good).is_ok() || f.rejects() != 2 {
        return false;
    }
    // 败格：值必须清零、码必须带上。
    let bad = count_cell(1, OP_CODE, Err(NF_ECODE));
    if bad.ok || bad.value != 0 || bad.code != NF_ECODE {
        return false;
    }
    if f.register(bad).is_err() || f.ok_count() != 1 || f.err_count() != 1 {
        return false;
    }
    // 越界槽位一律拒。
    let over = Sample {
        slot: SAMPLES as u8,
        op: OP_REPLAY,
        ok: true,
        code: 0,
        value: 0,
    };
    if f.register(over).is_ok() || f.rejects() != 3 || f.len() != 2 {
        return false;
    }
    // 半成品账本的对账必须红（长度不足）。
    if f.audit() != Err(NF_EBAL) {
        return false;
    }
    // 正式建账：14 格 8 成 6 败。
    let b = build_ledger();
    if b.len() != SAMPLES || b.ok_count() != ACCEPT || b.err_count() != REJECT {
        return false;
    }
    if b.is_empty() || b.rejects() != 0 || b.audit() != Ok(()) {
        return false;
    }
    if b.is_frozen() || b.root() != 0 {
        return false;
    }
    // 逐格对第二份陈述表：成格对 VALS、败格对 REJECT_CODES，槽位严格递增。
    let want_ops = [
        OP_CORPUS,
        OP_CORPUS,
        OP_CORPUS,
        OP_DISPATCH,
        OP_LOOKUP,
        OP_TEXT,
        OP_LEX,
        OP_REPLAY,
    ];
    let mut vi = 0usize;
    let mut ci = 0usize;
    let mut i = 0usize;
    while i < SAMPLES {
        let s = match b.sample(i) {
            Some(v) => v,
            None => return false,
        };
        if s.slot as usize != i {
            return false;
        }
        if s.ok {
            if s.code != 0 {
                return false;
            }
            let w = match VALS.get(vi) {
                Some(v) => *v,
                None => return false,
            };
            if s.value != w {
                return false;
            }
            if vi < ACCEPT {
                if let Some(o) = want_ops.get(vi) {
                    if s.op != *o {
                        return false;
                    }
                } else {
                    return false;
                }
            }
            vi += 1;
        } else {
            if s.value != 0 || s.op != OP_CODE {
                return false;
            }
            let w = match REJECT_CODES.get(ci) {
                Some(v) => *v,
                None => return false,
            };
            if s.code != w {
                return false;
            }
            ci += 1;
        }
        if s.seal() == 0 {
            return false;
        }
        i += 1;
    }
    if vi != ACCEPT || ci != REJECT {
        return false;
    }
    // 冻结：指纹非零、置位、重放同指纹。
    let mut fr = build_ledger();
    let root = match fr.freeze() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if root == 0 || !fr.is_frozen() || fr.root() != root || fr.seal() != root {
        return false;
    }
    if fr.replay() != Ok(root) {
        return false;
    }
    // 冻结后：登记与二次冻结都被拒且各自计数。
    if fr.register(count_cell(SAMPLES as u8, OP_CORPUS, Ok(1))).is_ok() || fr.rejects() != 1 {
        return false;
    }
    if fr.freeze().is_ok() || fr.rejects() != 2 || fr.freeze() != Err(NF_EBAL) {
        return false;
    }
    if fr.rejects() != 3 || !fr.is_frozen() {
        return false;
    }
    // 篡改一格：重放必须判指纹错，而不是悄悄通过。
    let mut t = Ledger::new();
    if t.register(count_cell(0, OP_CORPUS, Ok(13))).is_err() {
        return false;
    }
    if t.replay() != Err(NF_ESEAL) {
        return false;
    }
    // 空白格的指纹也不能与真实格混同。
    if Sample::blank().seal() == good.seal() {
        return false;
    }
    if Sample::blank().ok || Sample::blank().code != 0 {
        return false;
    }
    true
}

// ---------------------------------------------------------------------------
// 判据入口（13 判据）
// ---------------------------------------------------------------------------

/// 跑本卡全部 13 条判据。
///
/// 判据次序就是纵线次序：先名称、再枚举、再闸门、再文案、再对判、再统计、
/// 最后账本与冻结重放。任何一条红都要能单独指回一个维度，所以每条只挂
/// 一组互不重叠的谓词。
pub fn run_unx11661_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11661_bb_notfound");

    s.add("BN01-名称与段基址", name_ok(), "static");

    s.add("BN02-枚举与纵线", enums_ok(), "static");

    s.add("BN03-注册表与查表", registry_ok(), "static");

    s.add("BN04-调用路径六式", via_ok(), "static");

    s.add("BN05-七闸纵线", define_ok(), "static");

    s.add("BN06-主锚与变体档", anchor_ok(), "static");

    s.add("BN07-申报对判", judge_ok(), "static");

    s.add("BN08-矩阵统计", matrix_ok(), "static");

    s.add("BN09-语料与渲染", corpus_ok(), "static");

    s.add("BN10-红档显式", reds_ok() && slots_ok(), "static");

    s.add("BN11-文案逐字", message_ok(), "static");

    s.add("BN12-释义与账本", detail_ok() && ledger_ok() && gate_ok(), "static");

    s.add(
        "BN13-冻结重放四注入",
        {
            let mut l = build_ledger();
            let a = l.audit() == Ok(());
            let b = match l.freeze() {
                Ok(r) => r != 0 && l.is_frozen() && l.replay() == Ok(r),
                Err(_) => false,
            };
            let inj = inject_four();
            a && b && inj.iter().all(|v| *v)
        },
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 分隔符计数：清单到底有几项，只能自己数——`split` 在 no_std 下
    /// 依赖 alloc 的 Pattern，判据里不想掺这种不确定性。
    fn item_count(list: &str, sep: u8) -> usize {
        if list.is_empty() {
            return 0;
        }
        let b = list.as_bytes();
        let mut n = 1usize;
        let mut i = 0usize;
        while i < b.len() {
            if let Some(c) = b.get(i) {
                if *c == sep {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 带分隔符边界的成员判定：`ping` 绝不该因为含 `in` 而被判成在表里。
    fn member(list: &str, sep: char, name: &str) -> bool {
        if list.is_empty() || name.is_empty() {
            return false;
        }
        let mut p = String::new();
        p.push(sep);
        p.push_str(list);
        p.push(sep);
        let mut w = String::new();
        w.push(sep);
        w.push_str(name);
        w.push(sep);
        find_from(p.as_str(), w.as_str(), 0).is_some()
    }

    /// 一槽的现场摘要（只用公开维度，便于逐槽核对）。
    fn scene(c: &Case) -> (Stage, i32, u16, Note, bool) {
        let a = c.actual();
        (a.stage, a.code, a.value, a.note, a.default)
    }

    /// 换行计数：渲染文本的行数必须自己数出来。
    fn nl_count(s: &str) -> usize {
        let b = s.as_bytes();
        let mut n = 0usize;
        let mut i = 0usize;
        while i < b.len() {
            if let Some(c) = b.get(i) {
                if *c == b'\n' {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    #[test]
    fn seg_base_and_cardinalities() {
        assert_eq!(SEG_BASE, 0x6d100);
        assert_eq!(CORPUS, 14);
        assert_eq!(SAMPLES, 14);
        assert_eq!(ACCEPT, 8);
        assert_eq!(REJECT, 6);
        assert_eq!(TIER_COUNT, 4);
        assert_eq!(APLET_COUNT, 30);
        assert_eq!(ACCEPT + REJECT, CORPUS);
        assert_eq!(VALS.len(), ACCEPT);
        assert_eq!(REJECT_CODES.len(), REJECT);
        assert_eq!(REJECT_FAULTS.len(), REJECT);
        assert_eq!(VALS.last(), Some(&929u32));
        assert!(name_ok());
    }

    #[test]
    fn error_codes_are_a_distinct_ladder() {
        let codes = [
            NF_ESTAGE, NF_ECODE, NF_EVALUE, NF_ENOTE, NF_EDEFAULT, NF_EVIA, NF_ESTATE, NF_ECOUNT,
            NF_ESLOT, NF_ESEAL, NF_ENOBS, NF_EMATRIX, NF_EBAL,
        ];
        assert_eq!(codes.len(), 13);
        for (i, c) in codes.iter().enumerate() {
            assert_eq!(*c, SEG_BASE + (i as u32) + 1);
        }
        let mut a = 0usize;
        while a < codes.len() {
            let mut b = a + 1;
            while b < codes.len() {
                if let (Some(x), Some(y)) = (codes.get(a), codes.get(b)) {
                    assert_ne!(*x, *y);
                }
                b += 1;
            }
            a += 1;
        }
        assert_eq!(Fault::Stage.code(), NF_ESTAGE);
        assert_eq!(Fault::Default.code(), NF_EDEFAULT);
        assert_eq!(Fault::Via.code(), NF_EVIA);
        assert_eq!(Fault::Ok.code(), 0);
        assert!(!Fault::Ok.alive());
        assert!(Fault::Bal.alive());
        assert_eq!(fault_short(Fault::Nobs), "nobs");
        assert_eq!(fault_short(Fault::Estate), "estate");
        assert_eq!(fault_short(Fault::Seal), "seal");
    }

    #[test]
    fn exit_code_and_errno_ladder() {
        assert_eq!(EXIT_NOT_FOUND, 127);
        assert_eq!(EXIT_NO_EXEC, 126);
        assert_eq!(EXIT_NEED_ROOT, 1);
        assert_eq!(EXIT_USAGE, 1);
        assert_eq!(EXIT_RUN_OK, 0);
        assert_eq!(EXIT_RUN_FAIL, 1);
        assert_eq!(ERR_ENOENT, 2);
        assert_eq!(ERR_EACCES, 13);
        assert_eq!(ERR_EINVAL, 22);
        assert_eq!(ERR_ENAMETOOLONG, 36);
        assert_eq!(ERR_ENOSYS, 38);
        assert_eq!(ERR_NONE, 0);
        assert_eq!(NameKind::Overlong.errno(), ERR_ENAMETOOLONG);
        assert_eq!(NameKind::Ok.errno(), ERR_NONE);
        assert_eq!(NameKind::HasSlash.errno(), ERR_EINVAL);
        assert_eq!(NameKind::Empty.errno(), ERR_EINVAL);
        assert_eq!(NameKind::Dots.errno(), ERR_EINVAL);
    }

    #[test]
    fn main_anchor_is_verbatim() {
        assert_eq!(MAIN_ANCHOR_LIT, "busybox: 'foo': applet not found");
        assert_eq!(msg_main_anchor("busybox", "foo").as_str(), MAIN_ANCHOR_LIT);
        assert_eq!(msg_legacy_anchor("busybox").as_str(), "busybox: applet not found");
        assert_eq!(msg_stub_anchor("busybox", "foo").as_str(), "busybox: applet 'foo' not found");
        assert_eq!(
            msg_need_root("busybox", "ip").as_str(),
            "busybox: applet 'ip' requires root"
        );
        assert_eq!(
            msg_no_exec("busybox", "ls").as_str(),
            "busybox: applet 'ls' is not executable"
        );
        assert_eq!(
            msg_main_anchor("/bin/busybox", "foo").as_str(),
            "/bin/busybox: 'foo': applet not found"
        );
        assert_eq!(msg_main_anchor("busybox", "").as_str(), "busybox: '': applet not found");
        assert!(!msg_legacy_anchor("busybox").contains("foo"));
        assert!(!has_byte(msg_legacy_anchor("busybox").as_str(), b'\''));
        assert!(!msg_main_anchor("busybox", "foo").ends_with('.'));
    }

    #[test]
    fn fnv_helpers_are_stable_and_sensitive() {
        assert_eq!(fold_str(FNV_OFF, "ls"), fold_str(FNV_OFF, "ls"));
        assert_ne!(fold_str(FNV_OFF, "ls"), fold_str(FNV_OFF, "sl"));
        assert_ne!(fold_str(FNV_OFF, "ls"), FNV_OFF);
        assert_ne!(fold_byte(FNV_OFF, 7), fold_byte(FNV_OFF, 8));
        assert_ne!(fold_word(FNV_OFF, 1), fold_word(FNV_OFF, 256));
        assert_eq!(fold_word(FNV_OFF, 0), fold_word(FNV_OFF, 0));
        assert_eq!(name_hash(""), 5381);
        assert_eq!(name_hash("busybox"), name_hash("busybox"));
        assert_ne!(name_hash("ls"), name_hash("sl"));
    }

    #[test]
    fn number_text_helpers() {
        assert_eq!(dec_text(0).as_str(), "0");
        assert_eq!(dec_text(929).as_str(), "929");
        assert_eq!(u32_text(4294967295).as_str(), "4294967295");
        assert_eq!(i32_text(0).as_str(), "0");
        assert_eq!(i32_text(-2).as_str(), "-2");
        assert_eq!(i32_text(i32::MIN).as_str(), "-2147483648");
        assert_eq!(hex_text(0x6d100, 8).as_str(), "0006d100");
        assert_eq!(bool_text(true), "true");
        assert_eq!(bool_text(false), "false");
    }

    #[test]
    fn byte_primitives() {
        assert!(same_text("busybox", "busybox"));
        assert!(!same_text("busybox", "Busybox"));
        assert!(!same_text("ab", "abc"));
        assert_eq!(text_len("applet not found"), 16);
        assert!(has_byte("a/b", b'/'));
        assert!(!has_byte("abc", b'/'));
        assert_eq!(find_from("busybox foo", "foo", 0), Some(8));
        assert_eq!(find_from("busybox foo", "foo", 9), None);
        assert_eq!(find_from("abc", "", 0), None);
        assert_eq!(find_from("abc", "abcd", 0), None);
        assert_eq!(find_from("applet not found", "not found", 0), Some(7));
    }

    #[test]
    fn basename_and_at_prefix_rules() {
        assert_eq!(last_component("/bin/busybox"), "busybox");
        assert_eq!(last_component("ls"), "ls");
        assert_eq!(last_component("foo/"), "");
        assert_eq!(last_component("/"), "");
        assert_eq!(last_component("/usr/bin/less"), "less");
        assert_eq!(dir_prefix("/bin/busybox"), Some("/bin"));
        assert_eq!(dir_prefix("busybox"), None);
        assert_eq!(slash_shape("/bin/ls"), (true, false));
        assert_eq!(slash_shape("ls/"), (true, true));
        assert_eq!(slash_shape("ls"), (false, false));
        assert!(has_control("a\tb"));
        assert!(!has_control("applet"));
        assert!(is_dots(".."));
        assert!(!is_dots(""));
        assert!(!is_dots(".sh"));
        assert!(has_at_prefix("@busybox"));
        assert!(!has_at_prefix("busybox"));
        assert_eq!(strip_at("@busybox"), "busybox");
        assert_eq!(strip_at("busybox"), "busybox");
    }

    #[test]
    fn registry_counts_match_the_build() {
        let reg = registry();
        assert_eq!(reg.len(), APLET_COUNT);
        let mut prim = 0usize;
        let mut alias = 0usize;
        let mut trimmed = 0usize;
        let mut hidden = 0usize;
        let mut dispatchable = 0usize;
        let mut live_primary = 0usize;
        let mut i = 0usize;
        while i < reg.len() {
            if let Some(a) = reg.get(i) {
                if a.is_primary() {
                    prim += 1;
                    if a.dispatchable() {
                        live_primary += 1;
                    }
                } else {
                    alias += 1;
                }
                if !a.built {
                    trimmed += 1;
                }
                if a.hidden {
                    hidden += 1;
                }
                if a.dispatchable() {
                    dispatchable += 1;
                }
            }
            i += 1;
        }
        assert_eq!(prim, 26);
        assert_eq!(alias, 4);
        assert_eq!(trimmed, 7);
        assert_eq!(hidden, 2);
        assert_eq!(live_primary, 18);
        assert_eq!(dispatchable, 21);
        assert_eq!(prim + alias, APLET_COUNT);
        assert!(registry_ok());
    }

    #[test]
    fn applet_predicates_on_real_rows() {
        if let Some(a) = applet_at("sh") {
            assert!(a.is_primary());
            assert!(a.wired());
            assert!(a.dispatchable());
            assert_eq!(a.main, "sh_main");
            assert_eq!(a.prio, 0);
            assert_eq!(a.section, "busybox");
        } else {
            panic!("表里必须有 sh");
        }
        if let Some(a) = applet_at("ash") {
            assert!(a.is_alias());
            assert!(!a.wired());
            assert!(a.dispatchable());
            assert_eq!(a.alias_of, "sh");
        } else {
            panic!("表里必须有 ash");
        }
        if let Some(a) = applet_at("editor") {
            assert!(a.is_alias());
            assert!(!a.built);
            assert!(!a.dispatchable());
        } else {
            panic!("表里必须有 editor");
        }
        assert!(applet_at("foo").is_none());
        assert_eq!(applet_index("sh"), Some(0));
        assert_eq!(applet_index("noapplet"), Some(29));
        assert_eq!(applet_index("foo"), None);
        assert_eq!(Applet::blank().prio, 0);
        assert!(Applet::blank().is_primary());
    }

    #[test]
    fn applet_seal_and_render_are_field_sensitive() {
        let ls = applet_at("ls");
        let vi = applet_at("vi");
        assert!(ls.is_some());
        assert!(vi.is_some());
        if let (Some(a), Some(b)) = (ls, vi) {
            assert_ne!(a.seal(), 0);
            assert_eq!(a.seal(), a.seal());
            assert_ne!(a.seal(), b.seal());
            assert!(a.render().contains(" [coreutils] "));
            assert!(a.render().contains("main=ls_main"));
            assert!(b.render().contains("trimmed"));
            assert!(!b.render().contains("hidden"));
        }
        if let Some(a) = applet_at("ash") {
            assert!(a.render().contains("alias-of=sh"));
            assert!(!a.render().contains("main="));
        }
        if let Some(a) = applet_at("less") {
            assert!(a.render().contains("hidden"));
        }
    }

    #[test]
    fn lookup_classes_per_entry() {
        assert_eq!(lookup("sh").class, Known::Registered);
        assert_eq!(lookup("ash").class, Known::Alias);
        assert_eq!(lookup("ash").effective, "sh");
        assert_eq!(lookup("editor").class, Known::Trimmed);
        assert_eq!(lookup("editor").effective, "vi");
        assert_eq!(lookup("vi").class, Known::Trimmed);
        assert_eq!(lookup("less").class, Known::Hidden);
        assert_eq!(lookup("httpd").class, Known::Hidden);
        assert_eq!(lookup("foo").class, Known::Unknown);
        assert_eq!(lookup("foo").effective, "foo");
        assert_eq!(lookup("").class, Known::Unknown);
        assert!(lookup("sh").runnable());
        assert!(lookup("ash").runnable());
        assert!(!lookup("vi").runnable());
        assert!(!lookup("editor").runnable());
        assert!(lookup("vi").class.not_found());
        assert!(lookup("less").class.not_found());
        assert!(lookup("foo").class.not_found());
        assert!(lookup("vi").class.in_table());
        assert!(!lookup("foo").class.in_table());
    }

    #[test]
    fn known_and_found_metadata() {
        assert_eq!(Known::Registered.name(), "registered");
        assert_eq!(Known::Alias.name(), "alias");
        assert_eq!(Known::Trimmed.name(), "trimmed");
        assert_eq!(Known::Hidden.name(), "hidden");
        assert_eq!(Known::Unknown.name(), "unknown");
        assert!(!Known::Registered.not_found());
        assert!(!Known::Alias.not_found());
        assert!(Known::Registered.runnable());
        let none = Found::none("zz");
        assert_eq!(none.class, Known::Unknown);
        assert_eq!(none.effective, "zz");
        assert_eq!(none.index, 0);
        assert!(!none.runnable());
        assert_eq!(Found::ALIAS_DEPTH, 3);
    }

    #[test]
    fn lex_name_gate_order() {
        assert_eq!(lex_name(""), NameKind::Empty);
        assert_eq!(lex_name("\u{1}"), NameKind::Control);
        assert_eq!(lex_name("/\u{7}"), NameKind::Control);
        assert_eq!(lex_name("bin/ls"), NameKind::HasSlash);
        assert_eq!(lex_name("./ls"), NameKind::HasSlash);
        assert_eq!(lex_name("."), NameKind::Dots);
        assert_eq!(lex_name(".."), NameKind::Dots);
        assert_eq!(lex_name("ls"), NameKind::Ok);
        assert_eq!(lex_name("abcdefghijklmnopqrstuvwxyz0123"), NameKind::Ok);
        assert_eq!(lex_name("abcdefghijklmnopqrstuvwxyz01234"), NameKind::Overlong);
        assert_eq!(lex_name("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/"), NameKind::HasSlash);
        assert!(NameKind::Ok.usable());
        assert!(!NameKind::Empty.usable());
        assert!(!NameKind::Overlong.usable());
        assert_eq!(NameKind::Ok.name(), "ok");
        assert_eq!(NameKind::Empty.name(), "empty");
        assert_eq!(NameKind::HasSlash.name(), "has-slash");
        assert_eq!(NameKind::Dots.name(), "dots");
        assert_eq!(NameKind::Control.name(), "control");
        assert_eq!(NameKind::Overlong.name(), "overlong");
    }

    #[test]
    fn via_key_and_prog_sources() {
        let base = Shape {
            tag: "via-probe",
            argv0: "/bin/foo",
            req: "ls",
            via: Via::Direct,
            special: Special::None,
            flavor: Flavor::Modern,
            leak: false,
            root_only: false,
            am_root: true,
            stub_unwired: false,
            applet_rc: 0,
        };
        assert_eq!(base.key(), "ls");
        assert_eq!(base.prog(), "foo");
        let rel = base.clone_via(Via::LinkRel);
        assert_eq!(rel.key(), "foo");
        assert_eq!(rel.prog(), "foo");
        let abs = base.clone_via(Via::LinkAbs);
        assert_eq!(abs.key(), "foo");
        assert!(abs.via.from_argv0());
        assert!(abs.via.stripped());
        assert!(!abs.via.self_named());
        let raw = base.clone_via(Via::RawArgv0);
        assert_eq!(raw.key(), "/bin/foo");
        assert_eq!(raw.prog(), "/bin/foo");
        assert_eq!(lex_name(raw.key()), NameKind::HasSlash);
        assert!(!raw.via.stripped());
        assert!(!raw.via.self_named());
        let ret = Shape { argv0: "@busybox", via: Via::Retitle, ..base };
        assert_eq!(ret.key(), "ls");
        assert_eq!(ret.prog(), "busybox");
        assert!(ret.via.stripped());
        assert!(ret.via.self_named());
        assert!(!Via::Direct.from_argv0());
        assert!(!Via::Direct.stripped());
        assert!(Via::Direct.self_named());
        assert!(Via::DirectPath.self_named());
        assert_eq!(Via::Direct.name(), "direct");
        assert_eq!(Via::DirectPath.name(), "direct-path");
        assert_eq!(Via::LinkRel.name(), "link-rel");
        assert_eq!(Via::LinkAbs.name(), "link-abs");
        assert_eq!(Via::RawArgv0.name(), "raw-argv0");
        assert_eq!(Via::Retitle.name(), "retitle");
        assert!(via_ok());
    }

    #[test]
    fn shape_blank_stream_and_seal() {
        let b = Shape::blank();
        assert_eq!(b.prog(), "busybox");
        assert!(b.key().is_empty());
        assert_eq!(b.stream(), Stream::Stderr);
        assert_ne!(b.seal(), 0);
        assert_eq!(b.seal(), b.seal());
        assert_ne!(b.clone_req("ls").seal(), b.seal());
        assert_ne!(b.clone_via(Via::LinkAbs).seal(), b.seal());
        let leaked = Shape { leak: true, ..b };
        assert_eq!(leaked.stream(), Stream::Split);
        assert_ne!(leaked.seal(), b.seal());
        let info = Shape { special: Special::List, ..b };
        assert_eq!(info.stream(), Stream::Stdout);
        assert_eq!(info.name_kind(), NameKind::Ok);
        let b2 = Shape { tag: "b2", ..Shape::blank() };
        assert_eq!(b2.prog(), b.prog());
        assert_ne!(b2.seal(), b.seal());
    }

    #[test]
    fn define_bad_name_stops_at_lex() {
        let empty = Shape::blank();
        let o = define(&empty);
        assert_eq!(o.stage, Stage::Lex);
        assert_eq!(o.code, ERR_EINVAL);
        assert_eq!(o.value, EXIT_NOT_FOUND);
        assert_eq!(o.note, Note::BadName);
        assert!(o.default);
        assert_eq!(Tier::of(&o), Tier::Malformed);
        assert_eq!(message(&empty, &o).as_str(), "busybox: '': applet not found");
        let legacy = Shape { flavor: Flavor::Legacy, ..empty };
        assert!(!define(&legacy).default);
        let leaky = Shape { leak: true, ..empty };
        assert!(!define(&leaky).default);
        let long = Shape { req: "abcdefghijklmnopqrstuvwxyz01234", ..Shape::blank() };
        let l = define(&long);
        assert_eq!(l.code, ERR_ENAMETOOLONG);
        assert_eq!(l.stage, Stage::Lex);
        let dots = Shape { req: "..", ..Shape::blank() };
        assert_eq!(define(&dots).stage, Stage::Lex);
        let ctrl = Shape { req: "a\u{1}b", ..Shape::blank() };
        assert_eq!(define(&ctrl).code, ERR_EINVAL);
        assert!(define_ok());
    }

    #[test]
    fn define_switch_short_circuits_before_lookup() {
        for sp in [Special::List, Special::ListFull].iter() {
            let s = Shape { req: "--list", special: *sp, ..Shape::blank() };
            let o = define(&s);
            assert_eq!(o.stage, Stage::Ready);
            assert_eq!(o.note, Note::ListDump);
            assert_eq!(o.code, ERR_NONE);
            assert_eq!(o.value, EXIT_RUN_OK);
            assert!(!o.default);
            assert!(o.ok());
            assert_eq!(message(&s, &o).len(), 0);
            assert_eq!(Tier::of(&o), Tier::Run);
        }
        for sp in [Special::Help, Special::Version].iter() {
            let s = Shape { req: "--help", special: *sp, ..Shape::blank() };
            let o = define(&s);
            assert_eq!(o.stage, Stage::Ready);
            assert_eq!(o.note, Note::UsageDump);
            assert!(o.note.success());
            assert_eq!(s.stream(), Stream::Stdout);
        }
        let sw = Shape { req: "--install", special: Special::UnknownSwitch, ..Shape::blank() };
        let o = define(&sw);
        assert_eq!(o.stage, Stage::Dispatch);
        assert_eq!(o.code, ERR_EINVAL);
        assert_eq!(o.value, EXIT_NOT_FOUND);
        assert_eq!(o.note, Note::BadName);
        assert!(!o.default);
        assert_eq!(Tier::of(&o), Tier::Variant);
        assert!(Special::List.informational());
        assert!(Special::Version.informational());
        assert!(!Special::None.informational());
        assert!(!Special::UnknownSwitch.informational());
        assert!(Special::Help.skips_lookup());
        assert!(!Special::None.skips_lookup());
        assert_eq!(Special::None.name(), "dispatch");
        assert_eq!(Special::ListFull.name(), "--list-full");
        assert_eq!(Special::Version.name(), "-V");
        assert_eq!(Special::UnknownSwitch.name(), "--unknown");
    }

    #[test]
    fn define_run_root_and_unwired() {
        let sh = Shape { req: "sh", ..Shape::blank() };
        let o = define(&sh);
        assert_eq!(o.stage, Stage::Ready);
        assert_eq!(o.note, Note::Ran);
        assert_eq!(o.value, EXIT_RUN_OK);
        assert!(o.ok());
        let rc = Shape { applet_rc: 5, ..sh };
        let r = define(&rc);
        assert_eq!(r.value, 5);
        assert!(r.ok());
        let ash = Shape { req: "ash", ..Shape::blank() };
        let a = define(&ash);
        assert_eq!(a.note, Note::AliasRan);
        assert_eq!(message(&ash, &a).len(), 0);
        let need = Shape { req: "sh", root_only: true, am_root: false, ..Shape::blank() };
        let n = define(&need);
        assert_eq!(n.stage, Stage::Code);
        assert_eq!(n.code, ERR_EACCES);
        assert_eq!(n.value, EXIT_NEED_ROOT);
        assert_eq!(n.note, Note::NeedRoot);
        assert!(!n.default);
        assert_eq!(message(&need, &n).as_str(), "busybox: applet 'sh' requires root");
        let unwired = Shape { req: "ls", stub_unwired: true, ..Shape::blank() };
        let u = define(&unwired);
        assert_eq!(u.stage, Stage::Lookup);
        assert_eq!(u.code, ERR_ENOSYS);
        assert_eq!(u.value, EXIT_NO_EXEC);
        assert_eq!(u.note, Note::NoExec);
        assert_eq!(message(&unwired, &u).as_str(), "busybox: applet 'ls' is not executable");
        assert!(!u.not_found());
    }

    #[test]
    fn define_folds_every_unavailable_class() {
        let vi = define(&Shape { req: "vi", ..Shape::blank() });
        let less = define(&Shape { req: "less", ..Shape::blank() });
        let foo = define(&Shape { req: "foo", ..Shape::blank() });
        let editor = define(&Shape { req: "editor", ..Shape::blank() });
        for o in [vi, less, foo, editor].iter() {
            assert_eq!(o.stage, Stage::Code);
            assert_eq!(o.code, ERR_ENOENT);
            assert_eq!(o.value, EXIT_NOT_FOUND);
            assert_eq!(o.note, Note::Anchor);
            assert!(o.default);
            assert!(o.not_found());
            assert_eq!(Tier::of(o), Tier::Anchor);
        }
        assert_eq!(vi.seal(), foo.seal());
        assert_eq!(less.seal(), editor.seal());
    }

    #[test]
    fn define_flavor_and_stream_variants() {
        let legacy = Shape { req: "bash", flavor: Flavor::Legacy, ..Shape::blank() };
        let l = define(&legacy);
        assert_eq!(l.stage, Stage::Text);
        assert_eq!(l.note, Note::LegacyText);
        assert!(!l.default);
        assert_eq!(message(&legacy, &l).as_str(), "busybox: applet not found");
        assert_eq!(Tier::of(&l), Tier::Variant);
        let stub = Shape { req: "bash", flavor: Flavor::Stub, ..Shape::blank() };
        let s = define(&stub);
        assert_eq!(s.stage, Stage::Text);
        assert_eq!(s.note, Note::StubText);
        assert_eq!(message(&stub, &s).as_str(), "busybox: applet 'bash' not found");
        let leak = Shape { req: "foobar", leak: true, ..Shape::blank() };
        let e = define(&leak);
        assert_eq!(e.stage, Stage::Stream);
        assert_eq!(e.note, Note::Anchor);
        assert_eq!(e.code, ERR_ENOENT);
        assert!(!e.default);
        assert_eq!(message(&leak, &e).as_str(), "busybox: 'foobar': applet not found");
        assert_eq!(Tier::of(&e), Tier::Variant);
        assert_eq!(leak.stream(), Stream::Split);
        assert!(Stream::Stderr.valid_for_error());
        assert!(!Stream::Split.valid_for_error());
        assert!(!Stream::Stdout.valid_for_error());
        assert_eq!(Stream::Devnull.name(), "/dev/null");
        assert_eq!(Stream::Split.name(), "leaked-stdout");
        assert_eq!(Stream::Stdout.name(), "stdout");
        assert!(Flavor::Modern.is_main());
        assert!(!Flavor::Stub.is_main());
        assert_eq!(Flavor::Stub.name(), "noapplet-stub");
        assert_eq!(Flavor::Legacy.name(), "legacy");
        assert_eq!(Flavor::Modern.name(), "modern");
        assert!(Flavor::Stub.quotes_name());
        assert!(!Flavor::Legacy.quotes_name());
    }

    #[test]
    fn note_and_anchor_metadata() {
        assert!(Note::Ran.success());
        assert!(Note::AliasRan.success());
        assert!(Note::ListDump.success());
        assert!(Note::UsageDump.success());
        assert!(!Note::Anchor.success());
        assert!(Note::Anchor.not_found());
        assert!(Note::BadName.not_found());
        assert!(Note::LegacyText.not_found());
        assert!(Note::StubText.not_found());
        assert!(!Note::NeedRoot.not_found());
        assert!(!Note::NoExec.not_found());
        assert!(Note::LegacyText.text_variant());
        assert!(Note::StubText.text_variant());
        assert!(!Note::Anchor.text_variant());
        assert_eq!(Note::Anchor.family(), Anchor::Main);
        assert_eq!(Note::BadName.family(), Anchor::Main);
        assert_eq!(Note::LegacyText.family(), Anchor::Legacy);
        assert_eq!(Note::StubText.family(), Anchor::Stub);
        assert_eq!(Note::NeedRoot.family(), Anchor::Silent);
        assert_eq!(Note::Ran.family(), Anchor::Silent);
        assert_eq!(Note::NoExec.name(), "unwired-main");
        assert_eq!(Note::AliasRan.name(), "alias-ran");
        assert_eq!(Note::UsageDump.name(), "usage-dump");
        assert_eq!(Note::ListDump.name(), "list-dump");
        assert_eq!(Note::BadName.name(), "bad-name");
        assert_eq!(Note::NeedRoot.name(), "needs-root");
        assert_eq!(Anchor::Silent.name(), "silent");
        assert_eq!(Anchor::Main.name(), "main-anchor");
        assert_eq!(Anchor::Legacy.name(), "legacy-anchor");
        assert_eq!(Anchor::Stub.name(), "stub-anchor");
    }

    #[test]
    fn outcome_predicates_and_tier_projection() {
        let b = Outcome::blank();
        assert!(b.failed());
        assert!(!b.ok());
        assert!(b.not_found());
        assert_ne!(b.seal(), 0);
        assert_eq!(Tier::of(&b), Tier::Malformed);
        let anchorish = Outcome { note: Note::Anchor, ..b };
        assert_ne!(anchorish.seal(), b.seal());
        assert_eq!(Tier::of(&anchorish), Tier::Malformed);
        let ready = Outcome { stage: Stage::Ready, code: 0, value: 0, note: Note::Ran, default: false };
        assert!(ready.ok());
        assert!(!ready.not_found());
        assert_eq!(Tier::of(&ready), Tier::Run);
        let scene = Outcome { stage: Stage::Code, code: ERR_ENOENT, value: 127, note: Note::Anchor, default: true };
        assert_eq!(Tier::of(&scene), Tier::Anchor);
        let variant = Outcome { default: false, ..scene };
        assert_eq!(Tier::of(&variant), Tier::Variant);
        assert_eq!(Tier::Run.index(), 0);
        assert_eq!(Tier::Anchor.index(), 1);
        assert_eq!(Tier::Variant.index(), 2);
        assert_eq!(Tier::Malformed.index(), 3);
        assert!(Tier::Run.success());
        assert!(!Tier::Anchor.success());
        assert!(!Tier::Run.not_found_face());
        assert!(Tier::Variant.not_found_face());
        assert_eq!(Tier::Malformed.name(), "malformed");
        assert_eq!(Tier::Anchor.name(), "anchor");
        assert_eq!(Stage::Lex.order(), 0);
        assert_eq!(Stage::Dispatch.order(), 1);
        assert_eq!(Stage::Lookup.order(), 2);
        assert_eq!(Stage::Text.order(), 3);
        assert_eq!(Stage::Stream.order(), 4);
        assert_eq!(Stage::Code.order(), 5);
        assert_eq!(Stage::Ready.order(), 6);
        assert!(Stage::Code.failure());
        assert!(!Stage::Ready.failure());
        assert_eq!(Stage::Dispatch.name(), "dispatch");
    }

    #[test]
    fn corpus_expectation_tables_agree() {
        let cs = cases();
        assert_eq!(cs.len(), CORPUS);
        let mut i = 0usize;
        while i < CORPUS {
            if let (Some(c), Some(st), Some(co), Some(va), Some(no), Some(de), Some(ta), Some(ti)) = (
                cs.get(i),
                WANT_STAGE.get(i),
                WANT_CODE.get(i),
                WANT_VALUE.get(i),
                WANT_NOTE.get(i),
                WANT_DEFAULT.get(i),
                WANT_TAGS.get(i),
                WANT_TIER.get(i),
            ) {
                let a = c.actual();
                assert_eq!(c.tag(), *ta);
                assert_eq!(a.stage, *st);
                assert_eq!(a.code, *co);
                assert_eq!(a.value, *va);
                assert_eq!(a.note, *no);
                assert_eq!(a.default, *de);
                assert_eq!(Tier::of(&a), *ti);
            } else {
                panic!("期望表长度不齐");
            }
            i += 1;
        }
        assert!(corpus_ok());
        assert!(slots_ok());
    }

    #[test]
    fn live_slots_are_all_green() {
        let cs = cases();
        let mut i = 0usize;
        while i < ACCEPT {
            if let Some(c) = cs.get(i) {
                assert!(c.ok());
                assert_eq!(c.judge(), Fault::Ok);
                assert_eq!(expected(i), Some(Fault::Ok));
            } else {
                panic!("活槽缺位");
            }
            i += 1;
        }
    }

    #[test]
    fn dead_slots_are_one_dimension_each() {
        let mut i = 0usize;
        while i < REJECT {
            if let (Some(c), Some(f), Some(code)) =
                (case_at(i + ACCEPT), REJECT_FAULTS.get(i), REJECT_CODES.get(i))
            {
                assert_eq!(c.judge(), *f);
                assert!(!c.ok());
                assert_eq!(c.judge().code(), *code);
                assert_eq!(expected(i + ACCEPT), Some(*f));
                assert!(code >= &SEG_BASE);
            } else {
                panic!("死槽缺位");
            }
            i += 1;
        }
        let mut k = 1usize;
        while k < REJECT {
            if let Some(c) = case_at(k + ACCEPT) {
                assert_eq!(diff_dims(&c), 1);
            }
            k += 1;
        }
        if let Some(e) = case_at(ACCEPT) {
            assert_eq!(e.judge(), Fault::Estate);
            assert!(e.claim.success());
            assert!(e.actual().failed());
            assert_ne!(e.actual().ok(), e.claim.success());
        }
        assert!(reds_ok());
    }

    #[test]
    fn dead_slots_share_one_scene() {
        let base = match case_at(ACCEPT) {
            Some(c) => scene(&c),
            None => panic!("槽 8 缺位"),
        };
        let seal_of_8 = match case_at(ACCEPT) {
            Some(c) => c.actual().seal(),
            None => 0,
        };
        assert_eq!(base, (Stage::Code, ERR_ENOENT, EXIT_NOT_FOUND, Note::Anchor, true));
        assert_ne!(seal_of_8, 0);
        let mut i = 1usize;
        while i < REJECT {
            if let Some(c) = case_at(i + ACCEPT) {
                assert_eq!(scene(&c), base);
                assert_eq!(Tier::of(&c.actual()), Tier::Anchor);
                assert_eq!(c.actual().seal(), seal_of_8);
            }
            i += 1;
        }
    }

    #[test]
    fn dead_slot_shapes_are_distinct() {
        let mut i = ACCEPT;
        while i < CORPUS {
            let mut j = i + 1;
            if let Some(a) = case_at(i) {
                while j < CORPUS {
                    if let Some(b) = case_at(j) {
                        assert_ne!(a.shape.seal(), b.shape.seal());
                        assert_ne!(a.shape.tag, b.shape.tag);
                        assert_ne!(a.tag(), b.tag());
                    }
                    j += 1;
                }
            }
            i += 1;
        }
        let mut unknown = 0usize;
        let mut folded = 0usize;
        let all = cases();
        for c in all.iter().skip(ACCEPT) {
            match lookup(c.shape.key()).class {
                Known::Unknown => unknown += 1,
                Known::Trimmed | Known::Hidden => folded += 1,
                _ => panic!("死槽必须落在不可分发档"),
            }
        }
        assert_eq!(unknown, 1);
        assert_eq!(folded, 5);
    }

    #[test]
    fn case_helpers_and_bounds() {
        assert_eq!(case_at(CORPUS), None);
        assert_eq!(case_at(usize::MAX), None);
        assert!(case_at(0).is_some());
        assert_eq!(cover(CORPUS), Ok(()));
        assert_eq!(cover(CORPUS - 1), Err(NF_ENOBS));
        assert_eq!(cover(0), Err(NF_ENOBS));
        assert_eq!(expected(CORPUS), None);
        assert_eq!(expected(usize::MAX), None);
        let blank = Case::blank();
        assert!(blank.tag().is_empty());
        assert_eq!(blank.judge(), Fault::Estate);
        assert!(!blank.ok());
        assert!(Claim::blank().success());
        assert_eq!(Claim::blank().stage, Stage::Ready);
        assert!(Outcome::blank().failed());
    }

    #[test]
    fn matrix_rows_are_balanced() {
        let m = Matrix::of();
        assert_eq!(m.slots(), CORPUS);
        assert_eq!(m.goods(), ACCEPT);
        assert!(m.covered());
        assert_eq!(m.row(Tier::Run), Some(Row { total: 3, good: 3 }));
        assert_eq!(m.row(Tier::Anchor), Some(Row { total: 8, good: 2 }));
        assert_eq!(m.row(Tier::Variant), Some(Row { total: 2, good: 2 }));
        assert_eq!(m.row(Tier::Malformed), Some(Row { total: 1, good: 1 }));
        assert_eq!(m.perfect_rows(), 3);
        if let Some(r) = m.row(Tier::Anchor) {
            assert_eq!(r.red(), 6);
            assert!(!r.perfect());
        } else {
            panic!("主锚档缺位");
        }
        if let Some(r) = m.row(Tier::Run) {
            assert!(r.perfect());
        }
        assert_eq!(Row::blank().red(), 0);
        assert!(!Row::blank().perfect());
        assert_eq!(Row { total: 2, good: 5 }.red(), 2);
        assert_eq!(Matrix::blank().slots(), 0);
        assert!(!Matrix::blank().covered());
    }

    #[test]
    fn matrix_render_is_four_lines() {
        let m = Matrix::of();
        let t = m.render();
        assert!(t.starts_with("run total=3 good=3 red=0\n"));
        assert!(t.contains("anchor total=8 good=2 red=6\n"));
        assert!(t.contains("variant total=2 good=2 red=0\n"));
        assert!(t.ends_with("malformed total=1 good=1 red=0\n"));
        assert_eq!(nl_count(t.as_str()), TIER_COUNT);
        assert_eq!(item_count(t.as_str(), b'\n'), TIER_COUNT + 1);
        if let Some(r) = m.row(Tier::Variant) {
            assert_eq!(r.render(Tier::Variant).as_str(), "variant total=2 good=2 red=0\n");
        }
        assert!(matrix_ok());
    }

    #[test]
    fn report_fields_and_render() {
        let r = match run() {
            Ok(v) => v,
            Err(_) => panic!("全绿报表必须可取"),
        };
        assert_eq!(r.total, CORPUS as u32);
        assert_eq!(r.accept, ACCEPT as u32);
        assert_eq!(r.reject, REJECT as u32);
        assert_eq!(r.matched, r.total);
        assert_eq!(r.run, 3);
        assert_eq!(r.anchor, 2);
        assert_eq!(r.variant, 2);
        assert_eq!(r.malformed, 1);
        assert!(r.green());
        assert_eq!(r.gate(), Ok(()));
        assert_eq!(r.rate(), 1000);
        assert_eq!(r.success_cells(), 3);
        assert_eq!(r.failure_cells(), 5);
        assert_eq!(r.success_cells() + r.failure_cells(), r.accept);
        assert_eq!(r.not_found_share(), 62);
        assert_eq!(r.render().as_str(), RENDER_OK);
        let zero = Report {
            total: 0,
            accept: 0,
            reject: 0,
            run: 0,
            anchor: 0,
            variant: 0,
            malformed: 0,
            matched: 0,
        };
        assert_eq!(zero.rate(), 0);
        assert!(!zero.green());
        assert_eq!(zero.gate(), Err(NF_ECOUNT));
        assert_eq!(zero.not_found_share(), 0);
    }

    #[test]
    fn injection_leaves_the_green_state() {
        let red = match run_with(Some((0, Fault::Code))) {
            Ok(v) => v,
            Err(_) => panic!("注入槽合法"),
        };
        assert_eq!(red.matched, 13);
        assert_eq!(red.rate(), 929);
        assert!(!red.green());
        assert_eq!(red.gate(), Err(NF_ECOUNT));
        assert_eq!(red.accept, (ACCEPT - 1) as u32);
        assert_eq!(red.reject, (REJECT + 1) as u32);
        assert_eq!(red.run, 2);
        match run_with(Some((CORPUS, Fault::Code))) {
            Err(c) => assert_eq!(c, NF_ESLOT),
            Ok(_) => panic!("越界槽必须被拒"),
        }
        match run_with(Some((usize::MAX, Fault::Ok))) {
            Err(c) => assert_eq!(c, NF_ESLOT),
            Ok(_) => panic!("越界槽必须被拒"),
        }
        assert!(run_with(None).is_ok());
    }

    #[test]
    fn message_dispatch_per_slot() {
        for c in cases().iter() {
            let a = c.actual();
            let m = message(&c.shape, &a);
            assert_eq!(m.is_empty(), a.note.success());
            if !m.is_empty() {
                assert!(m.starts_with(c.shape.prog()));
                assert!(!has_byte(m.as_str(), b'\n'));
                assert!(!has_at_prefix(m.as_str()));
            }
            match a.note.family() {
                Anchor::Main => {
                    assert!(m.contains("applet not found"));
                    assert_eq!(
                        m.as_str(),
                        msg_main_anchor(c.shape.prog(), c.shape.key()).as_str()
                    );
                }
                Anchor::Legacy => {
                    assert_eq!(m.as_str(), msg_legacy_anchor(c.shape.prog()).as_str());
                    assert!(!m.contains(c.shape.key()));
                }
                Anchor::Stub => {
                    assert_eq!(
                        m.as_str(),
                        msg_stub_anchor(c.shape.prog(), c.shape.key()).as_str()
                    );
                }
                Anchor::Silent => assert!(m.is_empty()),
            }
        }
        assert!(message_ok());
    }

    #[test]
    fn applet_list_membership() {
        let l = applet_list(",");
        assert!(!l.is_empty());
        assert_eq!(item_count(l.as_str(), b','), 18);
        for n in ["sh", "busybox", "ls", "cat", "echo", "route", "ping", "noapplet"].iter() {
            assert!(member(l.as_str(), ',', n), "清单缺 {}", n);
        }
        for n in [
            "vi", "editor", "less", "httpd", "ifconfig", "ip", "wget", "udhcpc", "crond", "hush",
            "ping6", "ash", "foo",
        ]
        .iter()
        {
            assert!(!member(l.as_str(), ',', n), "清单多 {}", n);
        }
        let bar = applet_list("|");
        assert_eq!(bar.len(), l.len());
        assert!(!has_byte(bar.as_str(), b','));
        assert_eq!(item_count(bar.as_str(), b'|'), 18);
        assert!(member(bar.as_str(), '|', "sed"));
        assert_eq!(nl_count(l.as_str()), 0);
        assert!(!member("", ',', "sh"));
        assert!(!member(l.as_str(), ',', ""));
    }

    #[test]
    fn detail_lines_are_verbatim() {
        if let Some(c) = case_at(3) {
            assert_eq!(
                detail(&c).as_str(),
                "busybox/foo stage=code code=2 exit=127 anchor=true note=main-anchor \
                 msg=\"busybox: 'foo': applet not found\"\n"
            );
        } else {
            panic!("槽 3 缺位");
        }
        if let Some(c) = case_at(0) {
            assert_eq!(
                detail(&c).as_str(),
                "busybox/sh stage=ready code=0 exit=0 anchor=false note=ran\n"
            );
        }
        if let Some(c) = case_at(5) {
            assert_eq!(
                detail(&c).as_str(),
                "busybox/bash stage=text code=2 exit=127 anchor=false note=legacy-anchor \
                 msg=\"busybox: applet not found\"\n"
            );
        }
        if let Some(c) = case_at(7) {
            assert_eq!(
                detail(&c).as_str(),
                "busybox/bin/ls stage=lex code=22 exit=127 anchor=true note=bad-name \
                 msg=\"busybox: 'bin/ls': applet not found\"\n"
            );
        }
        if let Some(c) = case_at(4) {
            assert_eq!(
                detail(&c).as_str(),
                "wget/wget stage=code code=2 exit=127 anchor=true note=main-anchor \
                 msg=\"wget: 'wget': applet not found\"\n"
            );
        }
        assert!(detail_ok());
    }

    #[test]
    fn detail_ignores_the_claim() {
        let c3 = match case_at(3) { Some(v) => v, None => panic!("槽 3 缺位") };
        let mut alt = c3;
        alt.claim.note = Note::StubText;
        assert_eq!(c3.judge(), Fault::Ok);
        assert_eq!(alt.judge(), Fault::Note);
        assert_eq!(detail(&c3), detail(&alt));
        assert!(find_from(detail(&alt).as_str(), "main-anchor", 0).is_some());
    }

    #[test]
    fn gate_is_monotonic() {
        let mut g = Gate::new();
        assert_eq!(g.steps(), 0);
        assert_eq!(g.last(), -1);
        assert!(!g.closed());
        assert_eq!(g.step(Stage::Lex), Ok(()));
        assert_eq!(g.last(), 0);
        assert_eq!(g.step(Stage::Dispatch), Ok(()));
        assert_eq!(g.step(Stage::Dispatch), Ok(()));
        assert_eq!(g.step(Stage::Lookup), Ok(()));
        assert_eq!(g.step(Stage::Lex), Err(NF_ESTAGE));
        assert_eq!(g.steps(), 4);
        assert_eq!(g.step(Stage::Code), Ok(()));
        assert_eq!(g.step(Stage::Text), Err(NF_ESTAGE));
        assert_eq!(g.steps(), 5);
        assert_eq!(g.close(), Ok(5));
        assert!(g.closed());
        assert_eq!(g.close(), Err(NF_ESTATE));
        assert_eq!(g.step(Stage::Ready), Err(NF_ESTATE));
        let mut e = Gate::new();
        assert_eq!(e.close(), Ok(0));
        let seq = [
            Stage::Lex,
            Stage::Dispatch,
            Stage::Lookup,
            Stage::Text,
            Stage::Stream,
            Stage::Code,
            Stage::Ready,
        ];
        let mut f = Gate::new();
        let mut k = 0usize;
        while k < seq.len() {
            if let Some(s) = seq.get(k) {
                assert_eq!(f.step(*s), Ok(()));
            }
            k += 1;
        }
        assert_eq!(f.steps(), 7);
        assert_eq!(f.close(), Ok(7));
        assert!(gate_ok());
    }

    #[test]
    fn ledger_shape_and_audit() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert!(!l.is_empty());
        assert_eq!(l.ok_count(), ACCEPT);
        assert_eq!(l.err_count(), REJECT);
        assert_eq!(l.rejects(), 0);
        assert_eq!(l.audit(), Ok(()));
        assert!(l.sample(SAMPLES).is_none());
        let mut i = 0usize;
        while i < ACCEPT {
            if let (Some(s), Some(v)) = (l.sample(i), VALS.get(i)) {
                assert_eq!(s.slot as usize, i);
                assert!(s.ok);
                assert_eq!(s.code, 0);
                assert_eq!(s.value, *v);
                assert_ne!(s.op, 0);
            } else {
                panic!("成格缺位");
            }
            i += 1;
        }
        let mut j = 0usize;
        while j < REJECT {
            if let (Some(s), Some(c)) = (l.sample(j + ACCEPT), REJECT_CODES.get(j)) {
                assert_eq!(s.slot as usize, j + ACCEPT);
                assert!(!s.ok);
                assert_eq!(s.code, *c);
                assert_eq!(s.value, 0);
                assert_eq!(s.op, OP_CODE);
            } else {
                panic!("败格缺位");
            }
            j += 1;
        }
    }

    #[test]
    fn ledger_rejects_jump_slots() {
        let mut l = Ledger::new();
        assert!(l.is_empty());
        assert_eq!(l.len(), 0);
        assert_eq!(
            l.register(Sample { slot: 3, op: OP_CORPUS, ok: true, code: 0, value: 14 }),
            Err(NF_ESLOT)
        );
        assert_eq!(l.rejects(), 1);
        assert_eq!(
            l.register(Sample { slot: 1, op: OP_CORPUS, ok: true, code: 0, value: 14 }),
            Err(NF_ESLOT)
        );
        assert_eq!(l.rejects(), 2);
        assert_eq!(
            l.register(Sample { slot: 0, op: OP_CORPUS, ok: true, code: 0, value: 14 }),
            Ok(())
        );
        assert_eq!(l.len(), 1);
        assert_eq!(l.rejects(), 2);
        assert_eq!(
            l.register(Sample { slot: SAMPLES as u8, op: OP_LEX, ok: false, code: NF_EBAL, value: 0 }),
            Err(NF_ESLOT)
        );
        assert_eq!(l.rejects(), 3);
        assert_ne!(l.audit(), Ok(()));
        let fresh = Ledger::new();
        assert_eq!(fresh.audit(), Err(NF_EBAL));
        assert_eq!(fresh.ok_count(), 0);
        assert_eq!(fresh.err_count(), 0);
        assert_eq!(Sample::blank().slot, 0);
    }

    #[test]
    fn ledger_audit_catches_tampering() {
        let mut l = build_ledger();
        if let Some(s) = l.rows.get_mut(0usize) {
            s.value = 99;
        }
        assert_eq!(l.audit(), Err(NF_EVALUE));
        assert_eq!(l.replay(), Err(NF_ESEAL));
        let mut m = build_ledger();
        if let Some(s) = m.rows.get_mut(9usize) {
            s.slot = 0;
        }
        assert_eq!(m.audit(), Err(NF_ESLOT));
        let mut p = build_ledger();
        if let Some(s) = p.rows.get_mut(3usize) {
            s.ok = false;
        }
        assert_eq!(p.audit(), Err(NF_EBAL));
        assert!(ledger_ok());
    }

    #[test]
    fn ledger_freeze_and_replay() {
        let mut l = build_ledger();
        assert_eq!(l.audit(), Ok(()));
        assert_eq!(l.root(), 0);
        assert!(!l.is_frozen());
        let sealed = l.seal();
        assert_ne!(sealed, 0);
        let root = match l.freeze() {
            Ok(r) => r,
            Err(_) => panic!("冻结必须成功"),
        };
        assert_eq!(root, sealed);
        assert_eq!(l.root(), sealed);
        assert!(l.is_frozen());
        assert_eq!(l.replay(), Ok(sealed));
        assert_eq!(
            l.register(Sample { slot: 14, op: OP_LEX, ok: true, code: 0, value: 1 }),
            Err(NF_ESLOT)
        );
        assert_eq!(l.rejects(), 1);
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.freeze(), Err(NF_EBAL));
        assert_eq!(l.rejects(), 2);
    }

    #[test]
    fn inject_four_paths_are_all_true() {
        let inj = inject_four();
        assert_eq!(inj.len(), 4);
        assert!(inj.iter().all(|v| *v));
        assert!(anchor_ok());
        assert!(judge_ok());
    }

    #[test]
    fn all_thirteen_checks_pass() {
        let s = run_unx11661_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
        assert_eq!(s.failed_count(), 0);
        assert_eq!(s.passed_count(), 13);
        assert_eq!(s.tally(), (13usize, 0usize));
        assert_eq!(s.domain, "unx11661_bb_notfound");
        assert_eq!(s.get(13), None);
        assert!(s.get(0).is_some());
        assert!(!s.truncated());
        assert_eq!(s.dropped(), 0);
    }

    #[test]
    fn check_names_are_bn_prefixed_in_order() {
        let s = run_unx11661_checks();
        let want = [
            "BN01-名称与段基址",
            "BN02-枚举与纵线",
            "BN03-注册表与查表",
            "BN04-调用路径六式",
            "BN05-七闸纵线",
            "BN06-主锚与变体档",
            "BN07-申报对判",
            "BN08-矩阵统计",
            "BN09-语料与渲染",
            "BN10-红档显式",
            "BN11-文案逐字",
            "BN12-释义与账本",
            "BN13-冻结重放四注入",
        ];
        assert_eq!(want.len(), 13);
        let mut i = 0usize;
        while i < want.len() {
            if let (Some(c), Some(n)) = (s.get(i), want.get(i)) {
                assert_eq!(c.name, *n);
                assert!(c.passed);
                assert_eq!(c.detail, "static");
            } else {
                panic!("检查表不齐");
            }
            i += 1;
        }
        if let Some(last) = s.get(12) {
            assert_eq!(last.name, "BN13-冻结重放四注入");
        }
    }
}
