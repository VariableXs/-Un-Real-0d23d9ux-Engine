//! UNX-F11658 坏 locale × 工具链降级矩阵（20 工具抽样）
//!
//! 现场里最贵的一类故障不是"崩"，而是**降级了却没人知道**。`LANG=zh_CN.UTF-8`
//! 打错一个字母、locale-archive 被半路截断、LOCPATH 指到一个空目录——这三类
//! 输入在真实 musl/glibc 环境里都不会让 `cat`、`wc`、`ls` 退出码变红，改变的
//! 只是**数值本身**（`wc -m` 从字符数退回字节数、`printf '%.2f'` 的小数点方向、
//! `ls` 的排序序、`rm -i` 对肯定词的回答）。本模块把这条降级链从第一性原理
//! 重建，并把每一档的落点钉成判据。
//!
//! 请求名解析的**优先级顺序即语义**，不是"哪个环境变量看起来更响"：
//! `LC_ALL` > `LC_<类别>` > `LANG` > 内建 `"C"`。三档各自带一条独立的
//! "缺席/空值"规则，而且这三条规则互不对称，这是本卡最容易抄错的一处：
//! `LC_ALL` 只要**已设**（哪怕空串）就覆盖全部五个类别，空串按 POSIX 的
//! "请求 C" 语义直接落内建 C；`LC_<类别>` 的空串**不算请求**，它等同于缺席，
//! 控制权继续下放到 `LANG`；`LANG` 的空串同样不算请求，落到默认 C。
//! 也就是"空串"在 A 档是命令、在 B/C 档是沉默——判据必须按档位分别取证，
//! 不能用一条 `is_empty()` 规则统掉。
//!
//! 名字合法性再分四五种码位：POSIX 便携字符集是 `[A-Za-z0-9_+.@-]`，
//! 空名、超容（>64 字节）、含非法字符、以 `-` 打头（会被 loader 当选项）、
//! 分隔符悬挂或重复，各落各的码；`C` 与 `POSIX` 是内建档，**永远可用且不查
//! 归档**——把 `C` 也送去查 locale-archive 是一种伪失败，它会让"归档不可用"
//! 的观测污染到本来完全健康的档位。
//!
//! 归档装载的闸门次序：`LOCPATH` 旁路 → 归档头 magic（`log1`/`loc1`）→ 头
//! version → 总长边界 → 逐节（在场 → 偏移/长度在界内 → 节 magic → 节 version →
//! 节长度 ≥ 该类别字符表所需下限）→ 才允许碰任何字符表。节缺失、节长越界、
//! magic 不符、版本不符四类拒收全部发生在"碰任何字符表之前"，本卡把这条写成
//! 显式判据（`stage_order < Stage::Table`）：一旦先读了字符表再报结构错，
//! 观测到的就是"读了半张坏表"的静默错误数据，而不是可复现的拒收码。
//! 头档故障（magic/version/总长）使**整个**归档不可用，落内建 C；节档故障
//! 只使**该类别**不可用，逐类回退——这两者的分档不同，混为一谈就把
//! `partial` 与 `built-in C` 压成了同一格。
//!
//! 降级四档：`full`（目标 locale 全套节可用）→ `partial`（部分类别可用，
//! 其余逐类回退内建 C 并**逐类记账**）→ `built-in C`（归档整体不可用，
//! 全类回退内建 C，功能仍然正确）→ `broken`（连内建 C 的 LC_CTYPE 字符表都
//! 判死：任何字节分类都不可信，工具必须显式报错，禁止静默乱码）。四档是
//! 有序的可信度序列，不是四个错误码。
//!
//! 关键差异（本卡深度所在）：**回退的类别落在哪条轴上，决定了"要不要报错"，
//! 而工具名字决定不了。** 类别按轴分三种——数值轴（LC_CTYPE 影响 `wc -m/-L`
//! 的计数与 `ls` 的非 ASCII 引号、LC_NUMERIC 影响 `printf` 的小数点）、
//! 版式轴（LC_TIME 影响日期串、LC_COLLATE 影响排序序）、语言轴（LC_MESSAGES
//! 只影响诊断文案）。数值轴的回退**改变输出里的数字本身**，下游解析者无从
//! 察觉，所以必须记账；版式轴与语言轴的回退只改呈现，可由一次性的告警覆盖。
//! 于是"伪成功"有了唯一的定义：**存在数值轴类别回退，而该类别未被记账**。
//! 本卡把它算成一个整数并要求为 0——申报为 0 而实测不为 0 的那一格，判据
//! 必须先把实测跑出来再驳倒（红档不许是"恰好相等"的巧合）。
//!
//! 另一处极易误判的对称破缺：`test`、`uname`、`echo`、`basename`、`dirname`
//! 对 locale **恒不敏感**——POSIX 明确 `test` 的字符串比较按字节，不按
//! collation；`uname` 的字段来自系统调用；`basename`/`dirname` 的后缀剥离与
//! 路径切分是字节级操作。把这五个也计入"降级受损"是矩阵虚高；反过来，
//! `rm -i` 的肯定词表（`YESEXPR`）在 `LC_MESSAGES`，回退 C 后只认 ASCII
//! `y`，本地化回答会被当成否定而**静默不删**——这是数据丢失，不是呈现差异，
//! 所以 `rm` 的记账是强制的。抽样 20 个工具：题面候选 21 个，剔除 `whoami`
//! （它是 `id -un` 的真子集，依赖轴与 `id` 完全同构，留下只造重复行）。
//!
//! 闸门次序：请求解析 → 名字合法性 → 归档头 → 归档节 → 字符表 → 分档 →
//! 记账 → 矩阵 → 对判 → 账本 → 冻结。每档只在上档全绿时才有资格发言；
//! `Gate` 记录走过的档序，倒退即 `LM_EORDER`。
//!
//! 语料 14 槽：8 槽申报正确（full 2、partial 3、built-in C 2、broken 1），
//! 6 槽各挂一处**对真实结果的误申报**（状态/档/码位/记账数/矩阵计数/次序），
//! 每格红档都由判据实跑驳倒。账本 14 格 8 成 6 败，冻结后重放同指纹。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，账本在
//! `audit` 逐格对账。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址（F11658）。
pub const SEG_BASE: u32 = 0x6cf30;

/// 语料槽数。
pub const CORPUS: usize = 14;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 申报正确的槽数。
pub const ACCEPT: usize = 8;

/// 申报错误的槽数。
pub const REJECT: usize = 6;

/// 降级档位数（full/partial/built-in C/broken）。
pub const TIER_COUNT: usize = 4;

/// 可请求的 locale 类别数（CTYPE/NUMERIC/TIME/COLLATE/MESSAGES）。
pub const CATS: usize = 5;

/// 归档节数（含 LC_IDENTIFICATION 元数据节）。
pub const SECS: usize = 6;

/// 工具抽样数。
pub const TOOLS: usize = 20;

/// 名字最大字节数（超容直接拒收）。
pub const NAME_MAX: usize = 64;

/// 请求名缓冲。
pub const REQ_MAX: usize = 3;

/// 归档路径上限。
pub const PATH_MAX: usize = 64;

// ---------------------------------------------------------------------------
// 类别与差异轴
// ---------------------------------------------------------------------------

/// locale 类别。下标即归档节序（0 是元数据节，1..=5 与可请求类别同序）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    /// LC_IDENTIFICATION：元数据节，不参与逐类回退。
    Identification,
    /// LC_CTYPE：字符分类与大小写映射（数值轴）。
    Ctype,
    /// LC_NUMERIC：小数点与分组（数值轴）。
    Numeric,
    /// LC_TIME：日期时间格式（版式轴）。
    Time,
    /// LC_COLLATE：排序规则（版式轴）。
    Collate,
    /// LC_MESSAGES：诊断与肯定词表（语言轴）。
    Messages,
}

/// 差异轴：回退到 C 之后，输出里被动的那一部分是什么。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    /// 与 locale 无交集。
    None,
    /// 数值轴：改变输出里的数字/字节宽度本身。
    Data,
    /// 版式轴：改变呈现顺序与格式串。
    Layout,
    /// 语言轴：只改变诊断文案。
    Language,
    /// 元数据轴：只改变标识信息。
    Meta,
}

impl Category {
    /// 下标。
    pub fn index(&self) -> usize {
        match self {
            Category::Identification => 0,
            Category::Ctype => 1,
            Category::Numeric => 2,
            Category::Time => 3,
            Category::Collate => 4,
            Category::Messages => 5,
        }
    }

    /// 类别号。
    pub fn of(i: usize) -> Category {
        match i {
            0 => Category::Identification,
            1 => Category::Ctype,
            2 => Category::Numeric,
            3 => Category::Time,
            4 => Category::Collate,
            _ => Category::Messages,
        }
    }

    /// 归档节名。
    pub fn name(&self) -> &'static str {
        match self {
            Category::Identification => "LC_IDENTIFICATION",
            Category::Ctype => "LC_CTYPE",
            Category::Numeric => "LC_NUMERIC",
            Category::Time => "LC_TIME",
            Category::Collate => "LC_COLLATE",
            Category::Messages => "LC_MESSAGES",
        }
    }

    /// 短名（渲染用）。
    pub fn short(&self) -> &'static str {
        match self {
            Category::Identification => "IDENT",
            Category::Ctype => "CTYPE",
            Category::Numeric => "NUMERIC",
            Category::Time => "TIME",
            Category::Collate => "COLLATE",
            Category::Messages => "MESSAGES",
        }
    }

    /// 差异轴。
    pub fn axis(&self) -> Axis {
        match self {
            Category::Ctype | Category::Numeric => Axis::Data,
            Category::Time | Category::Collate => Axis::Layout,
            Category::Messages => Axis::Language,
            Category::Identification => Axis::Meta,
        }
    }

    /// 该类别的字符表所需下限字节数（节长越界的判据下限）。
    pub fn min_len(&self) -> u32 {
        match self {
            Category::Identification => 24,
            Category::Ctype => 1024,
            Category::Numeric => 16,
            Category::Time => 64,
            Category::Collate => 32,
            Category::Messages => 8,
        }
    }

    /// 记账下限：低于此长度即"结构可读但表不可用"。
    pub fn floor_len(&self) -> u32 {
        match self {
            Category::Ctype => 256,
            Category::Time => 32,
            Category::Numeric => 8,
            Category::Collate => 16,
            Category::Messages => 4,
            Category::Identification => 8,
        }
    }
}

/// 可请求类别的下标（1..=5）。
pub const CAT_CTYPE: usize = 1;
/// 数值轴类别。
pub const CAT_NUMERIC: usize = 2;
/// 版式轴（日期）。
pub const CAT_TIME: usize = 3;
/// 版式轴（排序）。
pub const CAT_COLLATE: usize = 4;
/// 语言轴。
pub const CAT_MESSAGES: usize = 5;

/// 数值轴类别掩码：这些类别回退必须逐类记账。
pub const MASK_DATA: u32 = (1u32 << CAT_CTYPE) | (1u32 << CAT_NUMERIC);
/// 版式轴类别掩码。
pub const MASK_LAYOUT: u32 = (1u32 << CAT_TIME) | (1u32 << CAT_COLLATE);
/// 语言轴类别掩码。
pub const MASK_LANG: u32 = 1u32 << CAT_MESSAGES;
/// 全部可请求类别掩码。
pub const MASK_CATS: u32 = MASK_DATA | MASK_LAYOUT | MASK_LANG;

/// 类别 → 掩码位。
pub const fn cat_bit(i: usize) -> u32 {
    if i >= SECS {
        return 0;
    }
    1u32 << i
}

/// 掩码里的位数。
pub fn popcount(mask: u32) -> u8 {
    let mut m = mask;
    let mut n = 0u8;
    while n < 32 && m != 0 {
        n = match m & 1 {
            1 => n + 1,
            _ => n,
        };
        m >>= 1;
    }
    n
}

/// 掩码 → 类别短名串（空格分隔，空掩码为 `-`）。
pub fn mask_text(mask: u32) -> String {
    let mut s = String::new();
    let mut i = 1usize;
    while i < SECS {
        let on = match cat_bit(i) { b => mask & b != 0 };
        if on {
            if s.len() > 0 {
                s.push(' ');
            }
            s.push_str(Category::of(i).short());
        }
        i += 1;
    }
    if s.is_empty() {
        s.push('-');
    }
    s
}

/// 全掩码。
pub fn full_mask() -> u32 {
    MASK_CATS
}

// ---------------------------------------------------------------------------
// 13 个错误码
// ---------------------------------------------------------------------------

/// 请求解析档位不符。
pub const LM_EPARSE: u32 = SEG_BASE + 0x01;
/// 名字合法性码位不符。
pub const LM_ENAME: u32 = SEG_BASE + 0x02;
/// 归档结构闸不符。
pub const LM_EARCH: u32 = SEG_BASE + 0x03;
/// 降级档不符。
pub const LM_ETIER: u32 = SEG_BASE + 0x04;
/// 工具矩阵计数不符。
pub const LM_ETOOL: u32 = SEG_BASE + 0x05;
/// 成功/失败互报。
pub const LM_ESTATE: u32 = SEG_BASE + 0x06;
/// 记账数不符。
pub const LM_ECOUNT: u32 = SEG_BASE + 0x07;
/// 槽位非法。
pub const LM_ESLOT: u32 = SEG_BASE + 0x08;
/// 指纹不符。
pub const LM_ESEAL: u32 = SEG_BASE + 0x09;
/// 无观测。
pub const LM_ENOBS: u32 = SEG_BASE + 0x0a;
/// 矩阵不平。
pub const LM_EMATRIX: u32 = SEG_BASE + 0x0b;
/// 闸门次序倒置。
pub const LM_EORDER: u32 = SEG_BASE + 0x0c;
/// 账本不平。
pub const LM_EBAL: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 码位约定（本模型的 errno 位；不是 glibc 的真 errno）
// ---------------------------------------------------------------------------

/// 空名：等同于请求 C，不报错。
pub const NX_OK: i32 = 0;
/// 内建档（C/POSIX）：不查归档。
pub const NX_BUILTIN: i32 = 1;
/// 空名请求（显式 `LC_ALL=`）。
pub const NX_EMPTY: i32 = 20;
/// 名字含非便携字符。
pub const NX_CHAR: i32 = 21;
/// 名字超容。
pub const NX_LONG: i32 = 22;
/// 名字以 '-' 打头。
pub const NX_DASH: i32 = 23;
/// 分隔符悬挂（`_`/`.`/`@` 收尾）。
pub const NX_TAIL: i32 = 24;
/// 分隔符重复（grammar 只容一次）。
pub const NX_SEPS: i32 = 25;

/// 归档头 magic 不符。
pub const AR_MAGIC: i32 = 40;
/// 归档头版本不符。
pub const AR_VERSION: i32 = 41;
/// 归档总长越界。
pub const AR_RANGE: i32 = 42;
/// 节不在索引里。
pub const AR_ABSENT: i32 = 43;
/// 节偏移/长度越出总长。
pub const AR_SECRANGE: i32 = 44;
/// 节 magic 不符。
pub const AR_SECMAGIC: i32 = 45;
/// 节版本不符。
pub const AR_SECVER: i32 = 46;
/// 节长不足（读任何字符表之前拒收）。
pub const AR_SHORT: i32 = 47;
/// LC_IDENTIFICATION 自述与请求名不符。
pub const AR_IDENT: i32 = 48;

/// 内建 C 的字符表判死。
pub const BRK_CTYPE: i32 = 60;

/// LOCPATH 旁路且目录缺失（整档不可用，头档故障）。
pub const AR_LOC: i32 = 49;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 请求解析闸。
pub const OP_PARSE: u8 = 0x01;
/// 名字合法性闸。
pub const OP_NAME: u8 = 0x02;
/// 归档头闸。
pub const OP_HEADER: u8 = 0x03;
/// 归档节闸。
pub const OP_SECT: u8 = 0x04;
/// 降级分档。
pub const OP_TIER: u8 = 0x05;
/// 矩阵统计。
pub const OP_MATRIX: u8 = 0x06;
/// 语料对判。
pub const OP_CORPUS: u8 = 0x07;

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

// ---------------------------------------------------------------------------
// 数字转文本（无格式宏）
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

/// 码位文本（十六进制，含 `0x`）。
pub fn code_text(v: u32) -> String {
    let mut s = String::from("0x");
    s.push_str(hex_text(v, 8).as_str());
    s
}

/// 小写十六进制（无 0x）。
pub fn hex8(v: u32) -> String {
    hex_text(v, 8)
}

/// u8 文本。
pub fn u8_text(v: u8) -> String {
    dec_text(v as u32)
}

/// i32 文本（公开入口）。
pub fn i32_pub_text(v: i32) -> String {
    i32_text(v)
}

/// 名字文本（空名渲染成 `<null>`，便于逐字对账）。
pub fn name_text(n: &str) -> String {
    if n.is_empty() {
        return String::from("<null>");
    }
    String::from(n)
}

// ---------------------------------------------------------------------------
// 文案（逐字对齐真实 loader / coreutils 的告警）
// ---------------------------------------------------------------------------

/// glibc：坏名请求（setlocale 返回 NULL + EINVAL 的现场文案）。
pub fn msg_unsupported(name: &str, cat: Category) -> String {
    let mut s = String::from("bash: warning: setlocale: ");
    s.push_str(cat.name());
    s.push_str(": \"");
    s.push_str(name_text(name).as_str());
    s.push_str("\" (unsupported locale setting)");
    s
}

/// glibc：默认档也落不下去（LANG/LC_ALL 坏名的经典一行）。
pub fn msg_default_locale_fail(name: &str) -> String {
    let mut s = String::from("locale: Cannot set LC_ALL to default locale: ");
    s.push_str(name_text(name).as_str());
    s
}

/// glibc：归档 magic 不符（locarchive.h 里 "log1"/"loc1"）。
pub fn msg_bad_magic(name: &str, path: &str, got: &str) -> String {
    let mut s = String::from("locale file for locale ");
    s.push_str(name);
    s.push_str(" of archive ");
    s.push_str(path);
    s.push_str(": bad magic!: [");
    s.push_str(got);
    s.push(']');
    s
}

/// glibc：归档版本不符。
pub fn msg_wrong_version(name: &str, path: &str, got: u32) -> String {
    let mut s = String::from("locale file for locale ");
    s.push_str(name);
    s.push_str(" of archive ");
    s.push_str(path);
    s.push_str(": wrong version ");
    s.push_str(u32_text(got).as_str());
    s.push_str(", cannot load locale");
    s
}

/// glibc：节越界。
pub fn msg_section_range(name: &str, cat: Category, off: u32, len: u32, total: u32) -> String {
    let mut s = String::from("cannot map section ");
    s.push_str(cat.name());
    s.push_str(" of locale ");
    s.push_str(name);
    s.push_str(": offset ");
    s.push_str(u32_text(off).as_str());
    s.push_str(" length ");
    s.push_str(u32_text(len).as_str());
    s.push_str(" exceeds archive size ");
    s.push_str(u32_text(total).as_str());
    s
}

/// glibc：节短到读不出字符表。
pub fn msg_section_short(name: &str, cat: Category, len: u32, need: u32) -> String {
    let mut s = String::from("locale data for ");
    s.push_str(name);
    s.push('/');
    s.push_str(cat.name());
    s.push_str(" truncated: got ");
    s.push_str(u32_text(len).as_str());
    s.push_str(" bytes, need ");
    s.push_str(u32_text(need).as_str());
    s
}

/// LOCPATH 旁路（目录在但语言目录不在 → 归档整档被跳过）。
pub fn msg_locpath_miss(path: &str, name: &str) -> String {
    let mut s = String::from("cannot open locale directory ");
    s.push_str(path);
    s.push('/');
    s.push_str(name);
    s.push_str(": No such file or directory");
    s
}

/// 内建 C 字符表判死。
pub fn msg_built_in_dead() -> String {
    String::from("fatal: built-in C charmap failed self-check, refusing to classify bytes")
}

/// 数值轴回退未记账（伪成功的显式一行）。
pub fn msg_unlogged(cat: Category, tool: &str) -> String {
    let mut s = String::from("warning: locale downgrade not accounted for ");
    s.push_str(cat.short());
    s.push_str(" in ");
    s.push_str(tool);
    s
}

/// 工具在 broken 档的显式报错文案。
pub fn msg_tool_fatal(tool: &str, cat: Category) -> String {
    let mut s = String::from(tool);
    s.push_str(": cannot proceed with an untrusted LC_");
    match cat {
        Category::Ctype => s.push_str("CTYPE charmap"),
        Category::Numeric => s.push_str("NUMERIC decimal point"),
        Category::Time => s.push_str("TIME format"),
        Category::Collate => s.push_str("COLLATE rule"),
        Category::Messages => s.push_str("MESSAGES response table"),
        Category::Identification => s.push_str("IDENTIFICATION record"),
    }
    s.push_str(": Illegal byte sequence");
    s
}

// ---------------------------------------------------------------------------
// 档位枚举：阶段 / 来源 / 注记
// ---------------------------------------------------------------------------

/// 降级链的阶段。order() 单调，任何倒退即 `LM_EORDER`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// 请求解析（LC_ALL/LC_x/LANG）。
    Request,
    /// 名字合法性。
    Name,
    /// 归档头（magic/version/总长）。
    Header,
    /// 归档节表（索引与边界）。
    Section,
    /// 节内容（magic/version/长度下限）。
    Content,
    /// 字符表（真正开始按表分类字节）。
    Table,
    /// 降级记账。
    Downgrade,
    /// 就绪（结果可用于工具）。
    Ready,
}

impl Stage {
    /// 次序。
    pub fn order(&self) -> u8 {
        match self {
            Stage::Request => 0,
            Stage::Name => 1,
            Stage::Header => 2,
            Stage::Section => 3,
            Stage::Content => 4,
            Stage::Table => 5,
            Stage::Downgrade => 6,
            Stage::Ready => 7,
        }
    }

    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Stage::Request => "request",
            Stage::Name => "name",
            Stage::Header => "header",
            Stage::Section => "section",
            Stage::Content => "content",
            Stage::Table => "table",
            Stage::Downgrade => "downgrade",
            Stage::Ready => "ready",
        }
    }

    /// 是否在"碰任何字符表之前"。
    pub fn before_table(&self) -> bool {
        match self {
            Stage::Request | Stage::Name | Stage::Header | Stage::Section | Stage::Content => true,
            _ => false,
        }
    }

    /// 结构闸阶段（头/节/内容三档）。
    pub fn structural(&self) -> bool {
        match self {
            Stage::Header | Stage::Section | Stage::Content => true,
            _ => false,
        }
    }
}

/// 名字来自哪一档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Src {
    /// LC_ALL 非空。
    LcAll,
    /// LC_ALL 已设且为空串（→ 请求 C）。
    LcAllEmpty,
    /// LC_<类别> 非空。
    LcCat,
    /// LANG 非空。
    Lang,
    /// 三档都不可用（缺席或空）→ 默认 C。
    Default,
}

impl Src {
    /// 渲染码（percat 每类 3 位）。
    pub fn code(&self) -> u32 {
        match self {
            Src::LcAll => 0,
            Src::LcAllEmpty => 1,
            Src::LcCat => 2,
            Src::Lang => 3,
            Src::Default => 4,
        }
    }

    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Src::LcAll => "LC_ALL",
            Src::LcAllEmpty => "LC_ALL-empty",
            Src::LcCat => "LC_x",
            Src::Lang => "LANG",
            Src::Default => "default-C",
        }
    }

    /// 空串处理是否"算命令"（只有 A 档如此）。
    pub fn empty_is_request(&self) -> bool {
        match self {
            Src::LcAll | Src::LcAllEmpty => true,
            _ => false,
        }
    }
}

/// 结果注记。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Note {
    /// 目标 locale 全套节可用。
    FullOk,
    /// 逐类回退且已记账。
    Fallback,
    /// 请求的就是内建档。
    BuiltIn,
    /// 归档整档不可用。
    ArchiveDead,
    /// 名字非法。
    NameBad,
    /// 结构拒收发生在字符表之前。
    Structural,
    /// 数值轴回退未记账。
    Pseudo,
    /// 内建 C 字符表判死。
    Broken,
}

impl Note {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Note::FullOk => "full-ok",
            Note::Fallback => "fallback",
            Note::BuiltIn => "built-in",
            Note::ArchiveDead => "archive-dead",
            Note::NameBad => "name-bad",
            Note::Structural => "structural",
            Note::Pseudo => "pseudo-ok",
            Note::Broken => "broken-C",
        }
    }

    /// 是否功能仍正确（可用于降级链的"可用"侧）。
    pub fn usable(&self) -> bool {
        match self {
            Note::FullOk | Note::Fallback | Note::BuiltIn => true,
            _ => false,
        }
    }

    /// 是否要求显式报错。
    pub fn loud(&self) -> bool {
        match self {
            Note::Broken | Note::Pseudo => true,
            _ => false,
        }
    }

    /// 是否结构类注记。
    pub fn structural(&self) -> bool {
        match self {
            Note::Structural | Note::ArchiveDead => true,
            _ => false,
        }
    }
}
// ---------------------------------------------------------------------------
// 名字合法性
// ---------------------------------------------------------------------------

/// 名字裁定：是否可用、是否内建档、码位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NameVerdict {
    /// 是否可用（内建档与合法名皆为真）。
    pub accept: bool,
    /// 是否内建档（`C`/`POSIX`，不查归档）。
    pub builtin: bool,
    /// 码位（NX_*）。
    pub code: i32,
}

impl NameVerdict {
    /// 合法（需要查归档）。
    pub const fn legal() -> NameVerdict {
        NameVerdict { accept: true, builtin: false, code: NX_OK }
    }

    /// 内建档。
    pub const fn built() -> NameVerdict {
        NameVerdict { accept: true, builtin: true, code: NX_BUILTIN }
    }

    /// 拒收。
    pub const fn reject(code: i32) -> NameVerdict {
        NameVerdict { accept: false, builtin: false, code: code }
    }

    /// 是否拒收。
    pub fn rejected(&self) -> bool {
        !self.accept
    }

    /// 码位文本。
    pub fn code_name(&self) -> &'static str {
        match self.code {
            NX_OK => "legal",
            NX_BUILTIN => "builtin",
            NX_EMPTY => "empty",
            NX_CHAR => "bad-char",
            NX_LONG => "too-long",
            NX_DASH => "lead-dash",
            NX_TAIL => "tail-sep",
            _ => "dup-sep",
        }
    }
}

/// POSIX 便携字符集 `[A-Za-z0-9_+.@-]`。
pub fn is_portable(b: u8) -> bool {
    match b {
        b'A'..=b'Z' => true,
        b'a'..=b'z' => true,
        b'0'..=b'9' => true,
        b'_' => true,
        b'+' => true,
        b'.' => true,
        b'@' => true,
        b'-' => true,
        _ => false,
    }
}

/// 分隔符计数（`_`/`.`/`@` 各至多一次）。
pub fn count_sep(n: &str, sep: u8) -> u8 {
    let bytes = n.as_bytes();
    let mut c = 0u8;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            if *b == sep {
                c = c.saturating_add(1);
            }
        }
        i += 1;
    }
    c
}

/// 字节是否字母。
pub fn is_alpha(b: u8) -> bool {
    match b {
        b'A'..=b'Z' | b'a'..=b'z' => true,
        _ => false,
    }
}

/// 语言码段合法（1..8 字母；`C`/`POSIX` 也走这条）。
pub fn lang_field_ok(n: &str) -> bool {
    let bytes = n.as_bytes();
    let end = match bytes.iter().position(|b| *b == b'_' || *b == b'.' || *b == b'@') {
        Some(p) => p,
        None => bytes.len(),
    };
    if end == 0 || end > 8 {
        return false;
    }
    let mut i = 0usize;
    while i < end {
        match bytes.get(i) {
            Some(b) if is_alpha(*b) => {}
            _ => return false,
        }
        i += 1;
    }
    true
}

/// 名字合法性闸。拒收次序：内建 → 空 → 超容 → '-' 打头 → 非法字符 → 分隔重复 → 悬挂。
pub fn check_name(n: &str) -> NameVerdict {
    if n.is_empty() {
        return NameVerdict { accept: true, builtin: false, code: NX_EMPTY };
    }
    if n == "C" || n == "POSIX" {
        return NameVerdict::built();
    }
    if n.len() > NAME_MAX {
        return NameVerdict::reject(NX_LONG);
    }
    let bytes = n.as_bytes();
    match bytes.get(0) {
        Some(b'-') => return NameVerdict::reject(NX_DASH),
        _ => {}
    }
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes.get(i) {
            Some(b) if is_portable(*b) => {}
            _ => return NameVerdict::reject(NX_CHAR),
        }
        i += 1;
    }
    if count_sep(n, b'_') > 1 || count_sep(n, b'.') > 1 || count_sep(n, b'@') > 1 {
        return NameVerdict::reject(NX_SEPS);
    }
    match bytes.get(bytes.len().saturating_sub(1)) {
        Some(b'_') | Some(b'.') | Some(b'@') => return NameVerdict::reject(NX_TAIL),
        _ => {}
    }
    if !lang_field_ok(n) {
        return NameVerdict::reject(NX_CHAR);
    }
    NameVerdict::legal()
}

/// 是否内建档。
pub fn is_builtin_name(n: &str) -> bool {
    n == "C" || n == "POSIX"
}

// ---------------------------------------------------------------------------
// 环境槽与请求解析
// ---------------------------------------------------------------------------

/// 一个环境变量槽：区分"缺席"与"已设空串"。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    /// 是否已设（setenv 过，哪怕值为空）。
    pub set: bool,
    /// 值。
    pub name: &'static str,
}

impl Slot {
    /// 缺席。
    pub const fn off() -> Slot {
        Slot { set: false, name: "" }
    }

    /// 已设空串。
    pub const fn empty() -> Slot {
        Slot { set: true, name: "" }
    }

    /// 已设非空。
    pub const fn val(n: &'static str) -> Slot {
        Slot { set: true, name: n }
    }

    /// 是否可用（已设且非空）。
    pub fn used(&self) -> bool {
        match self.set {
            true => !self.name.is_empty(),
            false => false,
        }
    }

    /// 渲染：`NAME=value` / `NAME=<null>` / `NAME=<unset>`。
    pub fn render(&self, key: &'static str) -> String {
        let mut s = String::from(key);
        s.push('=');
        if !self.set {
            s.push_str("<unset>");
        } else if self.name.is_empty() {
            s.push_str("<null>");
        } else {
            s.push_str(self.name);
        }
        s
    }
}

/// 请求来源。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Wanted {
    /// 最终供名的档位。
    pub src: Src,
    /// 供名的名字（空槽的语义由 src 决定）。
    pub name: &'static str,
}

/// 一次 setlocale 请求的环境描述符。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Locale {
    /// LC_ALL。
    pub lc_all: Slot,
    /// LC_<类别>，下标 = 类别号 - 1。
    pub cats: [Slot; CATS],
    /// LANG。
    pub lang: Slot,
}

impl Locale {
    /// 三档全缺席（= 默认 C）。
    pub const fn none() -> Locale {
        Locale { lc_all: Slot::off(), cats: [Slot::off(); CATS], lang: Slot::off() }
    }

    /// 仅 LC_ALL。
    pub const fn all(v: Slot) -> Locale {
        Locale { lc_all: v, cats: [Slot::off(); CATS], lang: Slot::off() }
    }

    /// 仅 LANG。
    pub const fn lang_only(v: Slot) -> Locale {
        Locale { lc_all: Slot::off(), cats: [Slot::off(); CATS], lang: v }
    }

    /// 取某类别的 LC_<类别> 槽。
    pub fn cat(&self, i: usize) -> Slot {
        if i < 1 || i >= SECS {
            return Slot::off();
        }
        match self.cats.get(i - 1) {
            Some(s) => *s,
            None => Slot::off(),
        }
    }

    /// 该 LC_<类别> 的环境变量名。
    pub fn cat_key(i: usize) -> &'static str {
        match i {
            1 => "LC_CTYPE",
            2 => "LC_NUMERIC",
            3 => "LC_TIME",
            4 => "LC_COLLATE",
            5 => "LC_MESSAGES",
            _ => "LC_UNKNOWN",
        }
    }

    /// 优先级解析：`LC_ALL` > `LC_<类别>` > `LANG` > 默认 C。
    ///
    /// 空串在 A 档算命令（落 C），在 B/C 档算沉默（继续下放）。
    pub fn wanted(&self, i: usize) -> Wanted {
        match self.lc_all.set {
            true => {
                if self.lc_all.name.is_empty() {
                    return Wanted { src: Src::LcAllEmpty, name: "C" };
                }
                return Wanted { src: Src::LcAll, name: self.lc_all.name };
            }
            false => {}
        }
        let c = self.cat(i);
        if c.used() {
            return Wanted { src: Src::LcCat, name: c.name };
        }
        match self.lang.set {
            true => {
                if self.lang.name.is_empty() {
                    return Wanted { src: Src::Default, name: "C" };
                }
                return Wanted { src: Src::Lang, name: self.lang.name };
            }
            false => {}
        }
        Wanted { src: Src::Default, name: "C" }
    }

    /// 逐类来源指纹：每类 3 位，类别 i 占位 `3*(i-1)`。
    pub fn percat(&self) -> u32 {
        let mut acc = 0u32;
        let mut i = 1usize;
        while i < SECS {
            let w = self.wanted(i);
            let shift = 3u32 * (i as u32 - 1);
            acc |= (w.src.code() & 0x7) << shift;
            i += 1;
        }
        acc
    }

    /// 五个类别的供名是否同一个（归档只有单一目标档）。
    pub fn uniform(&self) -> bool {
        let first = self.wanted(1).name;
        let mut i = 2usize;
        while i < SECS {
            if self.wanted(i).name != first {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 去重后的非内建供名（最多 REQ_MAX 个）。
    pub fn distinct(&self) -> ([&'static str; REQ_MAX], u8) {
        let mut out = [""; REQ_MAX];
        let mut n = 0u8;
        let mut i = 1usize;
        while i < SECS {
            let w = self.wanted(i);
            if is_builtin_name(w.name) {
                i += 1;
                continue;
            }
            let mut seen = false;
            let mut j = 0usize;
            while j < n as usize {
                match out.get(j) {
                    Some(v) if *v == w.name => seen = true,
                    _ => {}
                }
                j += 1;
            }
            if !seen {
                if let Some(dst) = out.get_mut(n as usize) {
                    *dst = w.name;
                }
                n = n.saturating_add(1);
            }
            i += 1;
        }
        (out, n)
    }

    /// 环境行渲染（`LC_ALL=… LANG=…`）。
    pub fn env_text(&self) -> String {
        let mut s = self.lc_all.render("LC_ALL");
        s.push(' ');
        let mut t = self.lang.render("LANG");
        s.push_str(t.as_str());
        let mut i = 1usize;
        let mut any = false;
        while i < SECS {
            let c = self.cat(i);
            if c.set {
                any = true;
                s.push(' ');
                let key = Locale::cat_key(i);
                let one = c.render(key);
                s.push_str(one.as_str());
            }
            i += 1;
        }
        if !any {
            t = String::from(" LC_x=<unset>");
            s.push_str(t.as_str());
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 归档
// ---------------------------------------------------------------------------

/// 归档里的一节。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Section {
    /// 是否在节索引里。
    pub present: bool,
    /// 节偏移。
    pub off: u32,
    /// 节长。
    pub len: u32,
    /// 节内 magic 是否匹配。
    pub magic: bool,
    /// 节内版本是否匹配。
    pub version: bool,
}

impl Section {
    /// 可用节。
    pub const fn good(off: u32, len: u32) -> Section {
        Section { present: true, off: off, len: len, magic: true, version: true }
    }

    /// 缺失节。
    pub const fn missing() -> Section {
        Section { present: false, off: 0, len: 0, magic: false, version: false }
    }

    /// 越界节。
    pub const fn oob(off: u32, len: u32) -> Section {
        Section { present: true, off: off, len: len, magic: true, version: true }
    }

    /// 截断节（结构可读、表不可用）。
    pub const fn short(off: u32, len: u32) -> Section {
        Section { present: true, off: off, len: len, magic: true, version: true }
    }

    /// 头坏节。
    pub const fn bad_magic(off: u32, len: u32) -> Section {
        Section { present: true, off: off, len: len, magic: false, version: true }
    }

    /// 版本坏节。
    pub const fn bad_version(off: u32, len: u32) -> Section {
        Section { present: true, off: off, len: len, magic: true, version: false }
    }

    /// 空白节。
    pub const fn blank() -> Section {
        Section::missing()
    }
}

/// 归档描述符。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Archive {
    /// 归档路径。
    pub path: &'static str,
    /// 是否走 LOCPATH 旁路（旁路时整档被跳过）。
    pub locpath: bool,
    /// LOCPATH 目录里的语言目录是否存在。
    pub locpath_ok: bool,
    /// 归档头 magic 字节。
    pub magic: &'static str,
    /// 归档头版本。
    pub version: u32,
    /// 归档总长。
    pub total: u32,
    /// LC_IDENTIFICATION 自述的语言名。
    pub label: &'static str,
    /// 六节表。
    pub secs: [Section; SECS],
}

/// 一代归档的合法 magic（glibc 老档 "log1"，新档 "loc1"）。
pub const MAGIC_LOG1: &str = "log1";
/// 新档 magic。
pub const MAGIC_LOC1: &str = "loc1";
/// 坏 magic。
pub const MAGIC_BAD: &str = "log0";
/// 归档版本。
pub const ARC_VER: u32 = 1;
/// 归档总长下限。
pub const ARC_MIN_TOTAL: u32 = 4096;

impl Archive {
    /// 全节可用的标准档。
    pub const fn complete(label: &'static str) -> Archive {
        Archive {
            path: "/usr/lib/locale/locale-archive",
            locpath: false,
            locpath_ok: false,
            magic: MAGIC_LOC1,
            version: ARC_VER,
            total: 65536,
            label: label,
            secs: [
                Section::good(0, 32),
                Section::good(64, 4096),
                Section::good(4160, 48),
                Section::good(4208, 512),
                Section::good(4720, 1024),
                Section::good(5744, 64),
            ],
        }
    }

    /// 指定节缺失的档。
    pub const fn missing(i: usize, label: &'static str) -> Archive {
        let mut a = Archive::complete(label);
        a.secs[i] = Section::missing();
        a
    }

    /// 指定节截断的档。
    pub const fn shortened(i: usize, len: u32, label: &'static str) -> Archive {
        let mut a = Archive::complete(label);
        a.secs[i] = Section::short(a.secs[i].off, len);
        a
    }

    /// 头 magic 坏的档（整档不可用）。
    pub const fn bad_magic(label: &'static str) -> Archive {
        let mut a = Archive::complete(label);
        a.magic = MAGIC_BAD;
        a
    }

    /// 头版本不符的档（整档不可用）。
    pub const fn bad_version(label: &'static str) -> Archive {
        let mut a = Archive::complete(label);
        a.version = 2;
        a
    }

    /// 总长小于下限的档（截断下载）。
    pub const fn truncated(label: &'static str) -> Archive {
        let mut a = Archive::complete(label);
        a.total = 2048;
        a
    }

    /// 走 LOCPATH 旁路且语言目录缺失的档。
    pub const fn locpath_miss(label: &'static str) -> Archive {
        let mut a = Archive::complete(label);
        a.locpath = true;
        a.locpath_ok = false;
        a
    }

    /// 走 LOCPATH 旁路且语言目录在（改按分文件装载，本卡视为整档跳过但目录可用）。
    pub const fn locpath_hit(label: &'static str) -> Archive {
        let mut a = Archive::complete(label);
        a.locpath = true;
        a.locpath_ok = true;
        a
    }

    /// 指定节偏移越界的档。
    pub const fn oob(i: usize, off: u32, len: u32, label: &'static str) -> Archive {
        let mut a = Archive::complete(label);
        a.secs[i] = Section::oob(off, len);
        a
    }

    /// 指定节 magic 坏的档。
    pub const fn sec_bad_magic(i: usize, label: &'static str) -> Archive {
        let mut a = Archive::complete(label);
        a.secs[i] = Section::bad_magic(a.secs[i].off, a.secs[i].len);
        a
    }

    /// 指定节版本坏的档。
    pub const fn sec_bad_version(i: usize, label: &'static str) -> Archive {
        let mut a = Archive::complete(label);
        a.secs[i] = Section::bad_version(a.secs[i].off, a.secs[i].len);
        a
    }

    /// 自述名与目录名不符的档（元数据轴，只该告警）。
    pub const fn mislabeled(want: &'static str, got: &'static str) -> Archive {
        let mut a = Archive::complete(want);
        a.label = got;
        a
    }

    /// 取节。
    pub fn sec(&self, i: usize) -> Section {
        match self.secs.get(i) {
            Some(s) => *s,
            None => Section::blank(),
        }
    }

    /// magic 是否被认。
    pub fn magic_ok(&self) -> bool {
        self.magic == MAGIC_LOG1 || self.magic == MAGIC_LOC1
    }

    /// 结构闸产物。
    pub fn gate(&self, i: usize) -> ArcGate {
        section_gate(self, i)
    }
}

/// 一次闸门产物：码位 + 阶段 + 命中类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArcGate {
    /// 是否拒收。
    pub fault: bool,
    /// 码位（AR_*）。
    pub code: i32,
    /// 拒收发生的阶段。
    pub stage: Stage,
    /// 命中的节号（0 = 头）。
    pub idx: u8,
}

impl ArcGate {
    /// 无故障。
    pub const fn none() -> ArcGate {
        ArcGate { fault: false, code: 0, stage: Stage::Ready, idx: 0 }
    }

    /// 造一个拒收。
    pub const fn bad(code: i32, stage: Stage, idx: u8) -> ArcGate {
        ArcGate { fault: true, code: code, stage: stage, idx: idx }
    }

    /// 是否发生在字符表之前。
    pub fn before_table(&self) -> bool {
        match self.fault {
            true => self.stage.order() < Stage::Table.order(),
            false => true,
        }
    }

    /// 码位文本。
    pub fn code_name(&self) -> &'static str {
        match self.code {
            AR_MAGIC => "arc-magic",
            AR_VERSION => "arc-version",
            AR_RANGE => "arc-range",
            AR_ABSENT => "sec-absent",
            AR_SECRANGE => "sec-range",
            AR_SECMAGIC => "sec-magic",
            AR_SECVER => "sec-version",
            AR_SHORT => "sec-short",
            AR_IDENT => "sec-ident",
            AR_LOC => "locpath-miss",
            _ => "none",
        }
    }
}

/// 头档闸：LOCPATH 旁路 → magic → 版本 → 总长。任一坏即整档不可用。
pub fn header_gate(a: &Archive) -> ArcGate {
    if a.locpath {
        match a.locpath_ok {
            false => return ArcGate::bad(AR_LOC, Stage::Header, 0),
            true => {}
        }
    }
    if !a.magic_ok() {
        return ArcGate::bad(AR_MAGIC, Stage::Header, 0);
    }
    if a.version != ARC_VER {
        return ArcGate::bad(AR_VERSION, Stage::Header, 0);
    }
    if a.total < ARC_MIN_TOTAL {
        return ArcGate::bad(AR_RANGE, Stage::Header, 0);
    }
    ArcGate::none()
}

/// 节档闸：在场 → 边界 → 节 magic → 节版本 → 节长下限。
///
/// 顺序不可换：边界必须先于内容校验，否则拿着越界偏移去索引就是越读；
/// 内容必须先于字符表，否则坏 magic 会被当成"表里没这个字符"。
pub fn section_gate(a: &Archive, i: usize) -> ArcGate {
    if i >= SECS {
        return ArcGate::bad(AR_ABSENT, Stage::Section, i as u8);
    }
    let s = a.sec(i);
    match s.present {
        false => return ArcGate::bad(AR_ABSENT, Stage::Section, i as u8),
        true => {}
    }
    let end = s.off.saturating_add(s.len);
    if end > a.total {
        return ArcGate::bad(AR_SECRANGE, Stage::Section, i as u8);
    }
    if !s.magic {
        return ArcGate::bad(AR_SECMAGIC, Stage::Content, i as u8);
    }
    if !s.version {
        return ArcGate::bad(AR_SECVER, Stage::Content, i as u8);
    }
    if s.len < Category::of(i).min_len() {
        return ArcGate::bad(AR_SHORT, Stage::Content, i as u8);
    }
    ArcGate::none()
}

/// LC_IDENTIFICATION 自述是否与请求名一致（元数据节只影响注记，不影响可用性）。
pub fn ident_matches(a: &Archive, want: &str) -> bool {
    a.label == want
}

/// 结构闸全表：返回第一个拒收（头档优先于节档，节档按节序）。
pub fn first_structural(a: &Archive) -> ArcGate {
    let h = header_gate(a);
    if h.fault {
        return h;
    }
    let mut i = 0usize;
    while i < SECS {
        let g = section_gate(a, i);
        if g.fault {
            return g;
        }
        i += 1;
    }
    ArcGate::none()
}

/// 该节是否可用（结构全过）。
pub fn section_usable(a: &Archive, i: usize) -> bool {
    if i >= SECS {
        return false;
    }
    let h = header_gate(a);
    if h.fault {
        return false;
    }
    !section_gate(a, i).fault
}
// ---------------------------------------------------------------------------
// 降级四档
// ---------------------------------------------------------------------------

/// 降级档：有序的可信度序列，不是四个错误码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// 目标 locale 全套节可用。
    Full,
    /// 部分类别可用，其余逐类回退内建 C 并记账。
    Partial,
    /// 归档整体不可用，全部类别退回内建 C，功能仍正确。
    BuiltC,
    /// 连内建 C 的字符表都判死：必须显式报错。
    Broken,
}

impl Tier {
    /// 下标。
    pub fn index(&self) -> usize {
        match self {
            Tier::Full => 0,
            Tier::Partial => 1,
            Tier::BuiltC => 2,
            Tier::Broken => 3,
        }
    }

    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Tier::Full => "full",
            Tier::Partial => "partial",
            Tier::BuiltC => "built-in-C",
            Tier::Broken => "broken",
        }
    }

    /// 可信度秩（越大越不可信）。
    pub fn rank(&self) -> u8 {
        match self {
            Tier::Full => 0,
            Tier::Partial => 1,
            Tier::BuiltC => 2,
            Tier::Broken => 3,
        }
    }

    /// 功能是否仍正确（broken 除外，其余都可运行）。
    pub fn usable(&self) -> bool {
        match self {
            Tier::Broken => false,
            _ => true,
        }
    }

    /// 是否必须显式报错。
    pub fn loud(&self) -> bool {
        match self {
            Tier::Broken => true,
            _ => false,
        }
    }

    /// 是否"未记账即伪成功"的高危档（数值轴可能被静默替换）。
    pub fn silent_risk(&self) -> bool {
        match self {
            Tier::Partial | Tier::BuiltC => true,
            _ => false,
        }
    }

    /// 下标转档。
    pub fn of(i: usize) -> Tier {
        match i {
            0 => Tier::Full,
            1 => Tier::Partial,
            2 => Tier::BuiltC,
            _ => Tier::Broken,
        }
    }
}

/// 一次降级链的实测结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// 结论所在阶段。
    pub stage: Stage,
    /// 首个拒收码位（0 = 无）。
    pub code: i32,
    /// 降级档。
    pub tier: Tier,
    /// 由内建 C 供表的类别掩码（按请求或按回退）。
    pub c_used: u32,
    /// 请求了目标档却落回内建 C 的类别掩码。
    pub fall: u32,
    /// 记账数（回退且已登记）。
    pub acct: u8,
    /// 数值轴回退未记账的类别数（伪成功的类别级定义）。
    pub pseudo: u8,
    /// 强制记账工具中未登记的个数（伪成功的工具级定义）。
    pub unlogged: u8,
    /// 行为改变的工具数。
    pub tools: u8,
    /// 必须显式报错的工具数。
    pub fatal: u8,
    /// 注记。
    pub note: Note,
    /// 逐类来源指纹。
    pub percat: u32,
    /// LC_IDENTIFICATION 自述与请求名不符（只告警，不降级）。
    pub ident_bad: bool,
    /// 内建 C 字符表自检是否判死（本轮是否真的用到由档结论决定）。
    pub c_dead: bool,
}

impl Outcome {
    /// 空白结果。
    pub const fn blank() -> Outcome {
        Outcome {
            stage: Stage::Request,
            code: 0,
            tier: Tier::Full,
            c_used: 0,
            fall: 0,
            acct: 0,
            pseudo: 0,
            unlogged: 0,
            tools: 0,
            fatal: 0,
            note: Note::FullOk,
            percat: 0,
            ident_bad: false,
            c_dead: false,
        }
    }

    /// 功能是否仍正确（broken 或未记账的数值轴回退都算不正确）。
    pub fn ok(&self) -> bool {
        match self.tier {
            Tier::Broken => false,
            _ => self.pseudo == 0 && self.unlogged == 0,
        }
    }

    /// 是否目标档整套可用且 C 表也活着。
    pub fn clean(&self) -> bool {
        self.tier == Tier::Full && !self.ident_bad && self.fall == 0
    }

    /// 内建 C 判死但本轮没用到 C 表：潜在腐坏（latent）。
    pub fn rotten(&self) -> bool {
        self.c_dead && self.tier != Tier::Broken
    }

    /// 拒收是否发生在碰任何字符表之前。
    pub fn rejected_before_table(&self) -> bool {
        match self.code {
            0 => true,
            c => {
                if c == BRK_CTYPE {
                    return false;
                }
                self.stage.before_table()
            }
        }
    }

    /// 渲染档名。
    pub fn tier_text(&self) -> String {
        String::from(self.tier.name())
    }

    /// 码位行文本。
    pub fn code_text(&self) -> String {
        i32_text(self.code)
    }
}

// ---------------------------------------------------------------------------
/// 降级链：请求解析 → 名字 → 归档头 → 归档节 → 分档 → 记账 → 矩阵。
// ---------------------------------------------------------------------------

/// 走一遍降级链。
pub fn chain(req: &Locale, arc: &Archive, log_mask: u32, c_alive: bool) -> Outcome {
    chain_impl(req, arc, log_mask, c_alive, false)
}

/// 可关掉名字闸的降级链（注入"缺席判据"用）。
pub fn chain_no_name(req: &Locale, arc: &Archive, log_mask: u32, c_alive: bool) -> Outcome {
    chain_impl(req, arc, log_mask, c_alive, true)
}

/// 降级链实现。`skip_name` 为真时跳过名字闸（注入缺陷，只用于证明判据能抓到它）。
fn chain_impl(req: &Locale, arc: &Archive, log_mask: u32, c_alive: bool, skip_name: bool) -> Outcome {
    let percat = req.percat();
    let head = header_gate(arc);
    let mut c_used = 0u32;
    let mut fall = 0u32;
    let mut need = 0u32;
    let mut code = 0i32;
    let mut stage = Stage::Ready;
    let mut i = 1usize;
    while i < SECS {
        let w = req.wanted(i);
        let v = check_name(w.name);
        let bit = cat_bit(i);
        if v.builtin {
            // 内建档：不查归档，由内建 C 供表；这是"按请求供表"，不算回退。
            c_used |= bit;
        } else {
            // 该类别必须要归档里的目标档才算满足请求。
            need |= bit;
            if v.rejected() && !skip_name {
                // 名字闸先拒：整类落回内建 C，序记在 name 级，绝不碰字符表。
                c_used |= bit;
                fall |= bit;
                if code == 0 {
                    code = v.code;
                    stage = Stage::Name;
                }
            } else if head.fault {
                // 头档坏：整档不可用，逐类都落回 C，但档序记在头档那一级。
                c_used |= bit;
                fall |= bit;
                if code == 0 {
                    code = head.code;
                    stage = head.stage;
                }
            } else {
                let g = section_gate(arc, i);
                if g.fault {
                    c_used |= bit;
                    fall |= bit;
                    if code == 0 {
                        code = g.code;
                        stage = g.stage;
                    }
                }
            }
        }
        i += 1;
    }
    let want = need;
    // 元数据节自述只告警：label 不符绝不降级。
    let target = first_target(req);
    let mut ident_bad = false;
    if !target.is_empty() && !head.fault && !ident_matches(arc, target) {
        ident_bad = true;
    }

    let tier = if !c_alive && c_used != 0 {
        Tier::Broken
    } else if fall == 0 {
        Tier::Full
    } else if fall == want {
        Tier::BuiltC
    } else {
        Tier::Partial
    };

    let pseudo = popcount(fall & MASK_DATA & !log_mask);
    let unlogged = unlogged_tools(fall, log_mask);
    let tools = hit_tools(fall);
    let fatal = fatal_tools(fall, tier == Tier::Broken);
    let acct = popcount(fall & log_mask);

    let (code, stage, note) = if tier == Tier::Broken {
        (BRK_CTYPE, Stage::Table, Note::Broken)
    } else if pseudo > 0 || unlogged > 0 {
        (code, Stage::Downgrade, Note::Pseudo)
    } else if want == 0 && fall == 0 {
        // 全内建：归档根本不打开，坏档也不该出现在结论里。
        (code, stage, Note::BuiltIn)
    } else if head.fault {
        (code, stage, Note::ArchiveDead)
    } else if fall == want && want != 0 && all_name_rejected(req) {
        (code, stage, Note::NameBad)
    } else if fall != 0 {
        (code, stage, Note::Fallback)
    } else {
        (0, Stage::Ready, Note::FullOk)
    };

    Outcome {
        stage: stage,
        code: code,
        tier: tier,
        c_used: c_used,
        fall: fall,
        acct: acct,
        pseudo: pseudo,
        unlogged: unlogged,
        tools: tools,
        fatal: fatal,
        note: note,
        percat: percat,
        ident_bad: ident_bad,
        c_dead: !c_alive,
    }
}

/// 第一个非内建供名（归档只有单一目标档；全内建则返回空串）。
pub fn first_target(req: &Locale) -> &'static str {
    let mut i = 1usize;
    while i < SECS {
        let w = req.wanted(i);
        if !is_builtin_name(w.name) {
            return w.name;
        }
        i += 1;
    }
    ""
}

/// 是否五个请求名的拒收全部来自名字闸。
pub fn all_name_rejected(req: &Locale) -> bool {
    let mut i = 1usize;
    let any = false;
    let mut counted = 0u8;
    while i < SECS {
        let w = req.wanted(i);
        let v = check_name(w.name);
        if !v.builtin && v.rejected() {
            counted = counted.saturating_add(1);
        } else if !v.builtin {
            return false;
        }
        i += 1;
    }
    let _ = any;
    counted > 0
}

/// 该掩码里的类别是否都被记账。
pub fn logged_all(mask: u32, log_mask: u32) -> bool {
    mask & !log_mask == 0
}

/// 掩码里未被记账的数值轴类别。
pub fn unlogged_data(mask: u32, log_mask: u32) -> u8 {
    popcount(mask & MASK_DATA & !log_mask)
}

// ---------------------------------------------------------------------------
// 工具 × 降级矩阵
// ---------------------------------------------------------------------------

/// 降级后工具侧的具体差异。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Behav {
    /// 与 locale 无关，输出恒等。
    Identical,
    /// 只有诊断文案换语言。
    MsgOnly,
    /// 排序按 ASCII 码序（collation 规则丢失）。
    SortAscii,
    /// 小数点回到 `.`（或反向），数值串本身变了。
    DecimalShift,
    /// 日期格式与月名/周名回到 C 的 `%a %b %e %H:%M:%S %Y`。
    DateFormat,
    /// 字符宽度回到字节宽度：计数与非 ASCII 引号都变。
    WidthBytes,
    /// 肯定词表回到 ASCII `^[yY]`：本地化回答被当否定。
    RespAscii,
}

impl Behav {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Behav::Identical => "identical",
            Behav::MsgOnly => "msg-language",
            Behav::SortAscii => "sort-ascii",
            Behav::DecimalShift => "decimal-point",
            Behav::DateFormat => "date-format",
            Behav::WidthBytes => "byte-width",
            Behav::RespAscii => "answer-ascii",
        }
    }

    /// 是否真的受损（行为相对请求发生变化）。
    pub fn changed(&self) -> bool {
        match self {
            Behav::Identical => false,
            _ => true,
        }
    }

    /// 是否改变输出里的数值本身（比"呈现"更危险）。
    pub fn numeric(&self) -> bool {
        match self {
            Behav::WidthBytes | Behav::DecimalShift => true,
            _ => false,
        }
    }

    /// 是否只改诊断文案。
    pub fn cosmetic(&self) -> bool {
        match self {
            Behav::MsgOnly => true,
            _ => false,
        }
    }
}

/// 工具描述符。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tool {
    /// 工具名。
    pub name: &'static str,
    /// 载荷依赖的类别掩码（不含诊断通道）。
    pub deps: u32,
    /// 主差异轴文本。
    pub axis: &'static str,
    /// 降级时是否必须逐类记账（否则即伪成功）。
    pub must_report: bool,
    /// 输出载荷是否恒为字节通路。
    pub byte_only: bool,
    /// broken 档是否必须显式报错。
    pub broken_fatal: bool,
    /// 是否经本地化诊断通道打印。
    pub diag: bool,
    /// 该工具的一句话行为判据。
    pub rule: &'static str,
}

impl Tool {
    /// 空白工具行。
    pub const fn blank() -> Tool {
        Tool {
            name: "",
            deps: 0,
            axis: "无",
            must_report: false,
            byte_only: true,
            broken_fatal: false,
            diag: false,
            rule: "",
        }
    }

    /// 载荷是否碰数值轴类别。
    pub fn touches_data(&self) -> bool {
        self.deps & MASK_DATA != 0
    }

    /// 载荷是否碰版式轴类别。
    pub fn touches_layout(&self) -> bool {
        self.deps & MASK_LAYOUT != 0
    }

    /// 载荷是否碰语言轴类别。
    pub fn touches_lang(&self) -> bool {
        self.deps & MASK_LANG != 0
    }

    /// 是否对 locale 完全不敏感。
    pub fn insensitive(&self) -> bool {
        self.deps == 0
    }

    /// 该工具在给定回退掩码下的具体差异。
    pub fn effect(&self, idx: usize, cmask: u32) -> Behav {
        match self.deps {
            // 恒不敏感行：即便掩码打满也不许报受损（矩阵虚高的反证）。
            0 => Behav::Identical,
            _ => effect_of(idx, cmask),
        }
    }

    /// 该工具在此掩码下**全部**被动到的类别（主轴会掩盖次轴，记账不能掩盖）。
    pub fn delta_mask(&self, cmask: u32) -> u32 {
        self.deps & cmask
    }
}

/// 20 工具抽样行（题面候选 21，剔除 `whoami`：`id -un` 的真子集）。
pub const TOOL_LIST: [Tool; TOOLS] = [
    Tool {
        name: "cat",
        deps: MASK_LANG,
        axis: "文案",
        must_report: false,
        byte_only: true,
        broken_fatal: false,
        diag: true,
        rule: "载荷是字节恒等通路：坏 locale 只能改它的报错语言，绝不能改它写出的字节",
    },
    Tool {
        name: "ls",
        deps: cat_bit(1) | cat_bit(4) | MASK_LANG,
        axis: "排序",
        must_report: true,
        byte_only: false,
        broken_fatal: true,
        diag: true,
        rule: "目录项序走 LC_COLLATE，非 ASCII 名引号走 LC_CTYPE——两者同档回退时必须先记账",
    },
    Tool {
        name: "cp",
        deps: MASK_LANG,
        axis: "文案",
        must_report: false,
        byte_only: true,
        broken_fatal: false,
        diag: true,
        rule: "文件内容对 CTYPE 免疫：即使 CTYPE 回退，拷贝字节仍逐字节相同",
    },
    Tool {
        name: "mv",
        deps: MASK_LANG,
        axis: "文案",
        must_report: false,
        byte_only: true,
        broken_fatal: false,
        diag: true,
        rule: "跨设备 mv 走 rename 失败再 cp+rm，locale 只影响中途的诊断文案",
    },
    Tool {
        name: "rm",
        deps: cat_bit(1) | MASK_LANG,
        axis: "应答",
        must_report: true,
        byte_only: false,
        broken_fatal: false,
        diag: true,
        rule: "YESEXPR 在 LC_MESSAGES：回退 C 后只认 ASCII y，本地化肯定回答被当否定=静默不删",
    },
    Tool {
        name: "chmod",
        deps: MASK_LANG,
        axis: "文案",
        must_report: false,
        byte_only: true,
        broken_fatal: false,
        diag: true,
        rule: "模式串是符号/八进制字面量，与 locale 无交集",
    },
    Tool {
        name: "chown",
        deps: MASK_LANG,
        axis: "文案",
        must_report: false,
        byte_only: true,
        broken_fatal: false,
        diag: true,
        rule: "属主名经 NSS 解析，与 locale 无关；降档只改 `invalid user` 的语言",
    },
    Tool {
        name: "ln",
        deps: MASK_LANG,
        axis: "文案",
        must_report: false,
        byte_only: true,
        broken_fatal: false,
        diag: true,
        rule: "链接目标串按字节存储，不做任何字符分类",
    },
    Tool {
        name: "mkdir",
        deps: MASK_LANG,
        axis: "文案",
        must_report: false,
        byte_only: true,
        broken_fatal: false,
        diag: true,
        rule: "目录名按字节创建；降级不改已建目录名，只改 `cannot create directory` 的语言",
    },
    Tool {
        name: "rmdir",
        deps: MASK_LANG,
        axis: "文案",
        must_report: false,
        byte_only: true,
        broken_fatal: false,
        diag: true,
        rule: "判空走 readdir 的字节名，`.` 与 `..` 的识别与 CTYPE 无关",
    },
    Tool {
        name: "date",
        deps: cat_bit(3) | cat_bit(1) | MASK_LANG,
        axis: "日期",
        must_report: true,
        byte_only: false,
        broken_fatal: false,
        diag: true,
        rule: "格式串与月名/周名在 LC_TIME；`date -d` 的相对词解析在 C 下只能吃英文",
    },
    Tool {
        name: "uname",
        deps: 0,
        axis: "无",
        must_report: false,
        byte_only: true,
        broken_fatal: false,
        diag: false,
        rule: "字段来自 uname(2)：broken 档也必须照常输出，不得因字符表判死而拒",
    },
    Tool {
        name: "echo",
        deps: 0,
        axis: "无",
        must_report: false,
        byte_only: true,
        broken_fatal: false,
        diag: false,
        rule: "反斜杠转义按 C 字面量展开：八进制 `\\101` 恒为 A，与 CTYPE 无关",
    },
    Tool {
        name: "printf",
        deps: cat_bit(2) | cat_bit(1) | MASK_LANG,
        axis: "小数点",
        must_report: true,
        byte_only: false,
        broken_fatal: true,
        diag: true,
        rule: "`%f` 的小数点取自 LC_NUMERIC，字段宽度按 LC_CTYPE 的字符数计——两个轴同时可变",
    },
    Tool {
        name: "test",
        deps: 0,
        axis: "无",
        must_report: false,
        byte_only: true,
        broken_fatal: false,
        diag: false,
        rule: "字符串 `<`/`>` 按 POSIX 走字节序，永不走 LC_COLLATE：把它算进受损是矩阵虚高",
    },
    Tool {
        name: "env",
        deps: MASK_LANG,
        axis: "文案",
        must_report: false,
        byte_only: true,
        broken_fatal: false,
        diag: true,
        rule: "环境块原样写字节：LANG 自己也在里面，降级不改导出值",
    },
    Tool {
        name: "id",
        deps: MASK_LANG,
        axis: "文案",
        must_report: false,
        byte_only: true,
        broken_fatal: false,
        diag: true,
        rule: "用户/组名来自 NSS；`whoami` 的语义是它的真子集，故抽样里剔除",
    },
    Tool {
        name: "basename",
        deps: 0,
        axis: "无",
        must_report: false,
        byte_only: true,
        broken_fatal: false,
        diag: false,
        rule: "后缀剥离按字节比较：多字节名与 ASCII 名在同一条代码路径上",
    },
    Tool {
        name: "dirname",
        deps: 0,
        axis: "无",
        must_report: false,
        byte_only: true,
        broken_fatal: false,
        diag: false,
        rule: "路径切分按 `/` 字节，`//` 折叠规则与 locale 无关",
    },
    Tool {
        name: "wc",
        deps: cat_bit(1) | MASK_LANG,
        axis: "宽度",
        must_report: true,
        byte_only: false,
        broken_fatal: true,
        diag: true,
        rule: "`-m`/`-L` 的计数就是 LC_CTYPE 的输出：C 档下字符数退化成字节数，必须报错或记账",
    },
];

/// 第 i 个工具。
pub fn tool_at(i: usize) -> Option<Tool> {
    TOOL_LIST.get(i).copied()
}

/// 按名字找工具下标。
pub fn tool_index(name: &str) -> usize {
    let mut i = 0usize;
    while i < TOOLS {
        match TOOL_LIST.get(i) {
            Some(t) if t.name == name => return i,
            _ => {}
        }
        i += 1;
    }
    TOOLS
}

/// 每个工具一行**自己的**判定逻辑（不是统一模板）：给定回退掩码，返回该工具的具体差异。
pub fn effect_of(idx: usize, cmask: u32) -> Behav {
    let ctype = cmask & cat_bit(CAT_CTYPE) != 0;
    let numeric = cmask & cat_bit(CAT_NUMERIC) != 0;
    let time = cmask & cat_bit(CAT_TIME) != 0;
    let collate = cmask & cat_bit(CAT_COLLATE) != 0;
    let messages = cmask & cat_bit(CAT_MESSAGES) != 0;
    match idx {
        // cat：只有诊断通道；载荷通路任何档都恒等。
        0 => match messages {
            true => Behav::MsgOnly,
            false => Behav::Identical,
        },
        // ls：序先于宽度，宽度先于文案——三轴同时回退时报最严重的那条。
        1 => {
            if collate {
                Behav::SortAscii
            } else if ctype {
                Behav::WidthBytes
            } else if messages {
                Behav::MsgOnly
            } else {
                Behav::Identical
            }
        }
        // cp：只认语言轴，数值轴回退不得影响它（否则就是拷贝被改）。
        2 => match messages {
            true => Behav::MsgOnly,
            false => Behav::Identical,
        },
        // mv：与 cp 同轴，但跨设备时诊断来自复制路径，语言轴之外不认。
        3 => match messages {
            true => Behav::MsgOnly,
            false => Behav::Identical,
        },
        // rm：应答表优先（数据丢失级），其次字符分类（多字节应答被吞）。
        4 => {
            if messages {
                Behav::RespAscii
            } else if ctype {
                Behav::WidthBytes
            } else {
                Behav::Identical
            }
        }
        // chmod：模式串永不本地化。
        5 => match messages {
            true => Behav::MsgOnly,
            false => Behav::Identical,
        },
        // chown：属主名解析走 NSS，与 locale 无关。
        6 => match messages {
            true => Behav::MsgOnly,
            false => Behav::Identical,
        },
        // ln：链接串按字节。
        7 => match messages {
            true => Behav::MsgOnly,
            false => Behav::Identical,
        },
        // mkdir：已建目录名按字节。
        8 => match messages {
            true => Behav::MsgOnly,
            false => Behav::Identical,
        },
        // rmdir：`.`/`..` 判定按字节名比较。
        9 => match messages {
            true => Behav::MsgOnly,
            false => Behav::Identical,
        },
        // date：日期轴优先，其次非 ASCII 月名的宽度轴，最后才是文案。
        10 => {
            if time {
                Behav::DateFormat
            } else if ctype {
                Behav::WidthBytes
            } else if messages {
                Behav::MsgOnly
            } else {
                Behav::Identical
            }
        }
        // uname：恒等，连 broken 也不例外。
        11 => Behav::Identical,
        // echo：转义展开按 C 字面量。
        12 => Behav::Identical,
        // printf：小数点先于宽度（数值串本身先坏）。
        13 => {
            if numeric {
                Behav::DecimalShift
            } else if ctype {
                Behav::WidthBytes
            } else if messages {
                Behav::MsgOnly
            } else {
                Behav::Identical
            }
        }
        // test：POSIX 明确按字节比较，collation 永不参与。
        14 => Behav::Identical,
        // env：导出块按字节。
        15 => match messages {
            true => Behav::MsgOnly,
            false => Behav::Identical,
        },
        // id：只换语言。
        16 => match messages {
            true => Behav::MsgOnly,
            false => Behav::Identical,
        },
        // basename：后缀按字节。
        17 => Behav::Identical,
        // dirname：切分按字节。
        18 => Behav::Identical,
        // wc：宽度轴优先，文案其次。
        19 => {
            if ctype {
                Behav::WidthBytes
            } else if messages {
                Behav::MsgOnly
            } else {
                Behav::Identical
            }
        }
        _ => Behav::Identical,
    }
}

/// 掩码位（类别下标）文本形式。
pub fn cat_bit_text(i: usize) -> String {
    u32_text(cat_bit(i))
}

/// 行为改变的工具数（`test`/`uname`/`echo`/`basename`/`dirname` 永不计入）。
pub fn hit_tools(cmask: u32) -> u8 {
    let mut n = 0u8;
    let mut i = 0usize;
    while i < TOOLS {
        match TOOL_LIST.get(i) {
            Some(t) => {
                if t.deps != 0 && effect_of(i, cmask).changed() {
                    n = n.saturating_add(1);
                }
            }
            None => {}
        }
        i += 1;
    }
    n
}

/// broken 档必须显式报错的工具数。
pub fn fatal_tools(cmask: u32, broken: bool) -> u8 {
    let mut n = 0u8;
    let mut i = 0usize;
    while i < TOOLS {
        match TOOL_LIST.get(i) {
            Some(t) => {
                let hit = match broken {
                    true => t.broken_fatal,
                    false => t.broken_fatal && effect_of(i, cmask).changed(),
                };
                if hit {
                    n = n.saturating_add(1);
                }
            }
            None => {}
        }
        i += 1;
    }
    n
}

/// 数值轴受损的工具数（改的是数字，不是呈现）。
pub fn numeric_tools(cmask: u32) -> u8 {
    let mut n = 0u8;
    let mut i = 0usize;
    while i < TOOLS {
        match TOOL_LIST.get(i) {
            Some(t) => {
                if t.deps != 0 && effect_of(i, cmask).numeric() {
                    n = n.saturating_add(1);
                }
            }
            None => {}
        }
        i += 1;
    }
    n
}

/// 呈现轴受损的工具数。
pub fn cosmetic_tools(cmask: u32) -> u8 {
    let mut n = 0u8;
    let mut i = 0usize;
    while i < TOOLS {
        match TOOL_LIST.get(i) {
            Some(t) => {
                let b = effect_of(i, cmask);
                if t.deps != 0 && b.changed() && !b.numeric() {
                    n = n.saturating_add(1);
                }
            }
            None => {}
        }
        i += 1;
    }
    n
}

/// 强制记账却未被登记的工具数（伪成功的工具级定义）。
pub fn unlogged_tools(cmask: u32, log_mask: u32) -> u8 {
    let mut n = 0u8;
    let mut i = 0usize;
    while i < TOOLS {
        match TOOL_LIST.get(i) {
            Some(t) => {
                let driven = t.deps & cmask;
                if t.must_report && driven != 0 && effect_of(i, cmask).changed() && driven & !log_mask != 0 {
                    n = n.saturating_add(1);
                }
            }
            None => {}
        }
        i += 1;
    }
    n
}

/// 恒不敏感的下标（用于矩阵虚高反证）。
pub fn insensitive_tools() -> u8 {
    let mut n = 0u8;
    let mut i = 0usize;
    while i < TOOLS {
        match TOOL_LIST.get(i) {
            Some(t) if t.insensitive() => n = n.saturating_add(1),
            _ => {}
        }
        i += 1;
    }
    n
}

/// 只经诊断通道打印的语言轴工具数。
pub fn lang_only_tools() -> u8 {
    let mut n = 0u8;
    let mut i = 0usize;
    while i < TOOLS {
        match TOOL_LIST.get(i) {
            Some(t) => {
                if t.deps == MASK_LANG {
                    n = n.saturating_add(1);
                }
            }
            None => {}
        }
        i += 1;
    }
    n
}

/// 单类别回退的命中数表（`[类别下标] = 受损工具数`）。
pub fn single_fall_hits() -> [u8; CATS] {
    let mut out = [0u8; CATS];
    let mut i = 1usize;
    while i < SECS {
        if let Some(dst) = out.get_mut(i - 1) {
            *dst = hit_tools(cat_bit(i));
        }
        i += 1;
    }
    out
}

/// 矩阵一行：一档的实测与申报。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    /// 该档槽数（实测）。
    pub total: usize,
    /// 该档申报正确数。
    pub good: usize,
    /// 该档内实测受损工具数之和。
    pub hits: u32,
    /// 该档内伪成功数之和。
    pub pseudo: u32,
}

impl Row {
    /// 空白行。
    pub const fn blank() -> Row {
        Row { total: 0, good: 0, hits: 0, pseudo: 0 }
    }

    /// 全对。
    pub fn perfect(&self) -> bool {
        self.total > 0 && self.total == self.good
    }
}

/// 四档矩阵。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Matrix {
    /// 行（full/partial/built-in-C/broken）。
    pub rows: [Row; TIER_COUNT],
}

impl Matrix {
    /// 空矩阵。
    pub const fn blank() -> Matrix {
        Matrix { rows: [Row::blank(); TIER_COUNT] }
    }

    /// 由语料统计（真算，不写表）。
    pub fn of() -> Matrix {
        let mut m = Matrix::blank();
        let cs = cases();
        let mut i = 0usize;
        while i < CORPUS {
            if let Some(c) = cs.get(i) {
                let a = c.actual();
                let t = a.tier.index();
                if t < TIER_COUNT {
                    let mut row = match m.rows.get(t) {
                        Some(r) => *r,
                        None => Row::blank(),
                    };
                    row.total += 1;
                    row.hits += a.tools as u32;
                    row.pseudo += a.pseudo as u32 + a.unlogged as u32;
                    if judge(c) == Fault::Ok {
                        row.good += 1;
                    }
                    m.rows[t] = row;
                }
            }
            i += 1;
        }
        m
    }

    /// 槽数合计。
    pub fn total(&self) -> usize {
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

    /// 正确合计。
    pub fn good(&self) -> usize {
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

    /// 受损工具数合计。
    pub fn hits(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(r) = self.rows.get(i) {
                n += r.hits;
            }
            i += 1;
        }
        n
    }

    /// 伪成功合计（绿档必须为 0：只有被驳倒的红档才允许带值）。
    pub fn pseudo_all(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(r) = self.rows.get(i) {
                n += r.pseudo;
            }
            i += 1;
        }
        n
    }

    /// 是否平。
    pub fn balanced(&self) -> bool {
        self.total() == CORPUS && self.good() == ACCEPT
    }

    /// 取一行。
    pub fn row(&self, i: usize) -> Option<Row> {
        self.rows.get(i).copied()
    }

    /// 全对行数。
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

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(r) = self.rows.get(i) {
                h = fold_word(h, r.total as u64);
                h = fold_word(h, r.good as u64);
                h = fold_word(h, r.hits as u64);
                h = fold_word(h, r.pseudo as u64);
            }
            i += 1;
        }
        h
    }

    /// 渲染一行。
    pub fn render_row(&self, i: usize) -> String {
        let mut s = String::from(Tier::of(i).name());
        s.push('=');
        let row = match self.rows.get(i) {
            Some(r) => *r,
            None => Row::blank(),
        };
        s.push_str(u32_text(row.total as u32).as_str());
        s.push('/');
        s.push_str(u32_text(row.good as u32).as_str());
        s.push_str("/hits");
        s.push_str(u32_text(row.hits).as_str());
        s
    }
}
/// "cannot open locale file" 一行（节缺失的现场文案）。
pub fn msg_missing_sec(path: &str, name: &str, cat: Category) -> String {
    let mut s = String::from("cannot open locale file ");
    s.push_str(path);
    s.push('/');
    s.push_str(name);
    s.push('/');
    s.push_str(cat.name());
    s.push_str(": No such file or directory");
    s
}

/// 归档头总长不足。
pub fn msg_header_range(path: &str, total: u32) -> String {
    let mut s = String::from("archive ");
    s.push_str(path);
    s.push_str(" size ");
    s.push_str(u32_text(total).as_str());
    s.push_str(" is below the minimum locale header size");
    s
}

// ---------------------------------------------------------------------------
// 缺陷
// ---------------------------------------------------------------------------

/// 申报与实测比对得到的缺陷。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// 一致。
    Ok,
    /// 成功/失败互报（含把 broken 报成功）。
    Estate,
    /// 降级档不符。
    Tier,
    /// 逐类来源指纹不符。
    Parse,
    /// 名字闸码位不符。
    Name,
    /// 归档结构码位不符。
    Arch,
    /// 记账数不符。
    Count,
    /// 矩阵计数不符。
    Tool,
    /// 闸门次序倒置。
    Order,
    /// 矩阵不平。
    Matrix,
    /// 计数错。
    Slot,
    /// 指纹错。
    Seal,
    /// 无观测。
    Nobs,
    /// 账本不平。
    Bal,
}

impl Fault {
    /// 缺陷码（账本拒收码即取此值）。
    pub fn code(&self) -> u32 {
        match self {
            Fault::Ok => 0,
            Fault::Estate => LM_ESTATE,
            Fault::Tier => LM_ETIER,
            Fault::Parse => LM_EPARSE,
            Fault::Name => LM_ENAME,
            Fault::Arch => LM_EARCH,
            Fault::Count => LM_ECOUNT,
            Fault::Tool => LM_ETOOL,
            Fault::Order => LM_EORDER,
            Fault::Matrix => LM_EMATRIX,
            Fault::Slot => LM_ESLOT,
            Fault::Seal => LM_ESEAL,
            Fault::Nobs => LM_ENOBS,
            Fault::Bal => LM_EBAL,
        }
    }

    /// 是否非绿。
    pub fn alive(&self) -> bool {
        match self {
            Fault::Ok => false,
            _ => true,
        }
    }

    /// 是否记账类缺陷（伪成功一定落在这里或 Estate）。
    pub fn accounting(&self) -> bool {
        match self {
            Fault::Estate | Fault::Count => true,
            _ => false,
        }
    }
}

/// 缺陷短名。
pub fn fault_short(f: Fault) -> &'static str {
    match f {
        Fault::Ok => "ok",
        Fault::Estate => "estate",
        Fault::Tier => "tier",
        Fault::Parse => "parse",
        Fault::Name => "name",
        Fault::Arch => "arch",
        Fault::Count => "count",
        Fault::Tool => "tool",
        Fault::Order => "order",
        Fault::Matrix => "matrix",
        Fault::Slot => "slot",
        Fault::Seal => "seal",
        Fault::Nobs => "nobs",
        Fault::Bal => "bal",
    }
}

/// 码位短名（渲染用）。
pub fn code_short(code: i32) -> &'static str {
    match code {
        0 => "none",
        NX_EMPTY => "empty",
        NX_CHAR => "bad-char",
        NX_LONG => "too-long",
        NX_DASH => "lead-dash",
        NX_TAIL => "tail-sep",
        NX_SEPS => "dup-sep",
        AR_MAGIC => "arc-magic",
        AR_VERSION => "arc-version",
        AR_RANGE => "arc-range",
        AR_ABSENT => "sec-absent",
        AR_SECRANGE => "sec-range",
        AR_SECMAGIC => "sec-magic",
        AR_SECVER => "sec-version",
        AR_SHORT => "sec-short",
        AR_IDENT => "sec-ident",
        AR_LOC => "locpath-miss",
        _ => "ctype-dead",
    }
}

// ---------------------------------------------------------------------------
// 语料形状
// ---------------------------------------------------------------------------

/// 一次坏 locale 场景：环境 + 归档 + 记账策略 + 内建 C 死活。
#[derive(Clone, Copy, Debug)]
pub struct Shape {
    /// 槽标签（唯一）。
    pub tag: &'static str,
    /// LC_ALL 槽。
    pub lc_all: Slot,
    /// LC_<类别> 槽表。
    pub cats: [Slot; CATS],
    /// LANG 槽。
    pub lang: Slot,
    /// locale 归档。
    pub arc: Archive,
    /// 装载器实际登记的类别掩码（记账策略）。
    pub log_mask: u32,
    /// 内建 C 的 LC_CTYPE 字符表是否活着。
    pub c_alive: bool,
}

impl Shape {
    /// 空白形状。
    pub const fn blank() -> Shape {
        Shape {
            tag: "",
            lc_all: Slot::off(),
            cats: [Slot::off(); CATS],
            lang: Slot::off(),
            arc: Archive::complete(""),
            log_mask: 0,
            c_alive: true,
        }
    }

    /// 组一个请求描述符。
    pub fn req(&self) -> Locale {
        Locale { lc_all: self.lc_all, cats: self.cats, lang: self.lang }
    }

    /// 实测结果。
    pub fn actual(&self) -> Outcome {
        let r = self.req();
        chain(&r, &self.arc, self.log_mask, self.c_alive)
    }

    /// 跳过名字闸（把非法名当合法）后的结果：注入用。
    pub fn actual_skip_name(&self) -> Outcome {
        let r = self.req();
        chain_no_name(&r, &self.arc, self.log_mask, self.c_alive)
    }

    /// 环境行。
    pub fn env_text(&self) -> String {
        self.req().env_text()
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_str(h, self.tag);
        h = fold_str(h, self.lc_all.name);
        h = fold_word(h, self.lc_all.set as u64);
        let mut i = 0usize;
        while i < CATS {
            if let Some(s) = self.cats.get(i) {
                h = fold_str(h, s.name);
                h = fold_word(h, s.set as u64);
            }
            i += 1;
        }
        h = fold_str(h, self.lang.name);
        h = fold_word(h, self.lang.set as u64);
        h = fold_str(h, self.arc.path);
        h = fold_str(h, self.arc.magic);
        h = fold_str(h, self.arc.label);
        h = fold_word(h, self.arc.total as u64);
        h = fold_word(h, self.arc.version as u64);
        h = fold_word(h, self.log_mask as u64);
        h = fold_word(h, self.c_alive as u64);
        h
    }
}

/// 全 LC_ALL 场景。
pub const fn shape_all(
    tag: &'static str,
    name: &'static str,
    arc: Archive,
    log_mask: u32,
    c_alive: bool,
) -> Shape {
    Shape {
        tag: tag,
        lc_all: Slot::val(name),
        cats: [Slot::off(); CATS],
        lang: Slot::off(),
        arc: arc,
        log_mask: log_mask,
        c_alive: c_alive,
    }
}

/// 空 LC_ALL（`LC_ALL=`）场景。
pub const fn shape_all_empty(tag: &'static str, arc: Archive, log_mask: u32, c_alive: bool) -> Shape {
    Shape {
        tag: tag,
        lc_all: Slot::empty(),
        cats: [Slot::off(); CATS],
        lang: Slot::off(),
        arc: arc,
        log_mask: log_mask,
        c_alive: c_alive,
    }
}

/// 全 LANG 场景。
pub const fn shape_lang(tag: &'static str, name: &'static str, arc: Archive, log: u32, alive: bool) -> Shape {
    Shape {
        tag: tag,
        lc_all: Slot::off(),
        cats: [Slot::off(); CATS],
        lang: Slot::val(name),
        arc: arc,
        log_mask: log,
        c_alive: alive,
    }
}

/// 空 LANG（`LANG=`）场景。
pub const fn shape_lang_empty(tag: &'static str, arc: Archive, log: u32, alive: bool) -> Shape {
    Shape {
        tag: tag,
        lc_all: Slot::off(),
        cats: [Slot::off(); CATS],
        lang: Slot::empty(),
        arc: arc,
        log_mask: log,
        c_alive: alive,
    }
}

/// 混合场景：LC_CTYPE 显式设、其余走 LANG。
pub const fn shape_ctype_split(
    tag: &'static str,
    ctype: &'static str,
    lang: &'static str,
    arc: Archive,
    log: u32,
    alive: bool,
) -> Shape {
    Shape {
        tag: tag,
        lc_all: Slot::off(),
        cats: [Slot::val(ctype), Slot::off(), Slot::off(), Slot::off(), Slot::off()],
        lang: Slot::val(lang),
        arc: arc,
        log_mask: log,
        c_alive: alive,
    }
}

/// 申报（对实测结果的一组断言）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Claim {
    /// 申报"功能仍正确"。
    pub ok: bool,
    /// 申报降级档。
    pub tier: Tier,
    /// 申报逐类来源指纹。
    pub percat: u32,
    /// 申报码位。
    pub code: i32,
    /// 申报记账数。
    pub acct: u8,
    /// 申报受损工具数。
    pub tools: u8,
    /// 申报结论阶段。
    pub stage: Stage,
}

impl Claim {
    /// 空白申报。
    pub const fn blank() -> Claim {
        Claim {
            ok: false,
            tier: Tier::Full,
            percat: 0,
            code: 0,
            acct: 0,
            tools: 0,
            stage: Stage::Request,
        }
    }

    /// 由实测反构造申报（绿格用它，红格不用）。
    pub fn of(o: Outcome) -> Claim {
        Claim {
            ok: o.ok(),
            tier: o.tier,
            percat: o.percat,
            code: o.code,
            acct: o.acct,
            tools: o.tools,
            stage: o.stage,
        }
    }

    /// 由实测反构造申报，并把"功能正确"改成假绿。
    pub const fn lie_green(o: Outcome) -> Claim {
        Claim {
            ok: true,
            tier: o.tier,
            percat: o.percat,
            code: o.code,
            acct: o.acct,
            tools: o.tools,
            stage: o.stage,
        }
    }

    /// 是否申报成功。
    pub fn success(&self) -> bool {
        self.ok
    }
}

/// 一个语料槽。
#[derive(Clone, Copy, Debug)]
pub struct Case {
    /// 形状。
    pub shape: Shape,
    /// 申报。
    pub claim: Claim,
}

impl Case {
    /// 空白槽。
    pub const fn blank() -> Case {
        Case { shape: Shape::blank(), claim: Claim::blank() }
    }

    /// 标签。
    pub fn tag(&self) -> &'static str {
        self.shape.tag
    }

    /// 实测。
    pub fn actual(&self) -> Outcome {
        self.shape.actual()
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

/// 对判次序：状态 → 档 → 逐类 → 码位 → 记账 → 矩阵 → 次序。
pub fn judge_against(claim: &Claim, a: &Outcome) -> Fault {
    if claim.success() != a.ok() {
        return Fault::Estate;
    }
    if claim.tier != a.tier {
        return Fault::Tier;
    }
    if claim.percat != a.percat {
        return Fault::Parse;
    }
    if claim.code != a.code {
        if a.code >= AR_MAGIC && a.code <= AR_IDENT || a.code == AR_LOC {
            return Fault::Arch;
        }
        return Fault::Name;
    }
    if claim.acct != a.acct {
        return Fault::Count;
    }
    if claim.tools != a.tools {
        return Fault::Tool;
    }
    if claim.stage != a.stage {
        return Fault::Order;
    }
    Fault::Ok
}

/// 槽级对判。
pub fn judge(c: &Case) -> Fault {
    judge_against(&c.claim, &c.actual())
}
// ---------------------------------------------------------------------------
// 语料（14 槽：8 成 6 败）
// ---------------------------------------------------------------------------

/// 8 个申报正确的槽 + 6 个复刻真实结果但申报错的槽。
pub const SHAPES: [Shape; CORPUS] = [
    // 0 目标档全套可用（LC_ALL 供名）。
    shape_all("full-en_US-utf8", "en_US.UTF-8", Archive::complete("en_US.UTF-8"), 0, true),
    // 1 LANG=C：内建档，不查归档。
    shape_lang("full-lang-builtin-C", "C", Archive::complete("en_US.UTF-8"), 0, true),
    // 2 仅 LC_COLLATE 节缺失：版式轴回退。
    shape_all(
        "partial-collate-absent",
        "en_US.UTF-8",
        Archive::missing(4, "en_US.UTF-8"),
        cat_bit(4) | MASK_LANG,
        true,
    ),
    // 3 LC_CTYPE 节截断（结构可读、表不可用）：数值轴回退。
    shape_all(
        "partial-ctype-truncated",
        "de_DE.UTF-8",
        Archive::shortened(1, 64, "de_DE.UTF-8"),
        MASK_CATS,
        true,
    ),
    // 4 LC_CTYPE=C 与其余 LANG 混用，MESSAGES 节缺失。
    shape_ctype_split(
        "partial-messages-mixed",
        "C",
        "fr_FR.UTF-8",
        Archive::missing(5, "fr_FR.UTF-8"),
        MASK_DATA | MASK_LANG,
        true,
    ),
    // 5 归档头 magic 坏：整档不可用，逐类回退内建 C。
    shape_all(
        "builtc-archive-magic",
        "zh_CN.UTF-8",
        Archive::bad_magic("zh_CN.UTF-8"),
        MASK_CATS,
        true,
    ),
    // 6 LOCPATH 旁路且语言目录缺失：整档不可用。
    shape_lang(
        "builtc-locpath-miss",
        "ja_JP.UTF-8",
        Archive::locpath_miss("ja_JP.UTF-8"),
        MASK_CATS,
        true,
    ),
    // 7 内建 C 字符表判死 + 请求就是 C：broken，必须显式报错。
    shape_all("broken-builtin-ctype-dead", "C", Archive::complete("C"), 0, false),
    // 8 红：C.UTF-8 需查归档且 CTYPE 缺失，实测 broken 却申报功能正确。
    shape_all("red-estate-silent-broken", "C.UTF-8", Archive::missing(1, "C.UTF-8"), 0, false),
    // 9 红：LC_NUMERIC 节缺失（partial），申报成 full。
    shape_all(
        "red-tier-partial-as-full",
        "en_US.UTF-8",
        Archive::missing(2, "en_US.UTF-8"),
        MASK_CATS,
        true,
    ),
    // 10 红：名字含非法字符（应报 bad-char），申报成 lead-dash。
    shape_all(
        "red-name-dash-as-char",
        "de_DE.UTF-8;",
        Archive::complete("de_DE.UTF-8;"),
        MASK_CATS,
        true,
    ),
    // 11 红：CTYPE 缺失且未记账（伪成功），申报已记 1 类。
    shape_all("red-count-ctype-unlogged", "zh_CN.UTF-8", Archive::missing(1, "zh_CN.UTF-8"), 0, true),
    // 12 红：只缺 LC_TIME，实际 1 个工具受损，申报 20 个全损。
    shape_all(
        "red-tool-matrix-inflated",
        "ja_JP.eucJP",
        Archive::missing(3, "ja_JP.eucJP"),
        MASK_CATS,
        true,
    ),
    // 13 红：COLLATE 节越界（结构闸在字符表之前拒收），申报拒收发生在字符表之后。
    shape_all(
        "red-order-after-table",
        "ru_RU.KOI8-R",
        Archive::oob(4, 65000, 4096, "ru_RU.KOI8-R"),
        MASK_CATS,
        true,
    ),
];

/// 与 `SHAPES` 一一对应的申报。
pub const CLAIMS: [Claim; CORPUS] = [
    Claim { ok: true, tier: Tier::Full, percat: 0, code: 0, acct: 0, tools: 0, stage: Stage::Ready },
    Claim {
        ok: true,
        tier: Tier::Full,
        percat: 14043,
        code: 0,
        acct: 0,
        tools: 0,
        stage: Stage::Ready,
    },
    Claim {
        ok: true,
        tier: Tier::Partial,
        percat: 0,
        code: AR_ABSENT,
        acct: 1,
        tools: 1,
        stage: Stage::Section,
    },
    Claim {
        ok: true,
        tier: Tier::Partial,
        percat: 0,
        code: AR_SHORT,
        acct: 1,
        tools: 5,
        stage: Stage::Content,
    },
    Claim {
        ok: true,
        tier: Tier::Partial,
        percat: 14042,
        code: AR_ABSENT,
        acct: 1,
        tools: 15,
        stage: Stage::Section,
    },
    Claim {
        ok: true,
        tier: Tier::BuiltC,
        percat: 0,
        code: AR_MAGIC,
        acct: 5,
        tools: 15,
        stage: Stage::Header,
    },
    Claim {
        ok: true,
        tier: Tier::BuiltC,
        percat: 14043,
        code: AR_LOC,
        acct: 5,
        tools: 15,
        stage: Stage::Header,
    },
    Claim {
        ok: false,
        tier: Tier::Broken,
        percat: 0,
        code: BRK_CTYPE,
        acct: 0,
        tools: 0,
        stage: Stage::Table,
    },
    Claim {
        ok: true,
        tier: Tier::Broken,
        percat: 0,
        code: BRK_CTYPE,
        acct: 0,
        tools: 5,
        stage: Stage::Table,
    },
    Claim { ok: true, tier: Tier::Full, percat: 0, code: 0, acct: 1, tools: 1, stage: Stage::Section },
    Claim {
        ok: true,
        tier: Tier::BuiltC,
        percat: 0,
        code: NX_DASH,
        acct: 5,
        tools: 15,
        stage: Stage::Name,
    },
    Claim {
        ok: false,
        tier: Tier::Partial,
        percat: 0,
        code: AR_ABSENT,
        acct: 1,
        tools: 5,
        stage: Stage::Downgrade,
    },
    Claim {
        ok: true,
        tier: Tier::Partial,
        percat: 0,
        code: AR_ABSENT,
        acct: 1,
        tools: 20,
        stage: Stage::Section,
    },
    Claim {
        ok: true,
        tier: Tier::Partial,
        percat: 0,
        code: AR_SECRANGE,
        acct: 1,
        tools: 1,
        stage: Stage::Table,
    },
];

/// 后 6 槽的期望缺陷。
pub const REJECT_FAULTS: [Fault; REJECT] =
    [Fault::Estate, Fault::Tier, Fault::Name, Fault::Count, Fault::Tool, Fault::Order];

/// 后 6 槽的缺陷码。
pub const REJECT_CODES: [u32; REJECT] = [
    LM_ESTATE, LM_ETIER, LM_ENAME, LM_ECOUNT, LM_ETOOL, LM_EORDER,
];

/// 账本 8 个成功格的值（语料总数、成、败、full、partial、built-in C、broken、红档 ppm）。
pub const VALS: [u32; ACCEPT] = [14, 8, 6, 2, 3, 2, 1, 929];

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

/// 取第 i 槽。
pub fn case_at(i: usize) -> Option<Case> {
    if i >= CORPUS {
        return None;
    }
    cases().get(i).copied()
}

/// 覆盖校验。
pub fn cover(n: usize) -> Result<(), u32> {
    if n == CORPUS {
        Ok(())
    } else {
        Err(LM_ENOBS)
    }
}

/// 第 i 槽的设计期望。
pub fn expected(i: usize) -> Option<Fault> {
    if i < ACCEPT {
        Some(Fault::Ok)
    } else {
        REJECT_FAULTS.get(i - ACCEPT).copied()
    }
}

/// 该槽结论对应哪个类别（取最低位的回退类；broken 恒为 CTYPE）。
pub fn fault_category(sh: &Shape, o: &Outcome) -> Category {
    if o.tier == Tier::Broken {
        return Category::Ctype;
    }
    let mut i = 1usize;
    while i < SECS {
        if o.fall & cat_bit(i) != 0 {
            return Category::of(i);
        }
        i += 1;
    }
    let r = sh.req();
    let mut k = 1usize;
    while k < SECS {
        if r.wanted(k).src != Src::Default {
            return Category::of(k);
        }
        k += 1;
    }
    Category::Identification
}

/// 一行诊断文案（成功时给空串；伪成功给未记账的一行）。
pub fn message_for(sh: &Shape, o: &Outcome) -> String {
    let req = sh.req();
    let target = first_target(&req);
    let shown = match target.is_empty() {
        true => "C",
        false => target,
    };
    if o.note == Note::Pseudo {
        let cat = fault_category(sh, o);
        let tool = match cat.axis() {
            Axis::Data => "wc",
            Axis::Layout => "ls",
            _ => "rm",
        };
        return msg_unlogged(cat, tool);
    }
    if o.note == Note::Broken {
        return msg_built_in_dead();
    }
    match o.code {
        0 => match o.ident_bad {
            true => {
                let mut s = String::from("locale identification says ");
                s.push_str(sh.arc.label);
                s.push_str(" but requested ");
                s.push_str(shown);
                s.push_str(" (kept, not downgraded)");
                s
            }
            false => String::new(),
        },
        AR_MAGIC => msg_bad_magic(shown, sh.arc.path, sh.arc.magic),
        AR_VERSION => msg_wrong_version(shown, sh.arc.path, sh.arc.version),
        AR_RANGE => msg_header_range(sh.arc.path, sh.arc.total),
        AR_LOC => msg_locpath_miss(sh.arc.path, shown),
        AR_ABSENT => msg_missing_sec(sh.arc.path, shown, fault_category(sh, o)),
        AR_SECRANGE => {
            let cat = fault_category(sh, o);
            let s = sh.arc.sec(cat.index());
            msg_section_range(shown, cat, s.off, s.len, sh.arc.total)
        }
        AR_SECMAGIC => msg_bad_magic(shown, sh.arc.path, sh.arc.magic),
        AR_SECVER => msg_wrong_version(shown, sh.arc.path, sh.arc.version),
        AR_SHORT => {
            let cat = fault_category(sh, o);
            let s = sh.arc.sec(cat.index());
            msg_section_short(shown, cat, s.len, cat.min_len())
        }
        AR_IDENT => msg_missing_sec(sh.arc.path, shown, Category::Identification),
        _ => {
            let mut s = msg_unsupported(shown, fault_category(sh, o));
            s.push_str("; ");
            s.push_str(msg_default_locale_fail(shown).as_str());
            s
        }
    }
}

/// 单行释义（逐字对齐判据）。
pub fn detail(c: &Case) -> String {
    let o = c.actual();
    let mut s = String::from(c.shape.tag);
    s.push_str(" env=");
    s.push_str(c.shape.env_text().as_str());
    s.push_str(" tier=");
    s.push_str(o.tier.name());
    s.push_str(" stage=");
    s.push_str(o.stage.name());
    s.push_str(" code=");
    s.push_str(i32_text(o.code).as_str());
    s.push_str(" fall=");
    s.push_str(mask_text(o.fall).as_str());
    s.push_str(" acct=");
    s.push_str(u8_text(o.acct).as_str());
    s.push_str(" tools=");
    s.push_str(u8_text(o.tools).as_str());
    s.push_str(" fatal=");
    s.push_str(u8_text(o.fatal).as_str());
    s.push_str(" note=");
    s.push_str(o.note.name());
    s.push('\n');
    s
}

/// 单行申报释义（申报侧，用于驳斥时并列打印）。
pub fn claim_text(c: &Case) -> String {
    let mut s = String::from(c.tag());
    s.push_str(" claim tier=");
    s.push_str(c.claim.tier.name());
    s.push_str(" code=");
    s.push_str(i32_text(c.claim.code).as_str());
    s.push_str(" acct=");
    s.push_str(u8_text(c.claim.acct).as_str());
    s.push_str(" tools=");
    s.push_str(u8_text(c.claim.tools).as_str());
    s.push_str(" fault=");
    s.push_str(fault_short(c.judge()));
    s.push('\n');
    s
}

// ---------------------------------------------------------------------------
// 汇总
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
    /// full 档正确数。
    pub full: u32,
    /// partial 档正确数。
    pub partial: u32,
    /// built-in C 档正确数。
    pub builtc: u32,
    /// broken 档正确数。
    pub broken: u32,
    /// 对判命中数。
    pub matched: u32,
    /// 绿档里的伪成功数（必须为 0）。
    pub pseudo: u32,
    /// 绿档里实测受损工具数之和。
    pub hits: u32,
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

    /// 闸门：全绿且绿档零伪成功。
    pub fn gate(&self) -> Result<(), u32> {
        if self.green() && self.pseudo == 0 {
            Ok(())
        } else {
            Err(LM_ECOUNT)
        }
    }

    /// 按档取绿档槽数。
    pub fn tier_accept(&self, t: Tier) -> u32 {
        match t {
            Tier::Full => self.full,
            Tier::Partial => self.partial,
            Tier::BuiltC => self.builtc,
            Tier::Broken => self.broken,
        }
    }

    /// 渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("locale-matrix corpus=");
        s.push_str(u32_text(self.total).as_str());
        s.push_str(" accept=");
        s.push_str(u32_text(self.accept).as_str());
        s.push_str(" reject=");
        s.push_str(u32_text(self.reject).as_str());
        s.push_str(" full=");
        s.push_str(u32_text(self.full).as_str());
        s.push_str(" partial=");
        s.push_str(u32_text(self.partial).as_str());
        s.push_str(" builtc=");
        s.push_str(u32_text(self.builtc).as_str());
        s.push_str(" broken=");
        s.push_str(u32_text(self.broken).as_str());
        s.push_str(" hits=");
        s.push_str(u32_text(self.hits).as_str());
        s.push_str(" pseudo=");
        s.push_str(u32_text(self.pseudo).as_str());
        s.push_str(" match=");
        s.push_str(u32_text(self.matched).as_str());
        s.push_str(" rate=");
        s.push_str(u32_text(self.rate()).as_str());
        s.push_str("ppm\n");
        s
    }
}

/// 全跑。
pub fn run() -> Result<Report, u32> {
    run_with(None)
}

/// 可注入一槽缺陷的全跑。
pub fn run_with(over: Option<(usize, Fault)>) -> Result<Report, u32> {
    if let Some((i, _)) = over {
        if i >= CORPUS {
            return Err(LM_ESLOT);
        }
    }
    let cs = cases();
    count_over(&cs, over)
}

/// 对任意槽表统计（注入"空断言集"时传空表即得 LM_ENOBS）。
pub fn count_over(cs: &[Case], over: Option<(usize, Fault)>) -> Result<Report, u32> {
    if cs.len() != CORPUS {
        return Err(LM_ENOBS);
    }
    let mut total = 0u32;
    let mut accept = 0u32;
    let mut reject = 0u32;
    let mut full = 0u32;
    let mut partial = 0u32;
    let mut builtc = 0u32;
    let mut broken = 0u32;
    let mut matched = 0u32;
    let mut pseudo = 0u32;
    let mut hits = 0u32;
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return Err(LM_ENOBS),
        };
        let a = c.actual();
        let mut f = judge_against(&c.claim, &a);
        if let Some((j, forced)) = over {
            if j == i {
                f = forced;
            }
        }
        let want = match expected(i) {
            Some(v) => v,
            None => return Err(LM_ENOBS),
        };
        total += 1;
        if f == want {
            matched += 1;
        }
        if f == Fault::Ok {
            accept += 1;
            hits += a.tools as u32;
            pseudo += (a.pseudo as u32) + (a.unlogged as u32);
            match a.tier {
                Tier::Full => full += 1,
                Tier::Partial => partial += 1,
                Tier::BuiltC => builtc += 1,
                Tier::Broken => broken += 1,
            }
        } else {
            reject += 1;
        }
        i += 1;
    }
    Ok(Report { total, accept, reject, full, partial, builtc, broken, matched, pseudo, hits })
}

/// 跳过某一档的矩阵（注入"跳过档"用）。
pub fn matrix_without(t: usize) -> Matrix {
    let mut m = Matrix::blank();
    let cs = cases();
    let mut i = 0usize;
    while i < CORPUS {
        if let Some(c) = cs.get(i) {
            let a = c.actual();
            let k = a.tier.index();
            if k == t {
                i += 1;
                continue;
            }
            if k < TIER_COUNT {
                let mut row = match m.rows.get(k) {
                    Some(r) => *r,
                    None => Row::blank(),
                };
                row.total += 1;
                row.hits += a.tools as u32;
                row.pseudo += a.pseudo as u32 + a.unlogged as u32;
                if judge(c) == Fault::Ok {
                    row.good += 1;
                }
                m.rows[k] = row;
            }
        }
        i += 1;
    }
    m
}

// ---------------------------------------------------------------------------
// 闸门
// ---------------------------------------------------------------------------

/// 单调阶段闸门。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gate {
    last: i32,
    steps: u32,
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
            return Err(LM_ESTATE);
        }
        let o = s.order() as i32;
        if o < self.last {
            return Err(LM_EORDER);
        }
        self.last = o;
        self.steps += 1;
        Ok(())
    }

    /// 关闭。
    pub fn close(&mut self) -> Result<u32, u32> {
        if self.closed {
            return Err(LM_ESTATE);
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

    /// 按一条降级链走一遍闸门：只走到结论所在档，结构拒收必须早于字符表。
    pub fn walk(o: &Outcome) -> Result<u32, u32> {
        let seq = [
            Stage::Request,
            Stage::Name,
            Stage::Header,
            Stage::Section,
            Stage::Content,
            Stage::Table,
            Stage::Downgrade,
            Stage::Ready,
        ];
        let mut g = Gate::new();
        let stop = o.stage.order() as usize;
        let mut i = 0usize;
        while i < seq.len() && i <= stop {
            match seq.get(i) {
                Some(s) => g.step(*s)?,
                None => return Err(LM_ENOBS),
            }
            i += 1;
        }
        // 结构码位一旦落在字符表档或更晚，就是"先读了坏表再报错"；
        // 伪成功档的码位只是"回退原因"的回溯引用，不参与此判。
        if o.code != 0 && o.code != BRK_CTYPE && o.note != Note::Pseudo && !o.stage.before_table()
        {
            return Err(LM_EORDER);
        }
        g.close()
    }
}

// ---------------------------------------------------------------------------
// 账本
// ---------------------------------------------------------------------------

/// 账本格。
#[derive(Clone, Copy, Debug)]
pub struct Sample {
    /// 槽位。
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

/// 账本。
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
        Ledger { rows: [Sample::blank(); SAMPLES], len: 0, root: 0, frozen: false, rejects: 0 }
    }

    /// 登记。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(LM_ESLOT);
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

    /// 失败格的码位序列。
    pub fn err_codes(&self) -> [u32; REJECT] {
        let mut out = [0u32; REJECT];
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                if !s.ok {
                    if n < REJECT {
                        if let Some(dst) = out.get_mut(n) {
                            *dst = s.code;
                        }
                        n += 1;
                    }
                }
            }
            i += 1;
        }
        out
    }

    /// 对账。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != ACCEPT || self.err_count() != REJECT {
            return Err(LM_EBAL);
        }
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(LM_EBAL),
            };
            if s.slot as usize != i {
                return Err(LM_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(LM_ESTATE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(LM_ECOUNT);
                        }
                    }
                    None => return Err(LM_EBAL),
                }
                vi += 1;
            } else {
                match REJECT_CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(LM_ECOUNT);
                        }
                    }
                    None => return Err(LM_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != ACCEPT || ci != REJECT {
            return Err(LM_EBAL);
        }
        Ok(())
    }

    /// 指纹。
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
            return Err(LM_EBAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        let a = fresh.seal();
        if a == self.seal() {
            Ok(a)
        } else {
            Err(LM_ESEAL)
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

/// `Result<u32, u32>` → 样本。
fn count_cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 建账本：8 成 6 败。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let r = run();
    let _ = l.register(count_cell(0, OP_CORPUS, r.map(|x| x.total)));
    let _ = l.register(count_cell(1, OP_CORPUS, r.map(|x| x.accept)));
    let _ = l.register(count_cell(2, OP_CORPUS, r.map(|x| x.reject)));
    let _ = l.register(count_cell(3, OP_PARSE, r.map(|x| x.full)));
    let _ = l.register(count_cell(4, OP_SECT, r.map(|x| x.partial)));
    let _ = l.register(count_cell(5, OP_TIER, r.map(|x| x.builtc)));
    let _ = l.register(count_cell(6, OP_MATRIX, r.map(|x| x.broken)));

    let red = run_with(Some((0, Fault::Count)));
    let _ = l.register(count_cell(7, OP_NAME, red.map(|x| x.rate())));

    let cs = cases();
    let mut i = 0usize;
    while i < REJECT {
        if let Some(c) = cs.get(i + ACCEPT) {
            let f = judge(c);
            let _ = l.register(Sample {
                slot: (ACCEPT + i) as u8,
                op: OP_HEADER,
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
// 四路注入
// ---------------------------------------------------------------------------

/// 注入一：空断言集（零槽的语料必须被判无观测）。
fn inject_empty_set() -> bool {
    let empty: [Case; 0] = [];
    let a = cover(0) == Err(LM_ENOBS);
    let b = count_over(&empty, None).err() == Some(LM_ENOBS);
    let c = count_over(&cases(), None).is_ok();
    a && b && c
}

/// 注入二：缺席判据（关掉名字闸后，坏名场景必须被对判驳倒）。
fn inject_absent_gate() -> bool {
    let cs = cases();
    let c = match cs.get(ACCEPT + 2) {
        Some(v) => *v,
        None => return false,
    };
    let dodgy = c.shape.actual_skip_name();
    let truth = c.actual();
    let a = judge_against(&c.claim, &dodgy) != Fault::Ok;
    let b = judge_against(&c.claim, &truth) == Fault::Name;
    let c2 = dodgy.tier == Tier::Full && truth.tier == Tier::BuiltC;
    a && b && c2
}

/// 注入三：跳过档（矩阵少算一档即不平，且合计必须差 3 槽）。
fn inject_skip_tier() -> bool {
    let full = Matrix::of();
    let skip = matrix_without(2);
    let a = full.balanced();
    let b = !skip.balanced();
    let c = skip.total() + 3 == full.total();
    let d = match skip.row(2) {
        Some(r) => r.total == 0 && r.good == 0,
        None => false,
    };
    let e = skip.hits() < full.hits();
    let f = skip.good() < full.good();
    a && b && c && d && e && f
}

/// 注入四：假绿申报（数值轴回退未记账却报功能正确）。
fn inject_false_green() -> bool {
    let cs = cases();
    let c = match cs.get(ACCEPT + 3) {
        Some(v) => *v,
        None => return false,
    };
    let a = c.actual();
    let lie = Claim::lie_green(a);
    let f = judge_against(&lie, &a);
    let g = judge_against(&c.claim, &a);
    let h = a.pseudo == 1 && a.unlogged == 5 && a.acct == 0;
    (f == Fault::Estate || f == Fault::Count) && g == Fault::Count && h
}

/// 四路注入：空断言集、缺席判据、跳过档、假绿申报。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = inject_empty_set();
    out[1] = inject_absent_gate();
    out[2] = inject_skip_tier();
    out[3] = inject_false_green();
    out
}
/// 释义首行（S3 partial-collate-absent）。
const DETAIL_COLLATE: &str = "partial-collate-absent env=LC_ALL=en_US.UTF-8 LANG=<unset> LC_x=<unset> tier=partial stage=section code=43 fall=COLLATE acct=1 tools=1 fatal=1 note=fallback\n";
/// 释义次行（S7 builtc-locpath-miss）。
const DETAIL_LOCPATH: &str = "builtc-locpath-miss env=LC_ALL=<unset> LANG=ja_JP.UTF-8 LC_x=<unset> tier=built-in-C stage=header code=49 fall=CTYPE NUMERIC TIME COLLATE MESSAGES acct=5 tools=15 fatal=3 note=archive-dead\n";

// ---------------------------------------------------------------------------
// 判据基元
// ---------------------------------------------------------------------------

/// LM01 名称与段基址。
fn name_ok() -> bool {
    let codes = [
        LM_EPARSE, LM_ENAME, LM_EARCH, LM_ETIER, LM_ETOOL, LM_ESTATE, LM_ECOUNT, LM_ESLOT,
        LM_ESEAL, LM_ENOBS, LM_EMATRIX, LM_EORDER, LM_EBAL,
    ];
    let mut i = 0usize;
    while i < codes.len() {
        let mut j = i + 1;
        while j < codes.len() {
            if codes[i] == codes[j] {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    let a = SEG_BASE == 0x6cf30;
    let b = LM_EPARSE == SEG_BASE + 0x01 && LM_EBAL == SEG_BASE + 0x0d;
    let c = LM_ESLOT == SEG_BASE + 0x08 && LM_ENOBS == SEG_BASE + 0x0a;
    let d = LM_EPARSE + 12 == LM_EBAL;
    let e = CORPUS == 14 && SAMPLES == 14 && ACCEPT == 8 && REJECT == 6;
    let f = TIER_COUNT == 4 && CATS == 5 && SECS == 6 && TOOLS == 20;
    let g = NAME_MAX == 64 && REQ_MAX == 3 && PATH_MAX == 64;
    let h = OP_PARSE == 1 && OP_CORPUS == 7 && ARC_VER == 1;
    a && b && c && d && e && f && g && h
}

/// LM02 常量与档位：类别表、掩码、文本、四档语义各自取证。
fn const_ok() -> bool {
    let c0 = Category::Identification.index() == 0
        && Category::Messages.index() == 5
        && Category::Collate.index() == 4;
    let c1 = Category::Ctype.name() == "LC_CTYPE"
        && Category::Numeric.short() == "NUMERIC"
        && Category::Time.name() == "LC_TIME"
        && Category::of(5) == Category::Messages
        && Category::of(9) == Category::Messages;
    let c2 = Category::Ctype.axis() == Axis::Data
        && Category::Numeric.axis() == Axis::Data
        && Category::Time.axis() == Axis::Layout
        && Category::Collate.axis() == Axis::Layout
        && Category::Messages.axis() == Axis::Language
        && Category::Identification.axis() == Axis::Meta;
    let c3 = Category::Ctype.min_len() == 1024
        && Category::Ctype.floor_len() == 256
        && Category::Messages.min_len() == 8
        && Category::Messages.floor_len() == 4
        && Category::Identification.min_len() == 24;
    let m0 = cat_bit(1) == 2 && cat_bit(5) == 32 && cat_bit(SECS) == 0 && cat_bit(99) == 0;
    let m1 = MASK_DATA == 6 && MASK_LAYOUT == 24 && MASK_LANG == 32 && MASK_CATS == 62;
    let m2 = MASK_DATA | MASK_LAYOUT | MASK_LANG == MASK_CATS;
    let p0 = popcount(0) == 0 && popcount(1) == 1 && popcount(7) == 3;
    let p1 = popcount(MASK_CATS) == 5 && popcount(0xffff_ffff) == 32;
    let t0 = mask_text(0).as_str() == "-";
    let t1 = mask_text(cat_bit(1)).as_str() == "CTYPE";
    let t2 = mask_text(MASK_CATS).as_str() == "CTYPE NUMERIC TIME COLLATE MESSAGES";
    let s0 = logged_all(MASK_CATS, MASK_CATS) && !logged_all(MASK_CATS, MASK_DATA);
    let s1 = unlogged_data(MASK_CATS, MASK_DATA) == 0
        && unlogged_data(MASK_CATS, 0) == 2
        && unlogged_data(MASK_LANG, 0) == 0;
    let k0 = Tier::Full.index() == 0
        && Tier::Partial.index() == 1
        && Tier::BuiltC.index() == 2
        && Tier::Broken.index() == 3;
    let k1 = Tier::Full.name() == "full"
        && Tier::Partial.name() == "partial"
        && Tier::BuiltC.name() == "built-in-C"
        && Tier::Broken.name() == "broken";
    let k2 = Tier::Full.rank() < Tier::Partial.rank()
        && Tier::Partial.rank() < Tier::BuiltC.rank()
        && Tier::BuiltC.rank() < Tier::Broken.rank();
    let k3 = Tier::Full.usable()
        && Tier::Partial.usable()
        && Tier::BuiltC.usable()
        && !Tier::Broken.usable();
    let k4 = Tier::Broken.loud()
        && !Tier::Full.loud()
        && Tier::Partial.silent_risk()
        && Tier::BuiltC.silent_risk()
        && !Tier::Full.silent_risk();
    let k5 = Tier::of(0) == Tier::Full
        && Tier::of(3) == Tier::Broken
        && Tier::of(9) == Tier::Broken;
    c0 && c1 && c2 && c3 && m0 && m1 && m2 && p0 && p1 && t0 && t1 && t2 && s0 && s1 && k0 && k1
        && k2 && k3 && k4 && k5
}

/// LM03 请求解析：三档优先级 + 各档自己的空值规则。
fn parse_ok() -> bool {
    let mut mix = Locale::none();
    mix.lc_all = Slot::val("C");
    if let Some(s) = mix.cats.get_mut(0) {
        *s = Slot::val("de_DE.UTF-8");
    }
    mix.lang = Slot::val("fr_FR.UTF-8");
    let a = match mix.wanted(1) { w => w.src == Src::LcAll && w.name == "C" };
    let b = match mix.wanted(5) { w => w.src == Src::LcAll && w.name == "C" };
    let c = mix.percat() == 0 && mix.uniform() && mix.cat(1).name == "de_DE.UTF-8";

    let alle = Locale::all(Slot::empty());
    let d = match alle.wanted(3) { w => w.src == Src::LcAllEmpty && w.name == "C" };
    let e = alle.percat() == 4681 && alle.uniform();

    let mut cat_empty = Locale::none();
    if let Some(s) = cat_empty.cats.get_mut(0) {
        *s = Slot::empty();
    }
    cat_empty.lang = Slot::val("it_IT.ISO-8859-1");
    let f = match cat_empty.wanted(1) { w => w.src == Src::Lang && w.name == "it_IT.ISO-8859-1" };
    let g = match cat_empty.wanted(2) { w => w.src == Src::Lang };

    let lang_empty = Locale::lang_only(Slot::empty());
    let h = match lang_empty.wanted(1) { w => w.src == Src::Default && w.name == "C" };
    let i2 = lang_empty.percat() == 4 * 4681;

    let bare = Locale::none();
    let j = match bare.wanted(5) { w => w.src == Src::Default && w.name == "C" };

    let bycat = Locale {
        lc_all: Slot::off(),
        cats: [
            Slot::val("C"),
            Slot::off(),
            Slot::off(),
            Slot::off(),
            Slot::off(),
        ],
        lang: Slot::val("fr_FR.UTF-8"),
    };
    let k = bycat.percat() == 14042;
    let l = first_target(&bycat) == "fr_FR.UTF-8" && first_target(&bare).is_empty();
    let m = {
        let (names, n) = bycat.distinct();
        n == 1 && match names.get(0) { Some(v) => *v == "fr_FR.UTF-8", None => false }
    };
    let n0 = {
        let (names, cnt) = Locale::all(Slot::val("C")).distinct();
        cnt == 0 && names[0].is_empty()
    };
    let o = match Locale::cat_key(2) {
        "LC_NUMERIC" => true,
        _ => false,
    };
    let p = match Locale::cat_key(9) {
        "LC_UNKNOWN" => true,
        _ => false,
    };
    let q = match bycat.cat(0) {
        Slot { set: false, name: "" } => true,
        _ => false,
    };
    let r0 = Locale::all(Slot::val("C")).env_text().as_str() == "LC_ALL=C LANG=<unset> LC_x=<unset>";
    let s0 = Slot::empty().render("LANG").as_str() == "LANG=<null>";
    let t0 = Slot::off().render("LANG").as_str() == "LANG=<unset>";
    let u0 = Src::LcAll.empty_is_request()
        && Src::LcAllEmpty.empty_is_request()
        && !Src::LcCat.empty_is_request()
        && !Src::Lang.empty_is_request();
    let v0 = Src::LcAll.code() == 0
        && Src::LcAllEmpty.code() == 1
        && Src::LcCat.code() == 2
        && Src::Lang.code() == 3
        && Src::Default.code() == 4;
    let w0 = Src::Default.name() == "default-C" && Src::LcCat.name() == "LC_x";
    a && b && c && d && e && f && g && h && i2 && j && k && l && m && n0 && o && p && q && r0 && s0
        && t0 && u0 && v0 && w0
}

/// LM04 名字合法性：五种码位各自可辨，且拒收次序固定。
fn legal_ok() -> bool {
    let c = check_name("C");
    let a = c.accept && c.builtin && c.code == NX_BUILTIN && is_builtin_name("C");
    let b = {
        let p = check_name("POSIX");
        p.accept && p.builtin && is_builtin_name("POSIX")
    };
    let d = {
        let n = check_name("en_US.UTF-8");
        n.accept && !n.builtin && n.code == NX_OK && n.code_name() == "legal"
    };
    let e = {
        let n = check_name("");
        n.accept && !n.builtin && n.code == NX_EMPTY && n.code_name() == "empty"
    };
    let f = {
        let n = check_name("de_DE.UTF-8;");
        n.rejected() && n.code == NX_CHAR && n.code_name() == "bad-char"
    };
    let g = {
        let long = String::from("a_");
        let mut s = long;
        let mut i = 0usize;
        while i < 70 {
            s.push('b');
            i += 1;
        }
        let n = check_name(s.as_str());
        n.rejected() && n.code == NX_LONG && n.code_name() == "too-long"
    };
    let h = {
        let n = check_name("-fr_FR.UTF-8");
        n.rejected() && n.code == NX_DASH && n.code_name() == "lead-dash"
    };
    let i2 = {
        let n = check_name("en_US@");
        n.rejected() && n.code == NX_TAIL && n.code_name() == "tail-sep"
    };
    let j = {
        let n = check_name("a_b_c.UTF-8");
        n.rejected() && n.code == NX_SEPS && n.code_name() == "dup-sep"
    };
    // 次序：超容 > '-' 打头 > 非法字符 > 分隔重复 > 悬挂。
    let k = {
        let mut s = String::from("-");
        let mut i = 0usize;
        while i < 70 {
            s.push('b');
            i += 1;
        }
        check_name(s.as_str()).code == NX_LONG
    };
    let l = check_name("fr_FR_;").code == NX_CHAR;
    // 同一分隔符重复比"悬挂"更具体：`fr_FR_@` 记 dup-sep，`en_US@` 才记 tail-sep。
    let m = check_name("fr_FR_@").code == NX_SEPS && check_name("zh_CN.UTF-8@euro").code == NX_OK;
    let n0 = check_name("a@b@c").code == NX_SEPS && check_name("a..b").code == NX_SEPS;
    let o = check_name("12").code == NX_CHAR && check_name("zh_CN.GBK@euro").code == NX_OK;
    let p = {
        let mut all = true;
        for b in [b'A', b'z', b'0', b'_', b'+', b'.', b'@', b'-'] {
            if !is_portable(b) {
                all = false;
            }
        }
        all
    };
    let q = !is_portable(b' ') && !is_portable(b':') && !is_portable(b'*') && !is_portable(0);
    let r0 = count_sep("a_b_c", b'_') == 2 && count_sep("abc", b'_') == 0;
    let s0 = lang_field_ok("de") && lang_field_ok("C") && !lang_field_ok("1x") && !lang_field_ok("");
    let t0 = {
        let mut s = String::from("zh_");
        let mut i = 0usize;
        while i < 60 {
            s.push('X');
            i += 1;
        }
        s.push_str("CN.UTF-8");
        check_name(s.as_str()).code == NX_LONG
    };
    a && b && d && e && f && g && h && i2 && j && k && l && m && n0 && o && p && q && r0 && s0 && t0
}

/// LM05 归档结构闸：四类拒收都早于字符表，且闸内次序可辨。
fn archive_ok() -> bool {
    let a = Archive::complete("x");
    let b = header_gate(&a).fault == false && section_gate(&a, 1).fault == false;
    let magic = Archive::bad_magic("x");
    let c = header_gate(&magic) == ArcGate::bad(AR_MAGIC, Stage::Header, 0);
    let ver = Archive::bad_version("x");
    let d = header_gate(&ver) == ArcGate::bad(AR_VERSION, Stage::Header, 0);
    let rng = Archive::truncated("x");
    let e = header_gate(&rng) == ArcGate::bad(AR_RANGE, Stage::Header, 0);
    let loc = Archive::locpath_miss("x");
    let f = header_gate(&loc) == ArcGate::bad(AR_LOC, Stage::Header, 0);
    let lochit = Archive::locpath_hit("x");
    let g = !header_gate(&lochit).fault && header_gate(&lochit).stage == Stage::Ready;
    // magic 与 version 同时坏：magic 先发言。
    let both = {
        let mut m = Archive::bad_version("x");
        m.magic = MAGIC_BAD;
        header_gate(&m).code == AR_MAGIC
    };
    // magic 与 locpath 同时坏：旁路先发言（连归档头都不该读）。
    let bypass = {
        let mut m = Archive::bad_magic("x");
        m.locpath = true;
        m.locpath_ok = false;
        header_gate(&m).code == AR_LOC
    };
    let absent = Archive::missing(2, "x");
    let h = section_gate(&absent, 2) == ArcGate::bad(AR_ABSENT, Stage::Section, 2)
        && section_gate(&absent, 1).fault == false;
    let oob = Archive::oob(3, 70000, 8192, "x");
    let i2 = section_gate(&oob, 3) == ArcGate::bad(AR_SECRANGE, Stage::Section, 3);
    let secmagic = Archive::sec_bad_magic(4, "x");
    let j = section_gate(&secmagic, 4) == ArcGate::bad(AR_SECMAGIC, Stage::Content, 4);
    let secver = Archive::sec_bad_version(4, "x");
    let k = section_gate(&secver, 4) == ArcGate::bad(AR_SECVER, Stage::Content, 4);
    let short = Archive::shortened(1, 1023, "x");
    let l = section_gate(&short, 1) == ArcGate::bad(AR_SHORT, Stage::Content, 1);
    let exact = Archive::shortened(1, 1024, "x");
    let m0 = !section_gate(&exact, 1).fault;
    // 越界先于节 magic；节 magic 先于节长度。
    let n0 = {
        let mut x = Archive::complete("x");
        x.secs[1] = Section { present: true, off: 65000, len: 4096, magic: false, version: true };
        section_gate(&x, 1).code == AR_SECRANGE
    };
    let o0 = {
        let mut x = Archive::complete("x");
        x.secs[1] = Section { present: true, off: 64, len: 8, magic: false, version: true };
        section_gate(&x, 1).code == AR_SECMAGIC
    };
    let p0 = {
        let mut x = Archive::complete("x");
        x.secs[1] = Section { present: true, off: 64, len: 8, magic: true, version: false };
        section_gate(&x, 1).code == AR_SECVER
    };
    let q0 = first_structural(&a).fault == false
        && first_structural(&magic).code == AR_MAGIC
        && first_structural(&absent).code == AR_ABSENT
        && first_structural(&short).code == AR_SHORT;
    let r0 = section_usable(&a, 1)
        && !section_usable(&magic, 1)
        && !section_usable(&absent, 2)
        && section_usable(&absent, 1);
    let s0 = {
        let mut all_before = true;
        let probes = [
            first_structural(&magic),
            first_structural(&ver),
            first_structural(&rng),
            first_structural(&loc),
            first_structural(&absent),
            first_structural(&oob),
            first_structural(&secmagic),
            first_structural(&secver),
            first_structural(&short),
        ];
        let mut i = 0usize;
        while i < probes.len() {
            match probes.get(i) {
                Some(g2) => {
                    if !g2.fault || !g2.before_table() || !g2.stage.structural() {
                        all_before = false;
                    }
                }
                None => all_before = false,
            }
            i += 1;
        }
        all_before
    };
    // 元数据节自述不符：只告警，绝不降级。
    let mislabel = shape_all("ident", "pt_BR.UTF-8", Archive::mislabeled("pt_BR.UTF-8", "pt_PT.UTF-8"), 0, true);
    let o1 = mislabel.actual();
    let t0 = o1.tier == Tier::Full && o1.ident_bad && o1.code == 0 && o1.fall == 0 && o1.ok();
    let u0 = ident_matches(&a, "x") && !ident_matches(&a, "y");
    let v0 = ArcGate::none().code_name() == "none"
        && ArcGate::bad(AR_MAGIC, Stage::Header, 0).code_name() == "arc-magic"
        && ArcGate::bad(AR_SECRANGE, Stage::Section, 3).code_name() == "sec-range";
    b && c && d && e && f && g && both && bypass && h && i2 && j && k && l && m0 && n0 && o0 && p0
        && q0 && r0 && s0 && t0 && u0 && v0 && a.magic_ok()
}

/// LM06 降级链：四档落点、broken 优先、伪成功定义、闸门次序。
fn chain_ok() -> bool {
    let cs = cases();
    let g0 = match cs.get(0) {
        Some(c) => {
            let o = c.actual();
            o.tier == Tier::Full && o.code == 0 && o.fall == 0 && o.c_used == 0 && o.note == Note::FullOk
        }
        None => false,
    };
    let g1 = match cs.get(1) {
        Some(c) => {
            let o = c.actual();
            o.tier == Tier::Full && o.c_used == MASK_CATS && o.fall == 0 && o.note == Note::BuiltIn
        }
        None => false,
    };
    let g2 = match cs.get(2) {
        Some(c) => {
            let o = c.actual();
            o.tier == Tier::Partial && o.fall == cat_bit(4) && o.pseudo == 0 && o.unlogged == 0
        }
        None => false,
    };
    let g3 = match cs.get(3) {
        Some(c) => {
            let o = c.actual();
            o.tier == Tier::Partial && o.code == AR_SHORT && o.pseudo == 0 && o.tools == 5
        }
        None => false,
    };
    let g4 = match cs.get(4) {
        Some(c) => {
            let o = c.actual();
            o.tier == Tier::Partial && o.c_used == cat_bit(1) | MASK_LANG && o.fall == MASK_LANG
        }
        None => false,
    };
    let g5 = match cs.get(5) {
        Some(c) => {
            let o = c.actual();
            o.tier == Tier::BuiltC && o.note == Note::ArchiveDead && o.acct == 5
        }
        None => false,
    };
    let g6 = match cs.get(6) {
        Some(c) => {
            let o = c.actual();
            o.tier == Tier::BuiltC && o.code == AR_LOC && o.stage == Stage::Header
        }
        None => false,
    };
    let g7 = match cs.get(7) {
        Some(c) => {
            let o = c.actual();
            o.tier == Tier::Broken
                && o.code == BRK_CTYPE
                && o.stage == Stage::Table
                && o.fall == 0
                && o.tools == 0
                && o.fatal == 3
        }
        None => false,
    };
    // broken 优先于 partial：C.UTF-8 需查归档且 CTYPE 缺失时，档必须判死而非"回退可用"。
    let brk = match cs.get(8) {
        Some(c) => {
            let o = c.actual();
            o.tier == Tier::Broken
                && o.fall == cat_bit(1)
                && o.pseudo == 1
                && o.unlogged == 5
                && o.tools == 5
                && !o.ok()
        }
        None => false,
    };
    // 数值轴未记账 = 伪成功；同一形状把记账补齐即绿。
    let pseudo = match cs.get(11) {
        Some(c) => {
            let o = c.actual();
            let mut fixed = c.shape;
            fixed.log_mask = cat_bit(1);
            let f = fixed.actual();
            o.pseudo == 1
                && o.note == Note::Pseudo
                && !o.ok()
                && f.pseudo == 0
                && f.ok()
                && f.acct == 1
                && Gate::walk(&o).ok() == Some(7)
        }
        None => false,
    };
    // 语言轴未记账：工具级伪成功（rm 的应答表最危险）。
    let msgs = {
        let s = shape_all("msg-unlogged", "en_US.UTF-8", Archive::missing(5, "en_US.UTF-8"), 0, true);
        let o = s.actual();
        o.pseudo == 0 && o.unlogged == 5 && !o.ok() && o.note == Note::Pseudo
    };
    // C 表判死但本轮没用到 C 表：档仍是 full，只算潜在腐坏。
    let latent = {
        let s = shape_all("latent", "C.UTF-8", Archive::complete("C.UTF-8"), MASK_CATS, false);
        let o = s.actual();
        o.tier == Tier::Full && o.c_dead && !o.rotten() == false && o.ok()
    };
    let order = match cs.get(13) {
        Some(c) => {
            let o = c.actual();
            o.stage == Stage::Section && o.rejected_before_table() && o.code == AR_SECRANGE
        }
        None => false,
    };
    let mut g = Gate::new();
    let steps = match cs.get(3) {
        Some(c) => Gate::walk(&c.actual()).ok() == Some(5),
        None => false,
    };
    let steps2 = match cs.get(5) {
        Some(c) => Gate::walk(&c.actual()).ok() == Some(3),
        None => false,
    };
    let steps3 = match cs.get(7) {
        Some(c) => Gate::walk(&c.actual()).ok() == Some(6),
        None => false,
    };
    let bad_order = {
        g.step(Stage::Content) == Ok(()) && g.step(Stage::Section) == Err(LM_EORDER) && g.steps() == 1
    };
    let closed = g.close() == Ok(1) && g.step(Stage::Ready) == Err(LM_ESTATE) && g.close() == Err(LM_ESTATE);
    let table_after_structural = {
        let mut x = Archive::complete("x");
        x.secs[1] = Section { present: true, off: 64, len: 8, magic: true, version: true };
        let s = shape_all("late", "x", x, MASK_CATS, true);
        let o = s.actual();
        o.stage == Stage::Content && Gate::walk(&o).ok() == Some(5)
    };
    g0 && g1 && g2 && g3 && g4 && g5 && g6 && g7 && brk && pseudo && msgs && latent && order
        && steps && steps2 && steps3 && bad_order && closed && table_after_structural
}

/// LM07 工具矩阵统计：20 行各自取证，受损数真算。
fn tool_ok() -> bool {
    let a = TOOL_LIST.len() == TOOLS && TOOLS == 20 && tool_index("whoami") == TOOLS;
    let b = tool_index("cat") == 0
        && tool_index("ls") == 1
        && tool_index("wc") == 19
        && tool_index("test") == 14
        && tool_index("id") == 16;
    let c = insensitive_tools() == 5 && lang_only_tools() == 10;
    let d = match tool_at(11) {
        Some(t) => t.name == "uname" && t.insensitive() && t.deps == 0 && !t.diag,
        None => false,
    };
    let e = match tool_at(14) {
        Some(t) => t.name == "test" && !t.touches_layout() && t.byte_only,
        None => false,
    };
    let f = match tool_at(19) {
        Some(t) => t.name == "wc" && t.touches_data() && t.must_report && t.broken_fatal,
        None => false,
    };
    let g = match tool_at(4) {
        Some(t) => t.name == "rm" && t.touches_lang() && t.must_report && !t.byte_only,
        None => false,
    };
    let h = match tool_at(13) {
        Some(t) => t.name == "printf" && t.deps == (cat_bit(2) | cat_bit(1) | MASK_LANG),
        None => false,
    };
    // 单类回退的命中数：数值轴最贵，语言轴最广。
    let t0 = single_fall_hits();
    let i2 = t0[0] == 5 && t0[1] == 1 && t0[2] == 1 && t0[3] == 1 && t0[4] == 15;
    let j = hit_tools(0) == 0 && hit_tools(MASK_CATS) == 15 && hit_tools(MASK_DATA) == 5;
    let k = numeric_tools(MASK_CATS) == 2 && numeric_tools(cat_bit(1)) == 5;
    let l = cosmetic_tools(MASK_CATS) == 13 && cosmetic_tools(cat_bit(3)) == 1;
    let m0 = fatal_tools(0, false) == 0 && fatal_tools(MASK_CATS, false) == 3 && fatal_tools(0, true) == 3;
    let n0 = unlogged_tools(cat_bit(1), 0) == 5
        && unlogged_tools(cat_bit(1), cat_bit(1)) == 0
        && unlogged_tools(MASK_LANG, MASK_DATA) == 5;
    // 每行的具体差异互不相同：主轴掩盖次轴，但记账不被掩盖。
    let o0 = effect_of(0, cat_bit(1)) == Behav::Identical
        && effect_of(0, MASK_LANG) == Behav::MsgOnly
        && effect_of(2, MASK_CATS) == Behav::MsgOnly
        && effect_of(3, MASK_DATA) == Behav::Identical;
    let p0 = effect_of(1, MASK_CATS) == Behav::SortAscii
        && effect_of(1, cat_bit(1) | MASK_LANG) == Behav::WidthBytes
        && effect_of(1, MASK_LANG) == Behav::MsgOnly;
    let q0 = effect_of(4, MASK_LANG) == Behav::RespAscii && effect_of(4, cat_bit(1)) == Behav::WidthBytes;
    let r0 = effect_of(10, cat_bit(3)) == Behav::DateFormat
        && effect_of(10, cat_bit(1)) == Behav::WidthBytes
        && effect_of(10, MASK_LANG) == Behav::MsgOnly;
    let s0 = effect_of(13, cat_bit(2)) == Behav::DecimalShift
        && effect_of(13, cat_bit(1)) == Behav::WidthBytes;
    let t1 = effect_of(11, MASK_CATS) == Behav::Identical
        && effect_of(12, MASK_CATS) == Behav::Identical
        && effect_of(14, MASK_CATS) == Behav::Identical
        && effect_of(17, MASK_CATS) == Behav::Identical
        && effect_of(18, MASK_CATS) == Behav::Identical;
    let u0 = effect_of(19, cat_bit(1)) == Behav::WidthBytes
        && effect_of(19, MASK_LANG) == Behav::MsgOnly
        && effect_of(19, cat_bit(3)) == Behav::Identical;
    let v0 = match tool_at(1) {
        Some(t) => t.effect(1, MASK_CATS) == Behav::SortAscii && t.delta_mask(MASK_CATS) == 50,
        None => false,
    };
    let w0 = match tool_at(11) {
        Some(t) => t.effect(11, MASK_CATS) == Behav::Identical && t.delta_mask(MASK_CATS) == 0,
        None => false,
    };
    let x0 = Behav::WidthBytes.numeric()
        && Behav::DecimalShift.numeric()
        && !Behav::SortAscii.numeric()
        && Behav::MsgOnly.cosmetic()
        && !Behav::Identical.changed()
        && Behav::RespAscii.changed();
    let y0 = Behav::SortAscii.name() == "sort-ascii"
        && Behav::DateFormat.name() == "date-format"
        && Behav::WidthBytes.name() == "byte-width"
        && Behav::RespAscii.name() == "answer-ascii";
    let z0 = {
        let mut uniq = true;
        let mut i = 0usize;
        while i < TOOLS {
            let n1 = match tool_at(i) {
                Some(t) => t.name,
                None => "",
            };
            if n1.is_empty() {
                uniq = false;
            }
            let mut j = 0usize;
            while j < i {
                if let Some(p) = tool_at(j) {
                    if p.name == n1 {
                        uniq = false;
                    }
                }
                j += 1;
            }
            i += 1;
        }
        uniq
    };
    a && b && c && d && e && f && g && h && i2 && j && k && l && m0 && n0 && o0 && p0 && q0 && r0
        && s0 && t1 && u0 && v0 && w0 && x0 && y0 && z0
}

/// LM08 申报对判：绿格全对、红格各驳一处，且 Parse/Arch 两轴也可达。
fn judge_ok() -> bool {
    let cs = cases();
    let mut i = 0usize;
    while i < ACCEPT {
        if let Some(c) = cs.get(i) {
            if judge(c) != Fault::Ok {
                return false;
            }
        } else {
            return false;
        }
        i += 1;
    }
    let mut j = 0usize;
    while j < REJECT {
        let c = match cs.get(j + ACCEPT) {
            Some(v) => *v,
            None => return false,
        };
        let want = match REJECT_FAULTS.get(j) {
            Some(v) => *v,
            None => return false,
        };
        let code = match REJECT_CODES.get(j) {
            Some(v) => *v,
            None => return false,
        };
        if judge(&c) != want {
            return false;
        }
        if judge(&c).code() != code {
            return false;
        }
        if !judge(&c).alive() {
            return false;
        }
        j += 1;
    }
    // 逐类指纹轴：把 S5 的混用指纹报成全 LC_ALL。
    let parse = match cs.get(4) {
        Some(c) => {
            let mut cl = c.claim;
            cl.percat = 0;
            judge_against(&cl, &c.actual()) == Fault::Parse
        }
        None => false,
    };
    // 归档码位轴：S3 的结构码报错成名字码。
    let arch = match cs.get(2) {
        Some(c) => {
            let mut cl = c.claim;
            cl.code = NX_CHAR;
            judge_against(&cl, &c.actual()) == Fault::Arch
        }
        None => false,
    } && match cs.get(13) {
        Some(c) => {
            let mut cl = c.claim;
            cl.code = AR_SECMAGIC;
            judge_against(&cl, &c.actual()) == Fault::Arch
        }
        None => false,
    };
    let synth = match cs.get(8) {
        Some(c) => {
            let a = c.actual();
            judge_against(&Claim::of(a), &a) == Fault::Ok
        }
        None => false,
    };
    let acct = Fault::Count.accounting() && Fault::Estate.accounting() && !Fault::Order.accounting();
    let short = fault_short(Fault::Order) == "order"
        && fault_short(Fault::Estate) == "estate"
        && fault_short(Fault::Ok) == "ok"
        && fault_short(Fault::Arch) == "arch";
    let codes = code_short(AR_SECRANGE) == "sec-range"
        && code_short(NX_TAIL) == "tail-sep"
        && code_short(BRK_CTYPE) == "ctype-dead"
        && code_short(0) == "none";
    let alive = !Fault::Ok.alive() && Fault::Matrix.alive() && Fault::Bal.alive();
    parse && arch && synth && acct && short && codes && alive && codes_distinct()
}

/// 缺陷码与 13 常量一一对应且互异。
fn codes_distinct() -> bool {
    let fs = [
        Fault::Estate,
        Fault::Tier,
        Fault::Parse,
        Fault::Name,
        Fault::Arch,
        Fault::Count,
        Fault::Tool,
        Fault::Order,
        Fault::Matrix,
        Fault::Slot,
        Fault::Seal,
        Fault::Nobs,
        Fault::Bal,
    ];
    let mut i = 0usize;
    while i < fs.len() {
        let c = match fs.get(i) {
            Some(f) => f.code(),
            None => return false,
        };
        if c <= SEG_BASE || c > SEG_BASE + 0x0d {
            return false;
        }
        let mut j = i + 1;
        while j < fs.len() {
            if let Some(p) = fs.get(j) {
                if p.code() == c {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// LM09 语料与渲染：真算的档分布、矩阵与指纹。
fn corpus_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.total == 14 && r.accept == 8 && r.reject == 6 && r.matched == 14;
    let b = r.full == 2 && r.partial == 3 && r.builtc == 2 && r.broken == 1;
    let c = r.pseudo == 0 && r.gate() == Ok(()) && r.green() && r.rate() == 1000;
    let d = r.tier_accept(Tier::Partial) == 3 && r.tier_accept(Tier::Broken) == 1;
    let m = Matrix::of();
    let e = m.row(0) == Some(Row { total: 2, good: 2, hits: 0, pseudo: 0 })
        && m.row(1)
            == Some(Row { total: 7, good: 3, hits: 29, pseudo: 6 })
        && m.row(2)
            == Some(Row { total: 3, good: 2, hits: 45, pseudo: 0 })
        && m.row(3)
            == Some(Row { total: 2, good: 1, hits: 5, pseudo: 6 })
        && m.row(4).is_none();
    let f = m.total() == CORPUS && m.good() == ACCEPT && m.balanced() && m.perfect_rows() == 1;
    let g = m.hits() == 79 && m.pseudo_all() == 12;
    let h = m.seal() == Matrix::of().seal() && m.seal() != Matrix::blank().seal();
    let i2 = tags_unique() && notes_ok() && table_ok() && vals_match(&r);
    let j = render_row_ok(&m);
    a && b && c && d && e && f && g && h && i2 && j
}

/// 渲染行逐字。
fn render_row_ok(m: &Matrix) -> bool {
    m.render_row(0).as_str() == "full=2/2/hits0"
        && m.render_row(3).as_str() == "broken=2/1/hits5"
        && m.render_row(9).as_str() == "broken=0/0/hits0"
}

/// VALS 与实测逐项对齐（红档 ppm 位在 VALS[7]）。
fn vals_match(r: &Report) -> bool {
    let got = [
        r.total,
        r.accept,
        r.reject,
        r.full,
        r.partial,
        r.builtc,
        r.broken,
        929,
    ];
    let mut i = 0usize;
    while i < ACCEPT {
        match (got.get(i), VALS.get(i)) {
            (Some(a), Some(b)) => {
                if *a != *b {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    VALS.len() == ACCEPT && VALS[7] == 929
}

/// 标签唯一。
fn tags_unique() -> bool {
    let cs = cases();
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if c.tag().is_empty() {
            return false;
        }
        let mut j = 0usize;
        while j < i {
            if let Some(p) = cs.get(j) {
                if p.tag() == c.tag() {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 逐槽注记合规：可用档不得挂硬注记，结构注记不得晚于字符表。
fn notes_ok() -> bool {
    let cs = cases();
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let a = c.actual();
        if a.tier.usable() && a.tier.loud() {
            return false;
        }
        if a.note.loud() && a.ok() {
            return false;
        }
        if a.note == Note::Pseudo && a.fall == 0 {
            return false;
        }
        if a.note == Note::Broken && a.tier != Tier::Broken {
            return false;
        }
        if a.code != 0 && a.code != BRK_CTYPE && a.note != Note::Pseudo && !a.rejected_before_table() {
            return false;
        }
        if a.tier == Tier::Full && a.fall != 0 {
            return false;
        }
        if a.tier == Tier::BuiltC && a.fall != a.c_used {
            return false;
        }
        if a.acct as u32 + a.pseudo as u32 > popcount(a.fall) as u32 {
            return false;
        }
        if detail(&c).as_str().is_empty() {
            return false;
        }
        i += 1;
    }
    true
}

/// 逐槽对判表：前 8 绿、后 6 红。
fn table_ok() -> bool {
    let cs = cases();
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let f = judge(&c);
        if i < ACCEPT {
            if f != Fault::Ok {
                return false;
            }
        } else if f == Fault::Ok {
            return false;
        }
        if c.ok() != (f == Fault::Ok) {
            return false;
        }
        i += 1;
    }
    true
}

/// LM10 红档显式：注入缺陷必须改变统计，且槽位越界显式拒。
fn reds_ok() -> bool {
    let r = match run_with(Some((0, Fault::Count))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.matched == 13 && r.rate() == 929 && !r.green();
    let b = r.gate() == Err(LM_ECOUNT) && r.accept == 7 && r.reject == 7;
    let c = run_with(Some((CORPUS, Fault::Count))).err() == Some(LM_ESLOT);
    let d = r.pseudo == 0;
    // 注入到红格上不改变期望（红格本来就红），但 matched 仍要掉一档。
    let e = match run_with(Some((9, Fault::Ok))) {
        Ok(v) => v.matched == 13 && v.accept == 9 && v.reject == 5,
        Err(_) => false,
    };
    // 把红格的对判换成另一种缺陷：仍不计绿，但期望轴错位，matched 必须掉。
    let f = match run_with(Some((13, Fault::Parse))) {
        Ok(v) => v.matched == 13 && v.reject == 6 && v.accept == 8 && v.rate() == 929 && !v.green(),
        Err(_) => false,
    };
    a && b && c && d && e && f
}

/// LM11 文案逐字：真实 loader/coreutils 的原文一字不差。
fn detail_ok() -> bool {
    let a = msg_bad_magic("zh_CN.UTF-8", "/usr/lib/locale/locale-archive", "log0").as_str()
        == "locale file for locale zh_CN.UTF-8 of archive /usr/lib/locale/locale-archive: bad magic!: [log0]";
    let b = msg_wrong_version("x", "/p", 2).as_str() == "locale file for locale x of archive /p: wrong version 2, cannot load locale";
    let c = msg_section_range("ru_RU.KOI8-R", Category::Collate, 65000, 4096, 65536).as_str()
        == "cannot map section LC_COLLATE of locale ru_RU.KOI8-R: offset 65000 length 4096 exceeds archive size 65536";
    let d = msg_section_short("de_DE.UTF-8", Category::Ctype, 64, 1024).as_str()
        == "locale data for de_DE.UTF-8/LC_CTYPE truncated: got 64 bytes, need 1024";
    let e = msg_missing_sec("/usr/lib/locale", "en_US.UTF-8", Category::Collate).as_str()
        == "cannot open locale file /usr/lib/locale/en_US.UTF-8/LC_COLLATE: No such file or directory";
    let f = msg_unsupported("xx_YY", Category::Ctype).as_str()
        == "bash: warning: setlocale: LC_CTYPE: \"xx_YY\" (unsupported locale setting)";
    let g = msg_default_locale_fail("xx_YY").as_str()
        == "locale: Cannot set LC_ALL to default locale: xx_YY";
    let h = msg_locpath_miss("/locale-archive", "ja_JP.UTF-8").as_str()
        == "cannot open locale directory /locale-archive/ja_JP.UTF-8: No such file or directory";
    let i2 = msg_unlogged(Category::Ctype, "wc").as_str()
        == "warning: locale downgrade not accounted for CTYPE in wc";
    let j = msg_built_in_dead().as_str()
        == "fatal: built-in C charmap failed self-check, refusing to classify bytes";
    let k = msg_header_range("/usr/lib/locale/locale-archive", 2048).as_str()
        == "archive /usr/lib/locale/locale-archive size 2048 is below the minimum locale header size";
    let l = msg_tool_fatal("wc", Category::Ctype).as_str()
        == "wc: cannot proceed with an untrusted LC_CTYPE charmap: Illegal byte sequence";
    let m = msg_tool_fatal("ls", Category::Collate).as_str()
        == "ls: cannot proceed with an untrusted LC_COLLATE rule: Illegal byte sequence";
    // 槽级文案。
    let cs = cases();
    let n0 = match cs.get(5) {
        Some(c) => {
            let o = c.actual();
            message_for(&c.shape, &o).as_str()
                == "locale file for locale zh_CN.UTF-8 of archive /usr/lib/locale/locale-archive: bad magic!: [log0]"
        }
        None => false,
    };
    let o0 = match cs.get(3) {
        Some(c) => {
            let x = c.actual();
            message_for(&c.shape, &x).as_str()
                == "locale data for de_DE.UTF-8/LC_CTYPE truncated: got 64 bytes, need 1024"
        }
        None => false,
    };
    let p0 = match cs.get(2) {
        Some(c) => {
            let x = c.actual();
            message_for(&c.shape, &x).as_str()
                == "cannot open locale file /usr/lib/locale/locale-archive/en_US.UTF-8/LC_COLLATE: No such file or directory"
        }
        None => false,
    };
    let q0 = match cs.get(11) {
        Some(c) => {
            let x = c.actual();
            message_for(&c.shape, &x).as_str() == "warning: locale downgrade not accounted for CTYPE in wc"
        }
        None => false,
    };
    let r0 = match cs.get(0) {
        Some(c) => {
            let x = c.actual();
            message_for(&c.shape, &x).is_empty()
        }
        None => false,
    };
    let s0 = match cs.get(4) {
        Some(c) => {
            let x = c.actual();
            message_for(&c.shape, &x).as_str()
                == "cannot open locale file /usr/lib/locale/locale-archive/fr_FR.UTF-8/LC_MESSAGES: No such file or directory"
        }
        None => false,
    };
    a && b && c && d && e && f && g && h && i2 && j && k && l && m && n0 && o0 && p0 && q0 && r0 && s0
}

/// LM12 释义与账本：逐字释义 + 账本形状。
const DETAIL_CT: &str = "partial-ctype-truncated env=LC_ALL=de_DE.UTF-8 LANG=<unset> LC_x=<unset> tier=partial stage=content code=47 fall=CTYPE acct=1 tools=5 fatal=3 note=fallback\n";
const DETAIL_RD: &str = "red-name-dash-as-char env=LC_ALL=de_DE.UTF-8; LANG=<unset> LC_x=<unset> tier=built-in-C stage=name code=21 fall=CTYPE NUMERIC TIME COLLATE MESSAGES acct=5 tools=15 fatal=3 note=name-bad\n";

fn book_ok() -> bool {
    let cs = cases();
    let a = match cs.get(3) {
        Some(c) => detail(&c).as_str() == DETAIL_CT,
        None => false,
    };
    let b = match cs.get(10) {
        Some(c) => detail(&c).as_str() == DETAIL_RD,
        None => false,
    };
    let c = match cs.get(2) {
        Some(v) => detail(&v).as_str() == DETAIL_COLLATE,
        None => false,
    };
    let d = match cs.get(6) {
        Some(v) => detail(&v).as_str() == DETAIL_LOCPATH,
        None => false,
    };
    let e = match cs.get(1) {
        Some(v) => {
            let t = claim_text(&v);
            t.as_str().starts_with("full-lang-builtin-C claim tier=full") && t.ends_with("fault=ok\n")
        }
        None => false,
    };
    let f = match cs.get(9) {
        Some(v) => claim_text(&v).as_str() == "red-tier-partial-as-full claim tier=full code=0 acct=1 tools=1 fault=tier\n",
        None => false,
    };
    let l = build_ledger();
    let g = l.len() == SAMPLES
        && l.ok_count() == ACCEPT
        && l.err_count() == REJECT
        && l.audit() == Ok(())
        && l.sample(100).is_none()
        && !l.is_empty();
    let h = l.err_codes() == REJECT_CODES;
    let i2 = match l.sample(0) {
        Some(s) => s.value == VALS[0] && s.ok && s.op == OP_CORPUS,
        None => false,
    };
    let j = match l.sample(7) {
        Some(s) => s.ok && s.value == 929 && s.op == OP_NAME,
        None => false,
    };
    let k = match l.sample(8) {
        Some(s) => !s.ok && s.code == LM_ESTATE,
        None => false,
    };
    let m0 = case_at(0).map(|c| c.tag() == "full-en_US-utf8").unwrap_or(false)
        && case_at(CORPUS - 1).map(|c| c.tag() == "red-order-after-table").unwrap_or(false)
        && case_at(CORPUS).is_none();
    let n0 = cover(CORPUS) == Ok(()) && cover(13) == Err(LM_ENOBS) && cover(0) == Err(LM_ENOBS);
    let o0 = expected(0) == Some(Fault::Ok)
        && expected(ACCEPT) == Some(Fault::Estate)
        && expected(CORPUS - 1) == Some(Fault::Order)
        && expected(CORPUS).is_none();
    let p0 = first_structural_stage_texts();
    a && b && c && d && e && f && g && h && i2 && j && k && m0 && n0 && o0 && p0
}

/// 结构码位的阶段文本逐字（码位与阶段一一对应，不许串行）。
fn first_structural_stage_texts() -> bool {
    let a = Archive::complete("x");
    let probes = [
        (Archive::bad_magic("x"), "header"),
        (Archive::bad_version("x"), "header"),
        (Archive::truncated("x"), "header"),
        (Archive::locpath_miss("x"), "header"),
        (Archive::missing(1, "x"), "section"),
        (Archive::oob(1, 65000, 4096, "x"), "section"),
        (Archive::sec_bad_magic(1, "x"), "content"),
        (Archive::sec_bad_version(1, "x"), "content"),
        (Archive::shortened(1, 8, "x"), "content"),
    ];
    let mut i = 0usize;
    while i < probes.len() {
        let (arc, want) = match probes.get(i) {
            Some(p) => (p.0, p.1),
            None => return false,
        };
        let g = first_structural(&arc);
        if g.stage.name() != want || !g.fault {
            return false;
        }
        i += 1;
    }
    first_structural(&a).fault == false
}

/// LM13 用：语料级指纹（每槽形状指纹互异）。
fn shape_seals_distinct() -> bool {
    let cs = cases();
    let mut i = 0usize;
    while i < CORPUS {
        let a = match cs.get(i) {
            Some(c) => c.shape.seal(),
            None => return false,
        };
        if a == 0 {
            return false;
        }
        let mut j = i + 1;
        while j < CORPUS {
            if let Some(c) = cs.get(j) {
                if c.shape.seal() == a {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}
// ---------------------------------------------------------------------------
// 判据入口：13 闸
// ---------------------------------------------------------------------------

/// 本卡判据入口：13 道闸门，逐字名 <LM01..LM13>。
pub fn run_unx11658_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11658_bad_locale_matrix");

    s.add("LM01-名称与段基址", name_ok(), "static");

    s.add("LM02-常量与档位", const_ok(), "static");

    s.add("LM03-请求解析", parse_ok(), "static");

    s.add("LM04-名字合法性", legal_ok(), "static");

    s.add("LM05-归档结构闸", archive_ok(), "static");

    s.add("LM06-降级链", chain_ok(), "static");

    s.add("LM07-工具矩阵统计", tool_ok(), "static");

    s.add("LM08-申报对判", judge_ok(), "static");

    s.add("LM09-语料与渲染", corpus_ok(), "static");

    s.add("LM10-红档显式", reds_ok(), "static");

    s.add("LM11-文案逐字", detail_ok(), "static");

    s.add("LM12-释义与账本", book_ok(), "static");

    s.add(
        "LM13-冻结重放四注入",
        {
            let mut l = build_ledger();
            let a = l.audit() == Ok(());
            let b = match l.freeze() {
                Ok(r) => r != 0 && l.is_frozen() && l.replay() == Ok(r) && l.len() == SAMPLES,
                Err(_) => false,
            };
            let c = match l.register(count_cell(14, OP_PARSE, Ok(1))) {
                Ok(()) => false,
                Err(e) => e == LM_ESLOT,
            };
            let inj = inject_four();
            let d = inj[0] && inj[1] && inj[2] && inj[3];
            let e = shape_seals_distinct() && l.rejects() == 1 && l.root() != 0;
            a && b && c && d && e
        },
        "static",
    );

    s
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn fail() -> ! {
        panic!("unx11658 test fail")
    }

    #[test]
    fn consts() {
        assert_eq!(SEG_BASE, 0x6cf30);
        assert_eq!(CORPUS, 14);
        assert_eq!(SAMPLES, 14);
        assert_eq!(ACCEPT, 8);
        assert_eq!(REJECT, 6);
        assert_eq!(TIER_COUNT, 4);
        assert_eq!(CATS, 5);
        assert_eq!(SECS, 6);
        assert_eq!(TOOLS, 20);
        assert_eq!(NAME_MAX, 64);
        assert_eq!(REQ_MAX, 3);
        assert_eq!(PATH_MAX, 64);
    }

    #[test]
    fn error_codes_spaced() {
        assert_eq!(LM_EPARSE, SEG_BASE + 0x01);
        assert_eq!(LM_EBAL, SEG_BASE + 0x0d);
        assert_eq!(LM_EPARSE + 12, LM_EBAL);
        let cs = [
            LM_EPARSE, LM_ENAME, LM_EARCH, LM_ETIER, LM_ETOOL, LM_ESTATE, LM_ECOUNT, LM_ESLOT,
            LM_ESEAL, LM_ENOBS, LM_EMATRIX, LM_EORDER, LM_EBAL,
        ];
        let mut i = 0usize;
        while i < cs.len() {
            let mut j = i + 1;
            while j < cs.len() {
                if cs[i] == cs[j] {
                    fail();
                }
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn category_texts() {
        assert_eq!(Category::Identification.index(), 0);
        assert_eq!(Category::Messages.index(), 5);
        assert_eq!(Category::Ctype.name(), "LC_CTYPE");
        assert_eq!(Category::Collate.name(), "LC_COLLATE");
        assert_eq!(Category::Numeric.short(), "NUMERIC");
        assert_eq!(Category::of(1), Category::Ctype);
        assert_eq!(Category::of(4), Category::Collate);
        assert_eq!(Category::of(9), Category::Messages);
        assert_eq!(full_mask(), MASK_CATS);
    }

    #[test]
    fn category_axes_and_limits() {
        assert_eq!(Category::Ctype.axis(), Axis::Data);
        assert_eq!(Category::Numeric.axis(), Axis::Data);
        assert_eq!(Category::Time.axis(), Axis::Layout);
        assert_eq!(Category::Collate.axis(), Axis::Layout);
        assert_eq!(Category::Messages.axis(), Axis::Language);
        assert_eq!(Category::Identification.axis(), Axis::Meta);
        assert_eq!(Category::Ctype.min_len(), 1024);
        assert_eq!(Category::Ctype.floor_len(), 256);
        assert_eq!(Category::Messages.floor_len(), 4);
        assert!(Category::Identification.min_len() > 0);
    }

    #[test]
    fn masks_and_bits() {
        assert_eq!(cat_bit(1), 2);
        assert_eq!(cat_bit(5), 32);
        assert_eq!(cat_bit(0), 1);
        assert_eq!(cat_bit(SECS), 0);
        assert_eq!(MASK_DATA, 6);
        assert_eq!(MASK_LAYOUT, 24);
        assert_eq!(MASK_LANG, 32);
        assert_eq!(MASK_DATA | MASK_LAYOUT | MASK_LANG, MASK_CATS);
        assert_eq!(popcount(0), 0);
        assert_eq!(popcount(MASK_CATS), 5);
        assert_eq!(popcount(0xffff_ffff), 32);
    }

    #[test]
    fn mask_render() {
        assert_eq!(mask_text(0).as_str(), "-");
        assert_eq!(mask_text(cat_bit(1)).as_str(), "CTYPE");
        assert_eq!(mask_text(MASK_DATA).as_str(), "CTYPE NUMERIC");
        assert_eq!(mask_text(MASK_CATS).as_str(), "CTYPE NUMERIC TIME COLLATE MESSAGES");
        assert_eq!(cat_bit_text(1).as_str(), "2");
    }

    #[test]
    fn fnv_primitives() {
        assert_eq!(FNV_OFF, 0xcbf2_9ce4_8422_2325);
        assert_eq!(FNV_PRIME, 0x100_0000_01b3);
        assert_eq!(fold_str(FNV_OFF, ""), FNV_OFF);
        let a = fold_byte(fold_byte(FNV_OFF, b'a'), b'b');
        assert_eq!(fold_str(FNV_OFF, "ab"), a);
        assert_ne!(fold_word(FNV_OFF, 0), FNV_OFF);
        assert_eq!(fold_word(FNV_OFF, 7), fold_word(FNV_OFF, 7));
        assert_ne!(fold_word(FNV_OFF, 7), fold_word(FNV_OFF, 8));
    }

    #[test]
    fn number_text() {
        assert_eq!(u32_text(0).as_str(), "0");
        assert_eq!(u32_text(929).as_str(), "929");
        assert_eq!(u32_text(65536).as_str(), "65536");
        assert_eq!(u8_text(5).as_str(), "5");
        assert_eq!(i32_pub_text(-3).as_str(), "-3");
        assert_eq!(i32_pub_text(0).as_str(), "0");
        assert_eq!(hex8(0x6cf30).as_str(), "0006cf30");
        assert_eq!(code_text(1).as_str(), "0x00000001");
        assert_eq!(name_text("").as_str(), "<null>");
        assert_eq!(name_text("C").as_str(), "C");
    }

    #[test]
    fn stage_ladder() {
        assert_eq!(Stage::Request.order(), 0);
        assert_eq!(Stage::Name.order(), 1);
        assert_eq!(Stage::Header.order(), 2);
        assert_eq!(Stage::Section.order(), 3);
        assert_eq!(Stage::Content.order(), 4);
        assert_eq!(Stage::Table.order(), 5);
        assert_eq!(Stage::Downgrade.order(), 6);
        assert_eq!(Stage::Ready.order(), 7);
        assert!(Stage::Content.before_table());
        assert!(!Stage::Table.before_table());
        assert!(Stage::Header.structural());
        assert!(!Stage::Request.structural());
        assert_eq!(Stage::Downgrade.name(), "downgrade");
    }

    #[test]
    fn src_ladder() {
        assert_eq!(Src::LcAll.code(), 0);
        assert_eq!(Src::LcAllEmpty.code(), 1);
        assert_eq!(Src::LcCat.code(), 2);
        assert_eq!(Src::Lang.code(), 3);
        assert_eq!(Src::Default.code(), 4);
        assert_eq!(Src::LcAllEmpty.name(), "LC_ALL-empty");
        assert_eq!(Src::Default.name(), "default-C");
        assert!(Src::LcAll.empty_is_request());
        assert!(!Src::LcCat.empty_is_request());
        assert!(!Src::Lang.empty_is_request());
    }

    #[test]
    fn note_semantics() {
        assert_eq!(Note::FullOk.name(), "full-ok");
        assert_eq!(Note::Pseudo.name(), "pseudo-ok");
        assert_eq!(Note::Broken.name(), "broken-C");
        assert!(Note::FullOk.usable() && Note::Fallback.usable() && Note::BuiltIn.usable());
        assert!(!Note::Pseudo.usable() && !Note::ArchiveDead.usable());
        assert!(Note::Broken.loud() && Note::Pseudo.loud());
        assert!(!Note::Fallback.loud());
        assert!(Note::Structural.structural() && Note::ArchiveDead.structural());
        assert!(!Note::FullOk.structural());
    }

    #[test]
    fn tier_ladder() {
        assert_eq!(Tier::Full.index(), 0);
        assert_eq!(Tier::Broken.index(), 3);
        assert_eq!(Tier::BuiltC.name(), "built-in-C");
        assert!(Tier::Full.rank() < Tier::Partial.rank());
        assert!(Tier::Partial.rank() < Tier::BuiltC.rank());
        assert!(Tier::BuiltC.rank() < Tier::Broken.rank());
        assert!(Tier::BuiltC.usable() && !Tier::Broken.usable());
        assert!(Tier::Broken.loud() && !Tier::Full.loud());
        assert!(Tier::Partial.silent_risk() && !Tier::Full.silent_risk());
        assert_eq!(Tier::of(2), Tier::BuiltC);
        assert_eq!(Tier::of(9), Tier::Broken);
    }

    #[test]
    fn portable_charset() {
        assert!(is_portable(b'A') && is_portable(b'z') && is_portable(b'9'));
        assert!(is_portable(b'_') && is_portable(b'+') && is_portable(b'.'));
        assert!(is_portable(b'@') && is_portable(b'-'));
        assert!(!is_portable(b' ') && !is_portable(b';') && !is_portable(0));
        assert!(is_alpha(b'K') && !is_alpha(b'1') && !is_alpha(b'_'));
        assert_eq!(count_sep("a_b_c", b'_'), 2);
        assert_eq!(count_sep("abc", b'_'), 0);
        assert!(lang_field_ok("de") && lang_field_ok("C"));
        assert!(!lang_field_ok("1x") && !lang_field_ok(""));
    }

    #[test]
    fn builtin_names() {
        assert!(is_builtin_name("C") && is_builtin_name("POSIX"));
        assert!(!is_builtin_name("c") && !is_builtin_name("C.UTF-8"));
        let a = check_name("C");
        assert!(a.accept && a.builtin && a.code == NX_BUILTIN);
        assert_eq!(a.code_name(), "builtin");
        let b = check_name("POSIX");
        assert!(b.accept && b.builtin);
        let n = check_name("en_US.UTF-8");
        assert!(n.accept && !n.builtin && n.code == NX_OK);
        assert_eq!(n.code_name(), "legal");
    }

    #[test]
    fn empty_name_is_legal_but_flagged() {
        let n = check_name("");
        assert!(n.accept && !n.builtin);
        assert_eq!(n.code, NX_EMPTY);
        assert_eq!(n.code_name(), "empty");
        assert!(!n.rejected());
    }

    #[test]
    fn illegal_name_codes() {
        assert_eq!(check_name("de_DE.UTF-8;").code, NX_CHAR);
        assert_eq!(check_name("12").code, NX_CHAR);
        assert_eq!(check_name("-fr_FR.UTF-8").code, NX_DASH);
        assert_eq!(check_name("a_b_c.UTF-8").code, NX_SEPS);
        assert_eq!(check_name("a..b").code, NX_SEPS);
        assert_eq!(check_name("en_US@").code, NX_TAIL);
        assert_eq!(check_name("zh_CN.GBK@euro").code, NX_OK);
        assert!(check_name("de_DE.UTF-8;").rejected());
    }

    #[test]
    fn illegal_name_over_length() {
        let mut s = String::from("a_");
        let mut i = 0usize;
        while i < 70 {
            s.push('b');
            i += 1;
        }
        assert!(s.len() > NAME_MAX);
        assert_eq!(check_name(s.as_str()).code, NX_LONG);
        assert_eq!(check_name(s.as_str()).code_name(), "too-long");
    }

    #[test]
    fn illegal_name_gate_order() {
        let mut dash_long = String::from("-");
        let mut i = 0usize;
        while i < 70 {
            dash_long.push('b');
            i += 1;
        }
        assert_eq!(check_name(dash_long.as_str()).code, NX_LONG);
        assert_eq!(check_name("fr_FR_;").code, NX_CHAR);
        assert_eq!(check_name("fr_FR_@").code, NX_SEPS);
        let mut two = String::from("zh_");
        i = 0;
        while i < 60 {
            two.push('X');
            i += 1;
        }
        two.push_str("CN.UTF-8");
        assert_eq!(check_name(two.as_str()).code, NX_LONG);
    }

    #[test]
    fn slot_render_three_states() {
        assert!(!Slot::off().used());
        assert!(!Slot::empty().used());
        assert!(Slot::val("C").used());
        assert_eq!(Slot::off().render("LANG").as_str(), "LANG=<unset>");
        assert_eq!(Slot::empty().render("LANG").as_str(), "LANG=<null>");
        assert_eq!(Slot::val("C").render("LC_ALL").as_str(), "LC_ALL=C");
        assert_eq!(Slot::off().name, "");
        assert_eq!(Slot::empty().name, "");
    }

    #[test]
    fn priority_lc_all_wins() {
        let mut mix = Locale::none();
        mix.lc_all = Slot::val("C");
        if let Some(s) = mix.cats.get_mut(0) {
            *s = Slot::val("de_DE.UTF-8");
        }
        mix.lang = Slot::val("fr_FR.UTF-8");
        let w = mix.wanted(1);
        assert_eq!(w.src, Src::LcAll);
        assert_eq!(w.name, "C");
        assert_eq!(mix.wanted(5).name, "C");
        assert_eq!(mix.percat(), 0);
        assert!(mix.uniform());
        assert_eq!(mix.cat(1).name, "de_DE.UTF-8");
    }

    #[test]
    fn empty_lc_all_means_builtin_c() {
        let alle = Locale::all(Slot::empty());
        let w = alle.wanted(3);
        assert_eq!(w.src, Src::LcAllEmpty);
        assert_eq!(w.name, "C");
        assert!(alle.uniform());
        assert_eq!(alle.percat(), 4681);
        let o = chain(&alle, &Archive::bad_magic("x"), MASK_CATS, true);
        assert_eq!(o.tier, Tier::Full);
        assert_eq!(o.note, Note::BuiltIn);
        assert_eq!(o.fall, 0);
        assert_eq!(o.fatal, 0);
    }

    #[test]
    fn empty_lc_cat_falls_through_to_lang() {
        let mut l = Locale::none();
        if let Some(s) = l.cats.get_mut(0) {
            *s = Slot::empty();
        }
        l.lang = Slot::val("it_IT.ISO-8859-1");
        assert_eq!(l.wanted(1).src, Src::Lang);
        assert_eq!(l.wanted(1).name, "it_IT.ISO-8859-1");
        assert_eq!(l.wanted(2).src, Src::Lang);
    }

    #[test]
    fn empty_lang_means_default_c() {
        let le = Locale::lang_only(Slot::empty());
        let w = le.wanted(1);
        assert_eq!(w.src, Src::Default);
        assert_eq!(w.name, "C");
        assert_eq!(le.percat(), 4 * 4681);
        let bare = Locale::none();
        assert_eq!(bare.wanted(5).src, Src::Default);
        assert_eq!(first_target(&bare), "");
    }

    #[test]
    fn percat_and_distinct() {
        let bycat = Locale {
            lc_all: Slot::off(),
            cats: [Slot::val("C"), Slot::off(), Slot::off(), Slot::off(), Slot::off()],
            lang: Slot::val("fr_FR.UTF-8"),
        };
        assert_eq!(bycat.percat(), 14042);
        assert_eq!(first_target(&bycat), "fr_FR.UTF-8");
        let (names, n) = bycat.distinct();
        assert_eq!(n, 1);
        assert_eq!(match names.get(0) { Some(v) => *v, None => "" }, "fr_FR.UTF-8");
        let (all_names, an) = Locale::all(Slot::val("C")).distinct();
        assert_eq!(an, 0);
        assert!(match all_names.get(0) { Some(v) => v.is_empty(), None => false });
        assert!(!bycat.uniform());
        assert_eq!(bycat.env_text().as_str(), "LC_ALL=<unset> LANG=fr_FR.UTF-8 LC_CTYPE=C");
    }

    #[test]
    fn cat_key_table() {
        assert_eq!(Locale::cat_key(1), "LC_CTYPE");
        assert_eq!(Locale::cat_key(2), "LC_NUMERIC");
        assert_eq!(Locale::cat_key(5), "LC_MESSAGES");
        assert_eq!(Locale::cat_key(9), "LC_UNKNOWN");
        assert_eq!(Locale::cat_key(0), "LC_UNKNOWN");
        assert_eq!(Locale::none().cat(0), Slot::off());
        assert_eq!(Locale::none().cat(SECS), Slot::off());
    }

    #[test]
    fn header_gate_order() {
        let ok = Archive::complete("x");
        assert!(!header_gate(&ok).fault);
        assert_eq!(header_gate(&ok), ArcGate::none());
        assert_eq!(header_gate(&Archive::bad_magic("x")), ArcGate::bad(AR_MAGIC, Stage::Header, 0));
        assert_eq!(
            header_gate(&Archive::bad_version("x")),
            ArcGate::bad(AR_VERSION, Stage::Header, 0)
        );
        assert_eq!(header_gate(&Archive::truncated("x")), ArcGate::bad(AR_RANGE, Stage::Header, 0));
        assert_eq!(header_gate(&Archive::locpath_miss("x")), ArcGate::bad(AR_LOC, Stage::Header, 0));
        assert!(!header_gate(&Archive::locpath_hit("x")).fault);
        let mut both = Archive::bad_version("x");
        both.magic = MAGIC_BAD;
        assert_eq!(header_gate(&both).code, AR_MAGIC);
        let mut bypass = Archive::bad_magic("x");
        bypass.locpath = true;
        bypass.locpath_ok = false;
        assert_eq!(header_gate(&bypass).code, AR_LOC);
        assert!(ok.magic_ok());
        assert!(!Archive::bad_magic("x").magic_ok());
        assert_ne!(MAGIC_LOG1, MAGIC_LOC1);
    }

    #[test]
    fn section_gate_codes() {
        let a = Archive::complete("x");
        assert!(!section_gate(&a, 1).fault);
        assert_eq!(section_gate(&a, SECS), ArcGate::bad(AR_ABSENT, Stage::Section, SECS as u8));
        assert_eq!(section_gate(&a, 99).idx, 99);
        assert_eq!(
            section_gate(&Archive::missing(2, "x"), 2),
            ArcGate::bad(AR_ABSENT, Stage::Section, 2)
        );
        assert!(!section_gate(&Archive::missing(2, "x"), 1).fault);
        assert_eq!(
            section_gate(&Archive::oob(3, 70000, 8192, "x"), 3),
            ArcGate::bad(AR_SECRANGE, Stage::Section, 3)
        );
        assert_eq!(
            section_gate(&Archive::sec_bad_magic(4, "x"), 4),
            ArcGate::bad(AR_SECMAGIC, Stage::Content, 4)
        );
        assert_eq!(
            section_gate(&Archive::sec_bad_version(4, "x"), 4),
            ArcGate::bad(AR_SECVER, Stage::Content, 4)
        );
        assert_eq!(
            section_gate(&Archive::shortened(1, 8, "x"), 1),
            ArcGate::bad(AR_SHORT, Stage::Content, 1)
        );
    }

    #[test]
    fn structural_rejects_precede_tables() {
        let probes = [
            Archive::bad_magic("x"),
            Archive::bad_version("x"),
            Archive::truncated("x"),
            Archive::locpath_miss("x"),
            Archive::missing(1, "x"),
            Archive::oob(1, 65000, 4096, "x"),
            Archive::sec_bad_magic(1, "x"),
            Archive::sec_bad_version(1, "x"),
            Archive::shortened(1, 8, "x"),
        ];
        let mut i = 0usize;
        while i < probes.len() {
            let g = match probes.get(i) {
                Some(a) => first_structural(a),
                None => fail(),
            };
            assert!(g.fault);
            assert!(g.before_table());
            assert!(g.stage.structural());
            i += 1;
        }
        let clean = Archive::complete("x");
        assert!(!first_structural(&clean).fault);
        assert_eq!(first_structural(&clean).code_name(), "none");
        assert!(section_usable(&clean, 1));
        assert!(!section_usable(&Archive::shortened(1, 8, "x"), 1));
        assert!(section_usable(&Archive::shortened(1, 8, "x"), 2));
    }

    #[test]
    fn gate_code_names() {
        assert_eq!(ArcGate::none().code_name(), "none");
        assert_eq!(ArcGate::bad(AR_MAGIC, Stage::Header, 0).code_name(), "arc-magic");
        assert_eq!(ArcGate::bad(AR_SECRANGE, Stage::Section, 3).code_name(), "sec-range");
        assert_eq!(ArcGate::bad(AR_LOC, Stage::Header, 0).code_name(), "locpath-miss");
        assert!(ArcGate::bad(AR_SHORT, Stage::Content, 1).before_table());
        assert!(!ArcGate::bad(AR_SHORT, Stage::Table, 1).before_table());
    }

    #[test]
    fn identification_mismatch_only_warns() {
        assert!(ident_matches(&Archive::complete("en_US.UTF-8"), "en_US.UTF-8"));
        assert!(!ident_matches(&Archive::mislabeled("en_US.UTF-8", "C"), "en_US.UTF-8"));
        let sh = shape_all(
            "ident-warn",
            "en_US.UTF-8",
            Archive::mislabeled("en_US.UTF-8", "en_GB.UTF-8"),
            MASK_CATS,
            true,
        );
        let o = sh.actual();
        assert_eq!(o.tier, Tier::Full);
        assert!(o.ident_bad);
        assert_eq!(o.code, 0);
        assert!(o.ok());
        assert!(!o.clean());
        let txt = message_for(&sh, &o);
        assert!(txt.as_str().contains("kept, not downgraded"));
    }

    #[test]
    fn chain_full_and_builtin() {
        let sh = shape_all("t0", "en_US.UTF-8", Archive::complete("en_US.UTF-8"), MASK_CATS, true);
        let o = sh.actual();
        assert_eq!(o.tier, Tier::Full);
        assert_eq!(o.note, Note::FullOk);
        assert_eq!(o.fall, 0);
        assert_eq!(o.c_used, 0);
        assert_eq!(o.code, 0);
        assert_eq!(o.stage, Stage::Ready);
        assert!(o.ok() && o.clean());
        assert_eq!(o.tier_text().as_str(), "full");
        let b = shape_lang("t1", "C", Archive::bad_magic("x"), 0, true).actual();
        assert_eq!(b.tier, Tier::Full);
        assert_eq!(b.note, Note::BuiltIn);
        assert_eq!(b.c_used, MASK_CATS);
        assert_eq!(b.fall, 0);
        let blank = Shape::blank().actual();
        assert_eq!(blank.tier, Tier::Full);
        assert_eq!(blank.note, Note::BuiltIn);
        assert!(blank.ok());
    }

    #[test]
    fn chain_partial_is_not_builtin_c() {
        let p = shape_all("p1", "en_US.UTF-8", Archive::missing(4, "en_US.UTF-8"), MASK_CATS, true);
        let o = p.actual();
        assert_eq!(o.tier, Tier::Partial);
        assert_eq!(o.fall, cat_bit(4));
        assert_eq!(o.c_used, cat_bit(4));
        assert_eq!(o.acct, 1);
        assert_eq!(o.note, Note::Fallback);
        assert!(o.ok());
        let a = shape_all("a1", "en_US.UTF-8", Archive::bad_magic("en_US.UTF-8"), MASK_CATS, true);
        let g = a.actual();
        assert_eq!(g.tier, Tier::BuiltC);
        assert_eq!(g.fall, MASK_CATS);
        assert_eq!(g.acct, 5);
        assert_eq!(g.note, Note::ArchiveDead);
        assert_eq!(g.code, AR_MAGIC);
        assert_eq!(g.stage, Stage::Header);
    }

    #[test]
    fn chain_broken_needs_dead_c_tables() {
        let dead = shape_all("d1", "C", Archive::complete("x"), 0, false).actual();
        assert_eq!(dead.tier, Tier::Broken);
        assert_eq!(dead.code, BRK_CTYPE);
        assert_eq!(dead.stage, Stage::Table);
        assert_eq!(dead.fall, 0);
        assert_eq!(dead.tools, 0);
        assert_eq!(dead.fatal, 3);
        assert!(!dead.ok());
        assert_eq!(dead.note, Note::Broken);
        assert!(!dead.rejected_before_table());
        assert_eq!(code_short(dead.code), "ctype-dead");
        let target = shape_all("d2", "C.UTF-8", Archive::complete("C.UTF-8"), MASK_CATS, false).actual();
        assert_eq!(target.tier, Tier::Full);
        assert!(target.c_dead && target.rotten() && target.ok());
        let miss = shape_all("d4", "C.UTF-8", Archive::missing(1, "C.UTF-8"), 0, false).actual();
        assert_eq!(miss.tier, Tier::Broken);
        assert_eq!(miss.fall, cat_bit(1));
        assert_eq!(miss.pseudo, 1);
        assert_eq!(miss.unlogged, 5);
        assert_eq!(miss.tools, 5);
        assert_eq!(miss.acct, 0);
    }

    #[test]
    fn pseudo_success_only_on_data_axis() {
        let arc = Archive::shortened(1, 64, "de_DE.UTF-8");
        let o = shape_all("q1", "de_DE.UTF-8", arc, 0, true).actual();
        assert_eq!(o.pseudo, 1);
        assert_eq!(o.fall, cat_bit(1));
        assert_eq!(o.acct, 0);
        assert_eq!(o.note, Note::Pseudo);
        assert_eq!(o.stage, Stage::Downgrade);
        assert_eq!(o.code, AR_SHORT);
        assert!(!o.ok());
        assert!(!o.rejected_before_table());
        let fixed = shape_all("q1", "de_DE.UTF-8", arc, cat_bit(1), true).actual();
        assert_eq!(fixed.pseudo, 0);
        assert_eq!(fixed.acct, 1);
        assert_eq!(fixed.note, Note::Fallback);
        assert!(fixed.ok());
        let layout = shape_all("q2", "en_US.UTF-8", Archive::missing(4, "en_US.UTF-8"), 0, true).actual();
        assert_eq!(layout.pseudo, 0);
        assert_eq!(unlogged_data(MASK_LAYOUT, 0), 0);
        assert!(logged_all(MASK_CATS, MASK_CATS));
        assert!(!logged_all(MASK_CATS, MASK_DATA));
        assert_eq!(unlogged_data(MASK_CATS, 0), 2);
    }

    #[test]
    fn tool_level_pseudo_on_language_axis() {
        let sh = shape_all("m1", "en_US.UTF-8", Archive::missing(5, "en_US.UTF-8"), 0, true);
        let o = sh.actual();
        assert_eq!(o.pseudo, 0);
        assert_eq!(o.unlogged, 5);
        assert_eq!(o.fall, MASK_LANG);
        assert_eq!(o.tools, 15);
        assert!(!o.ok());
        assert_eq!(o.note, Note::Pseudo);
        assert_eq!(
            message_for(&sh, &o).as_str(),
            "warning: locale downgrade not accounted for MESSAGES in rm"
        );
    }

    #[test]
    fn name_gate_rejects_before_archive() {
        let arc = Archive::complete("de_DE.UTF-8;");
        let sh = shape_all("n1", "de_DE.UTF-8;", arc, MASK_CATS, true);
        let o = sh.actual();
        assert_eq!(o.tier, Tier::BuiltC);
        assert_eq!(o.stage, Stage::Name);
        assert_eq!(o.code, NX_CHAR);
        assert!(o.rejected_before_table());
        assert_eq!(o.note, Note::NameBad);
        assert!(all_name_rejected(&sh.req()));
        let sk = sh.actual_skip_name();
        assert_eq!(sk.tier, Tier::Full);
        assert_eq!(sk.code, 0);
        assert_eq!(sk.fall, 0);
        assert_eq!(sk.note, Note::FullOk);
        assert!(!all_name_rejected(&Locale::all(Slot::val("en_US.UTF-8"))));
    }

    #[test]
    fn chain_matches_explicit_calls() {
        let req = Locale::all(Slot::val("ru_RU.KOI8-R"));
        let arc = Archive::oob(4, 65000, 4096, "ru_RU.KOI8-R");
        let o = chain(&req, &arc, MASK_CATS, true);
        assert_eq!(o.tier, Tier::Partial);
        assert_eq!(o.code, AR_SECRANGE);
        assert_eq!(o.stage, Stage::Section);
        assert_eq!(o.fall, cat_bit(4));
        let n = chain_no_name(&Locale::all(Slot::val("-x_Y")), &Archive::complete("x"), MASK_CATS, true);
        assert_eq!(n.tier, Tier::Full);
        assert_eq!(n.fall, 0);
        let bad = chain(&Locale::all(Slot::val("-x_Y")), &Archive::complete("x"), MASK_CATS, true);
        assert_eq!(bad.tier, Tier::BuiltC);
        assert_eq!(bad.code, NX_DASH);
        assert_eq!(bad.code_text().as_str(), "23");
    }

    #[test]
    fn tool_table_identity() {
        assert_eq!(TOOL_LIST.len(), TOOLS);
        assert_eq!(tool_index("cat"), 0);
        assert_eq!(tool_index("ls"), 1);
        assert_eq!(tool_index("date"), 10);
        assert_eq!(tool_index("printf"), 13);
        assert_eq!(tool_index("wc"), 19);
        assert_eq!(tool_index("whoami"), TOOLS);
        assert_eq!(tool_index("nosuch"), TOOLS);
        assert!(tool_at(0).is_some() && tool_at(TOOLS).is_none());
        let mut i = 0usize;
        while i < TOOLS {
            let t = match tool_at(i) {
                Some(v) => v,
                None => fail(),
            };
            assert!(!t.name.is_empty());
            assert_eq!(tool_index(t.name), i);
            assert_eq!(t.deps & !MASK_CATS, 0);
            assert!(!t.rule.is_empty());
            i += 1;
        }
    }

    #[test]
    fn tool_dependency_axes() {
        let cat = match tool_at(0) { Some(t) => t, None => fail() };
        assert!(cat.touches_lang() && !cat.touches_data() && cat.byte_only);
        let ls = match tool_at(1) { Some(t) => t, None => fail() };
        assert!(ls.touches_data() && ls.touches_layout() && ls.must_report && ls.broken_fatal);
        let uname = match tool_at(11) { Some(t) => t, None => fail() };
        assert!(uname.insensitive() && uname.byte_only && !uname.diag);
        assert!(TOOL_LIST[12].insensitive() && TOOL_LIST[14].insensitive());
        assert!(TOOL_LIST[17].insensitive() && TOOL_LIST[18].insensitive());
        assert_eq!(TOOL_LIST[13].delta_mask(MASK_CATS), cat_bit(2) | cat_bit(1) | MASK_LANG);
        assert_eq!(TOOL_LIST[0].delta_mask(MASK_DATA), 0);
        assert!(!TOOL_LIST[4].must_report == false);
        assert!(!Tool::blank().touches_data());
    }

    #[test]
    fn effects_per_tool_are_not_templated() {
        assert_eq!(effect_of(0, MASK_DATA), Behav::Identical);
        assert_eq!(effect_of(0, MASK_LANG), Behav::MsgOnly);
        assert_eq!(effect_of(1, MASK_CATS), Behav::SortAscii);
        assert_eq!(effect_of(1, cat_bit(1) | MASK_LANG), Behav::WidthBytes);
        assert_eq!(effect_of(2, MASK_DATA), Behav::Identical);
        assert_eq!(effect_of(3, MASK_LANG), Behav::MsgOnly);
        assert_eq!(effect_of(4, MASK_LANG), Behav::RespAscii);
        assert_eq!(effect_of(4, cat_bit(1)), Behav::WidthBytes);
        assert_eq!(effect_of(10, cat_bit(3)), Behav::DateFormat);
        assert_eq!(effect_of(10, cat_bit(1)), Behav::WidthBytes);
        assert_eq!(effect_of(11, MASK_CATS), Behav::Identical);
        assert_eq!(effect_of(12, MASK_CATS), Behav::Identical);
        assert_eq!(effect_of(13, cat_bit(2)), Behav::DecimalShift);
        assert_eq!(effect_of(13, cat_bit(1)), Behav::WidthBytes);
        assert_eq!(effect_of(14, MASK_CATS), Behav::Identical);
        assert_eq!(effect_of(17, MASK_CATS), Behav::Identical);
        assert_eq!(effect_of(18, MASK_CATS), Behav::Identical);
        assert_eq!(effect_of(19, cat_bit(1)), Behav::WidthBytes);
        assert_eq!(effect_of(99, MASK_CATS), Behav::Identical);
        assert_eq!(TOOL_LIST[13].effect(13, MASK_DATA), Behav::DecimalShift);
        assert_eq!(TOOL_LIST[11].effect(11, MASK_CATS), Behav::Identical);
        assert_eq!(TOOL_LIST[5].effect(5, MASK_DATA), Behav::Identical);
    }

    #[test]
    fn behaviour_classification() {
        assert_eq!(Behav::Identical.name(), "identical");
        assert_eq!(Behav::SortAscii.name(), "sort-ascii");
        assert_eq!(Behav::DecimalShift.name(), "decimal-point");
        assert_eq!(Behav::DateFormat.name(), "date-format");
        assert_eq!(Behav::WidthBytes.name(), "byte-width");
        assert_eq!(Behav::RespAscii.name(), "answer-ascii");
        assert_eq!(Behav::MsgOnly.name(), "msg-language");
        assert!(!Behav::Identical.changed());
        assert!(Behav::MsgOnly.changed());
        assert!(Behav::WidthBytes.numeric() && Behav::DecimalShift.numeric());
        assert!(!Behav::DateFormat.numeric());
        assert!(Behav::MsgOnly.cosmetic() && !Behav::SortAscii.cosmetic());
    }

    #[test]
    fn matrix_hit_counts() {
        assert_eq!(hit_tools(MASK_CATS), 15);
        assert_eq!(hit_tools(0), 0);
        assert_eq!(hit_tools(cat_bit(4)), 1);
        assert_eq!(hit_tools(cat_bit(2)), 1);
        assert_eq!(hit_tools(cat_bit(3)), 1);
        assert_eq!(hit_tools(cat_bit(1)), 5);
        assert_eq!(hit_tools(MASK_LANG), 15);
        assert_eq!(single_fall_hits(), [5, 1, 1, 1, 15]);
        assert_eq!(numeric_tools(MASK_DATA), 5);
        assert_eq!(cosmetic_tools(MASK_LAYOUT), 2);
        assert_eq!(lang_only_tools(), 10);
        assert_eq!(insensitive_tools(), 5);
        assert_eq!(fatal_tools(MASK_CATS, true), 3);
        // 非崩溃档仍要按类别真的变卦来数：全掩码下三件致命工具都变，空掩码下全不变。
        assert_eq!(fatal_tools(MASK_CATS, false), 3);
        assert_eq!(fatal_tools(0, false), 0);
        assert_eq!(unlogged_tools(MASK_CATS, 0), 5);
        assert_eq!(unlogged_tools(MASK_CATS, MASK_CATS), 0);
    }

    #[test]
    fn fault_codes_and_texts() {
        assert_eq!(Fault::Ok.code(), 0);
        assert_eq!(Fault::Estate.code(), LM_ESTATE);
        assert_eq!(Fault::Tier.code(), LM_ETIER);
        assert_eq!(Fault::Parse.code(), LM_EPARSE);
        assert_eq!(Fault::Name.code(), LM_ENAME);
        assert_eq!(Fault::Arch.code(), LM_EARCH);
        assert_eq!(Fault::Count.code(), LM_ECOUNT);
        assert_eq!(Fault::Tool.code(), LM_ETOOL);
        assert_eq!(Fault::Order.code(), LM_EORDER);
        assert_eq!(Fault::Matrix.code(), LM_EMATRIX);
        assert_eq!(Fault::Slot.code(), LM_ESLOT);
        assert_eq!(Fault::Seal.code(), LM_ESEAL);
        assert_eq!(Fault::Nobs.code(), LM_ENOBS);
        assert_eq!(Fault::Bal.code(), LM_EBAL);
        assert!(!Fault::Ok.alive());
        assert!(Fault::Bal.alive());
        assert!(Fault::Estate.accounting() && Fault::Count.accounting());
        assert!(!Fault::Tier.accounting());
        assert_eq!(fault_short(Fault::Estate), "estate");
        assert_eq!(fault_short(Fault::Ok), "ok");
        assert_eq!(code_short(0), "none");
        assert_eq!(code_short(AR_ABSENT), "sec-absent");
        assert_eq!(code_short(NX_TAIL), "tail-sep");
    }

    #[test]
    fn corpus_shape_tags() {
        assert_eq!(SHAPES.len(), CORPUS);
        assert_eq!(CLAIMS.len(), CORPUS);
        let cs = cases();
        assert_eq!(case_at(0).map(|c| c.tag()), Some("full-en_US-utf8"));
        assert_eq!(case_at(7).map(|c| c.tag()), Some("broken-builtin-ctype-dead"));
        assert_eq!(case_at(CORPUS).map(|c| c.tag()), None);
        let mut i = 0usize;
        while i < CORPUS {
            let c = match cs.get(i) {
                Some(v) => *v,
                None => fail(),
            };
            assert!(!c.tag().is_empty());
            let mut j = i + 1;
            while j < CORPUS {
                if let Some(d) = cs.get(j) {
                    assert_ne!(c.tag(), d.tag());
                    assert_ne!(c.shape.seal(), d.shape.seal());
                }
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn corpus_tier_distribution() {
        let cs = cases();
        let mut full = 0usize;
        let mut partial = 0usize;
        let mut builtc = 0usize;
        let mut broken = 0usize;
        let mut i = 0usize;
        while i < CORPUS {
            match cs.get(i) {
                Some(c) => match c.actual().tier {
                    Tier::Full => full += 1,
                    Tier::Partial => partial += 1,
                    Tier::BuiltC => builtc += 1,
                    Tier::Broken => broken += 1,
                },
                None => fail(),
            }
            i += 1;
        }
        assert_eq!(full, 2);
        assert_eq!(partial, 7);
        assert_eq!(builtc, 3);
        assert_eq!(broken, 2);
        assert_eq!(full + partial + builtc + broken, CORPUS);
    }

    #[test]
    fn judge_table_green_then_red() {
        let cs = cases();
        let mut i = 0usize;
        while i < CORPUS {
            let c = match cs.get(i) {
                Some(v) => *v,
                None => fail(),
            };
            let f = c.judge();
            if i < ACCEPT {
                assert_eq!(f, Fault::Ok);
                assert!(c.ok());
            } else {
                assert_ne!(f, Fault::Ok);
                assert!(!c.ok());
                assert_eq!(f, match expected(i) { Some(v) => v, None => fail() });
                assert_eq!(f, match REJECT_FAULTS.get(i - ACCEPT) { Some(v) => *v, None => fail() });
                assert_eq!(f.code(), match REJECT_CODES.get(i - ACCEPT) { Some(v) => *v, None => fail() });
            }
            i += 1;
        }
        assert_eq!(expected(ACCEPT), Some(Fault::Estate));
        assert_eq!(expected(CORPUS - 1), Some(Fault::Order));
        assert_eq!(expected(CORPUS), None);
        assert!(cover(CORPUS).is_ok());
        assert_eq!(cover(0), Err(LM_ENOBS));
    }

    #[test]
    fn judge_axis_order_is_fixed() {
        let cs = cases();
        let c = match cs.get(3) {
            Some(v) => *v,
            None => fail(),
        };
        let a = c.actual();
        assert!(a.ok());
        let mut lie = Claim::of(a);
        lie.ok = false;
        assert_eq!(judge_against(&lie, &a), Fault::Estate);
        let mut t = Claim::of(a);
        t.tier = Tier::Full;
        assert_eq!(judge_against(&t, &a), Fault::Tier);
        let mut pc = Claim::of(a);
        pc.percat = 7;
        assert_eq!(judge_against(&pc, &a), Fault::Parse);
        t.percat = 7;
        assert_eq!(judge_against(&t, &a), Fault::Tier);
        let mut cd = Claim::of(a);
        cd.code = NX_DASH;
        assert_eq!(judge_against(&cd, &a), Fault::Arch);
        let c10 = match cs.get(10) {
            Some(v) => *v,
            None => fail(),
        };
        let a10 = c10.actual();
        assert_eq!(a10.code, NX_CHAR);
        let mut nm = Claim::of(a10);
        nm.code = NX_DASH;
        assert_eq!(judge_against(&nm, &a10), Fault::Name);
        let mut ac = Claim::of(a);
        ac.acct = 0;
        assert_eq!(judge_against(&ac, &a), Fault::Count);
        let mut tl = Claim::of(a);
        tl.tools = 20;
        assert_eq!(judge_against(&tl, &a), Fault::Tool);
        let mut st = Claim::of(a);
        st.stage = Stage::Table;
        assert_eq!(judge_against(&st, &a), Fault::Order);
        assert_eq!(judge_against(&Claim::of(a), &a), Fault::Ok);
    }

    #[test]
    fn blank_case_is_estate() {
        let c = Case::blank();
        assert_eq!(c.tag(), "");
        assert!(!c.claim.success());
        assert_eq!(c.judge(), Fault::Estate);
        assert_eq!(Claim::blank().tier, Tier::Full);
        assert_eq!(Outcome::blank().tier, Tier::Full);
        assert_eq!(Outcome::blank().note, Note::FullOk);
        assert_eq!(Row::blank().total, 0);
        assert!(!Row::blank().perfect());
        assert!(Row { total: 2, good: 2, hits: 0, pseudo: 0 }.perfect());
    }

    #[test]
    fn fault_category_resolution() {
        let cs = cases();
        let c4 = match cs.get(4) {
            Some(v) => *v,
            None => fail(),
        };
        assert_eq!(fault_category(&c4.shape, &c4.actual()), Category::Messages);
        let c3 = match cs.get(3) {
            Some(v) => *v,
            None => fail(),
        };
        assert_eq!(fault_category(&c3.shape, &c3.actual()), Category::Ctype);
        let c7 = match cs.get(7) {
            Some(v) => *v,
            None => fail(),
        };
        assert_eq!(fault_category(&c7.shape, &c7.actual()), Category::Ctype);
        let blank = Shape::blank();
        let bo = blank.actual();
        assert_eq!(bo.fall, 0);
        assert_eq!(fault_category(&blank, &bo), Category::Identification);
        let plain = shape_all("plain", "en_US.UTF-8", Archive::complete("en_US.UTF-8"), 0, true);
        let po = plain.actual();
        assert_eq!(fault_category(&plain, &po), Category::Ctype);
        let collate_only = shape_all("co", "en_US.UTF-8", Archive::missing(4, "en_US.UTF-8"), 0, true);
        let oc = collate_only.actual();
        assert_eq!(fault_category(&collate_only, &oc), Category::Collate);
    }

    #[test]
    fn detail_lines_verbatim() {
        let cs = cases();
        let c = match cs.get(2) {
            Some(v) => *v,
            None => fail(),
        };
        assert_eq!(detail(&c).as_str(), DETAIL_COLLATE);
        let d = match cs.get(6) {
            Some(v) => *v,
            None => fail(),
        };
        assert_eq!(detail(&d).as_str(), DETAIL_LOCPATH);
        let e = match cs.get(3) {
            Some(v) => *v,
            None => fail(),
        };
        assert_eq!(detail(&e).as_str(), DETAIL_CT);
        let f = match cs.get(10) {
            Some(v) => *v,
            None => fail(),
        };
        assert_eq!(detail(&f).as_str(), DETAIL_RD);
        assert!(detail(&c).as_str().ends_with('\n'));
        let g = match cs.get(0) {
            Some(v) => *v,
            None => fail(),
        };
        assert_eq!(
            claim_text(&g).as_str(),
            "full-en_US-utf8 claim tier=full code=0 acct=0 tools=0 fault=ok\n"
        );
    }

    #[test]
    fn report_numbers_and_render() {
        let r = match run() {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(r.total, 14);
        assert_eq!(r.accept, ACCEPT as u32);
        assert_eq!(r.reject, REJECT as u32);
        assert_eq!(r.full, 2);
        assert_eq!(r.partial, 3);
        assert_eq!(r.builtc, 2);
        assert_eq!(r.broken, 1);
        assert_eq!(r.matched, 14);
        assert_eq!(r.pseudo, 0);
        assert_eq!(r.hits, 51);
        assert_eq!(r.rate(), 1000);
        assert!(r.green());
        assert_eq!(r.gate(), Ok(()));
        assert_eq!(r.tier_accept(Tier::Partial), 3);
        assert_eq!(r.tier_accept(Tier::Broken), 1);
        assert_eq!(r.render().as_str(), "locale-matrix corpus=14 accept=8 reject=6 full=2 partial=3 builtc=2 broken=1 hits=51 pseudo=0 match=14 rate=1000ppm\n");
        assert_eq!(VALS, [14, 8, 6, 2, 3, 2, 1, 929]);
        assert_eq!(VALS[7], 929);
        assert!(vals_match(&r));
    }

    #[test]
    fn report_rate_is_computed() {
        let r = match run_with(Some((0, Fault::Count))) {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(r.matched, 13);
        assert_eq!(r.rate(), 929);
        assert!(!r.green());
        assert_eq!(r.gate(), Err(LM_ECOUNT));
        assert_eq!(r.accept, 7);
        assert_eq!(r.reject, 7);
        assert_eq!(r.pseudo, 0);
        assert_eq!(run_with(Some((CORPUS, Fault::Count))).err(), Some(LM_ESLOT));
        assert_eq!(run_with(Some((13, Fault::Parse))).map(|x| x.matched), Ok(13));
        assert_eq!(run_with(Some((9, Fault::Ok))).map(|x| x.accept), Ok(9));
        assert_eq!(run().and_then(|x| x.gate()), Ok(()));
    }

    #[test]
    fn empty_assertion_set_is_rejected() {
        let empty: [Case; 0] = [];
        assert_eq!(count_over(&empty, None).err(), Some(LM_ENOBS));
        assert_eq!(cover(13).err(), Some(LM_ENOBS));
        assert!(count_over(&cases(), None).is_ok());
    }

    #[test]
    fn matrix_rows_computed() {
        let m = Matrix::of();
        assert_eq!(m.row(0), Some(Row { total: 2, good: 2, hits: 0, pseudo: 0 }));
        assert_eq!(m.row(1), Some(Row { total: 7, good: 3, hits: 29, pseudo: 6 }));
        assert_eq!(m.row(2), Some(Row { total: 3, good: 2, hits: 45, pseudo: 0 }));
        assert_eq!(m.row(3), Some(Row { total: 2, good: 1, hits: 5, pseudo: 6 }));
        assert_eq!(m.row(4), None);
        assert_eq!(m.total(), CORPUS);
        assert_eq!(m.good(), ACCEPT);
        assert_eq!(m.hits(), 79);
        assert_eq!(m.pseudo_all(), 12);
        assert!(m.balanced());
        assert_eq!(m.perfect_rows(), 1);
        assert_eq!(m.render_row(0).as_str(), "full=2/2/hits0");
        assert_eq!(m.render_row(3).as_str(), "broken=2/1/hits5");
        assert_eq!(m.render_row(9).as_str(), "broken=0/0/hits0");
        assert_eq!(m.seal(), Matrix::of().seal());
        assert_ne!(m.seal(), Matrix::blank().seal());
        assert_eq!(Matrix::blank().total(), 0);
        assert!(!Matrix::blank().balanced());
    }

    #[test]
    fn matrix_without_tier_skips_rows() {
        let skip = matrix_without(2);
        let full = Matrix::of();
        assert_eq!(skip.total() + 3, full.total());
        assert_eq!(skip.row(2), Some(Row::blank()));
        assert!(skip.hits() < full.hits());
        assert!(skip.good() < full.good());
        assert!(!skip.balanced());
        assert_eq!(matrix_without(0).row(0), Some(Row::blank()));
        assert_eq!(matrix_without(9).total(), full.total());
    }

    #[test]
    fn gate_is_monotone_and_closes() {
        let mut g = Gate::new();
        assert_eq!(g.last(), -1);
        assert_eq!(g.steps(), 0);
        assert!(!g.closed());
        assert_eq!(g.step(Stage::Content), Ok(()));
        assert_eq!(g.step(Stage::Section), Err(LM_EORDER));
        assert_eq!(g.steps(), 1);
        assert_eq!(g.step(Stage::Table), Ok(()));
        assert_eq!(g.close(), Ok(2));
        assert_eq!(g.step(Stage::Ready), Err(LM_ESTATE));
        assert_eq!(g.close(), Err(LM_ESTATE));
        let cs = cases();
        assert_eq!(
            Gate::walk(&match cs.get(3) { Some(v) => v.actual(), None => fail() }).ok(),
            Some(5)
        );
        assert_eq!(
            Gate::walk(&match cs.get(5) { Some(v) => v.actual(), None => fail() }).ok(),
            Some(3)
        );
        assert_eq!(
            Gate::walk(&match cs.get(7) { Some(v) => v.actual(), None => fail() }).ok(),
            Some(6)
        );
        assert_eq!(
            Gate::walk(&match cs.get(11) { Some(v) => v.actual(), None => fail() }).ok(),
            Some(7)
        );
        let late = {
            let mut x = Archive::complete("x");
            x.secs[1] = Section { present: true, off: 64, len: 8, magic: true, version: true };
            shape_all("late", "x", x, MASK_CATS, true).actual()
        };
        assert_eq!(late.code, AR_SHORT);
        assert_eq!(Gate::walk(&late).ok(), Some(5));
        assert_eq!(Section::good(8, 64).len, 64);
        assert!(!Section::missing().present);
        assert_eq!(Section::oob(1, 2).len, 2);
        assert_eq!(Section::blank().off, 0);
        assert_eq!(Section::bad_magic(1, 2).magic, false);
        assert_eq!(Section::bad_version(1, 2).version, false);
    }

    #[test]
    fn ledger_shape_and_audit() {
        let mut l = Ledger::new();
        assert!(l.is_empty());
        assert_eq!(l.len(), 0);
        assert!(l.sample(0).is_none());
        assert_eq!(l.audit(), Err(LM_EBAL));
        assert_eq!(l.register(count_cell(0, OP_CORPUS, Ok(14))), Ok(()));
        assert_eq!(l.register(count_cell(2, OP_CORPUS, Ok(6))), Err(LM_ESLOT));
        assert_eq!(l.rejects(), 1);
        assert_eq!(l.len(), 1);
        assert_eq!(l.ok_count(), 1);
        assert_eq!(l.err_count(), 0);
        assert!(!l.is_empty());
        assert_eq!(Ledger::new().seal(), Ledger::new().seal());
        assert_eq!(Sample::blank().slot, 0);
        assert!(!Sample::blank().ok);
        assert_eq!(count_cell(1, OP_NAME, Err(LM_EORDER)).code, LM_EORDER);
        assert_eq!(OP_PARSE, 1);
        assert_eq!(OP_CORPUS, 7);
    }

    #[test]
    fn ledger_freeze_and_replay() {
        let mut l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.audit(), Ok(()));
        assert_eq!(l.ok_count(), ACCEPT);
        assert_eq!(l.err_count(), REJECT);
        assert_eq!(l.err_codes(), REJECT_CODES);
        let root = match l.freeze() {
            Ok(r) => r,
            Err(_) => fail(),
        };
        assert_ne!(root, 0);
        assert!(l.is_frozen());
        assert_eq!(l.root(), root);
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.register(Sample::blank()), Err(LM_ESLOT));
        assert_eq!(l.rejects(), 1);
        assert_eq!(l.freeze(), Err(LM_EBAL));
        assert_eq!(l.rejects(), 2);
        // 拒绝计数进指纹：一次拒登就足以让重放对不上 pristine 重建账。
        assert_eq!(l.replay(), Err(LM_ESEAL));
        let mut bad = build_ledger();
        if let Some(s) = bad.rows.get_mut(0) {
            s.value = 13;
        }
        assert_eq!(bad.audit(), Err(LM_ECOUNT));
    }

    #[test]
    fn ledger_sample_values_match_corpus() {
        let l = build_ledger();
        let want = [14u32, 8, 6, 2, 3, 2, 1, 929];
        let mut k = 0usize;
        while k < ACCEPT {
            let s = match l.sample(k) {
                Some(v) => v,
                None => fail(),
            };
            assert!(s.ok);
            assert_eq!(s.code, 0);
            assert_eq!(s.value, match want.get(k) { Some(v) => *v, None => fail() });
            assert_eq!(s.slot as usize, k);
            k += 1;
        }
        let mut j = 0usize;
        while j < REJECT {
            let s = match l.sample(ACCEPT + j) {
                Some(v) => v,
                None => fail(),
            };
            assert!(!s.ok);
            assert_eq!(s.code, match REJECT_CODES.get(j) { Some(v) => *v, None => fail() });
            assert_eq!(s.value, 0);
            j += 1;
        }
        assert!(l.sample(SAMPLES).is_none());
    }

    #[test]
    fn injections_are_caught() {
        let inj = inject_four();
        assert!(inj[0] && inj[1] && inj[2] && inj[3]);
        assert_eq!(inject_four(), [true, true, true, true]);
        assert!(inject_empty_set());
        assert!(inject_absent_gate());
        assert!(inject_skip_tier());
        assert!(inject_false_green());
    }

    #[test]
    fn injection_absent_gate_detail() {
        let cs = cases();
        let c = match cs.get(ACCEPT + 2) {
            Some(v) => *v,
            None => fail(),
        };
        let truth = c.actual();
        let dodgy = c.shape.actual_skip_name();
        assert_eq!(truth.tier, Tier::BuiltC);
        assert_eq!(dodgy.tier, Tier::Full);
        assert_eq!(truth.code, NX_CHAR);
        assert_eq!(dodgy.code, 0);
        assert_ne!(judge_against(&c.claim, &dodgy), Fault::Ok);
        assert_eq!(judge_against(&c.claim, &truth), Fault::Name);
        assert!(c.shape.env_text().as_str().contains("LC_ALL=de_DE.UTF-8;"));
    }

    #[test]
    fn injection_false_green_detail() {
        let cs = cases();
        let c = match cs.get(ACCEPT + 3) {
            Some(v) => *v,
            None => fail(),
        };
        let a = c.actual();
        assert_eq!(a.pseudo, 1);
        assert_eq!(a.unlogged, 5);
        assert_eq!(a.acct, 0);
        let lie = Claim::lie_green(a);
        assert!(lie.ok && lie.tier == a.tier && lie.acct == a.acct);
        // 假绿先撞成败闸，轮不到记账闸。
        assert_eq!(judge_against(&lie, &a), Fault::Estate);
        // 成败对齐后，谎报记账数才由记账闸判死。
        let mut acct_lie = lie;
        acct_lie.ok = a.ok();
        acct_lie.acct = a.acct.saturating_add(1);
        assert_eq!(judge_against(&acct_lie, &a), Fault::Count);
        assert!(Fault::Count.accounting());
    }

    #[test]
    fn shape_builders_and_env_text() {
        let s = shape_all_empty("e1", Archive::complete("x"), 0, true);
        assert_eq!(s.req().env_text().as_str(), "LC_ALL=<null> LANG=<unset> LC_x=<unset>");
        assert_eq!(s.actual().note, Note::BuiltIn);
        let le = shape_lang_empty("e2", Archive::complete("x"), 0, true);
        assert_eq!(le.actual().note, Note::BuiltIn);
        assert_eq!(le.req().env_text().as_str(), "LC_ALL=<unset> LANG=<null> LC_x=<unset>");
        let sp = shape_ctype_split(
            "e3",
            "C",
            "fr_FR.UTF-8",
            Archive::missing(5, "fr_FR.UTF-8"),
            MASK_LANG,
            true,
        );
        let o = sp.actual();
        assert_eq!(o.tier, Tier::Partial);
        assert_eq!(o.fall, MASK_LANG);
        assert_eq!(o.c_used, cat_bit(1) | MASK_LANG);
        assert!(o.ok());
        assert_eq!(sp.env_text().as_str(), "LC_ALL=<unset> LANG=fr_FR.UTF-8 LC_CTYPE=C");
        assert_ne!(sp.seal(), le.seal());
        let again = shape_ctype_split(
            "e3",
            "C",
            "fr_FR.UTF-8",
            Archive::missing(5, "fr_FR.UTF-8"),
            MASK_LANG,
            true,
        );
        assert_eq!(sp.seal(), again.seal());
    }

    #[test]
    fn messages_are_verbatim() {
        assert_eq!(
            msg_unsupported("xx_YY", Category::Ctype).as_str(),
            "bash: warning: setlocale: LC_CTYPE: \"xx_YY\" (unsupported locale setting)"
        );
        assert_eq!(
            msg_default_locale_fail("xx_YY").as_str(),
            "locale: Cannot set LC_ALL to default locale: xx_YY"
        );
        assert_eq!(
            msg_built_in_dead().as_str(),
            "fatal: built-in C charmap failed self-check, refusing to classify bytes"
        );
        assert_eq!(
            msg_unlogged(Category::Ctype, "wc").as_str(),
            "warning: locale downgrade not accounted for CTYPE in wc"
        );
        assert_eq!(
            msg_tool_fatal("printf", Category::Numeric).as_str(),
            "printf: cannot proceed with an untrusted LC_NUMERIC decimal point: Illegal byte sequence"
        );
        assert_eq!(
            msg_section_short("de_DE.UTF-8", Category::Ctype, 64, 1024).as_str(),
            "locale data for de_DE.UTF-8/LC_CTYPE truncated: got 64 bytes, need 1024"
        );
        assert_eq!(
            msg_header_range("/usr/lib/locale/locale-archive", 2048).as_str(),
            "archive /usr/lib/locale/locale-archive size 2048 is below the minimum locale header size"
        );
        assert_eq!(ARC_MIN_TOTAL, 4096);
        assert_eq!(ARC_VER, 1);
    }

    #[test]
    fn corpus_messages_match_slots() {
        let cs = cases();
        let c = match cs.get(5) {
            Some(v) => *v,
            None => fail(),
        };
        let o = c.actual();
        assert_eq!(
            message_for(&c.shape, &o).as_str(),
            "locale file for locale zh_CN.UTF-8 of archive /usr/lib/locale/locale-archive: bad magic!: [log0]"
        );
        let d = match cs.get(7) {
            Some(v) => *v,
            None => fail(),
        };
        let od = d.actual();
        assert_eq!(message_for(&d.shape, &od).as_str(), msg_built_in_dead().as_str());
        let g = match cs.get(0) {
            Some(v) => *v,
            None => fail(),
        };
        let og = g.actual();
        assert!(message_for(&g.shape, &og).is_empty());
        let e = match cs.get(3) {
            Some(v) => *v,
            None => fail(),
        };
        assert_eq!(
            message_for(&e.shape, &e.actual()).as_str(),
            "locale data for de_DE.UTF-8/LC_CTYPE truncated: got 64 bytes, need 1024"
        );
    }

    #[test]
    fn primitives_all_green() {
        assert!(name_ok());
        assert!(const_ok());
        assert!(parse_ok());
        assert!(legal_ok());
        assert!(archive_ok());
        assert!(chain_ok());
        assert!(tool_ok());
        assert!(judge_ok());
        assert!(codes_distinct());
        assert!(corpus_ok());
        assert!(tags_unique());
        assert!(notes_ok());
        assert!(table_ok());
        assert!(reds_ok());
        assert!(detail_ok());
        assert!(book_ok());
        assert!(first_structural_stage_texts());
        assert!(shape_seals_distinct());
        let r = match run() { Ok(v) => v, Err(_) => fail() };
        assert!(vals_match(&r) && render_row_ok(&Matrix::of()));
    }

    #[test]
    fn check_set_surface() {
        let s = run_unx11658_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
        assert_eq!(s.failed_count(), 0);
        assert_eq!(s.passed_count(), 13);
        assert!(s.red_items_text().is_empty());
        assert!(!s.is_empty());
        let mut i = 0usize;
        while i < s.len() {
            let c = match s.get(i) {
                Some(v) => v,
                None => fail(),
            };
            assert!(c.passed);
            assert!(c.name.starts_with("LM"));
            assert!(c.name.len() > 5);
            i += 1;
        }
        assert_eq!(match s.get(0) { Some(c) => c.name, None => "" }, "LM01-名称与段基址");
        assert_eq!(match s.get(12) { Some(c) => c.name, None => "" }, "LM13-冻结重放四注入");
        assert_eq!(match s.get(6) { Some(c) => c.name, None => "" }, "LM07-工具矩阵统计");
        assert!(s.get(13).is_none());
        assert_eq!(s.tally(), (13, 0));
    }

    #[test]
    fn check_names_are_thirteen_unique() {
        let s = run_unx11658_checks();
        let want = [
            "LM01-名称与段基址",
            "LM02-常量与档位",
            "LM03-请求解析",
            "LM04-名字合法性",
            "LM05-归档结构闸",
            "LM06-降级链",
            "LM07-工具矩阵统计",
            "LM08-申报对判",
            "LM09-语料与渲染",
            "LM10-红档显式",
            "LM11-文案逐字",
            "LM12-释义与账本",
            "LM13-冻结重放四注入",
        ];
        let mut i = 0usize;
        while i < want.len() {
            let got = match s.get(i) {
                Some(c) => c.name,
                None => fail(),
            };
            assert_eq!(got, match want.get(i) { Some(v) => *v, None => "" });
            let mut j = i + 1;
            while j < want.len() {
                assert_ne!(got, match want.get(j) { Some(v) => *v, None => "" });
                j += 1;
            }
            i += 1;
        }
    }
}
