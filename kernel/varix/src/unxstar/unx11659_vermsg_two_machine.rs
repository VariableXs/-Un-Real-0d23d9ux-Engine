//! UNX-F11659 符号版本不匹配报错文案双机逐字对照
//!
//! 符号版本不匹配是动态链接期最常见的失败面：装载器（ld.so）在 `.gnu.version_r`
//! 里查不到被依赖对象声明的版本，或链接器（ld）发现同一库的两个 soname 相互冲
//! 突。这类失败的**唯一可观测物**是一行终端文案，而工程上真正的问题是：同一条
//! 失败在**基准机**（构建主机）与**目标机**（本内核承载机）上报出的字节序列是不
//! 是逐字一致。一致 → 双机可以联签同一条主锚；不一致 → 必须显式落成 CONFLICT 档
//! 并给出首个差异字节偏移与归因，绝不允许"看一眼差不多就当一致"。
//!
//! 本卡因此把"文案"当被测对象本体，而不是副产品。全部判据围绕两件事：
//!
//! 1. **逐字**：对照走的是字节序列，不做任何"友好化"归一。反引号 `` ` `` 与撇号
//!    `'` 的差别是一个字节，也是 glibc 原文与随手转写之间的差别；末尾换行是
//!    一个字节，也是管道下游行为差别；`(required by X)` 里 X 是程序名还是依赖
//!    库名，是内容差别不是格式差别。
//! 2. **显式**：差异必须被登记成 CONFLICT 行，带偏移与归因。差异不可见比差异
//!    本身更危险——它会让双机验收在"文案差不多"的错觉下盖章放行。
//!
//! # 报文种类（六种真实文案族）
//!
//! 每一族都有自己的语法与自己的引号规则，混用即错：
//!
//! - `Vfn`：装载器版本缺失。原文形如
//!   `foo: /lib/x86_64-linux-gnu/libc.so.6: version \`GLIBC_2.34' not found (required by foo)`
//!   版本名被 **反引号起、撇号止** 包裹（glibc 用的是 TeX 式引号），且
//!   `(required by …)` 里是被依赖方，不是报错程序。
//! - `Undef`：延迟绑定符号查不到。原文形如
//!   `foo: symbol lookup error: foo: undefined symbol: bar, version GLIBC_2.34`
//!   这一族的版本名**不加任何引号**——这是与 `Vfn` 最容易混写的一处，也是本卡
//!   专门立一维的原因。
//! - `Open`：共享对象打不开。原文形如
//!   `foo: error while loading shared libraries: libfoo.so.1: cannot open shared object file: No such file or directory`
//!   句尾是 `strerror` 文本，随 errno 变（2/13/28），属于载荷内容。
//!   `error while loading shared libraries` 为全小写，且只在装载期出现。
//! - `Conflict`：链接期 soname 冲突警告。原文形如
//!   `ld: warning: libstdc++.so.6, needed by /usr/lib/gcc/x86_64-linux-gnu/11/libstdc++.so, may conflict with libstdc++.so.6`
//!   注意 `warning:` 前缀与逗号分隔，`needed by` 后是路径，`may conflict with`
//!   后是另一个 soname。
//! - `Dlopen`：`dlopen`/`dlerror` 面。原文形如
//!   `libfoo.so.1: cannot open shared object file: No such file or directory`
//!   与 `Open` 同族文本但**没有** `error while loading shared libraries`，也
//!   **没有**程序名前缀——差一段前缀就是另一个 API 面。
//! - `Preload`：`LD_PRELOAD` 面。原文形如
//!   `ERROR: ld.so: object 'libfoo.so' from LD_PRELOAD cannot be preloaded (cannot open shared object file): ignored.`
//!   这一族的 `ERROR:` 全大写、对象名用**成对撇号**、结尾有 `: ignored.`，与
//!   `Vfn` 的 TeX 式引号形成第三套引号规则。
//!
//! # 闸门次序（这条纵线为什么不能倒）
//!
//! 采集 → 种类 → 标签切分 → 可比性 → 载荷形状 → 逐字对照 → 归因 → 就绪。
//!
//! 1. `collect`：两台机器各产出一份报文采集件（标签、种类、各段文本、引号档、
//!    区域设置、末尾换行）。没有采集件就没有对照，这一步只保证"两侧都在"。
//! 2. `classify`：种类必须落在已注册的五族内。种类未知时**后面全部不可推**——
//!    连"哪一段算标签、哪一段算载荷"都无从判定，所以必须先死在这里。
//! 3. `split`：切出标签段与载荷段。装载器报文形如 `<标签>: <载荷>`，标签是调用
//!    形态（`./foo`、`/usr/bin/foo`），载荷才是可比内容。切分必须在对照之前，否
//!    则双机命令行路径不同会被误判成文案冲突——那是一条假红。
//! 4. `comparability`：形状自身是否可比。引号档写成混合（同一条报文里既用 `` ` ``
//!    又用 `"`）、区域设置非 C（文案被 gettext 翻译，词序都变了）时，对照出来的
//!    差异没有归因价值，必须在对照**之前**判死，并给出形状码。这一步与第 6 步的
//!    边界是本卡的关键差异：形状不可比 = 本侧问题，双机漂移 = 对侧问题。
//! 5. `payload`：载荷形状闸。需要依赖对象段的种类（`Vfn`/`Open`）不许段名为空，
//!    `Undef` 不许符号名为空，`Conflict` 需要 `warning:` 前缀在场。空载荷会让对照
//!    得到"前缀全等"的假绿，因此同样先于对照。
//! 6. `compare`：字节走查，产出**一致前缀长度**（全等时等于长度）。走查只读两串
//!    字节，绝不读漂移声明——这是交叉校验的根：漂移是"事实侧"的建模，归因是"证据
//!    侧"的推断，两者独立算出再对撞。
//! 7. `attribute`：按差异落在哪个段、差异字节的形态、长度谁长谁短，归因到
//!    引号/大小写/换行/errno 文本/符号名/对象路径/区域 七档之一，或全等、或仅标签差。
//! 8. `ready`：登记主锚联签资格。只有 `Vfn`/`Undef` 两族是符号版本主锚面，别的
//!    族就算逐字一致也不给联签位——这是"default"维独立于"code"维的理由。
//!
//! # 关键差异（本卡深度所在）
//!
//! - **形状档与漂移档分两维**。同是"引号不对"，本侧写成混合引号是形状不可比
//!   （`VM_EQUOTE`，判死），对侧把 TeX 式引号写成成对撇号是双机漂移（`VM_EVALUE`，
//!   判 CONFLICT，但这条对照本身是**成功**的）。两者若共用一码，本卡的验收就退
//!   化成"文案不一致一律红"，双机对照失去意义。
//! - **标签差不算冲突**。基准机用 `./foo` 拉起、目标机用 `/usr/bin/foo` 拉起，是
//!   同一个失败的两个视图；归因走 `LabelOnly`，判绿并保留联签位。这条判据把
//!   "路径不同"与"文案不同"分开，是双机对照能落地的前提。
//! - **一致前缀长度即结论**。全等时它等于载荷长度，不等时它等于首差异偏移。同
//!   一个 `u32` 字段既承担"是否一致"又承担"差在哪"，因此申报值必须由实测推出，
//!   不许手填——手填的偏移会在段表变化后悄悄失真。
//! - **主锚联签位独立**。`Vfn`/`Undef` 才配占符号版本主锚；`Open`/`Conflict`/
//!   `Dlopen`/`Preload` 即便全等也只算旁证。少这一维，主锚会被四类旁证挤满，
//!   联签失去筛选作用。
//! - **红档率单独占一格**。14 槽里注入一槽错判，一致数 13/14 → 929（千分），
//!   这一格不许与任何统计数同值，否则"全绿"与"差一绿"不可分辨。
//!
//! 判序：状态 → 阶段 → 判定码 → 一致前缀 → 归因 → 联签位。
//!

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址：全仓唯一，本卡取 0x6cf40。
pub const SEG_BASE: u32 = 0x6cf40;

/// 申报态与实际态互相保密——双机对照里最恶性的一类，优先判死。
pub const VM_ESTATE: u32 = SEG_BASE + 0x01;
/// 阶段申报与纵线实际停驻点不符：闸门还没走完就报就绪。
pub const VM_ESTAGE: u32 = SEG_BASE + 0x02;
/// 判定码不符：形状码/漂移码张冠李戴。
pub const VM_ECODE: u32 = SEG_BASE + 0x03;
/// 一致前缀长度不符：把差异偏移填成了全长，或反之。
pub const VM_EVALUE: u32 = SEG_BASE + 0x04;
/// 归因不符：差异落在哪一段判错。
pub const VM_ENOTE: u32 = SEG_BASE + 0x05;
/// 主锚联签位不符：该联签的没联签，不该联签的占了位。
pub const VM_EDEFAULT: u32 = SEG_BASE + 0x06;
/// 种类未注册：报文族不在五族表内，后面全部不可推。
pub const VM_EKTAG: u32 = SEG_BASE + 0x07;
/// 本侧引号档混合：同一条报文里 TeX 式引号与成对撇号混写，形状不可比。
pub const VM_EQUOTE: u32 = SEG_BASE + 0x08;
/// 区域设置非 C：文案被翻译，逐字对照失去基准。
pub const VM_ELANG: u32 = SEG_BASE + 0x09;
/// 标签切分失败：报文里没有 `<标签>: ` 分隔，载荷无从分离。
pub const VM_ELBL: u32 = SEG_BASE + 0x0a;
/// 载荷形状残缺：依赖对象段或符号段为空。
pub const VM_EBODY: u32 = SEG_BASE + 0x0b;
/// 对照结论矛盾：走查所得与申报的归因方向互斥（封印复核用）。
pub const VM_ESEAL: u32 = SEG_BASE + 0x0c;
/// 槽位越界：语料下标超出 14 槽。
pub const ESLOT: u32 = SEG_BASE + 0x0d;

/// 语料总槽数。
pub const CORPUS: usize = 14;
/// 账本样本数。
pub const SAMPLES: usize = 14;
/// 活槽数（申报与实测一致）。
pub const ACCEPT: usize = 8;
/// 死槽数（每槽只坏一维）。
pub const REJECT: usize = 6;
/// 档位四分类。
pub const TIER_COUNT: usize = 4;

/// 主锚族数：`Vfn`/`Undef` 两族具备符号版本主锚联签资格。
pub const ANCHOR_KINDS: usize = 2;
/// 报文族总数。
pub const KIND_COUNT: usize = 6;

/// FNV-1a 64 位初始偏置。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64 位素数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// ENOENT：文件不存在，`Open`/`Dlopen` 族的默认 errno 档。
pub const ENOENT: u8 = 2;
/// EACCES：权限不足，`Open` 族的第二个真实档。
pub const EACCES: u8 = 13;
/// ENOSPC：设备满，装载期写入临时映射失败时的档，别与本族混用。
pub const ENOSPC: u8 = 28;
/// 无 errno 后缀的族使用 0 档。
pub const ENONE: u8 = 0;

/// 逐字节折进指纹。
pub fn fold_byte(h: u64, b: u8) -> u64 {
    let x = h ^ (b as u64);
    x.wrapping_mul(FNV_PRIME)
}

/// 逐字折进指纹（只折低 16 位，用于档位与枚举判别值）。
pub fn fold_word(h: u64, w: u16) -> u64 {
    let hi = ((w >> 8) & 0xff) as u8;
    let lo = (w & 0xff) as u8;
    fold_byte(fold_byte(h, hi), lo)
}

/// 字符串折进指纹，含末尾显式终止字节，避免 "ab"+"c" 与 "a"+"bc" 同指纹。
pub fn fold_str(h: u64, s: &str) -> u64 {
    let mut acc = h;
    for b in s.as_bytes() {
        acc = fold_byte(acc, *b);
    }
    fold_byte(acc, 0)
}

/// 十进制文本（迭代实现，不递归，不借 format!）。
pub fn dec_text(v: u32) -> String {
    let mut buf = [0u8; 12];
    let mut i = buf.len();
    let mut n = v;
    if n == 0 {
        let mut z = String::new();
        z.push('0');
        return z;
    }
    while n > 0 {
        let d = (n % 10) as u8;
        n /= 10;
        i -= 1;
        if let Some(slot) = buf.get_mut(i) {
            *slot = b'0' + d;
        }
    }
    let mut out = String::new();
    while i < buf.len() {
        if let Some(b) = buf.get(i) {
            out.push(*b as char);
        }
        i += 1;
    }
    out
}

/// 十六进制文本，固定宽度、小写，高位不足补零。
pub fn hex_text(v: u32, width: usize) -> String {
    let digits: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::new();
    let mut i = 0usize;
    while i < width {
        let shift = ((width - 1 - i) * 4) as u32;
        let d = ((v >> shift) & 0xf) as usize;
        if let Some(b) = digits.get(d) {
            out.push(*b as char);
        }
        i += 1;
    }
    out
}

/// u32 的规范文本：十进制。
pub fn u32_text(v: u32) -> String {
    dec_text(v)
}

/// 带符号整数文本，单独走正负号分支。
pub fn i32_text(v: i32) -> String {
    if v < 0 {
        let mut s = String::new();
        s.push('-');
        s.push_str(dec_text((v.unsigned_abs()) as u32).as_str());
        s
    } else {
        dec_text(v as u32)
    }
}

/// 布尔文本用工程惯例三字母/两字母，便于逐字拼进诊断行。
pub fn bool_text(b: bool) -> String {
    let mut s = String::new();
    if b {
        s.push_str("true");
    } else {
        s.push_str("false");
    }
    s
}

/// 采集侧：基准机（构建主机）还是目标机（本内核承载机）。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Machine {
    /// 基准机：文案原始出处，验收的参照侧。
    Host,
    /// 目标机：被对照侧，漂移只可能出现在这一侧。
    Target,
}

impl Machine {
    /// 稳定序号，用于指纹与档位拼合。
    pub const fn rank(self) -> u8 {
        match self {
            Machine::Host => 0,
            Machine::Target => 1,
        }
    }

    /// 对侧机器。
    pub const fn peer(self) -> Machine {
        match self {
            Machine::Host => Machine::Target,
            Machine::Target => Machine::Host,
        }
    }

    /// 短名，逐字进诊断行。
    pub const fn text(self) -> &'static str {
        match self {
            Machine::Host => "host",
            Machine::Target => "target",
        }
    }
}

/// 报文族。取值次序与文档头列举一致。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Kind {
    /// 装载器版本缺失（TeX 式引号）。
    Vfn,
    /// 延迟绑定符号查不到（版本名无引号）。
    Undef,
    /// 装载期共享对象打不开（含 strerror 尾）。
    Open,
    /// 链接期 soname 冲突警告。
    Conflict,
    /// `dlerror` 面（无程序名前缀、无 loading 短语）。
    Dlopen,
    /// `LD_PRELOAD` 面（大写 ERROR、成对撇号、ignored 尾）。
    Preload,
}

impl Kind {
    /// 稳定序号。
    pub const fn rank(self) -> u8 {
        match self {
            Kind::Vfn => 1,
            Kind::Undef => 2,
            Kind::Open => 3,
            Kind::Conflict => 4,
            Kind::Dlopen => 5,
            Kind::Preload => 6,
        }
    }

    /// 族短名。
    pub const fn slug(self) -> &'static str {
        match self {
            Kind::Vfn => "vfn",
            Kind::Undef => "undef",
            Kind::Open => "open",
            Kind::Conflict => "conflict",
            Kind::Dlopen => "dlopen",
            Kind::Preload => "preload",
        }
    }

    /// 载荷里必须出现依赖对象段的两族。
    pub const fn needs_obj(self) -> bool {
        matches!(self, Kind::Vfn | Kind::Open)
    }

    /// 载荷里必须出现符号名的两族。
    pub const fn needs_sym(self) -> bool {
        matches!(self, Kind::Undef | Kind::Preload)
    }

    /// 是否具备符号版本主锚联签资格——只有这两族。
    pub const fn anchor(self) -> bool {
        matches!(self, Kind::Vfn | Kind::Undef)
    }

    /// 从序号反推族，0 与 7.. 视为未注册。
    pub const fn from_rank(r: u8) -> Option<Kind> {
        match r {
            1 => Some(Kind::Vfn),
            2 => Some(Kind::Undef),
            3 => Some(Kind::Open),
            4 => Some(Kind::Conflict),
            5 => Some(Kind::Dlopen),
            6 => Some(Kind::Preload),
            _ => None,
        }
    }

    /// 族自己的引号规则文本，供释义。
    pub const fn quote_rule(self) -> &'static str {
        match self {
            Kind::Vfn => "backtick-open apos-close",
            Kind::Undef => "bare version",
            Kind::Open => "no quoting",
            Kind::Conflict => "no quoting",
            Kind::Dlopen => "no quoting",
            Kind::Preload => "paired apos",
        }
    }
}

/// 本侧引号档——形状维度，与对侧漂移无关。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Quote {
    /// TeX 式：反引号起、撇号止，glibc `Vfn` 原文用法。
    Tick,
    /// 成对撇号，`Preload` 原文用法；写在 `Vfn` 上就是转写。
    Apos,
    /// 混合：同一条报文里两种都有，形状不可比。
    Mixed,
}

impl Quote {
    /// 稳定序号。
    pub const fn rank(self) -> u8 {
        match self {
            Quote::Tick => 1,
            Quote::Apos => 2,
            Quote::Mixed => 3,
        }
    }

    /// 该档在 `Vfn` 族上是否合法（只有纯 TeX 式合法）。
    pub const fn legal_for_vfn(self) -> bool {
        matches!(self, Quote::Tick)
    }
}

/// 双机漂移档：只作用在目标机渲染上，是"事实侧"建模。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Drift {
    /// 无漂移。
    None,
    /// TeX 式引号被写成成对撇号。
    Quote,
    /// 版本名字母大小写变化。
    TagCase,
    /// 末尾换行丢失。
    Tail,
    /// strerror 尾文本随 errno 档变化。
    ErrnoText,
    /// 区域非 C，文案被翻译。
    Locale,
    /// 符号名字母大小写变化。
    SymName,
    /// 依赖对象路径前缀不同。
    ObjPath,
}

impl Drift {
    /// 稳定序号。
    pub const fn rank(self) -> u8 {
        match self {
            Drift::None => 0,
            Drift::Quote => 1,
            Drift::TagCase => 2,
            Drift::Tail => 3,
            Drift::ErrnoText => 4,
            Drift::Locale => 5,
            Drift::SymName => 6,
            Drift::ObjPath => 7,
        }
    }

    /// 该漂移是否会把报文推出可对照范围。
    pub const fn breaks_compare(self) -> bool {
        matches!(self, Drift::Locale)
    }

    /// 短名。
    pub const fn slug(self) -> &'static str {
        match self {
            Drift::None => "none",
            Drift::Quote => "quote",
            Drift::TagCase => "tag-case",
            Drift::Tail => "no-tail",
            Drift::ErrnoText => "errno-text",
            Drift::Locale => "locale",
            Drift::SymName => "sym-case",
            Drift::ObjPath => "obj-path",
        }
    }
}

/// 纵线阶段。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Stage {
    /// 采集。
    Collect,
    /// 种类注册。
    Classify,
    /// 标签切分。
    Split,
    /// 可比性。
    Comparability,
    /// 载荷形状。
    Payload,
    /// 逐字对照。
    Compare,
    /// 归因。
    Attribute,
    /// 就绪。
    Ready,
}

impl Stage {
    /// 单调序号，用于闸门前进量校验。
    pub const fn rank(self) -> u8 {
        match self {
            Stage::Collect => 0,
            Stage::Classify => 1,
            Stage::Split => 2,
            Stage::Comparability => 3,
            Stage::Payload => 4,
            Stage::Compare => 5,
            Stage::Attribute => 6,
            Stage::Ready => 7,
        }
    }

    /// 族短名。
    pub const fn slug(self) -> &'static str {
        match self {
            Stage::Collect => "collect",
            Stage::Classify => "classify",
            Stage::Split => "split",
            Stage::Comparability => "comparability",
            Stage::Payload => "payload",
            Stage::Compare => "compare",
            Stage::Attribute => "attribute",
            Stage::Ready => "ready",
        }
    }

    /// 下一阶段；就绪态没有后继。
    pub const fn next(self) -> Option<Stage> {
        match self {
            Stage::Collect => Some(Stage::Classify),
            Stage::Classify => Some(Stage::Split),
            Stage::Split => Some(Stage::Comparability),
            Stage::Comparability => Some(Stage::Payload),
            Stage::Payload => Some(Stage::Compare),
            Stage::Compare => Some(Stage::Attribute),
            Stage::Attribute => Some(Stage::Ready),
            Stage::Ready => None,
        }
    }

    /// 对照是否已经发生（只有走完 Compare 才有实测偏移）。
    pub const fn past_compare(self) -> bool {
        matches!(
            self,
            Stage::Compare | Stage::Attribute | Stage::Ready
        )
    }
}

/// 差异归因档——由走查证据推出，不许手填。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Note {
    /// 载荷逐字全等。
    Same,
    /// 仅标签段不同，载荷全等。
    LabelOnly,
    /// 差异落在引号字节。
    Quote,
    /// 差异落在版本名字母大小写。
    TagCase,
    /// 差异是长度（一侧多出末尾换行）。
    Tail,
    /// 差异落在 strerror 尾文本。
    ErrnoText,
    /// 差异落在符号名段。
    SymName,
    /// 差异落在依赖对象路径段。
    ObjPath,
    /// 差异落在翻译文案（非 ASCII）。
    Locale,
    /// 载荷形状残缺，没有可比内容。
    Body,
}

impl Note {
    /// 稳定序号。
    pub const fn rank(self) -> u8 {
        match self {
            Note::Same => 0,
            Note::LabelOnly => 1,
            Note::Quote => 2,
            Note::TagCase => 3,
            Note::Tail => 4,
            Note::ErrnoText => 5,
            Note::SymName => 6,
            Note::ObjPath => 7,
            Note::Locale => 8,
            Note::Body => 9,
        }
    }

    /// 短名。
    pub const fn slug(self) -> &'static str {
        match self {
            Note::Same => "same",
            Note::LabelOnly => "label-only",
            Note::Quote => "quote",
            Note::TagCase => "tag-case",
            Note::Tail => "tail",
            Note::ErrnoText => "errno-text",
            Note::SymName => "sym-name",
            Note::ObjPath => "obj-path",
            Note::Locale => "locale",
            Note::Body => "body",
        }
    }

    /// 该归因是否意味着 CONFLICT（载荷确有逐字差异）。
    pub const fn conflicts(self) -> bool {
        matches!(
            self,
            Note::Quote
                | Note::TagCase
                | Note::Tail
                | Note::ErrnoText
                | Note::SymName
                | Note::ObjPath
                | Note::Locale
        )
    }
}

/// 档位四分类。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Tier {
    /// 双机载荷逐字一致，且标签也一致。
    Identical,
    /// 载荷逐字一致，仅标签（调用路径）不同。
    LabelOnly,
    /// 载荷逐字不一致，已显式登记为 CONFLICT。
    Conflict,
    /// 形状不可比/残缺，对照没发生。
    Malformed,
}

impl Tier {
    /// 稳定序号。
    pub const fn rank(self) -> u8 {
        match self {
            Tier::Identical => 0,
            Tier::LabelOnly => 1,
            Tier::Conflict => 2,
            Tier::Malformed => 3,
        }
    }

    /// 短名。
    pub const fn slug(self) -> &'static str {
        match self {
            Tier::Identical => "identical",
            Tier::LabelOnly => "label-only",
            Tier::Conflict => "conflict",
            Tier::Malformed => "malformed",
        }
    }

    /// 由实测结论分档。分档只看结论，不看申报。
    pub const fn of(o: &Outcome) -> Tier {
        if !o.ok && !o.stage.past_compare() {
            return Tier::Malformed;
        }
        match o.note {
            Note::Same => Tier::Identical,
            Note::LabelOnly => Tier::LabelOnly,
            _ => Tier::Conflict,
        }
    }

    /// 该档是否算"绿"（可盖章）。
    pub const fn good(self) -> bool {
        !matches!(self, Tier::Malformed)
    }
}

/// 判定缺陷维。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Fault {
    /// 无缺陷。
    Ok,
    /// 申报态与实际态互斥。
    Estate,
    /// 阶段申报不符。
    Stage,
    /// 判定码不符。
    Code,
    /// 一致前缀不符。
    Value,
    /// 归因不符。
    Note,
    /// 联签位不符。
    Default,
}

impl Fault {
    /// 稳定序号，与判序一致。
    pub const fn rank(self) -> u8 {
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
    pub const fn slug(self) -> &'static str {
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

    /// 该维对应的段内错误码。
    pub const fn code(self) -> u32 {
        match self {
            Fault::Ok => 0,
            Fault::Estate => VM_ESTATE,
            Fault::Stage => VM_ESTAGE,
            Fault::Code => VM_ECODE,
            Fault::Value => VM_EVALUE,
            Fault::Note => VM_ENOTE,
            Fault::Default => VM_EDEFAULT,
        }
    }

    /// 判序表：六维，顺序不可调。
    pub const fn order() -> [Fault; 6] {
        [
            Fault::Estate,
            Fault::Stage,
            Fault::Code,
            Fault::Value,
            Fault::Note,
            Fault::Default,
        ]
    }
}

/// 本卡输入的全部维度：一条双机版本不匹配报文的两份采集件。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Shape {
    /// 槽位标签，逐字进诊断行。
    pub tag: &'static str,
    /// 报文族。
    pub kind: Kind,
    /// 族是否注册（false 用于构造未注册面）。
    pub registered: bool,
    /// 基准机标签（调用形态）。
    pub label_a: &'static str,
    /// 目标机标签。
    pub label_b: &'static str,
    /// 依赖对象名（`Vfn`/`Open`/`Dlopen` 段）。
    pub obj: &'static str,
    /// 依赖对象在目标机上的路径前缀形式（`Drift::ObjPath` 用）。
    pub obj_b: &'static str,
    /// 版本标记。
    pub vt: &'static str,
    /// 符号名。
    pub sym: &'static str,
    /// `(required by …)` 或被依赖方。
    pub req: &'static str,
    /// strerror 尾文本。
    pub err_txt: &'static str,
    /// strerror 尾文本在目标机上的变体。
    pub err_txt_b: &'static str,
    /// errno 档。
    pub err_no: u8,
    /// 本侧引号档。
    pub quote: Quote,
    /// 区域是否为 C（false 表示文案被翻译）。
    pub lang_c: bool,
    /// 报文里是否存在 `<标签>: ` 分隔。
    pub colon: bool,
    /// 基准机侧是否输出末尾换行。
    pub trail: bool,
    /// 目标机漂移档。
    pub drift: Drift,
}

/// 实测结论。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Outcome {
    /// 对照本身是否成功（形状不可比/残缺为 false）。
    pub ok: bool,
    /// 纵线停驻阶段。
    pub stage: Stage,
    /// 判定码：0 = 全等；非 0 = 形状码或漂移码。
    pub code: u32,
    /// 一致前缀长度（全等时等于载荷长度）。
    pub value: u32,
    /// 归因。
    pub note: Note,
    /// 主锚联签位。
    pub default: bool,
    /// 载荷字节数（用于渲染与释义）。
    pub len: u32,
}

impl Outcome {
    /// 实际成败。
    pub const fn ok(&self) -> bool {
        self.ok
    }

    /// 是否全等（一致前缀等于全长）。
    pub const fn identical(&self) -> bool {
        self.ok && self.value == self.len
    }

    /// 指纹折入的全部字段。
    pub fn fold(&self, h: u64) -> u64 {
        let a = fold_byte(h, self.ok as u8);
        let b = fold_word(a, self.stage.rank() as u16);
        let c = fold_word(b, (self.code & 0xffff) as u16);
        let d = fold_word(c, (self.code >> 16) as u16);
        let e = fold_word(d, self.value as u16);
        let f = fold_word(e, (self.value >> 16) as u16);
        let g = fold_word(f, self.note.rank() as u16);
        let i = fold_byte(g, self.default as u8);
        fold_word(i, self.len as u16)
    }
}

/// 申报表条目：对结论的六维断言。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Claim {
    /// 申报成败。
    pub success: bool,
    /// 申报阶段。
    pub stage: Stage,
    /// 申报判定码。
    pub code: u32,
    /// 申报一致前缀。
    pub value: u32,
    /// 申报归因。
    pub note: Note,
    /// 申报联签位。
    pub default: bool,
}

impl Claim {
    /// 从实测结论直接抄一张申报表——活槽的构造入口。
    pub const fn from_outcome(o: &Outcome) -> Claim {
        Claim {
            success: o.ok,
            stage: o.stage,
            code: o.code,
            value: o.value,
            note: o.note,
            default: o.default,
        }
    }

    /// 把某一维改坏，用于构造死槽。
    pub const fn break_dim(self, f: Fault) -> Claim {
        match f {
            Fault::Ok => self,
            Fault::Estate => Claim {
                success: !self.success,
                stage: self.stage,
                code: self.code,
                value: self.value,
                note: self.note,
                default: self.default,
            },
            Fault::Stage => Claim {
                success: self.success,
                stage: Stage::Ready,
                code: self.code,
                value: self.value,
                note: self.note,
                default: self.default,
            },
            Fault::Code => Claim {
                success: self.success,
                stage: self.stage,
                code: VM_ESEAL,
                value: self.value,
                note: self.note,
                default: self.default,
            },
            Fault::Value => Claim {
                success: self.success,
                stage: self.stage,
                code: self.code,
                value: self.value.saturating_add(1),
                note: self.note,
                default: self.default,
            },
            Fault::Note => Claim {
                success: self.success,
                stage: self.stage,
                code: self.code,
                value: self.value,
                note: Note::Same,
                default: self.default,
            },
            Fault::Default => Claim {
                success: self.success,
                stage: self.stage,
                code: self.code,
                value: self.value,
                note: self.note,
                default: !self.default,
            },
        }
    }

    /// 指纹折入。
    pub fn fold(&self, h: u64) -> u64 {
        let a = fold_byte(h, self.success as u8);
        let b = fold_word(a, self.stage.rank() as u16);
        let c = fold_word(b, (self.code & 0xffff) as u16);
        let d = fold_word(c, (self.code >> 16) as u16);
        let e = fold_word(d, self.value as u16);
        let f = fold_word(e, (self.value >> 16) as u16);
        let g = fold_word(f, self.note.rank() as u16);
        fold_byte(g, self.default as u8)
    }
}

/// 语料槽：输入 + 申报。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Case {
    /// 输入。
    pub shape: Shape,
    /// 申报。
    pub claim: Claim,
}

// ---------------------------------------------------------------------------
// 文案构件：逐字复刻，含空格与标点
// ---------------------------------------------------------------------------

/// 装载期短语（`Open` 族）。
const PH_LOAING: &str = "error while loading shared libraries: ";
/// 版本段前缀（TeX 式反引号起）。
const PH_VER_OPEN: &str = ": version `";
/// 版本段前缀（成对撇号起，转写形态）。
const PH_VER_OPEN_APOS: &str = ": version '";
/// 版本段中缀（撇号止 + not found）。
const PH_VER_MID: &str = "' not found (required by ";
/// 右括号。
const PH_CLOSE: &str = ")";
/// 符号查找错误短语（`Undef` 族）。
const PH_LOOKUP: &str = "symbol lookup error: ";
/// 未定义符号短语。
const PH_UNDEF: &str = "undefined symbol: ";
/// 版本后缀短语（`Undef` 族，无引号）。
const PH_VSUFFIX: &str = ", version ";
/// 打不开短语（`Open`/`Dlopen`/`Preload` 共用）。
const PH_CANNOT: &str = "cannot open shared object file";
/// 冒号空格分隔符。
const PH_COLON: &str = ": ";
/// 逗号空格分隔符。
const PH_COMMA: &str = ", ";
/// 链接警告前缀。
const PH_WARN: &str = "ld: warning: ";
/// needed by 短语。
const PH_NEEDED: &str = ", needed by ";
/// may conflict with 短语。
const PH_MAYCONF: &str = ", may conflict with ";
/// dlerror 面没有 loading 短语，仅 soname 起。
const PH_DL_PREFIX: &str = "";
/// `Preload` 族固定头（大写 ERROR 是原文特征）。
const PH_PRE_HEAD: &str = "ERROR: ld.so: object '";
/// `Preload` 族中段。
const PH_PRE_MID: &str = "' from LD_PRELOAD cannot be preloaded (";
/// `Preload` 族尾段（含句号）。
const PH_PRE_TAIL: &str = "): ignored.";
/// 分隔符：标签与载荷之间。
const PH_SEP: char = ':';
/// 末尾换行。
const PH_NL: char = '\n';
/// 反引号字节值。
const BBT: u8 = 0x60;
/// 撇号字节值。
const BAP: u8 = 0x27;

/// 拼接一份采集件的完整终端文本（含末尾换行，按 `trail`/`drift` 决定）。
pub fn msg_render(sh: &Shape, m: Machine) -> String {
    let mut s = String::new();
    let target = matches!(m, Machine::Target);
    match sh.kind {
        Kind::Vfn => {
            if sh.colon {
                s.push_str(if target { sh.label_b } else { sh.label_a });
                s.push(PH_SEP);
                s.push(' ');
            }
            s.push_str(if target && matches!(sh.drift, Drift::ObjPath) {
                sh.obj_b
            } else {
                sh.obj
            });
            s.push_str(if sh.quote == Quote::Apos || (target && matches!(sh.drift, Drift::Quote)) {
                PH_VER_OPEN_APOS
            } else {
                PH_VER_OPEN
            });
            s.push_str(&vt_variant(sh, target));
            s.push_str(PH_VER_MID);
            s.push_str(sh.req);
            s.push_str(PH_CLOSE);
        }
        Kind::Undef => {
            if sh.colon {
                s.push_str(if target { sh.label_b } else { sh.label_a });
                s.push(PH_SEP);
                s.push(' ');
            }
            s.push_str(PH_LOOKUP);
            if sh.colon {
                s.push_str(if target { sh.label_b } else { sh.label_a });
                s.push(PH_SEP);
                s.push(' ');
            }
            s.push_str(PH_UNDEF);
            s.push_str(&sym_variant(sh, target));
            s.push_str(PH_VSUFFIX);
            s.push_str(&vt_variant(sh, target));
        }
        Kind::Open => {
            if sh.colon {
                s.push_str(if target { sh.label_b } else { sh.label_a });
                s.push(PH_SEP);
                s.push(' ');
            }
            s.push_str(PH_LOAING);
            s.push_str(if target && matches!(sh.drift, Drift::ObjPath) {
                sh.obj_b
            } else {
                sh.obj
            });
            s.push_str(PH_COLON);
            s.push_str(PH_CANNOT);
            s.push_str(PH_COLON);
            s.push_str(&err_variant(sh, target));
        }
        Kind::Conflict => {
            s.push_str(PH_WARN);
            s.push_str(sh.obj);
            s.push_str(PH_NEEDED);
            s.push_str(sh.req);
            s.push_str(PH_MAYCONF);
            s.push_str(sh.obj);
        }
        Kind::Dlopen => {
            s.push_str(PH_DL_PREFIX);
            s.push_str(sh.obj);
            s.push_str(PH_COLON);
            s.push_str(PH_CANNOT);
            s.push_str(PH_COLON);
            s.push_str(&err_variant(sh, target));
        }
        Kind::Preload => {
            s.push_str(PH_PRE_HEAD);
            s.push_str(if target && matches!(sh.drift, Drift::ObjPath) {
                sh.obj_b
            } else {
                sh.obj
            });
            s.push_str(PH_PRE_MID);
            s.push_str(PH_CANNOT);
            s.push_str("): ignored.");
        }
    }
    if sh.lang_c == false && !(target && matches!(sh.drift, Drift::Locale)) {
        // 非 C 区域但漂移档不是 Locale：本侧文案整体走翻译面，此处按形状档
        // 判定交给 comparability 闸处理，渲染仍走原文，便于对照时定位。
    }
    if target && matches!(sh.drift, Drift::Locale) {
        let mut t = String::new();
        t.push_str("ld.so: ");
        t.push_str(if sh.sym.len() > 0 { sh.sym } else { sh.obj });
        t.push_str(": 版本 `");
        t.push_str(sh.vt);
        t.push_str("' 未找到");
        return push_tail(t, sh, target);
    }
    if target && matches!(sh.drift, Drift::TagCase) {
        // TagCase 已在 vt_variant 内处理，这里只做长度保护。
        return push_tail(s, sh, target);
    }
    push_tail(s, sh, target)
}

/// 末尾换行档：`Drift::Tail` 表示目标机不输出换行。
fn push_tail(mut s: String, sh: &Shape, target: bool) -> String {
    let nl = sh.trail && !(target && matches!(sh.drift, Drift::Tail));
    if nl {
        s.push(PH_NL);
    }
    s
}

/// 版本标记在某一侧的文本（`Drift::TagCase` 只在目标机生效）。
fn vt_variant(sh: &Shape, target: bool) -> String {
    let mut s = String::new();
    if target && matches!(sh.drift, Drift::TagCase) {
        for b in sh.vt.as_bytes() {
            s.push(ascii_lower(*b) as char);
        }
    } else {
        s.push_str(sh.vt);
    }
    s
}

/// 符号名在某一侧的文本（`Drift::SymName` 只在目标机生效）。
fn sym_variant(sh: &Shape, target: bool) -> String {
    let mut s = String::new();
    if target && matches!(sh.drift, Drift::SymName) {
        for b in sh.sym.as_bytes() {
            s.push(ascii_upper(*b) as char);
        }
    } else {
        s.push_str(sh.sym);
    }
    s
}

/// strerror 尾文本在某一侧的文本（`Drift::ErrnoText` 只在目标机生效）。
fn err_variant(sh: &Shape, target: bool) -> String {
    let mut s = String::new();
    if target && matches!(sh.drift, Drift::ErrnoText) {
        s.push_str(sh.err_txt_b);
    } else {
        s.push_str(sh.err_txt);
    }
    s
}

/// ASCII 小写。
pub const fn ascii_lower(b: u8) -> u8 {
    if b >= b'A' && b <= b'Z' {
        b + 32
    } else {
        b
    }
}

/// ASCII 大写。
pub const fn ascii_upper(b: u8) -> u8 {
    if b >= b'a' && b <= b'z' {
        b - 32
    } else {
        b
    }
}

/// 只渲染正文（不含末尾换行），用于释义与对照展示。
pub fn msg_body(sh: &Shape, m: Machine) -> String {
    let full = msg_render(sh, m);
    let mut out = String::new();
    let bytes = full.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            if *b != b'\n' {
                out.push(*b as char);
            }
        }
        i += 1;
    }
    out
}

/// `Vfn` 族模板示例，逐字钉住 glibc 原文格式。
pub fn msg_vfn(label: &str, obj: &str, vt: &str) -> String {
    let mut s = String::new();
    s.push_str(label);
    s.push(PH_SEP);
    s.push(' ');
    s.push_str(obj);
    s.push_str(PH_VER_OPEN);
    s.push_str(vt);
    s.push_str(PH_VER_MID);
    s.push_str(label);
    s.push_str(PH_CLOSE);
    s.push(PH_NL);
    s
}

/// `Undef` 族模板示例，逐字钉住版本名不带引号这一条。
pub fn msg_undef(label: &str, sym: &str, vt: &str) -> String {
    let mut s = String::new();
    s.push_str(label);
    s.push(PH_SEP);
    s.push(' ');
    s.push_str(PH_LOOKUP);
    s.push_str(label);
    s.push(PH_SEP);
    s.push(' ');
    s.push_str(PH_UNDEF);
    s.push_str(sym);
    s.push_str(PH_VSUFFIX);
    s.push_str(vt);
    s.push(PH_NL);
    s
}

/// `Open` 族模板示例。
pub fn msg_open(label: &str, obj: &str, err_txt: &str) -> String {
    let mut s = String::new();
    s.push_str(label);
    s.push(PH_SEP);
    s.push(' ');
    s.push_str(PH_LOAING);
    s.push_str(obj);
    s.push_str(PH_COLON);
    s.push_str(PH_CANNOT);
    s.push_str(PH_COLON);
    s.push_str(err_txt);
    s.push(PH_NL);
    s
}

/// `Conflict` 族模板示例。
pub fn msg_conflict(obj: &str, req: &str) -> String {
    let mut s = String::new();
    s.push_str(PH_WARN);
    s.push_str(obj);
    s.push_str(PH_NEEDED);
    s.push_str(req);
    s.push_str(PH_MAYCONF);
    s.push_str(obj);
    s.push(PH_NL);
    s
}

/// `Dlopen` 族模板示例：没有 loading 短语，也没有程序名。
pub fn msg_dlopen(obj: &str, err_txt: &str) -> String {
    let mut s = String::new();
    s.push_str(obj);
    s.push_str(PH_COLON);
    s.push_str(PH_CANNOT);
    s.push_str(PH_COLON);
    s.push_str(err_txt);
    s.push(PH_NL);
    s
}

/// `Preload` 族模板示例：大写 ERROR、成对撇号、ignored 句号尾。
pub fn msg_preload(obj: &str) -> String {
    let mut s = String::new();
    s.push_str(PH_PRE_HEAD);
    s.push_str(obj);
    s.push_str(PH_PRE_MID);
    s.push_str(PH_CANNOT);
    s.push_str(PH_PRE_TAIL);
    s.push(PH_NL);
    s
}

/// 一份采集件的对照视图：文本 + 各段字节区间。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Slices {
    /// 标签段长度（含 `: `），0 表示无标签。
    pub label_len: usize,
    /// 依赖对象段起点与长度。
    pub obj_at: usize,
    pub obj_len: usize,
    /// 版本标记段起点与长度（不含引号）。
    pub vt_at: usize,
    pub vt_len: usize,
    /// 符号段起点与长度。
    pub sym_at: usize,
    pub sym_len: usize,
    /// strerror 段起点与长度。
    pub err_at: usize,
    pub err_len: usize,
    /// 版本段的起始引号字节（0 表示无引号）。
    pub quote_byte: u8,
    /// 载荷（去标签）总长。
    pub body_len: usize,
    /// 是否带末尾换行。
    pub trail: bool,
}

impl Slices {
    /// 全零切片，未渲染时使用。
    pub const fn zero() -> Slices {
        Slices {
            label_len: 0,
            obj_at: 0,
            obj_len: 0,
            vt_at: 0,
            vt_len: 0,
            sym_at: 0,
            sym_len: 0,
            err_at: 0,
            err_len: 0,
            quote_byte: 0,
            body_len: 0,
            trail: false,
        }
    }

    /// 版本段是否覆盖给定偏移。
    pub const fn has_vt(&self, off: usize) -> bool {
        off >= self.vt_at && off < self.vt_at.saturating_add(self.vt_len)
    }

    /// 符号段是否覆盖给定偏移。
    pub const fn has_sym(&self, off: usize) -> bool {
        off >= self.sym_at && off < self.sym_at.saturating_add(self.sym_len)
    }

    /// 依赖对象段是否覆盖给定偏移。
    pub const fn has_obj(&self, off: usize) -> bool {
        off >= self.obj_at && off < self.obj_at.saturating_add(self.obj_len)
    }

    /// strerror 段是否覆盖给定偏移。
    pub const fn has_err(&self, off: usize) -> bool {
        off >= self.err_at && off < self.err_at.saturating_add(self.err_len)
    }

    /// 引号字节本身所在的偏移（版本段前一位）。
    pub const fn quote_at(&self) -> usize {
        if self.quote_byte == 0 {
            0
        } else {
            self.vt_at.saturating_sub(1)
        }
    }
}

/// 渲染并切片：一次遍历把各段的字节区间记录下来。
pub fn render_slice(sh: &Shape, m: Machine) -> (String, Slices) {
    let text = msg_render(sh, m);
    let mut sl = Slices::zero();
    let bytes = text.as_bytes();
    sl.trail = bytes.len() > 0 && *bytes.get(bytes.len() - 1).unwrap_or(&0) == b'\n';
    if sh.colon {
        // 标签段：第一个 ": " 之前（含分隔符三字节：冒号 + 空格）。
        let mut i = 0usize;
        while i + 1 < bytes.len() {
            let a = *bytes.get(i).unwrap_or(&0);
            let b = *bytes.get(i + 1).unwrap_or(&0);
            if a == b':' && b == b' ' {
                break;
            }
            i += 1;
        }
        sl.label_len = if i + 1 < bytes.len() { i + 2 } else { 0 };
    }
    // Conflict 族以 "ld: warning: " 起头，标签段就是这段前缀，切片按文本查。
    match sh.kind {
        Kind::Vfn => {
            let (o_at, o_len) = find_span(bytes, sl.label_len, sh.obj.as_bytes());
            sl.obj_at = o_at;
            sl.obj_len = o_len;
            let (v_at, v_len) = find_span(bytes, o_at.saturating_add(o_len), sh.vt.as_bytes());
            sl.vt_at = v_at;
            sl.vt_len = v_len;
            // 引号档取版本名前一位字节：反引号是原文，撇号是转写，别的是无引号。
            let qb = match bytes.get(v_at.saturating_sub(1)) {
                Some(&BBT) => BBT,
                Some(&BAP) => BAP,
                _ => 0,
            };
            sl.quote_byte = qb;
        }
        Kind::Undef => {
            let (s_at, s_len) = find_span(bytes, sl.label_len, sh.sym.as_bytes());
            sl.sym_at = s_at;
            sl.sym_len = s_len;
            let (v_at, v_len) = find_span(bytes, s_at.saturating_add(s_len), sh.vt.as_bytes());
            sl.vt_at = v_at;
            sl.vt_len = v_len;
        }
        Kind::Open => {
            let (o_at, o_len) = find_span(bytes, sl.label_len, sh.obj.as_bytes());
            sl.obj_at = o_at;
            sl.obj_len = o_len;
            let (e_at, e_len) = find_span(bytes, o_at.saturating_add(o_len), sh.err_txt.as_bytes());
            sl.err_at = e_at;
            sl.err_len = e_len;
        }
        Kind::Dlopen => {
            let (o_at, o_len) = find_span(bytes, 0, sh.obj.as_bytes());
            sl.obj_at = o_at;
            sl.obj_len = o_len;
            let (e_at, e_len) = find_span(bytes, o_at.saturating_add(o_len), sh.err_txt.as_bytes());
            sl.err_at = e_at;
            sl.err_len = e_len;
        }
        Kind::Preload => {
            let (o_at, o_len) = find_span(bytes, 0, sh.obj.as_bytes());
            sl.obj_at = o_at;
            sl.obj_len = o_len;
        }
        Kind::Conflict => {
            let (o_at, o_len) = find_span(bytes, PH_WARN.len(), sh.obj.as_bytes());
            sl.obj_at = o_at;
            sl.obj_len = o_len;
        }
    }
    sl.body_len = if sl.label_len == 0 {
        bytes.len()
    } else {
        bytes.len().saturating_sub(sl.label_len)
    };
    (text, sl)
}

/// 在 `hay[from..]` 里找 `needle` 第一次出现的区间。
fn find_span(hay: &[u8], from: usize, needle: &[u8]) -> (usize, usize) {
    if needle.len() == 0 || from > hay.len() {
        return (from, 0);
    }
    let last = match hay.len().checked_sub(needle.len()) {
        Some(v) => v,
        None => return (from, 0),
    };
    let mut i = from;
    while i <= last {
        let mut j = 0usize;
        while j < needle.len() {
            let h = match hay.get(i + j) {
                Some(v) => *v,
                None => break,
            };
            let n = match needle.get(j) {
                Some(v) => *v,
                None => break,
            };
            if h != n {
                break;
            }
            j += 1;
        }
        if j == needle.len() {
            return (i, needle.len());
        }
        i += 1;
    }
    (from, 0)
}

/// 载荷（去掉标签段与其冒号空格）的字节序列。
pub fn payload_bytes(text: &str, label_len: usize) -> &[u8] {
    let b = text.as_bytes();
    if label_len >= b.len() {
        return &[];
    }
    &b[label_len..]
}

/// 该族是否必须带程序名标签段。
pub const fn needs_label(k: Kind) -> bool {
    matches!(k, Kind::Vfn | Kind::Undef | Kind::Open)
}

/// 闸三：标签切分。装载器三族必须有 `<标签>: ` 分隔；链接器与 dlerror 面本来就
/// 没有标签段，缺冒号不算违约。
fn gate_split(sh: &Shape) -> bool {
    if needs_label(sh.kind) {
        sh.colon && sh.label_a.len() > 0
    } else {
        true
    }
}

/// 闸四：可比性。本侧引号档混写、或本侧区域非 C，都不许进入对照——对照出来的
/// 差异无法归因到"对侧漂移"，只能算本侧形状问题。
fn gate_comparability(sh: &Shape) -> Result<(), u32> {
    if !sh.lang_c {
        return Err(VM_ELANG);
    }
    if sh.kind == Kind::Vfn && sh.quote != Quote::Tick {
        return Err(VM_EQUOTE);
    }
    if sh.quote == Quote::Mixed && sh.kind != Kind::Preload {
        return Err(VM_EQUOTE);
    }
    Ok(())
}

/// 闸五：载荷形状。需要对象段的族不许段为空；需要符号段的族不许符号为空；
/// 冲突族必须有被依赖方。
fn gate_payload(sh: &Shape) -> Result<(), u32> {
    if sh.kind.needs_obj() && sh.obj.len() == 0 {
        return Err(VM_EBODY);
    }
    if sh.kind.needs_sym() && sh.sym.len() == 0 {
        return Err(VM_EBODY);
    }
    if sh.kind == Kind::Conflict && sh.req.len() == 0 {
        return Err(VM_EBODY);
    }
    Ok(())
}

/// 走查结果：一致前缀长度与首差异字节对。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Walk {
    /// 两侧载荷的一致前缀字节数。
    pub prefix: usize,
    /// 较短一侧的长度。
    pub min_len: usize,
    /// 甲侧载荷长度。
    pub a_len: usize,
    /// 乙侧载荷长度。
    pub b_len: usize,
    /// 首差异处甲侧字节（越界时为 0）。
    pub a_byte: u8,
    /// 首差异处乙侧字节（越界时为 0）。
    pub b_byte: u8,
    /// 长侧在 `min_len` 处的首个多余字节（等长时为 0）。
    pub extra_first: u8,
    /// 长侧多出的字节数。
    pub extra_len: usize,
    /// 多出的部分来自哪一侧。
    pub extra_on_b: bool,
    /// 是否全等。
    pub same: bool,
}

impl Walk {
    /// 差异偏移是否落在一侧末尾之外（长度差型差异）。
    pub const fn length_diff(&self) -> bool {
        self.a_len != self.b_len
    }

    /// 首差异处是否存在甲侧字节。
    pub const fn has_a(&self) -> bool {
        self.prefix < self.a_len
    }

    /// 首差异处是否存在乙侧字节。
    pub const fn has_b(&self) -> bool {
        self.prefix < self.b_len
    }

    /// 走查是否可用作就绪判定的输入（长度差与内容差都算走完）。
    pub const fn ok_ready(&self) -> bool {
        self.a_len > 0
    }

    /// 多出的部分是否恰好是一个换行——`Tail` 档的唯一证据。
    pub const fn extra_is_nl(&self) -> bool {
        self.extra_len == 1 && self.extra_first == b'\n'
    }
}

/// 逐字节走查两段载荷，只读字节，不读任何声明。
pub fn walk(a: &[u8], b: &[u8]) -> Walk {
    let mut i = 0usize;
    let limit = if a.len() < b.len() { a.len() } else { b.len() };
    while i < limit {
        let x = match a.get(i) {
            Some(v) => *v,
            None => 0,
        };
        let y = match b.get(i) {
            Some(v) => *v,
            None => 0,
        };
        if x != y {
            return Walk {
                prefix: i,
                min_len: limit,
                a_len: a.len(),
                b_len: b.len(),
                a_byte: x,
                b_byte: y,
                extra_first: 0,
                extra_len: 0,
                extra_on_b: false,
                same: false,
            };
        }
        i += 1;
    }
    // 前缀全等：剩下的只有长度差。把长侧多出的首字节一并记下，供归因使用。
    let (extra_first, extra_len, extra_on_b) = if a.len() > b.len() {
        let f = match a.get(limit) {
            Some(v) => *v,
            None => 0,
        };
        (f, a.len() - limit, false)
    } else if b.len() > a.len() {
        let f = match b.get(limit) {
            Some(v) => *v,
            None => 0,
        };
        (f, b.len() - limit, true)
    } else {
        (0u8, 0usize, false)
    };
    Walk {
        prefix: limit,
        min_len: limit,
        a_len: a.len(),
        b_len: b.len(),
        a_byte: 0,
        b_byte: 0,
        extra_first,
        extra_len,
        extra_on_b,
        same: a.len() == b.len(),
    }
}

/// 走查证据 + 甲侧切片 → 归因。归因是"证据侧"推断，与漂移声明无关。
pub fn attribute(sh: &Shape, w: &Walk, sa: &Slices) -> Note {
    if w.same {
        if sh.colon && needs_label(sh.kind) && sh.label_a != sh.label_b {
            return Note::LabelOnly;
        }
        return Note::Same;
    }
    if w.length_diff() && w.prefix == w.min_len {
        // 一侧更长：多出来的若只是末尾换行，归 Tail；出现非 ASCII 起始字节，
        // 说明长侧走的是翻译文案，归 Locale；否则按分歧点所在段落归。
        if w.extra_is_nl() {
            return Note::Tail;
        }
        if w.extra_first >= 0x80 {
            return Note::Locale;
        }
        let owner = segment_of(sh, w.prefix, sa);
        if owner == Note::Quote {
            // 段表覆盖不到（例如短侧提前结束）：按 strerror 尾差处理。
            return Note::ErrnoText;
        }
        return owner;
    }
    let a = w.a_byte;
    let b = w.b_byte;
    if (a == BBT && b == BAP) || (a == BAP && b == BBT) {
        return Note::Quote;
    }
    if a >= 0x80 || b >= 0x80 {
        return Note::Locale;
    }
    if ascii_lower(a) == ascii_lower(b) && a != b {
        // 同字母异大小写：落在版本段是 TagCase，落在符号段是 SymName。
        if sa.has_vt(sa.label_len.saturating_add(w.prefix)) {
            return Note::TagCase;
        }
        if sa.has_sym(sa.label_len.saturating_add(w.prefix)) {
            return Note::SymName;
        }
        return Note::TagCase;
    }
    segment_of(sh, w.prefix, sa)
}

/// 按甲侧切片把差异偏移落到段上。
fn segment_of(sh: &Shape, prefix: usize, sa: &Slices) -> Note {
    let abs = sa.label_len.saturating_add(prefix);
    if sa.quote_byte != 0 && sa.quote_at() == abs {
        return Note::Quote;
    }
    if sa.has_vt(abs) {
        return Note::TagCase;
    }
    if sa.has_sym(abs) {
        return Note::SymName;
    }
    if sa.has_obj(abs) {
        return Note::ObjPath;
    }
    if sa.has_err(abs) {
        return Note::ErrnoText;
    }
    // 落在所有段之外：报文结构本身缺内容，归形状。
    if !sh.colon && needs_label(sh.kind) {
        return Note::Body;
    }
    Note::Quote
}

/// 纵线：一条 Shape 走完八闸得到 Outcome。任何一闸不过即就地停驻，后面的闸
/// 不再执行——倒序执行会让"种类未知"的报文被当成"载荷全等"放行。
pub fn define(sh: &Shape) -> Outcome {
    // 闸一：采集。两侧标签与族都在场才叫采集完成。
    let mut stage = Stage::Collect;
    if sh.tag.len() == 0 {
        return Outcome {
            ok: false,
            stage,
            code: VM_EBODY,
            value: 0,
            note: Note::Body,
            default: false,
            len: 0,
        };
    }
    stage = Stage::Classify;
    // 闸二：种类注册。
    if !sh.registered || Kind::from_rank(sh.kind.rank()).is_none() {
        return Outcome {
            ok: false,
            stage,
            code: VM_EKTAG,
            value: 0,
            note: Note::Body,
            default: false,
            len: 0,
        };
    }
    stage = Stage::Split;
    // 闸三：标签切分。
    if !gate_split(sh) {
        return Outcome {
            ok: false,
            stage,
            code: VM_ELBL,
            value: 0,
            note: Note::Body,
            default: false,
            len: 0,
        };
    }
    stage = Stage::Comparability;
    // 闸四：可比性。
    if let Err(c) = gate_comparability(sh) {
        return Outcome {
            ok: false,
            stage,
            code: c,
            value: 0,
            note: Note::Body,
            default: false,
            len: 0,
        };
    }
    stage = Stage::Payload;
    // 闸五：载荷形状。
    if let Err(c) = gate_payload(sh) {
        return Outcome {
            ok: false,
            stage,
            code: c,
            value: 0,
            note: Note::Body,
            default: false,
            len: 0,
        };
    }
    stage = Stage::Compare;
    // 闸六：逐字对照。切片一次，取载荷段。
    let (ta, sa) = render_slice(sh, Machine::Host);
    let (tb, sb) = render_slice(sh, Machine::Target);
    let pa = payload_bytes(ta.as_str(), sa.label_len);
    let pb = payload_bytes(tb.as_str(), sb.label_len);
    // 载荷为空没有可比内容：走查在对照闸口判死，不许进入归因。
    if pa.len() == 0 {
        return Outcome {
            ok: false,
            stage,
            code: VM_EBODY,
            value: 0,
            note: Note::Body,
            default: false,
            len: 0,
        };
    }
    let w = walk(pa, pb);
    let value = w.prefix as u32;
    let len = pa.len() as u32;
    stage = Stage::Attribute;
    // 闸七：归因。
    let note = attribute(sh, &w, &sa);
    // 归因与走查必须自洽：全等只允许 Same/LabelOnly，非全等不许落在这两档。
    if w.same && note != Note::Same && note != Note::LabelOnly {
        return Outcome {
            ok: false,
            stage,
            code: VM_ESEAL,
            value,
            note,
            default: false,
            len,
        };
    }
    if !w.same && (note == Note::Same || note == Note::LabelOnly) {
        return Outcome {
            ok: false,
            stage,
            code: VM_ESEAL,
            value,
            note,
            default: false,
            len,
        };
    }
    let code = if w.same { 0 } else { VM_EVALUE };
    stage = Stage::Ready;
    // 闸八：联签位。只有符号版本两族占位，且必须走完对照。
    let default = w.ok_ready() && sh.kind.anchor();
    Outcome {
        ok: true,
        stage,
        code,
        value,
        note,
        default,
        len,
    }
}

impl Walk {
    /// 走查是否可用作就绪判定的输入（长度差与内容差都算走完）。
    pub const fn a_empty(&self) -> bool {
        self.a_len == 0
    }
}

/// 判序：状态 → 阶段 → 判定码 → 一致前缀 → 归因 → 联签位。
pub fn judge(c: &Case) -> Fault {
    let a = define(&c.shape);
    // 状态维优先：成功/失败互报，先于阶段与码。双机对照里这一维最恶性——把不可
    // 比报成一致，等于给假绿盖章。
    if a.ok() != c.claim.success {
        return Fault::Estate;
    }
    if a.stage != c.claim.stage {
        return Fault::Stage;
    }
    if a.code != c.claim.code {
        return Fault::Code;
    }
    if a.value != c.claim.value {
        return Fault::Value;
    }
    if a.note != c.claim.note {
        return Fault::Note;
    }
    if a.default != c.claim.default {
        return Fault::Default;
    }
    Fault::Ok
}

/// 判序期望：活槽期望 Ok，死槽按位次期望对应缺陷维。
pub const REJECT_FAULTS: [Fault; REJECT] = [
    Fault::Estate,
    Fault::Stage,
    Fault::Code,
    Fault::Value,
    Fault::Note,
    Fault::Default,
];

/// 六个死槽的**实际**判定码，互不相同——同码意味着两槽坏在同一处，语料失去
/// 区分力。次序与 REJECT_FAULTS 对齐。
pub const REJECT_CODES: [u32; REJECT] = [
    VM_EQUOTE,
    VM_ELBL,
    VM_EKTAG,
    VM_EVALUE,
    VM_ELANG,
    VM_EBODY,
];

/// 统计表：语料规模、活死、四档、联签位数、红档率。第 8 格必须是 929。
pub const VALS: [u32; ACCEPT] = [14, 8, 6, 4, 1, 4, 6, 929];

/// 标签表：逐槽短名，用于交叉校验标签与槽位。
pub const WANT_TAGS: [&'static str; CORPUS] = [
    "vfn-label-only",
    "vfn-identical",
    "undef-identical",
    "open-identical",
    "conflict-identical",
    "vfn-drift-quote",
    "undef-drift-tail",
    "dlopen-drift-errno",
    "estate-quote-mixed",
    "stage-no-colon",
    "code-unregistered",
    "value-tag-case",
    "note-non-c-locale",
    "default-empty-obj",
];

/// 阶段期望表：把申报表用第二种形式复写一遍，任一处笔误都会与实测对撞。
pub const WANT_STAGE: [Stage; CORPUS] = [
    Stage::Ready,
    Stage::Ready,
    Stage::Ready,
    Stage::Ready,
    Stage::Ready,
    Stage::Ready,
    Stage::Ready,
    Stage::Ready,
    Stage::Comparability,
    Stage::Split,
    Stage::Classify,
    Stage::Ready,
    Stage::Comparability,
    Stage::Payload,
];

/// 判定码期望表。
pub const WANT_CODE: [u32; CORPUS] = [
    0,
    0,
    0,
    0,
    0,
    VM_EVALUE,
    VM_EVALUE,
    VM_EVALUE,
    VM_EQUOTE,
    VM_ELBL,
    VM_EKTAG,
    VM_EVALUE,
    VM_ELANG,
    VM_EBODY,
];

/// 一致前缀期望表：活槽填实际算出的偏移；这里用 0 起算的相对档，真实值在
/// `case_at` 构造时由 define 反查一次，本表只做方向校验（0 = 有差异，1 = 全等）。
pub const WANT_NDX: [u8; CORPUS] = [
    1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

/// 归因期望表。
pub const WANT_NOTE: [Note; CORPUS] = [
    Note::LabelOnly,
    Note::Same,
    Note::Same,
    Note::Same,
    Note::Same,
    Note::Quote,
    Note::Tail,
    Note::ErrnoText,
    Note::Body,
    Note::Body,
    Note::Body,
    Note::TagCase,
    Note::Body,
    Note::Body,
];

/// 联签位期望表。
pub const WANT_DEFAULT: [bool; CORPUS] = [
    true, true, true, false, false, true, true, false, false, false, false, true, false, false,
];

/// 成败申报表（Estate 维交叉校验）。
pub const WANT_OK: [bool; CORPUS] = [
    true, true, true, true, true, true, true, true, false, false, false, true, false, false,
];

/// 语料：14 槽输入。前 8 槽是活槽（申报与实测一致），后 6 槽每槽只坏一维。
///
/// 活槽覆盖：三种全等族（`Vfn`/`Undef`/`Open`/`Conflict`）、仅标签差、三种漂移
/// 实测不一致（引号、末尾换行、strerror 文本）。死槽覆盖：引号档混写、缺冒号
/// 分隔、种类未注册、偏移误报、区域非 C、对象段为空。
pub const SHAPES: [Shape; CORPUS] = [
    // 0 活：主锚族 Vfn，双机调用路径不同、载荷全等。
    Shape {
        tag: WANT_TAGS[0],
        kind: Kind::Vfn,
        registered: true,
        label_a: "./svchk",
        label_b: "/usr/bin/svchk",
        obj: "libc.so.6",
        obj_b: "libc.so.6",
        vt: "GLIBC_2.34",
        sym: "fwrite_unlocked",
        req: "libneed.so",
        err_txt: "No such file or directory",
        err_txt_b: "Permission denied",
        err_no: ENOENT,
        quote: Quote::Tick,
        lang_c: true,
        colon: true,
        trail: true,
        drift: Drift::None,
    },
    // 1 活：主锚族 Vfn，双机逐字全等且标签也一致。
    Shape {
        tag: WANT_TAGS[1],
        kind: Kind::Vfn,
        registered: true,
        label_a: "./svchk",
        label_b: "./svchk",
        obj: "libc.so.6",
        obj_b: "libc.so.6",
        vt: "GLIBC_2.34",
        sym: "fwrite_unlocked",
        req: "libneed.so",
        err_txt: "No such file or directory",
        err_txt_b: "Permission denied",
        err_no: ENOENT,
        quote: Quote::Tick,
        lang_c: true,
        colon: true,
        trail: true,
        drift: Drift::None,
    },
    // 2 活：主锚族 Undef，延迟绑定面全等（版本名无引号）。
    Shape {
        tag: WANT_TAGS[2],
        kind: Kind::Undef,
        registered: true,
        label_a: "./svchk",
        label_b: "./svchk",
        obj: "libc.so.6",
        obj_b: "libc.so.6",
        vt: "GLIBC_2.34",
        sym: "bar",
        req: "libneed.so",
        err_txt: "No such file or directory",
        err_txt_b: "Permission denied",
        err_no: ENOENT,
        quote: Quote::Tick,
        lang_c: true,
        colon: true,
        trail: true,
        drift: Drift::None,
    },
    // 3 活：旁证族 Open，装载期打不开，含 strerror 尾。
    Shape {
        tag: WANT_TAGS[3],
        kind: Kind::Open,
        registered: true,
        label_a: "./svchk",
        label_b: "./svchk",
        obj: "libfoo.so.1",
        obj_b: "/opt/lib/libfoo.so.1",
        vt: "FOO_1",
        sym: "foo_init",
        req: "libfoo.so.1",
        err_txt: "No such file or directory",
        err_txt_b: "Permission denied",
        err_no: ENOENT,
        quote: Quote::Tick,
        lang_c: true,
        colon: true,
        trail: true,
        drift: Drift::None,
    },
    // 4 活：旁证族 Conflict，链接期 soname 冲突警告（无标签段）。
    Shape {
        tag: WANT_TAGS[4],
        kind: Kind::Conflict,
        registered: true,
        label_a: "ld",
        label_b: "ld",
        obj: "libstdc++.so.6",
        obj_b: "libstdc++.so.6",
        vt: "CXXABI_1.3.13",
        sym: "_ZStls",
        req: "/usr/lib/gcc/x86_64-linux-gnu/11/libstdc++.so",
        err_txt: "No such file or directory",
        err_txt_b: "Permission denied",
        err_no: ENONE,
        quote: Quote::Tick,
        lang_c: true,
        colon: false,
        trail: true,
        drift: Drift::None,
    },
    // 5 活：主锚族 Vfn，目标机把 TeX 式引号写成成对撇号 → CONFLICT 显式。
    Shape {
        tag: WANT_TAGS[5],
        kind: Kind::Vfn,
        registered: true,
        label_a: "./svchk",
        label_b: "./svchk",
        obj: "libm.so.6",
        obj_b: "libm.so.6",
        vt: "GLIBC_2.29",
        sym: "exp10",
        req: "libm.so.6",
        err_txt: "No such file or directory",
        err_txt_b: "Permission denied",
        err_no: ENOENT,
        quote: Quote::Tick,
        lang_c: true,
        colon: true,
        trail: true,
        drift: Drift::Quote,
    },
    // 6 活：主锚族 Undef，目标机丢末尾换行 → CONFLICT 显式（长度差档）。
    Shape {
        tag: WANT_TAGS[6],
        kind: Kind::Undef,
        registered: true,
        label_a: "./svchk",
        label_b: "./svchk",
        obj: "libc.so.6",
        obj_b: "libc.so.6",
        vt: "GLIBC_2.28",
        sym: "quick_exit",
        req: "libneed.so",
        err_txt: "No such file or directory",
        err_txt_b: "Permission denied",
        err_no: ENOENT,
        quote: Quote::Tick,
        lang_c: true,
        colon: true,
        trail: true,
        drift: Drift::Tail,
    },
    // 7 活：旁证族 Dlopen，目标机 errno 文本不同 → CONFLICT 显式。
    Shape {
        tag: WANT_TAGS[7],
        kind: Kind::Dlopen,
        registered: true,
        label_a: "libfoo.so.1",
        label_b: "libfoo.so.1",
        obj: "libfoo.so.1",
        obj_b: "libfoo.so.1",
        vt: "FOO_1",
        sym: "foo_init",
        req: "libfoo.so.1",
        err_txt: "No such file or directory",
        err_txt_b: "Permission denied",
        err_no: ENOENT,
        quote: Quote::Tick,
        lang_c: true,
        colon: false,
        trail: true,
        drift: Drift::ErrnoText,
    },
    // 8 死·状态：Vfn 本侧引号档混写（形状不可比），申报谎报成功。
    Shape {
        tag: WANT_TAGS[8],
        kind: Kind::Vfn,
        registered: true,
        label_a: "./svchk",
        label_b: "./svchk",
        obj: "libc.so.6",
        obj_b: "libc.so.6",
        vt: "GLIBC_2.30",
        sym: "memcpy",
        req: "libneed.so",
        err_txt: "No such file or directory",
        err_txt_b: "Permission denied",
        err_no: ENOENT,
        quote: Quote::Mixed,
        lang_c: true,
        colon: true,
        trail: true,
        drift: Drift::None,
    },
    // 9 死·阶段：Vfn 缺 `<标签>: ` 分隔，申报却报就绪。
    Shape {
        tag: WANT_TAGS[9],
        kind: Kind::Vfn,
        registered: true,
        label_a: "./svchk",
        label_b: "./svchk",
        obj: "libc.so.6",
        obj_b: "libc.so.6",
        vt: "GLIBC_2.33",
        sym: "pthread_create",
        req: "libneed.so",
        err_txt: "No such file or directory",
        err_txt_b: "Permission denied",
        err_no: ENOENT,
        quote: Quote::Tick,
        lang_c: true,
        colon: false,
        trail: true,
        drift: Drift::None,
    },
    // 10 死·码：第七族（未注册）报文，申报成对照结论矛盾码。
    Shape {
        tag: WANT_TAGS[10],
        kind: Kind::Preload,
        registered: false,
        label_a: "./svchk",
        label_b: "./svchk",
        obj: "libhook.so",
        obj_b: "libhook.so",
        vt: "HOOK_1",
        sym: "hook_entry",
        req: "libhook.so",
        err_txt: "No such file or directory",
        err_txt_b: "Permission denied",
        err_no: ENOENT,
        quote: Quote::Apos,
        lang_c: true,
        colon: false,
        trail: true,
        drift: Drift::None,
    },
    // 11 死·值：Undef 目标机版本名大小写漂移，申报把偏移填成全长。
    Shape {
        tag: WANT_TAGS[11],
        kind: Kind::Undef,
        registered: true,
        label_a: "./svchk",
        label_b: "./svchk",
        obj: "libc.so.6",
        obj_b: "libc.so.6",
        vt: "GLIBC_2.27",
        sym: "explicit_bzero",
        req: "libneed.so",
        err_txt: "No such file or directory",
        err_txt_b: "Permission denied",
        err_no: ENOENT,
        quote: Quote::Tick,
        lang_c: true,
        colon: true,
        trail: true,
        drift: Drift::TagCase,
    },
    // 12 死·归因：本侧区域非 C（不可比），申报归因写成全等。
    Shape {
        tag: WANT_TAGS[12],
        kind: Kind::Vfn,
        registered: true,
        label_a: "./svchk",
        label_b: "./svchk",
        obj: "libc.so.6",
        obj_b: "libc.so.6",
        vt: "GLIBC_2.26",
        sym: "aligned_alloc",
        req: "libneed.so",
        err_txt: "No such file or directory",
        err_txt_b: "Permission denied",
        err_no: ENOENT,
        quote: Quote::Tick,
        lang_c: false,
        colon: true,
        trail: true,
        drift: Drift::None,
    },
    // 13 死·联签位：Vfn 对象段为空（载荷残缺），申报占了主锚位。
    Shape {
        tag: WANT_TAGS[13],
        kind: Kind::Vfn,
        registered: true,
        label_a: "./svchk",
        label_b: "./svchk",
        obj: "",
        obj_b: "libc.so.6",
        vt: "GLIBC_2.25",
        sym: "timespec_get",
        req: "libneed.so",
        err_txt: "No such file or directory",
        err_txt_b: "Permission denied",
        err_no: ENOENT,
        quote: Quote::Tick,
        lang_c: true,
        colon: true,
        trail: true,
        drift: Drift::None,
    },
];

/// 死槽位次对应的缺陷维；活槽为 `Fault::Ok`。
pub const CLAIM_BROKEN: [Fault; CORPUS] = [
    Fault::Ok,
    Fault::Ok,
    Fault::Ok,
    Fault::Ok,
    Fault::Ok,
    Fault::Ok,
    Fault::Ok,
    Fault::Ok,
    Fault::Estate,
    Fault::Stage,
    Fault::Code,
    Fault::Value,
    Fault::Note,
    Fault::Default,
];

/// 取第 i 槽输入。
pub fn shape_at(i: usize) -> Option<Shape> {
    match SHAPES.get(i) {
        Some(s) => Some(*s),
        None => None,
    }
}

/// 第 i 槽的实测结论。
pub fn outcome_at(i: usize) -> Option<Outcome> {
    match shape_at(i) {
        Some(s) => Some(define(&s)),
        None => None,
    }
}

/// 第 i 槽的申报表：从实测抄一遍，再按位次把唯一一维改坏。改坏只此一处，
/// 其余五维必须与实测同值——一槽多坏会让缺陷维互相遮蔽。
pub fn claim_at(i: usize) -> Option<Claim> {
    let o = match outcome_at(i) {
        Some(v) => v,
        None => return None,
    };
    let base = Claim::from_outcome(&o);
    match CLAIM_BROKEN.get(i) {
        Some(f) => Some(base.break_dim(*f)),
        None => Some(base),
    }
}

/// 第 i 槽完整语料条目。
pub fn case_at(i: usize) -> Option<Case> {
    let shape = match shape_at(i) {
        Some(v) => v,
        None => return None,
    };
    let claim = match claim_at(i) {
        Some(v) => v,
        None => return None,
    };
    Some(Case { shape, claim })
}

/// 全量申报表。
pub fn claims() -> [Option<Claim>; CORPUS] {
    let mut out: [Option<Claim>; CORPUS] = [None; CORPUS];
    let mut i = 0usize;
    while i < CORPUS {
        if let Some(c) = claim_at(i) {
            if let Some(slot) = out.get_mut(i) {
                *slot = Some(c);
            }
        }
        i += 1;
    }
    out
}

/// 第 i 槽的期望缺陷维：活槽 `Ok`，死槽按位次取 `REJECT_FAULTS`。
pub fn expected_fault(i: usize) -> Fault {
    if i < ACCEPT {
        return Fault::Ok;
    }
    match REJECT_FAULTS.get(i.saturating_sub(ACCEPT)) {
        Some(f) => *f,
        None => Fault::Estate,
    }
}

/// 四档计数（按实测结论分档，覆盖全部 14 槽）。
pub fn tier_counts() -> [usize; TIER_COUNT] {
    let mut c = [0usize; TIER_COUNT];
    let mut i = 0usize;
    while i < CORPUS {
        if let Some(o) = outcome_at(i) {
            let t = Tier::of(&o);
            let k = t.rank() as usize;
            if k < TIER_COUNT {
                if let Some(slot) = c.get_mut(k) {
                    *slot = slot.saturating_add(1);
                }
            }
        }
        i += 1;
    }
    c
}

/// 具备主锚联签资格的槽数（实测 default 为真）。
pub fn anchored_count() -> usize {
    let mut n = 0usize;
    let mut i = 0usize;
    while i < CORPUS {
        if let Some(o) = outcome_at(i) {
            if o.default {
                n = n.saturating_add(1);
            }
        }
        i += 1;
    }
    n
}

/// 申报与实测完全一致的槽数（活槽数）。
pub fn live_count() -> usize {
    let mut n = 0usize;
    let mut i = 0usize;
    while i < CORPUS {
        if let Some(c) = case_at(i) {
            if judge(&c) == Fault::Ok {
                n = n.saturating_add(1);
            }
        }
        i += 1;
    }
    n
}

/// 恰好坏一维的槽数（死槽数）。
pub fn dead_count() -> usize {
    let mut n = 0usize;
    let mut i = 0usize;
    while i < CORPUS {
        if let Some(c) = case_at(i) {
            if judge(&c) != Fault::Ok {
                n = n.saturating_add(1);
            }
        }
        i += 1;
    }
    n
}

/// 全等档（`Note::Same`）槽数——perfect_rows 的证据来源。
pub fn identical_count() -> usize {
    let mut n = 0usize;
    let mut i = 0usize;
    while i < CORPUS {
        if let Some(o) = outcome_at(i) {
            if o.ok && o.note == Note::Same {
                n = n.saturating_add(1);
            }
        }
        i += 1;
    }
    n
}

/// 矩阵行：一个档位及其计数与联签数。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Row {
    /// 档位。
    pub tier: Tier,
    /// 该档槽数。
    pub total: usize,
    /// 该档中实测走完成功的槽数。
    pub good: usize,
    /// 该档中占主锚联签位的槽数。
    pub anchored: usize,
}

impl Row {
    /// 由计数构造一行。
    pub const fn new(tier: Tier, total: usize, good: usize, anchored: usize) -> Row {
        Row {
            tier,
            total,
            good,
            anchored,
        }
    }

    /// 该档是否算绿。
    pub const fn is_green(&self) -> bool {
        self.tier.good() && self.total == self.good
    }

    /// 行文本，逐字进渲染面。
    pub fn text(&self) -> String {
        let mut s = String::new();
        s.push_str(self.tier.slug());
        s.push_str(" total=");
        s.push_str(dec_text(self.total as u32).as_str());
        s.push_str(" good=");
        s.push_str(dec_text(self.good as u32).as_str());
        s.push_str(" anchor=");
        s.push_str(dec_text(self.anchored as u32).as_str());
        s
    }
}

/// 四档矩阵。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Matrix {
    /// 四行。
    pub rows: [Row; TIER_COUNT],
    /// 总槽数。
    pub total: usize,
    /// 实测成功槽数（与 total 两维独立）。
    pub good: usize,
}

impl Matrix {
    /// 由语料实测构造。
    pub fn build() -> Matrix {
        let counts = tier_counts();
        let mut rows: [Row; TIER_COUNT] = [
            Row::new(Tier::Identical, 0, 0, 0),
            Row::new(Tier::LabelOnly, 0, 0, 0),
            Row::new(Tier::Conflict, 0, 0, 0),
            Row::new(Tier::Malformed, 0, 0, 0),
        ];
        let mut total_good = 0usize;
        let mut i = 0usize;
        while i < CORPUS {
            if let Some(o) = outcome_at(i) {
                let t = Tier::of(&o);
                if let Some(r) = rows.get_mut(t.rank() as usize) {
                    r.total = r.total.saturating_add(1);
                    r.anchored = r.anchored.saturating_add(if o.default { 1 } else { 0 });
                    if o.ok {
                        r.good = r.good.saturating_add(1);
                    }
                }
                if o.ok {
                    total_good = total_good.saturating_add(1);
                }
            }
            i += 1;
        }
        let _ = counts;
        Matrix {
            rows,
            total: CORPUS,
            good: total_good,
        }
    }

    /// 取档行。
    pub fn row(&self, t: Tier) -> Option<&Row> {
        match self.rows.get(t.rank() as usize) {
            Some(r) => Some(r),
            None => None,
        }
    }

    /// 全等档行数（逐字一致且标签一致）。
    pub fn perfect_rows(&self) -> usize {
        match self.row(Tier::Identical) {
            Some(r) => r.total,
            None => 0,
        }
    }

    /// CONFLICT 档行数——本卡要求这些行全部显式在册。
    pub fn conflict_rows(&self) -> usize {
        match self.row(Tier::Conflict) {
            Some(r) => r.total,
            None => 0,
        }
    }

    /// 仅标签差档行数。
    pub fn label_rows(&self) -> usize {
        match self.row(Tier::LabelOnly) {
            Some(r) => r.total,
            None => 0,
        }
    }

    /// 形状不可比档行数。
    pub fn malformed_rows(&self) -> usize {
        match self.row(Tier::Malformed) {
            Some(r) => r.total,
            None => 0,
        }
    }

    /// 四档计数之和是否等于总槽数（覆盖性判据）。
    pub fn covered(&self) -> bool {
        let mut sum = 0usize;
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(r) = self.rows.get(i) {
                sum = sum.saturating_add(r.total);
            }
            i += 1;
        }
        sum == self.total
    }

    /// 每个 CONFLICT 行都带非零码且偏移小于全长。
    pub fn conflict_explicit(&self) -> bool {
        let mut i = 0usize;
        let mut seen = 0usize;
        while i < CORPUS {
            if let Some(o) = outcome_at(i) {
                if Tier::of(&o) == Tier::Conflict {
                    seen = seen.saturating_add(1);
                    if o.code != VM_EVALUE {
                        return false;
                    }
                    if o.value >= o.len && o.note != Note::Locale {
                        // 全等偏移却落在 CONFLICT 档，只有翻译面（长度也变）可能，
                        // 其余一律视为隐式吞差。
                        if !(o.note == Note::Tail || o.note == Note::ErrnoText) {
                            return false;
                        }
                    }
                }
            }
            i += 1;
        }
        seen == self.conflict_rows()
    }

    /// 联签位总数。
    pub fn anchored(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(r) = self.rows.get(i) {
                n = n.saturating_add(r.anchored);
            }
            i += 1;
        }
        n
    }

    /// 逐档渲染。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str("matrix");
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(r) = self.rows.get(i) {
                s.push_str(" | ");
                s.push_str(r.text().as_str());
            }
            i += 1;
        }
        s.push_str(" | total=");
        s.push_str(dec_text(self.total as u32).as_str());
        s.push_str(" good=");
        s.push_str(dec_text(self.good as u32).as_str());
        s
    }
}

/// 汇总报告。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Report {
    /// 总槽数。
    pub total: usize,
    /// 活槽数。
    pub accept: usize,
    /// 死槽数。
    pub reject: usize,
    /// 全等档数。
    pub identical: usize,
    /// 仅标签差档数。
    pub label_only: usize,
    /// CONFLICT 档数。
    pub conflict: usize,
    /// 形状不可比档数。
    pub malformed: usize,
    /// 判序命中期望的槽数。
    pub matched: usize,
    /// 主锚联签数。
    pub anchored: usize,
}

impl Report {
    /// 一致率（千分），四舍五入。
    pub fn rate(&self) -> u32 {
        if self.total == 0 {
            return 0;
        }
        let num = (self.matched as u64).saturating_mul(1000);
        let adj = num.saturating_add((self.total / 2) as u64);
        (adj / (self.total as u64)) as u32
    }

    /// 是否全绿：每槽判序命中期望，且死槽数正好 6。
    pub fn green(&self) -> bool {
        self.matched == self.total
            && self.reject == REJECT
            && self.accept == ACCEPT
            && self.total == CORPUS
    }

    /// 闸门：不绿就给出码。
    pub fn gate(&self) -> Result<(), u32> {
        if self.matched != self.total {
            return Err(VM_ESTATE);
        }
        if self.total != CORPUS {
            return Err(ESLOT);
        }
        if self.accept != ACCEPT || self.reject != REJECT {
            return Err(VM_ECODE);
        }
        if self.identical.saturating_add(self.label_only) != anchored_off(self) {
            return Err(VM_EVALUE);
        }
        if !self.conflict_explicit() {
            return Err(VM_ESEAL);
        }
        Ok(())
    }

    /// CONFLICT 档是否全部显式在册（偏移与归因都在）。
    pub fn conflict_explicit(&self) -> bool {
        Matrix::build().conflict_explicit()
    }

    /// 逐字渲染全绿文本。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str("vermsg-two-machine total=");
        s.push_str(dec_text(self.total as u32).as_str());
        s.push_str(" accept=");
        s.push_str(dec_text(self.accept as u32).as_str());
        s.push_str(" reject=");
        s.push_str(dec_text(self.reject as u32).as_str());
        s.push_str(" identical=");
        s.push_str(dec_text(self.identical as u32).as_str());
        s.push_str(" label=");
        s.push_str(dec_text(self.label_only as u32).as_str());
        s.push_str(" conflict=");
        s.push_str(dec_text(self.conflict as u32).as_str());
        s.push_str(" malformed=");
        s.push_str(dec_text(self.malformed as u32).as_str());
        s.push_str(" anchor=");
        s.push_str(dec_text(self.anchored as u32).as_str());
        s.push_str(" rate=");
        s.push_str(dec_text(self.rate()).as_str());
        s
    }
}

/// 报告面判据：全等档与仅标签差档之和应等于"载荷全等"槽数。
fn anchored_off(r: &Report) -> usize {
    let mut n = 0usize;
    let mut i = 0usize;
    while i < CORPUS {
        if let Some(o) = outcome_at(i) {
            if o.identical() {
                n = n.saturating_add(1);
            }
        }
        i += 1;
    }
    let _ = r;
    n
}

/// 全绿渲染文本，逐字钉住。
const RENDER_OK: &str = "vermsg-two-machine total=14 accept=8 reject=6 identical=4 label=1 conflict=4 malformed=5 anchor=6 rate=1000";

/// 汇总一次双机对照验收。
pub fn run_with(over: Option<(usize, Fault)>) -> Result<Report, u32> {
    let (idx, f) = match over {
        Some(p) => p,
        None => (usize::MAX, Fault::Ok),
    };
    if idx != usize::MAX && idx >= CORPUS {
        return Err(ESLOT);
    }
    let mut matched = 0usize;
    let mut accept = 0usize;
    let mut reject = 0usize;
    let mut i = 0usize;
    while i < CORPUS {
        let c = match case_at(i) {
            Some(v) => v,
            None => return Err(ESLOT),
        };
        let got = judge(&c);
        let exp = if i == idx {
            f
        } else {
            expected_fault(i)
        };
        if got == exp {
            matched = matched.saturating_add(1);
        }
        if got == Fault::Ok {
            accept = accept.saturating_add(1);
        } else {
            reject = reject.saturating_add(1);
        }
        i += 1;
    }
    let counts = tier_counts();
    let malformed = match counts.get(Tier::Malformed.rank() as usize) {
        Some(v) => *v,
        None => 0,
    };
    let anchor = match counts.get(Tier::Conflict.rank() as usize) {
        Some(_) => anchored_count(),
        None => 0,
    };
    Ok(Report {
        total: CORPUS,
        accept,
        reject,
        identical: match counts.get(Tier::Identical.rank() as usize) {
            Some(v) => *v,
            None => 0,
        },
        label_only: match counts.get(Tier::LabelOnly.rank() as usize) {
            Some(v) => *v,
            None => 0,
        },
        conflict: match counts.get(Tier::Conflict.rank() as usize) {
            Some(v) => *v,
            None => 0,
        },
        malformed,
        matched,
        anchored: anchor,
    })
}

/// 无注入验收。
pub fn run() -> Result<Report, u32> {
    run_with(None)
}

/// 结论码的可读名。
pub fn code_name(code: u32) -> &'static str {
    if code == 0 {
        return "identical";
    } else if code == VM_ESTATE {
        return "estate";
    } else if code == VM_ESTAGE {
        return "estage";
    } else if code == VM_ECODE {
        return "ecode";
    } else if code == VM_EVALUE {
        return "evalue";
    } else if code == VM_ENOTE {
        return "enote";
    } else if code == VM_EDEFAULT {
        return "edefault";
    } else if code == VM_EKTAG {
        return "ektag";
    } else if code == VM_EQUOTE {
        return "equote";
    } else if code == VM_ELANG {
        return "elang";
    } else if code == VM_ELBL {
        return "elbl";
    } else if code == VM_EBODY {
        return "ebody";
    } else if code == VM_ESEAL {
        return "eseal";
    } else if code == ESLOT {
        return "eslot";
    } else {
        return "unknown";
    }
}

/// 释义行：把一槽的两份采集件与结论逐字摊开，供人工复核。
pub fn detail(c: &Case) -> String {
    let a = define(&c.shape);
    let mut s = String::new();
    s.push_str(c.shape.tag);
    s.push_str(" kind=");
    s.push_str(c.shape.kind.slug());
    s.push_str(" drift=");
    s.push_str(c.shape.drift.slug());
    s.push_str(" stage=");
    s.push_str(a.stage.slug());
    s.push_str(" code=");
    s.push_str(code_name(a.code));
    s.push_str("(0x");
    s.push_str(hex_text(a.code, 5).as_str());
    s.push_str(") prefix=");
    s.push_str(dec_text(a.value).as_str());
    s.push_str("/");
    s.push_str(dec_text(a.len).as_str());
    s.push_str(" note=");
    s.push_str(a.note.slug());
    s.push_str(" anchor=");
    s.push_str(bool_text(a.default).as_str());
    s
}

/// 一槽的对照诊断全文（两份终端文本并排）。
pub fn diagnostic(c: &Case) -> String {
    let mut s = String::new();
    s.push_str("[host]  ");
    s.push_str(msg_body(&c.shape, Machine::Host).as_str());
    s.push('\n');
    s.push_str("[target] ");
    s.push_str(msg_body(&c.shape, Machine::Target).as_str());
    s.push('\n');
    s.push_str(detail(c).as_str());
    s
}

/// 判定缺陷维的一句话释义。
pub fn fault_text(f: Fault) -> &'static str {
    match f {
        Fault::Ok => "申报与实测逐字一致",
        Fault::Estate => "成败互报：把不可比报成一致是最恶性的一类",
        Fault::Stage => "阶段不符：纵线没走完就报就绪",
        Fault::Code => "判定码不符：形状码与漂移码混用",
        Fault::Value => "一致前缀不符：偏移填成了全长",
        Fault::Note => "归因不符：差异落段判错",
        Fault::Default => "联签位不符：旁证族占了主锚位",
    }
}

/// 一槽的判序结论。
pub fn verdict(i: usize) -> Option<Fault> {
    match case_at(i) {
        Some(c) => Some(judge(&c)),
        None => None,
    }
}

/// 阶段闸门：单调推进，倒退即判阶段码；关闭后一切动作判状态码。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Gate {
    /// 当前停驻阶段。
    stage: Stage,
    /// 是否已关闭。
    closed: bool,
    /// 被拒绝的动作数。
    denied: usize,
}

impl Gate {
    /// 从采集起步。
    pub const fn new() -> Gate {
        Gate {
            stage: Stage::Collect,
            closed: false,
            denied: 0,
        }
    }

    /// 当前阶段。
    pub const fn stage(&self) -> Stage {
        self.stage
    }

    /// 是否已关闭。
    pub const fn closed(&self) -> bool {
        self.closed
    }

    /// 被拒动作数。
    pub const fn denied(&self) -> usize {
        self.denied
    }

    /// 推进一步；倒退判 `VM_ESTAGE`，关闭后判 `VM_ESTATE`。
    pub fn step(&mut self, to: Stage) -> Result<(), u32> {
        if self.closed {
            self.denied = self.denied.saturating_add(1);
            return Err(VM_ESTATE);
        }
        if to.rank() < self.stage.rank() {
            self.denied = self.denied.saturating_add(1);
            return Err(VM_ESTAGE);
        }
        if to.rank() == self.stage.rank() {
            // 同阶段重复推进不算前进，也不算倒退。
            return Ok(());
        }
        // 只许一级一级走：跳过任何一闸都判阶段码。
        let nxt = match self.stage.next() {
            Some(v) => v,
            None => {
                self.denied = self.denied.saturating_add(1);
                return Err(VM_ESTAGE);
            }
        };
        if nxt != to {
            self.denied = self.denied.saturating_add(1);
            return Err(VM_ESTAGE);
        }
        self.stage = to;
        Ok(())
    }

    /// 关闭闸门；关闭后任何 step/close 都被拒。
    pub fn close(&mut self) -> Result<(), u32> {
        if self.closed {
            self.denied = self.denied.saturating_add(1);
            return Err(VM_ESTATE);
        }
        if self.stage != Stage::Ready {
            return Err(VM_ESTAGE);
        }
        self.closed = true;
        Ok(())
    }

    /// 走完全部八闸的便捷路径。
    pub fn walk_all(&mut self) -> Result<(), u32> {
        let seq = [
            Stage::Classify,
            Stage::Split,
            Stage::Comparability,
            Stage::Payload,
            Stage::Compare,
            Stage::Attribute,
            Stage::Ready,
        ];
        let mut i = 0usize;
        while i < seq.len() {
            if let Some(s) = seq.get(i) {
                self.step(*s)?;
            }
            i += 1;
        }
        self.close()
    }
}

impl Default for Gate {
    fn default() -> Gate {
        Gate::new()
    }
}

/// 账本样本：一格入账记录。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Sample {
    /// 槽位（必须等于登记时的长度）。
    pub slot: u8,
    /// 操作码：1..7 统计格，8 红档格，9..14 缺陷码格。
    pub op: u8,
    /// 该格是否为成功档。
    pub ok: bool,
    /// 判定码（缺陷格填实际缺陷码，统计格填 0）。
    pub code: u32,
    /// 数值（统计格填统计值，红档格填 929，缺陷格填 0）。
    pub value: u32,
}

impl Sample {
    /// 统计格构造。
    pub const fn stat(op: u8, value: u32) -> Sample {
        Sample {
            slot: 0,
            op,
            ok: true,
            code: 0,
            value,
        }
    }

    /// 缺陷码格构造：值必须为 0，缺陷码进 code 维。
    pub const fn defect(op: u8, code: u32) -> Sample {
        Sample {
            slot: 0,
            op,
            ok: false,
            code,
            value: 0,
        }
    }

    /// 折进指纹。
    pub fn fold(&self, h: u64) -> u64 {
        let a = fold_byte(h, self.slot);
        let b = fold_byte(a, self.op);
        let c = fold_byte(b, self.ok as u8);
        let d = fold_word(c, (self.code & 0xffff) as u16);
        let e = fold_word(d, (self.code >> 16) as u16);
        let f = fold_word(e, (self.value & 0xffff) as u16);
        fold_word(f, (self.value >> 16) as u16)
    }

    /// 行的可读文本。
    pub fn text(&self) -> String {
        let mut s = String::new();
        s.push_str("slot=");
        s.push_str(dec_text(self.slot as u32).as_str());
        s.push_str(" op=");
        s.push_str(dec_text(self.op as u32).as_str());
        s.push_str(" ok=");
        s.push_str(bool_text(self.ok).as_str());
        s.push_str(" code=");
        s.push_str(code_name(self.code));
        s.push_str(" value=");
        s.push_str(dec_text(self.value).as_str());
        s
    }
}

/// 账本：槽位严格递增，封印折叠全账，冻结后拒登记，重放比指纹。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Ledger {
    /// 已登记样本。
    items: [Sample; SAMPLES],
    /// 长度。
    n: usize,
    /// 冻结标记。
    frozen: bool,
    /// 封印指纹。
    root_val: u64,
    /// 被拒登记次数。
    rejected: usize,
}

impl Ledger {
    /// 空账。
    pub const fn new() -> Ledger {
        Ledger {
            items: [Sample {
                slot: 0,
                op: 0,
                ok: false,
                code: 0,
                value: 0,
            }; SAMPLES],
            n: 0,
            frozen: false,
            root_val: 0,
            rejected: 0,
        }
    }

    /// 登记一格：槽位必须等于当前长度，否则拒入账并计数。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen {
            self.rejected = self.rejected.saturating_add(1);
            return Err(VM_ESTATE);
        }
        if self.n >= SAMPLES {
            self.rejected = self.rejected.saturating_add(1);
            return Err(ESLOT);
        }
        if (s.slot as usize) != self.n {
            self.rejected = self.rejected.saturating_add(1);
            return Err(ESLOT);
        }
        if let Some(slot) = self.items.get_mut(self.n) {
            *slot = s;
        }
        self.n = self.n.saturating_add(1);
        Ok(())
    }

    /// 已登记格数。
    pub const fn len(&self) -> usize {
        self.n
    }

    /// 是否空账。
    pub const fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// 取格。
    pub fn sample(&self, i: usize) -> Option<Sample> {
        if i >= self.n {
            return None;
        }
        match self.items.get(i) {
            Some(v) => Some(*v),
            None => None,
        }
    }

    /// 成功档格数。
    pub fn ok_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.n {
            if let Some(s) = self.sample(i) {
                if s.ok {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 缺陷档格数。
    pub fn err_count(&self) -> usize {
        self.n.saturating_sub(self.ok_count())
    }

    /// 被拒登记次数。
    pub const fn rejects(&self) -> usize {
        self.rejected
    }

    /// 是否已冻结。
    pub const fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 折叠全账（不冻结），得到指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.n {
            if let Some(s) = self.sample(i) {
                h = s.fold(h);
            }
            i += 1;
        }
        fold_word(h, self.n as u16)
    }

    /// 逐格复核：统计格对齐实测、缺陷码格对齐 `REJECT_CODES`、败格 value 必为 0。
    pub fn audit(&self) -> Result<(), u32> {
        if self.n != SAMPLES {
            return Err(ESLOT);
        }
        let counts = tier_counts();
        let matrix = Matrix::build();
        let expect_stat: [u32; 7] = [
            CORPUS as u32,
            live_count() as u32,
            dead_count() as u32,
            match counts.get(Tier::Identical.rank() as usize) {
                Some(v) => *v as u32,
                None => 0,
            },
            matrix.label_rows() as u32,
            matrix.conflict_rows() as u32,
            matrix.anchored() as u32,
        ];
        let mut i = 0usize;
        while i < self.n {
            let s = match self.sample(i) {
                Some(v) => v,
                None => return Err(ESLOT),
            };
            if (s.slot as usize) != i {
                return Err(ESLOT);
            }
            if !s.ok {
                if s.value != 0 {
                    return Err(VM_EVALUE);
                }
                if s.op < 9 || s.op > 14 {
                    return Err(VM_ECODE);
                }
                let k = (s.op as usize).saturating_sub(9);
                let want = match REJECT_CODES.get(k) {
                    Some(v) => *v,
                    None => return Err(VM_ECODE),
                };
                if s.code != want {
                    return Err(VM_ECODE);
                }
            } else if s.op == 8 {
                // 红档格：必须是 929，且与任何统计格不同值。
                if s.value != 929 {
                    return Err(VM_EVALUE);
                }
                let mut j = 0usize;
                while j < 7 {
                    if let Some(v) = expect_stat.get(j) {
                        if *v == 929 {
                            return Err(VM_EVALUE);
                        }
                    }
                    j += 1;
                }
            } else {
                if s.code != 0 {
                    return Err(VM_ECODE);
                }
                if s.op < 1 || s.op > 7 {
                    return Err(VM_ECODE);
                }
                let k = (s.op as usize).saturating_sub(1);
                let want = match expect_stat.get(k) {
                    Some(v) => *v,
                    None => return Err(VM_ECODE),
                };
                if s.value != want {
                    return Err(VM_EVALUE);
                }
            }
            i += 1;
        }
        Ok(())
    }

    /// 封印并冻结；冻结后指纹非零。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(VM_ESTATE);
        }
        if self.n != SAMPLES {
            return Err(ESLOT);
        }
        let r = self.seal();
        if r == FNV_OFF || r == 0 {
            return Err(VM_ESEAL);
        }
        self.root_val = r;
        self.frozen = true;
        Ok(r)
    }

    /// 指纹（未冻结为 0）。
    pub const fn root(&self) -> u64 {
        self.root_val
    }

    /// 重放：按同一构造序重建账本并比指纹。
    pub fn replay(&self) -> Result<u64, u32> {
        if !self.frozen {
            return Err(VM_ESTATE);
        }
        let fresh = build_ledger();
        let r = fresh.seal();
        if r != self.root_val {
            return Err(VM_ESEAL);
        }
        Ok(r)
    }

    /// 渲染全账文本。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str("ledger n=");
        s.push_str(dec_text(self.n as u32).as_str());
        s.push_str(" ok=");
        s.push_str(dec_text(self.ok_count() as u32).as_str());
        s.push_str(" err=");
        s.push_str(dec_text(self.err_count() as u32).as_str());
        s.push_str(" reject=");
        s.push_str(dec_text(self.rejected as u32).as_str());
        s.push_str(" root=0x");
        s.push_str(hex_text((self.root_val & 0xffff_ffff) as u32, 8).as_str());
        s
    }
}

impl Default for Ledger {
    fn default() -> Ledger {
        Ledger::new()
    }
}

/// 建满账：7 统计格 + 1 红档格 + 6 缺陷码格 = 14 格。
pub fn build_ledger() -> Ledger {
    let counts = tier_counts();
    let matrix = Matrix::build();
    let mut l = Ledger::new();
    let stats: [u32; 7] = [
        CORPUS as u32,
        live_count() as u32,
        dead_count() as u32,
        match counts.get(Tier::Identical.rank() as usize) {
            Some(v) => *v as u32,
            None => 0,
        },
        matrix.label_rows() as u32,
        matrix.conflict_rows() as u32,
        matrix.anchored() as u32,
    ];
    let mut i = 0usize;
    while i < 7 {
        let v = match stats.get(i) {
            Some(x) => *x,
            None => 0,
        };
        let op = (i as u8).saturating_add(1);
        let s = Sample::stat(op, v);
        let cell = Sample {
            slot: i as u8,
            op: s.op,
            ok: s.ok,
            code: s.code,
            value: s.value,
        };
        let _ = l.register(cell);
        i += 1;
    }
    // 红档格：注入一槽错判后的一致率。
    let red = match run_with(Some((0, Fault::Code))) {
        Ok(r) => r.rate(),
        Err(_) => 0,
    };
    let red_cell = Sample {
        slot: 7,
        op: 8,
        ok: true,
        code: 0,
        value: red,
    };
    let _ = l.register(red_cell);
    // 六格缺陷码：逐格对齐 REJECT_CODES，value 恒 0。
    let mut k = 0usize;
    while k < REJECT {
        let code = match REJECT_CODES.get(k) {
            Some(v) => *v,
            None => 0,
        };
        let d = Sample::defect((k as u8).saturating_add(9), code);
        let cell = Sample {
            slot: (8 + k) as u8,
            op: d.op,
            ok: d.ok,
            code: d.code,
            value: d.value,
        };
        let _ = l.register(cell);
        k += 1;
    }
    l
}

/// 四路独立自检：表路（语料与交叉表）/ 判路（判序与缺陷维）/ 渲染路（逐字文案
/// 与渲染文本）/ 账本路（建账、审计、封印、重放）。
pub fn inject_four() -> [bool; 4] {
    [
        table_route(),
        judge_route(),
        render_route(),
        ledger_route(),
    ]
}

/// 表路：交叉表与实测逐槽对齐。
fn table_route() -> bool {
    let mut i = 0usize;
    while i < CORPUS {
        let o = match outcome_at(i) {
            Some(v) => v,
            None => return false,
        };
        let c = match case_at(i) {
            Some(v) => v,
            None => return false,
        };
        if let Some(w) = WANT_STAGE.get(i) {
            if *w != o.stage {
                return false;
            }
        }
        if let Some(w) = WANT_CODE.get(i) {
            if *w != o.code {
                return false;
            }
        }
        if let Some(w) = WANT_NOTE.get(i) {
            if *w != o.note {
                return false;
            }
        }
        if let Some(w) = WANT_DEFAULT.get(i) {
            if *w != o.default {
                return false;
            }
        }
        if let Some(w) = WANT_OK.get(i) {
            if *w != o.ok {
                return false;
            }
        }
        if let Some(w) = WANT_TAGS.get(i) {
            if *w != c.shape.tag {
                return false;
            }
        }
        if let Some(w) = WANT_NDX.get(i) {
            let full = if o.value == o.len && o.ok { 1u8 } else { 0u8 };
            if *w != full {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 判路：每槽判序命中期望缺陷维，且死槽只坏一维。
fn judge_route() -> bool {
    let mut i = 0usize;
    while i < CORPUS {
        let c = match case_at(i) {
            Some(v) => v,
            None => return false,
        };
        if judge(&c) != expected_fault(i) {
            return false;
        }
        if i >= ACCEPT {
            if let Some(f) = CLAIM_BROKEN.get(i) {
                if judge(&c) != *f {
                    return false;
                }
            }
        }
        i += 1;
    }
    true
}

/// 渲染路：两份采集件文本非空、可对齐，且报告文本逐字命中钉住的常量。
fn render_route() -> bool {
    let mut i = 0usize;
    while i < CORPUS {
        if let Some(c) = case_at(i) {
            let a = msg_render(&c.shape, Machine::Host);
            let b = msg_render(&c.shape, Machine::Target);
            if a.len() == 0 || b.len() == 0 {
                return false;
            }
            let d = diagnostic(&c);
            if d.len() == 0 {
                return false;
            }
        } else {
            return false;
        }
        i += 1;
    }
    match run() {
        Ok(r) => r.render() == RENDER_OK && r.green() && r.gate() == Ok(()),
        Err(_) => false,
    }
}

/// 账本路：建账 → 审计 → 封印冻结 → 重放一致。
fn ledger_route() -> bool {
    let mut l = build_ledger();
    if l.audit() != Ok(()) {
        return false;
    }
    if l.len() != SAMPLES || l.ok_count() != ACCEPT || l.err_count() != REJECT {
        return false;
    }
    match l.freeze() {
        Ok(r) => r != 0 && l.is_frozen() && l.replay() == Ok(r) && l.root() == r,
        Err(_) => false,
    }
}

/// 申报与实测之间不符的维数。死槽必须恰好 1，活槽必须恰好 0——一槽坏两维会让
/// 判序在前的那维遮蔽后面的那维，缺陷维覆盖就失真。
pub fn mismatch_dims(c: &Case) -> usize {
    let a = define(&c.shape);
    let mut n = 0usize;
    if a.ok() != c.claim.success {
        n = n.saturating_add(1);
    }
    if a.stage != c.claim.stage {
        n = n.saturating_add(1);
    }
    if a.code != c.claim.code {
        n = n.saturating_add(1);
    }
    if a.value != c.claim.value {
        n = n.saturating_add(1);
    }
    if a.note != c.claim.note {
        n = n.saturating_add(1);
    }
    if a.default != c.claim.default {
        n = n.saturating_add(1);
    }
    n
}

/// 判据一：段基址、十三码、规模常量与名称。
fn name_ok() -> bool {
    if SEG_BASE != 0x6cf40 {
        return false;
    }
    let codes = [
        VM_ESTATE,
        VM_ESTAGE,
        VM_ECODE,
        VM_EVALUE,
        VM_ENOTE,
        VM_EDEFAULT,
        VM_EKTAG,
        VM_EQUOTE,
        VM_ELANG,
        VM_ELBL,
        VM_EBODY,
        VM_ESEAL,
        ESLOT,
    ];
    if codes.len() != 13 {
        return false;
    }
    let mut i = 0usize;
    while i < codes.len() {
        let want = match SEG_BASE.checked_add((i as u32) + 1) {
            Some(v) => v,
            None => return false,
        };
        if let Some(got) = codes.get(i) {
            if *got != want {
                return false;
            }
        }
        let mut j = i.saturating_add(1);
        while j < codes.len() {
            if let (Some(a), Some(b)) = (codes.get(i), codes.get(j)) {
                if a == b {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    if CORPUS != 14 || SAMPLES != 14 || ACCEPT != 8 || REJECT != 6 || TIER_COUNT != 4 {
        return false;
    }
    if ANCHOR_KINDS != 2 || KIND_COUNT != 6 {
        return false;
    }
    if FNV_OFF != 0xcbf2_9ce4_8422_2325 || FNV_PRIME != 0x100_0000_01b3 {
        return false;
    }
    if ENOENT != 2 || EACCES != 13 || ENOSPC != 28 || ENONE != 0 {
        return false;
    }
    if BBT != 0x60 || BAP != 0x27 {
        return false;
    }
    if code_name(0) != "identical" || code_name(ESLOT) != "eslot" {
        return false;
    }
    // 十三个码的名字必须各不相同，否则释义面会把两码读成一码。
    let mut names = [""; 13];
    let mut k = 0usize;
    while k < 13 {
        if let Some(slot) = names.get_mut(k) {
            *slot = code_name(match SEG_BASE.checked_add((k as u32) + 1) {
                Some(v) => v,
                None => return false,
            });
        }
        k += 1;
    }
    let mut p = 0usize;
    while p < 13 {
        let mut q = p.saturating_add(1);
        while q < 13 {
            if let (Some(a), Some(b)) = (names.get(p), names.get(q)) {
                if a == b {
                    return false;
                }
            }
            q += 1;
        }
        p += 1;
    }
    true
}

/// 判据二：枚举取值、次序与纵线链条。
fn enums_ok() -> bool {
    if Machine::Host.peer() != Machine::Target || Machine::Target.peer() != Machine::Host {
        return false;
    }
    if Machine::Host.rank() == Machine::Target.rank() {
        return false;
    }
    let kinds = [
        Kind::Vfn,
        Kind::Undef,
        Kind::Open,
        Kind::Conflict,
        Kind::Dlopen,
        Kind::Preload,
    ];
    if kinds.len() != KIND_COUNT {
        return false;
    }
    let mut anchors = 0usize;
    let mut i = 0usize;
    while i < kinds.len() {
        if let Some(k) = kinds.get(i) {
            if Kind::from_rank(k.rank()) != Some(*k) {
                return false;
            }
            if k.anchor() {
                anchors = anchors.saturating_add(1);
            }
            // 版本名带引号的族与不带引号的族必须分家。
            if *k == Kind::Vfn && k.quote_rule() != "backtick-open apos-close" {
                return false;
            }
            if *k == Kind::Undef && k.quote_rule() != "bare version" {
                return false;
            }
        }
        i += 1;
    }
    if anchors != ANCHOR_KINDS {
        return false;
    }
    if Kind::from_rank(0).is_some() || Kind::from_rank(7).is_some() {
        return false;
    }
    if Kind::Vfn.needs_obj() != true || Kind::Vfn.needs_sym() != false {
        return false;
    }
    if Kind::Undef.needs_sym() != true || Kind::Undef.needs_obj() != false {
        return false;
    }
    if needs_label(Kind::Conflict) || needs_label(Kind::Dlopen) || needs_label(Kind::Preload) {
        return false;
    }
    if Quote::Tick.rank() == Quote::Apos.rank() || Quote::Apos.rank() == Quote::Mixed.rank() {
        return false;
    }
    if !Quote::Tick.legal_for_vfn() || Quote::Apos.legal_for_vfn() || Quote::Mixed.legal_for_vfn()
    {
        return false;
    }
    let drifts = [
        Drift::None,
        Drift::Quote,
        Drift::TagCase,
        Drift::Tail,
        Drift::ErrnoText,
        Drift::Locale,
        Drift::SymName,
        Drift::ObjPath,
    ];
    let mut d = 0usize;
    while d < drifts.len() {
        let mut e = d.saturating_add(1);
        while e < drifts.len() {
            if let (Some(x), Some(y)) = (drifts.get(d), drifts.get(e)) {
                if x.rank() == y.rank() {
                    return false;
                }
            }
            e += 1;
        }
        d += 1;
    }
    if !Drift::Locale.breaks_compare() || Drift::Tail.breaks_compare() {
        return false;
    }
    // 纵线：八闸逐级可达，Ready 无后继，past_compare 只对最后三段为真。
    let chain = [
        Stage::Collect,
        Stage::Classify,
        Stage::Split,
        Stage::Comparability,
        Stage::Payload,
        Stage::Compare,
        Stage::Attribute,
        Stage::Ready,
    ];
    let mut s = 0usize;
    while s < chain.len() {
        if let Some(cur) = chain.get(s) {
            if cur.rank() as usize != s {
                return false;
            }
            let nxt = cur.next();
            if s + 1 == chain.len() {
                if nxt.is_some() {
                    return false;
                }
            } else if nxt != Some(*chain.get(s + 1).unwrap_or(&Stage::Ready)) {
                return false;
            }
            if cur.past_compare() != (cur.rank() >= Stage::Compare.rank()) {
                return false;
            }
        }
        s += 1;
    }
    let fo = Fault::order();
    let mut f = 0usize;
    while f < fo.len() {
        if let Some(x) = fo.get(f) {
            if x.rank() as usize != f.saturating_add(1) {
                return false;
            }
            if x.code() == 0 && *x != Fault::Ok {
                return false;
            }
        }
        f += 1;
    }
    if fo.len() != REJECT || REJECT_FAULTS.len() != REJECT {
        return false;
    }
    let mut g = 0usize;
    while g < REJECT {
        if let (Some(a), Some(b)) = (fo.get(g), REJECT_FAULTS.get(g)) {
            if a != b {
                return false;
            }
        }
        g += 1;
    }
    let notes = [
        Note::Same,
        Note::LabelOnly,
        Note::Quote,
        Note::TagCase,
        Note::Tail,
        Note::ErrnoText,
        Note::SymName,
        Note::ObjPath,
        Note::Locale,
        Note::Body,
    ];
    if Note::Same.conflicts() || Note::LabelOnly.conflicts() || Note::Body.conflicts() {
        return false;
    }
    if !Note::Quote.conflicts() || !Note::Locale.conflicts() {
        return false;
    }
    let tiers = [
        Tier::Identical,
        Tier::LabelOnly,
        Tier::Conflict,
        Tier::Malformed,
    ];
    if tiers.len() != TIER_COUNT {
        return false;
    }
    if Tier::Malformed.good() {
        return false;
    }
    let mut t = 0usize;
    while t < tiers.len() {
        if let Some(x) = tiers.get(t) {
            if x.rank() as usize != t {
                return false;
            }
            if !x.good() && *x != Tier::Malformed {
                return false;
            }
        }
        t += 1;
    }
    // notes 表用来确保归因档互不相同（同档会让归因失去区分力）。
    let mut n1 = 0usize;
    while n1 < notes.len() {
        let mut n2 = n1.saturating_add(1);
        while n2 < notes.len() {
            if let (Some(a), Some(b)) = (notes.get(n1), notes.get(n2)) {
                if a.rank() == b.rank() {
                    return false;
                }
            }
            n2 += 1;
        }
        n1 += 1;
    }
    true
}

/// 判据三：六族文案逐字复刻——引号规则、短语有无、大小写与标点都不许差一字节。
fn message_ok() -> bool {
    if msg_vfn("./svchk", "libc.so.6", "GLIBC_2.34")
        != "./svchk: libc.so.6: version `GLIBC_2.34' not found (required by ./svchk)\n"
    {
        return false;
    }
    if msg_undef("./svchk", "bar", "GLIBC_2.34")
        != "./svchk: symbol lookup error: ./svchk: undefined symbol: bar, version GLIBC_2.34\n"
    {
        return false;
    }
    if msg_open("./svchk", "libfoo.so.1", "No such file or directory")
        != "./svchk: error while loading shared libraries: libfoo.so.1: cannot open shared object file: No such file or directory\n"
    {
        return false;
    }
    if msg_conflict("libstdc++.so.6", "/usr/lib/gcc/11/libstdc++.so")
        != "ld: warning: libstdc++.so.6, needed by /usr/lib/gcc/11/libstdc++.so, may conflict with libstdc++.so.6\n"
    {
        return false;
    }
    if msg_dlopen("libfoo.so.1", "Permission denied")
        != "libfoo.so.1: cannot open shared object file: Permission denied\n"
    {
        return false;
    }
    if msg_preload("libhook.so")
        != "ERROR: ld.so: object 'libhook.so' from LD_PRELOAD cannot be preloaded (cannot open shared object file): ignored.\n"
    {
        return false;
    }
    // Undef 族的版本名必须裸写：出现反引号即错。
    let u = msg_undef("./p", "s", "V1");
    if u.find('`').is_some() {
        return false;
    }
    // Vfn 族必须成对 TeX 式引号：反引号在前、撇号在后。
    let v = msg_vfn("./p", "l.so", "V1");
    if !v.contains("`V1'") {
        return false;
    }
    // Open 与 Dlopen 的边界：前者有 loading 短语，后者没有。
    if !msg_open("./p", "l.so", "x").contains(PH_LOAING) {
        return false;
    }
    if msg_dlopen("l.so", "x").contains(PH_LOAING) {
        return false;
    }
    // Preload 族必须大写起头且以句号收尾。
    let pr = msg_preload("l.so");
    if !pr.starts_with("ERROR: ld.so: object '") || !pr.ends_with(": ignored.\n") {
        return false;
    }
    // Conflict 族必须带 warning 前缀且不得带程序名标签。
    if !msg_conflict("a.so", "b.so").starts_with(PH_WARN) {
        return false;
    }
    if msg_conflict("a.so", "b.so").contains("error while loading") {
        return false;
    }
    true
}

/// 判据四：切片区间——标签、依赖对象、版本名、符号、strerror 各段的字节位置。
fn slices_ok() -> bool {
    let sh = match shape_at(1) {
        Some(v) => v,
        None => return false,
    };
    let (text, sl) = render_slice(&sh, Machine::Host);
    if sl.label_len != 9 {
        return false;
    }
    if sl.obj_at != 9 || sl.obj_len != 9 {
        return false;
    }
    if sl.quote_byte != BBT {
        return false;
    }
    if sl.vt_len != 10 || sl.vt_at != 29 {
        return false;
    }
    if sl.quote_at() != 28 {
        return false;
    }
    if !sl.has_vt(29) || sl.has_vt(39) || sl.has_vt(28) {
        return false;
    }
    if !sl.has_obj(9) || sl.has_obj(18) {
        return false;
    }
    if sl.sym_len != 0 {
        return false;
    }
    if !sl.trail || sl.body_len == 0 {
        return false;
    }
    // 标签差两侧：载荷长度必须相同，标签长度不同。
    let (_, sb) = render_slice(&sh, Machine::Target);
    if sb.label_len != sl.label_len {
        return false;
    }
    // Undef 槽：符号段与版本段都要能定位，且版本段在符号段之后。
    let su = match shape_at(2) {
        Some(v) => v,
        None => return false,
    };
    let (_, slu) = render_slice(&su, Machine::Host);
    if slu.sym_len != 3 || slu.vt_len != 10 {
        return false;
    }
    if slu.vt_at <= slu.sym_at {
        return false;
    }
    if slu.quote_byte != 0 {
        return false;
    }
    // Open 槽：strerror 段必须存在且落在对象段之后。
    let so = match shape_at(3) {
        Some(v) => v,
        None => return false,
    };
    let (_, slo) = render_slice(&so, Machine::Host);
    if slo.err_len == 0 || slo.err_at <= slo.obj_at {
        return false;
    }
    if !slo.has_err(slo.err_at) || slo.has_err(slo.err_at.saturating_add(slo.err_len)) {
        return false;
    }
    // 无标签族：label_len 必须为 0，载荷就是全文。
    let sc = match shape_at(4) {
        Some(v) => v,
        None => return false,
    };
    let (tc, slc) = render_slice(&sc, Machine::Host);
    if slc.label_len != 0 || slc.body_len != tc.len() {
        return false;
    }
    if text.len() <= sl.label_len {
        return false;
    }
    true
}

/// 判据五：逐字走查——前缀、长度差、多出字节、引号字节识别。
fn walk_ok() -> bool {
    let w = walk(b"abc", b"abc");
    if !w.same || w.prefix != 3 || w.length_diff() {
        return false;
    }
    let w2 = walk(b"ab", b"abc");
    if w2.same || w2.prefix != 2 || !w2.length_diff() || w2.extra_len != 1 {
        return false;
    }
    if !w2.extra_on_b || w2.extra_first != b'c' {
        return false;
    }
    let w3 = walk(b"abc", b"ab");
    if w3.extra_on_b || w3.extra_first != b'c' || w3.prefix != 2 {
        return false;
    }
    let w4 = walk(b"a`b", b"a'b");
    if w4.same || w4.prefix != 1 || w4.a_byte != BBT || w4.b_byte != BAP {
        return false;
    }
    let w5 = walk(b"", b"x");
    if w5.prefix != 0 || !w5.length_diff() || w5.ok_ready() {
        return false;
    }
    let w6 = walk(b"a\n", b"a");
    if !w6.extra_is_nl() || w6.extra_on_b {
        return false;
    }
    // 空载荷不许被判成全等就绪。
    if w.has_a() == false && w.has_b() == false && w.prefix != 3 {
        return false;
    }
    true
}

/// 判据六：八闸纵线——每闸的停驻点、码、以及"前置闸没过就不许看见后置闸"。
fn define_ok() -> bool {
    // 活槽一律走完到 Ready。
    let mut i = 0usize;
    while i < ACCEPT {
        let sh = match shape_at(i) {
            Some(v) => v,
            None => return false,
        };
        let o = define(&sh);
        if !o.ok || o.stage != Stage::Ready {
            return false;
        }
        if i < 5 {
            if o.code != 0 || !o.identical() {
                return false;
            }
        } else if o.code != VM_EVALUE || o.identical() {
            return false;
        }
        i += 1;
    }
    // 死槽按位次停在各自闸口。
    let expect: [(Stage, u32, bool); REJECT] = [
        (Stage::Comparability, VM_EQUOTE, false),
        (Stage::Split, VM_ELBL, false),
        (Stage::Classify, VM_EKTAG, false),
        (Stage::Ready, VM_EVALUE, true),
        (Stage::Comparability, VM_ELANG, false),
        (Stage::Payload, VM_EBODY, false),
    ];
    let mut k = 0usize;
    while k < REJECT {
        let sh = match shape_at(ACCEPT.saturating_add(k)) {
            Some(v) => v,
            None => return false,
        };
        let o = define(&sh);
        let (st, cd, ok) = match expect.get(k) {
            Some(v) => *v,
            None => return false,
        };
        if o.stage != st || o.code != cd || o.ok != ok {
            return false;
        }
        // 未走完对照的槽不许带偏移。
        if !st.past_compare() && o.value != 0 {
            return false;
        }
        k += 1;
    }
    // 归因与走查自洽：全等档只许 Same/LabelOnly，非全等不许落这两档。
    let mut m = 0usize;
    while m < CORPUS {
        if let Some(o) = outcome_at(m) {
            if o.stage == Stage::Ready {
                let full = o.value == o.len;
                if full != (o.note == Note::Same || o.note == Note::LabelOnly) {
                    return false;
                }
            }
            if o.code == 0 && o.note.conflicts() {
                return false;
            }
            if o.code == VM_EVALUE && !o.note.conflicts() {
                return false;
            }
        }
        m += 1;
    }
    // 空标签槽必须在第一闸就死掉。
    let mut bare = match shape_at(1) {
        Some(v) => v,
        None => return false,
    };
    bare.tag = "";
    if define(&bare).code != VM_EBODY || define(&bare).stage != Stage::Collect {
        return false;
    }
    // 漂移只作用在目标机：无漂移槽两侧必须逐字相同，有漂移槽必须只在目标机显形。
    let d5 = match shape_at(5) {
        Some(v) => v,
        None => return false,
    };
    let d1 = match shape_at(1) {
        Some(v) => v,
        None => return false,
    };
    if msg_render(&d5, Machine::Host) == msg_render(&d5, Machine::Target) {
        return false;
    }
    if msg_render(&d1, Machine::Host) != msg_render(&d1, Machine::Target) {
        return false;
    }
    if msg_body(&d5, Machine::Host).len() == 0 {
        return false;
    }
    true
}

/// 判据七：联签规则——只有 `Vfn`/`Undef` 两族占主锚位，旁证族全等也不给位。
fn anchor_ok() -> bool {
    let mut i = 0usize;
    while i < CORPUS {
        let sh = match shape_at(i) {
            Some(v) => v,
            None => return false,
        };
        let o = match outcome_at(i) {
            Some(v) => v,
            None => return false,
        };
        let want = sh.kind.anchor() && o.ok && o.stage == Stage::Ready;
        if o.default != want {
            return false;
        }
        if !sh.kind.anchor() && o.default {
            return false;
        }
        i += 1;
    }
    if anchored_count() != 6 {
        return false;
    }
    // 旁证族里必须有全等但不联签的槽，否则这一维是空转。
    let mut non_anchor_identical = 0usize;
    let mut j = 0usize;
    while j < CORPUS {
        if let (Some(sh), Some(o)) = (shape_at(j), outcome_at(j)) {
            if !sh.kind.anchor() && o.ok && o.identical() {
                non_anchor_identical = non_anchor_identical.saturating_add(1);
            }
        }
        j += 1;
    }
    if non_anchor_identical < 2 {
        return false;
    }
    true
}

/// 判据八：判序与"只坏一维"。
fn judge_ok() -> bool {
    let mut i = 0usize;
    while i < CORPUS {
        let c = match case_at(i) {
            Some(v) => v,
            None => return false,
        };
        let got = judge(&c);
        if got != expected_fault(i) {
            return false;
        }
        let dims = mismatch_dims(&c);
        if i < ACCEPT {
            if dims != 0 || got != Fault::Ok {
                return false;
            }
        } else if dims != 1 {
            return false;
        }
        i += 1;
    }
    // REJECT_CODES 必须互不相同，且逐格与死槽实测码对齐。
    let mut k = 0usize;
    while k < REJECT {
        let o = match outcome_at(ACCEPT.saturating_add(k)) {
            Some(v) => v,
            None => return false,
        };
        if let Some(want) = REJECT_CODES.get(k) {
            if o.code != *want {
                return false;
            }
        }
        let mut m = k.saturating_add(1);
        while m < REJECT {
            if let (Some(a), Some(b)) = (REJECT_CODES.get(k), REJECT_CODES.get(m)) {
                if a == b {
                    return false;
                }
            }
            m += 1;
        }
        k += 1;
    }
    // 状态维优先：一槽同时错报状态与阶段，必须判 Estate 而不是 Stage。
    // 第 8 槽本身已谎报成败，再把阶段也推到 Ready，两维同坏，判序仍须落在 Estate。
    if let Some(c) = case_at(8) {
        let probe = Case {
            shape: c.shape,
            claim: Claim::break_dim(c.claim, Fault::Stage),
        };
        if probe.claim.success == define(&probe.shape).ok {
            return false;
        }
        if judge(&probe) != Fault::Estate {
            return false;
        }
    }
    // 码表与缺陷维一一对应。
    let mut f = 0usize;
    while f < REJECT {
        if let Some(x) = REJECT_FAULTS.get(f) {
            if x.code() == 0 {
                return false;
            }
        }
        f += 1;
    }
    true
}

/// 判据九：矩阵四档——计数独立、覆盖完备、CONFLICT 全显式。
fn matrix_ok() -> bool {
    let mx = Matrix::build();
    if !mx.covered() || mx.total != CORPUS {
        return false;
    }
    if mx.perfect_rows() != 4 || mx.label_rows() != 1 || mx.conflict_rows() != 4 {
        return false;
    }
    if mx.malformed_rows() != 5 {
        return false;
    }
    if !mx.conflict_explicit() {
        return false;
    }
    if mx.anchored() != 6 {
        return false;
    }
    // 三个可盖章档的 total 与 good 必须相等；malformed 档必须全不 good。
    let good_tiers = [Tier::Identical, Tier::LabelOnly, Tier::Conflict];
    let mut i = 0usize;
    while i < good_tiers.len() {
        if let Some(t) = good_tiers.get(i) {
            if let Some(r) = mx.row(*t) {
                if r.total != r.good || !r.is_green() {
                    return false;
                }
            } else {
                return false;
            }
        }
        i += 1;
    }
    if let Some(r) = mx.row(Tier::Malformed) {
        if r.good != 0 || r.is_green() {
            return false;
        }
    } else {
        return false;
    }
    // 行文本必须带档位名，渲染面不许出现空行。
    if let Some(r) = mx.row(Tier::Conflict) {
        if !r.text().contains("conflict total=") {
            return false;
        }
    }
    if mx.render().len() < 40 || !mx.render().starts_with("matrix") {
        return false;
    }
    // 档位分派只看实测：把一行的申报改掉不影响分档。
    if let (Some(o), Some(c)) = (outcome_at(11), case_at(11)) {
        if Tier::of(&o) != Tier::Conflict {
            return false;
        }
        let mut changed = c;
        changed.claim.value = changed.claim.value.saturating_add(5);
        if Tier::of(&define(&changed.shape)) != Tier::Conflict {
            return false;
        }
    }
    true
}

/// 判据十：语料规模、交叉表与全绿渲染。
fn corpus_ok() -> bool {
    if SHAPES.len() != CORPUS {
        return false;
    }
    if case_at(CORPUS).is_some() {
        return false;
    }
    if case_at(0).is_none() {
        return false;
    }
    if live_count() != ACCEPT || dead_count() != REJECT {
        return false;
    }
    if identical_count() != 4 {
        return false;
    }
    // 交叉表与实测逐槽对齐（表路与判路互验）。
    if !table_route() {
        return false;
    }
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if !r.green() || r.gate() != Ok(()) {
        return false;
    }
    if r.render() != RENDER_OK {
        return false;
    }
    if r.rate() != 1000 {
        return false;
    }
    // 标签唯一性：两槽同标签会让诊断行无法定位。
    let mut i = 0usize;
    while i < CORPUS {
        let mut j = i.saturating_add(1);
        while j < CORPUS {
            if let (Some(a), Some(b)) = (shape_at(i), shape_at(j)) {
                if a.tag == b.tag {
                    return false;
                }
                if a.kind == b.kind && a.drift == b.drift && a.tag == b.tag {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    // VALS 的前七格必须与实测统计逐格相等。
    let counts = tier_counts();
    let mx = Matrix::build();
    let stat: [u32; 7] = [
        CORPUS as u32,
        live_count() as u32,
        dead_count() as u32,
        match counts.get(Tier::Identical.rank() as usize) {
            Some(v) => *v as u32,
            None => 0,
        },
        mx.label_rows() as u32,
        mx.conflict_rows() as u32,
        mx.anchored() as u32,
    ];
    let mut k = 0usize;
    while k < 7 {
        if let (Some(got), Some(want)) = (VALS.get(k), stat.get(k)) {
            if got != want {
                return false;
            }
        }
        k += 1;
    }
    true
}

/// 判据十一（其一）：红档率单独占一格，注入一槽即掉到 929。
fn reds_ok() -> bool {
    let clean = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let red = match run_with(Some((0, Fault::Code))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if clean.rate() != 1000 {
        return false;
    }
    if red.rate() != 929 {
        return false;
    }
    if red.matched != 13 || red.total != 14 {
        return false;
    }
    if red.green() {
        return false;
    }
    if red.gate() != Err(VM_ESTATE) {
        return false;
    }
    if let Some(last) = VALS.last() {
        if *last != 929 {
            return false;
        }
    } else {
        return false;
    }
    let mut i = 0usize;
    while i < 7 {
        if let Some(v) = VALS.get(i) {
            if *v == 929 {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 判据十一（其二）：槽位越界与跳号登记都被拒且计数。
fn slots_ok() -> bool {
    let mut l = Ledger::new();
    if !l.is_empty() || l.len() != 0 || l.rejects() != 0 {
        return false;
    }
    // 跳号登记：槽位写 5 但账长为 0，必须拒。
    let bad = Sample {
        slot: 5,
        op: 1,
        ok: true,
        code: 0,
        value: 14,
    };
    if l.register(bad) != Err(ESLOT) || l.len() != 0 || l.rejects() != 1 {
        return false;
    }
    let good = Sample {
        slot: 0,
        op: 1,
        ok: true,
        code: 0,
        value: 14,
    };
    if l.register(good) != Ok(()) || l.len() != 1 {
        return false;
    }
    if l.sample(1).is_some() || l.sample(0) != Some(good) {
        return false;
    }
    // 越界：填满之后再多登一格必须被拒。
    let mut full = build_ledger();
    if full.len() != SAMPLES {
        return false;
    }
    let extra = Sample {
        slot: SAMPLES as u8,
        op: 1,
        ok: true,
        code: 0,
        value: 0,
    };
    if full.register(extra) != Err(ESLOT) || full.len() != SAMPLES {
        return false;
    }
    // 冻结后一律拒登并计数。
    let sealed = full.seal();
    if full.freeze() != Ok(sealed) {
        return false;
    }
    let r0 = full.rejects();
    if full.register(extra) != Err(VM_ESTATE) || full.rejects() != r0.saturating_add(1) {
        return false;
    }
    if run_with(Some((CORPUS, Fault::Ok))).is_ok() {
        return false;
    }
    true
}

/// 判据十二（其一）：释义行与诊断行——每槽都能把两份采集件摊开。
fn detail_ok() -> bool {
    let mut i = 0usize;
    while i < CORPUS {
        let c = match case_at(i) {
            Some(v) => v,
            None => return false,
        };
        let d = detail(&c);
        if d.len() < 8 || !d.contains(c.shape.tag) {
            return false;
        }
        if !d.contains("prefix=") || !d.contains("anchor=") {
            return false;
        }
        let dg = diagnostic(&c);
        if !dg.starts_with("[host]") || !dg.contains("[target]") {
            return false;
        }
        i += 1;
    }
    // 码表必须覆盖全部十三码，且 unknown 只留给表外值。
    let mut k = 1usize;
    while k <= 13 {
        let code = match SEG_BASE.checked_add(k as u32) {
            Some(v) => v,
            None => return false,
        };
        if code_name(code) == "unknown" {
            return false;
        }
        k += 1;
    }
    if code_name(SEG_BASE.saturating_add(99)) != "unknown" {
        return false;
    }
    if fault_text(Fault::Ok).len() == 0 || fault_text(Fault::Default).len() == 0 {
        return false;
    }
    // 数字文本自洽。
    if dec_text(0) != "0" || dec_text(929) != "929" || dec_text(4294967295) != "4294967295" {
        return false;
    }
    if hex_text(0x6cf40, 5) != "6cf40" || hex_text(1, 4) != "0001" {
        return false;
    }
    if i32_text(-12) != "-12" || i32_text(12) != "12" {
        return false;
    }
    if !bool_text(true).contains("true") || bool_text(false).contains("true") {
        return false;
    }
    if u32_text(14) != "14" {
        return false;
    }
    // 指纹：折序不同则指纹不同，终止字节防拼接歧义。
    let h1 = fold_str(fold_str(FNV_OFF, "a"), "bc");
    let h2 = fold_str(fold_str(FNV_OFF, "ab"), "c");
    if h1 == h2 {
        return false;
    }
    if fold_word(FNV_OFF, 1) == FNV_OFF {
        return false;
    }
    true
}

/// 判据十二（其二）：账本——逐格审计、封印、冻结、重放。
fn ledger_ok() -> bool {
    let l = build_ledger();
    if l.len() != SAMPLES {
        return false;
    }
    if l.ok_count() != ACCEPT || l.err_count() != REJECT {
        return false;
    }
    if l.audit() != Ok(()) {
        return false;
    }
    if l.rejects() != 0 || l.is_frozen() || l.root() != 0 {
        return false;
    }
    // 每格槽位等于序号，且 op 覆盖 1..14。
    let mut seen = [false; 15];
    let mut i = 0usize;
    while i < l.len() {
        let s = match l.sample(i) {
            Some(v) => v,
            None => return false,
        };
        if (s.slot as usize) != i {
            return false;
        }
        let op = s.op as usize;
        if op == 0 || op > 14 {
            return false;
        }
        if let Some(slot) = seen.get_mut(op) {
            if *slot {
                return false;
            }
            *slot = true;
        }
        i += 1;
    }
    if !seen[8] {
        return false;
    }
    // 红档格的值必须等于注入一槽后的实测一致率。
    let red_cell = match l.sample(7) {
        Some(v) => v,
        None => return false,
    };
    let red = match run_with(Some((0, Fault::Code))) {
        Ok(v) => v.rate(),
        Err(_) => return false,
    };
    if red_cell.value != red || red_cell.value != 929 {
        return false;
    }
    if red_cell.op != 8 || !red_cell.ok {
        return false;
    }
    // 缺陷码六格逐格对齐 REJECT_CODES。
    let mut k = 0usize;
    while k < REJECT {
        if let Some(s) = l.sample((8usize).saturating_add(k)) {
            if s.ok {
                return false;
            }
            if s.value != 0 {
                return false;
            }
            if let Some(want) = REJECT_CODES.get(k) {
                if s.code != *want {
                    return false;
                }
            }
            if s.op != (k as u8).saturating_add(9) {
                return false;
            }
        } else {
            return false;
        }
        k += 1;
    }
    // 篡改一格后审计必须露红。
    let mut tampered = build_ledger();
    if let Some(x) = tampered.items.get_mut(3) {
        x.value = x.value.saturating_add(1);
    }
    if tampered.audit() != Err(VM_EVALUE) {
        return false;
    }
    // 封印折叠全账：长度不同的账本指纹不同。
    let a = build_ledger();
    let mut b = build_ledger();
    let root_a = a.seal();
    if root_a == 0 {
        return false;
    }
    if b.freeze() != Ok(root_a) || b.replay() != Ok(root_a) {
        return false;
    }
    if b.root() != root_a || !b.is_frozen() {
        return false;
    }
    if b.freeze() != Err(VM_ESTATE) {
        return false;
    }
    if b.render().len() < 20 {
        return false;
    }
    let mut short = Ledger::new();
    if let Some(x) = short.items.get_mut(0) {
        *x = Sample::stat(1, CORPUS as u32);
    }
    if short.register(Sample {
        slot: 0,
        op: 1,
        ok: true,
        code: 0,
        value: CORPUS as u32,
    }) != Ok(())
    {
        return false;
    }
    if short.audit() == Ok(()) {
        return false;
    }
    if short.seal() == root_a {
        return false;
    }
    true
}

/// 判据十二（其三）：阶段闸门——单调、跳过即死、关闭后一切被拒。
fn gate_ok() -> bool {
    let mut g = Gate::new();
    if g.stage() != Stage::Collect || g.closed() || g.denied() != 0 {
        return false;
    }
    if g.step(Stage::Collect) != Ok(()) {
        return false;
    }
    // 跳过一闸：从 Classify 直接跳 Split 之后的 Comparability，判阶段码。
    if g.step(Stage::Classify) != Ok(()) {
        return false;
    }
    if g.step(Stage::Comparability) != Err(VM_ESTAGE) || g.stage() != Stage::Classify {
        return false;
    }
    if g.denied() != 1 {
        return false;
    }
    // 倒退判阶段码。
    if g.step(Stage::Collect) != Err(VM_ESTAGE) {
        return false;
    }
    if g.walk_all() != Ok(()) {
        return false;
    }
    if g.stage() != Stage::Ready || !g.closed() {
        return false;
    }
    // 关闭之后任何动作都判状态码。
    if g.step(Stage::Ready) != Err(VM_ESTATE) {
        return false;
    }
    if g.close() != Err(VM_ESTATE) {
        return false;
    }
    if g.denied() < 3 {
        return false;
    }
    // 没走到 Ready 不许关闭。
    let mut early = Gate::new();
    if early.close() != Err(VM_ESTAGE) {
        return false;
    }
    if early.step(Stage::Classify) != Ok(()) || early.close() != Err(VM_ESTAGE) {
        return false;
    }
    if Gate::default().stage() != Stage::Collect {
        return false;
    }
    true
}

/// 本卡判据入口：十三条，一条不多一条不少。
///
/// 次序即纵线：先验名称与码表，再验枚举与闸门语义，然后逐层验文案、切片、走查、
/// 纵线、联签、判序、矩阵、语料、红档，最后落到释义/账本/闸门与冻结重放。
pub fn run_unx11659_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11659_vermsg_two_machine");

    s.add("VM01-名称与段基址", name_ok(), "static");

    s.add("VM02-枚举与纵线", enums_ok(), "static");

    s.add("VM03-文案族模板", message_ok(), "static");

    s.add("VM04-切片与段区间", slices_ok(), "static");

    s.add("VM05-逐字走查", walk_ok(), "static");

    s.add("VM06-八闸纵线", define_ok(), "static");

    s.add("VM07-联签规则", anchor_ok(), "static");

    s.add("VM08-申报对判", judge_ok(), "static");

    s.add("VM09-矩阵档位", matrix_ok(), "static");

    s.add("VM10-语料与渲染", corpus_ok(), "static");

    s.add("VM11-红档显式", reds_ok() && slots_ok(), "static");

    s.add("VM12-释义与账本", detail_ok() && ledger_ok() && gate_ok(), "static");

    s.add(
        "VM13-冻结重放四注入",
        {
            let mut l = build_ledger();
            let a = l.audit() == Ok(());
            let b = match l.freeze() {
                Ok(r) => r != 0 && l.is_frozen() && l.replay() == Ok(r),
                Err(_) => false,
            };
            let inj = inject_four();
            a && b && inj[0] && inj[1] && inj[2] && inj[3]
        },
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_and_codes() {
        assert_eq!(SEG_BASE, 0x6cf40);
        assert_eq!(VM_ESTATE, SEG_BASE + 1);
        assert_eq!(ESLOT, SEG_BASE + 13);
        assert!(name_ok());
    }

    #[test]
    fn scale_constants() {
        assert_eq!(CORPUS, 14);
        assert_eq!(SAMPLES, 14);
        assert_eq!(ACCEPT, 8);
        assert_eq!(REJECT, 6);
        assert_eq!(TIER_COUNT, 4);
        assert_eq!(ANCHOR_KINDS, 2);
    }

    #[test]
    fn vals_eighth_is_red_rate() {
        assert_eq!(VALS.len(), ACCEPT);
        assert_eq!(VALS[7], 929);
        for v in VALS.iter().take(7) {
            assert_ne!(*v, 929);
        }
    }

    #[test]
    fn machine_peer_is_involution() {
        assert_eq!(Machine::Host.peer(), Machine::Target);
        assert_eq!(Machine::Target.peer().peer(), Machine::Target);
        assert_eq!(Machine::Host.text(), "host");
    }

    #[test]
    fn kind_roundtrip_and_anchors() {
        for r in 1..=6u8 {
            let k = Kind::from_rank(r);
            assert!(k.is_some());
            assert_eq!(k.map(|x| x.rank()), Some(r));
        }
        assert!(Kind::from_rank(0).is_none());
        assert!(Kind::from_rank(7).is_none());
        assert!(Kind::Vfn.anchor());
        assert!(Kind::Undef.anchor());
        assert!(!Kind::Open.anchor());
        assert!(!Kind::Dlopen.anchor());
        assert!(!Kind::Preload.anchor());
        assert!(!Kind::Conflict.anchor());
    }

    #[test]
    fn kind_segment_requirements() {
        assert!(Kind::Vfn.needs_obj());
        assert!(Kind::Open.needs_obj());
        assert!(!Kind::Undef.needs_obj());
        assert!(Kind::Undef.needs_sym());
        assert!(Kind::Preload.needs_sym());
        assert!(needs_label(Kind::Vfn));
        assert!(!needs_label(Kind::Conflict));
        assert!(!needs_label(Kind::Dlopen));
    }

    #[test]
    fn quote_rules_are_distinct_per_family() {
        assert_eq!(Kind::Vfn.quote_rule(), "backtick-open apos-close");
        assert_eq!(Kind::Undef.quote_rule(), "bare version");
        assert_eq!(Kind::Preload.quote_rule(), "paired apos");
        assert!(Quote::Tick.legal_for_vfn());
        assert!(!Quote::Apos.legal_for_vfn());
        assert!(!Quote::Mixed.legal_for_vfn());
    }

    #[test]
    fn vfn_template_is_verbatim() {
        let t = msg_vfn("./svchk", "libc.so.6", "GLIBC_2.34");
        assert_eq!(
            t,
            "./svchk: libc.so.6: version `GLIBC_2.34' not found (required by ./svchk)\n"
        );
    }

    #[test]
    fn undef_template_keeps_version_bare() {
        let t = msg_undef("./svchk", "bar", "GLIBC_2.34");
        assert_eq!(
            t,
            "./svchk: symbol lookup error: ./svchk: undefined symbol: bar, version GLIBC_2.34\n"
        );
        assert!(!t.contains('`'));
    }

    #[test]
    fn open_and_dlopen_differ_by_loading_phrase() {
        let o = msg_open("./svchk", "libfoo.so.1", "No such file or directory");
        let d = msg_dlopen("libfoo.so.1", "No such file or directory");
        assert!(o.contains("error while loading shared libraries: "));
        assert!(!d.contains("error while loading shared libraries"));
        assert!(d.starts_with("libfoo.so.1: cannot open shared object file: "));
    }

    #[test]
    fn conflict_and_preload_templates() {
        assert_eq!(
            msg_conflict("libstdc++.so.6", "/usr/lib/gcc/11/libstdc++.so"),
            "ld: warning: libstdc++.so.6, needed by /usr/lib/gcc/11/libstdc++.so, may conflict with libstdc++.so.6\n"
        );
        assert_eq!(
            msg_preload("libhook.so"),
            "ERROR: ld.so: object 'libhook.so' from LD_PRELOAD cannot be preloaded (cannot open shared object file): ignored.\n"
        );
    }

    #[test]
    fn message_predicate_holds() {
        assert!(message_ok());
    }

    #[test]
    fn stage_chain_is_linear() {
        let mut cur = Stage::Collect;
        let mut steps = 0;
        while let Some(n) = cur.next() {
            assert_eq!(n.rank(), cur.rank() + 1);
            cur = n;
            steps += 1;
        }
        assert_eq!(steps, 7);
        assert_eq!(cur, Stage::Ready);
    }

    #[test]
    fn past_compare_only_for_tail_stages() {
        assert!(!Stage::Payload.past_compare());
        assert!(Stage::Compare.past_compare());
        assert!(Stage::Attribute.past_compare());
        assert!(Stage::Ready.past_compare());
    }

    #[test]
    fn drift_ranks_unique() {
        let all = [
            Drift::None,
            Drift::Quote,
            Drift::TagCase,
            Drift::Tail,
            Drift::ErrnoText,
            Drift::Locale,
            Drift::SymName,
            Drift::ObjPath,
        ];
        let mut seen = 0u32;
        for d in all.iter() {
            let bit = 1u32 << d.rank();
            assert_eq!(seen & bit, 0);
            seen |= bit;
        }
        assert!(Drift::Locale.breaks_compare());
        assert!(!Drift::ObjPath.breaks_compare());
    }

    #[test]
    fn note_conflicts_partition() {
        assert!(!Note::Same.conflicts());
        assert!(!Note::LabelOnly.conflicts());
        assert!(!Note::Body.conflicts());
        assert!(Note::Quote.conflicts());
        assert!(Note::TagCase.conflicts());
        assert!(Note::Tail.conflicts());
        assert!(Note::ErrnoText.conflicts());
        assert!(Note::SymName.conflicts());
        assert!(Note::ObjPath.conflicts());
        assert!(Note::Locale.conflicts());
    }

    #[test]
    fn slice_offsets_for_vfn() {
        let sh = shape_at(1).unwrap();
        let (text, sl) = render_slice(&sh, Machine::Host);
        assert_eq!(sl.label_len, 9);
        assert_eq!(sl.obj_at, 9);
        assert_eq!(sl.obj_len, 9);
        assert_eq!(sl.vt_at, 29);
        assert_eq!(sl.vt_len, 10);
        assert_eq!(sl.quote_byte, BBT);
        assert_eq!(text.as_bytes()[28], BBT);
        assert!(sl.trail);
    }

    #[test]
    fn slice_offsets_for_undef_and_open() {
        let su = shape_at(2).unwrap();
        let (_, slu) = render_slice(&su, Machine::Host);
        assert_eq!(slu.sym_len, 3);
        assert_eq!(slu.vt_len, 10);
        assert!(slu.vt_at > slu.sym_at);
        assert_eq!(slu.quote_byte, 0);
        let so = shape_at(3).unwrap();
        let (_, slo) = render_slice(&so, Machine::Host);
        assert!(slo.err_len > 0);
        assert!(slo.err_at > slo.obj_at);
    }

    #[test]
    fn no_label_families_slice_from_zero() {
        for i in [4usize, 7usize] {
            let sh = shape_at(i).unwrap();
            let (t, sl) = render_slice(&sh, Machine::Host);
            assert_eq!(sl.label_len, 0);
            assert_eq!(sl.body_len, t.len());
        }
    }

    #[test]
    fn slices_predicate_holds() {
        assert!(slices_ok());
    }

    #[test]
    fn walk_detects_prefix_and_length() {
        let a = walk(b"abc", b"abc");
        assert!(a.same && a.prefix == 3 && !a.length_diff());
        let b = walk(b"abc", b"ab");
        assert!(!b.same && b.prefix == 2 && b.extra_len == 1 && !b.extra_on_b);
        assert!(b.extra_is_nl() == false);
        let c = walk(b"a\n", b"a");
        assert!(c.extra_is_nl());
    }

    #[test]
    fn walk_detects_quote_bytes() {
        let w = walk(b"a`b", b"a'b");
        assert_eq!(w.a_byte, BBT);
        assert_eq!(w.b_byte, BAP);
        assert_eq!(w.prefix, 1);
    }

    #[test]
    fn walk_predicate_holds() {
        assert!(walk_ok());
    }

    #[test]
    fn live_slots_reach_ready() {
        for i in 0..ACCEPT {
            let o = outcome_at(i).unwrap();
            assert_eq!(o.stage, Stage::Ready);
            assert!(o.ok);
        }
    }

    #[test]
    fn identical_slots_have_zero_code() {
        for i in 0..5 {
            let o = outcome_at(i).unwrap();
            assert_eq!(o.code, 0);
            assert_eq!(o.value, o.len);
        }
    }

    #[test]
    fn drift_slots_report_value_code() {
        for i in 5..ACCEPT {
            let o = outcome_at(i).unwrap();
            assert_eq!(o.code, VM_EVALUE);
            assert!(o.note.conflicts());
        }
    }

    #[test]
    fn bail_slots_stop_at_their_gates() {
        let cases = [
            (8usize, Stage::Comparability, VM_EQUOTE),
            (9usize, Stage::Split, VM_ELBL),
            (10usize, Stage::Classify, VM_EKTAG),
            (12usize, Stage::Comparability, VM_ELANG),
            (13usize, Stage::Payload, VM_EBODY),
        ];
        for (i, st, code) in cases.iter() {
            let o = outcome_at(*i).unwrap();
            assert_eq!(o.stage, *st);
            assert_eq!(o.code, *code);
            assert!(!o.ok);
            assert_eq!(o.value, 0);
        }
    }

    #[test]
    fn empty_tag_dies_at_collect() {
        let mut sh = shape_at(0).unwrap();
        sh.tag = "";
        let o = define(&sh);
        assert_eq!(o.stage, Stage::Collect);
        assert_eq!(o.code, VM_EBODY);
    }

    #[test]
    fn define_predicate_holds() {
        assert!(define_ok());
    }

    #[test]
    fn anchor_only_for_two_families() {
        for i in 0..CORPUS {
            let sh = shape_at(i).unwrap();
            let o = outcome_at(i).unwrap();
            assert_eq!(o.default, sh.kind.anchor() && o.ok && o.stage == Stage::Ready);
        }
        assert_eq!(anchored_count(), 6);
    }

    #[test]
    fn non_anchor_identical_rows_are_not_anchored() {
        let o3 = outcome_at(3).unwrap();
        assert!(o3.identical());
        assert!(!o3.default);
        let o4 = outcome_at(4).unwrap();
        assert!(o4.identical());
        assert!(!o4.default);
        assert!(anchor_ok());
    }

    #[test]
    fn label_difference_is_not_a_conflict() {
        let c = case_at(0).unwrap();
        let o = define(&c.shape);
        assert_eq!(o.note, Note::LabelOnly);
        assert_eq!(o.code, 0);
        assert_eq!(Tier::of(&o), Tier::LabelOnly);
        assert!(o.default);
    }

    #[test]
    fn judge_order_matches_expectation() {
        for i in 0..CORPUS {
            let c = case_at(i).unwrap();
            assert_eq!(judge(&c), expected_fault(i));
        }
        assert!(judge_ok());
    }

    #[test]
    fn dead_slots_break_exactly_one_dimension() {
        for i in ACCEPT..CORPUS {
            let c = case_at(i).unwrap();
            assert_eq!(mismatch_dims(&c), 1);
        }
        for i in 0..ACCEPT {
            let c = case_at(i).unwrap();
            assert_eq!(mismatch_dims(&c), 0);
        }
    }

    #[test]
    fn reject_codes_are_six_distinct() {
        for i in 0..REJECT {
            for j in (i + 1)..REJECT {
                assert_ne!(REJECT_CODES[i], REJECT_CODES[j]);
            }
            let o = outcome_at(ACCEPT + i).unwrap();
            assert_eq!(o.code, REJECT_CODES[i]);
        }
    }

    #[test]
    fn estate_wins_over_stage() {
        let c = case_at(8).unwrap();
        let probe = Case {
            shape: c.shape,
            claim: Claim::break_dim(c.claim, Fault::Stage),
        };
        assert_eq!(judge(&probe), Fault::Estate);
    }

    #[test]
    fn cross_tables_match_measurement() {
        assert!(table_route());
        for i in 0..CORPUS {
            let o = outcome_at(i).unwrap();
            assert_eq!(WANT_STAGE[i], o.stage);
            assert_eq!(WANT_CODE[i], o.code);
            assert_eq!(WANT_NOTE[i], o.note);
            assert_eq!(WANT_DEFAULT[i], o.default);
            assert_eq!(WANT_OK[i], o.ok);
            assert_eq!(WANT_TAGS[i], shape_at(i).unwrap().tag);
        }
    }

    #[test]
    fn matrix_tier_counts() {
        let mx = Matrix::build();
        assert!(mx.covered());
        assert_eq!(mx.perfect_rows(), 4);
        assert_eq!(mx.label_rows(), 1);
        assert_eq!(mx.conflict_rows(), 4);
        assert_eq!(mx.malformed_rows(), 5);
        assert_eq!(mx.anchored(), 6);
        assert_eq!(mx.total, 14);
    }

    #[test]
    fn matrix_conflicts_are_explicit() {
        let mx = Matrix::build();
        assert!(mx.conflict_explicit());
        assert!(matrix_ok());
    }

    #[test]
    fn tier_of_ignores_claim() {
        let o = outcome_at(11).unwrap();
        assert_eq!(Tier::of(&o), Tier::Conflict);
        let c = case_at(11).unwrap();
        assert_eq!(Tier::of(&define(&c.shape)), Tier::Conflict);
    }

    #[test]
    fn run_is_green_and_renders_pinned_text() {
        let r = run().unwrap();
        assert!(r.green());
        assert_eq!(r.gate(), Ok(()));
        assert_eq!(r.rate(), 1000);
        assert_eq!(r.render(), RENDER_OK);
    }

    #[test]
    fn one_injection_drops_rate_to_929() {
        let r = run_with(Some((0, Fault::Code))).unwrap();
        assert_eq!(r.matched, 13);
        assert_eq!(r.rate(), 929);
        assert!(!r.green());
        assert_eq!(r.gate(), Err(VM_ESTATE));
        assert!(reds_ok());
    }

    #[test]
    fn injection_out_of_range_is_eslot() {
        assert_eq!(run_with(Some((CORPUS, Fault::Ok))), Err(ESLOT));
        assert_eq!(shape_at(CORPUS), None);
        assert_eq!(case_at(CORPUS), None);
        assert!(case_at(0).is_some());
    }

    #[test]
    fn corpus_and_render_predicates() {
        assert!(corpus_ok());
        assert!(render_route());
    }

    #[test]
    fn gate_steps_monotonically() {
        let mut g = Gate::new();
        assert_eq!(g.step(Stage::Classify), Ok(()));
        assert_eq!(g.step(Stage::Split), Ok(()));
        assert_eq!(g.stage(), Stage::Split);
        assert_eq!(g.step(Stage::Classify), Err(VM_ESTAGE));
        assert_eq!(g.step(Stage::Ready), Err(VM_ESTAGE));
    }

    #[test]
    fn gate_close_blocks_everything() {
        let mut g = Gate::new();
        assert_eq!(g.walk_all(), Ok(()));
        assert!(g.closed());
        assert_eq!(g.step(Stage::Ready), Err(VM_ESTATE));
        assert_eq!(g.close(), Err(VM_ESTATE));
        assert!(g.denied() >= 2);
        assert!(gate_ok());
    }

    #[test]
    fn gate_cannot_close_early() {
        let mut g = Gate::new();
        assert_eq!(g.close(), Err(VM_ESTAGE));
        assert_eq!(g.step(Stage::Classify), Ok(()));
        assert_eq!(g.close(), Err(VM_ESTAGE));
    }

    #[test]
    fn ledger_registers_in_strict_order() {
        let mut l = Ledger::new();
        let bad = Sample {
            slot: 3,
            op: 1,
            ok: true,
            code: 0,
            value: 14,
        };
        assert_eq!(l.register(bad), Err(ESLOT));
        assert_eq!(l.rejects(), 1);
        assert_eq!(l.len(), 0);
        let good = Sample {
            slot: 0,
            op: 1,
            ok: true,
            code: 0,
            value: 14,
        };
        assert_eq!(l.register(good), Ok(()));
        assert_eq!(l.sample(0), Some(good));
        assert_eq!(l.sample(1), None);
    }

    #[test]
    fn ledger_shape_and_audit() {
        let mut l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), ACCEPT);
        assert_eq!(l.err_count(), REJECT);
        assert_eq!(l.rejects(), 0);
        assert_eq!(l.audit(), Ok(()));
        assert!(l.render().starts_with("ledger n=14"));
        let root = l.seal();
        assert_eq!(l.freeze(), Ok(root));
    }

    #[test]
    fn ledger_eighth_cell_is_red_rate() {
        let l = build_ledger();
        let s = l.sample(7).unwrap();
        assert_eq!(s.op, 8);
        assert_eq!(s.value, 929);
        assert!(s.ok);
        assert_eq!(s.code, 0);
    }

    #[test]
    fn ledger_defect_cells_align_codes() {
        let l = build_ledger();
        for k in 0..REJECT {
            let s = l.sample(8 + k).unwrap();
            assert!(!s.ok);
            assert_eq!(s.value, 0);
            assert_eq!(s.code, REJECT_CODES[k]);
            assert_eq!(s.op, (k as u8) + 9);
        }
    }

    #[test]
    fn tampered_ledger_fails_audit() {
        let mut l = build_ledger();
        let before = l.audit();
        assert_eq!(before, Ok(()));
        l.items[3].value += 1;
        assert_eq!(l.audit(), Err(VM_EVALUE));
    }

    #[test]
    fn frozen_ledger_refuses_and_counts() {
        let mut l = build_ledger();
        let r = l.freeze().unwrap();
        assert_ne!(r, 0);
        assert!(l.is_frozen());
        assert_eq!(l.root(), r);
        assert_eq!(l.replay(), Ok(r));
        let n = l.rejects();
        let extra = Sample {
            slot: 0,
            op: 1,
            ok: true,
            code: 0,
            value: 0,
        };
        assert_eq!(l.register(extra), Err(VM_ESTATE));
        assert_eq!(l.rejects(), n + 1);
        assert_eq!(l.freeze(), Err(VM_ESTATE));
    }

    #[test]
    fn seal_depends_on_content_and_length() {
        let a = build_ledger();
        let mut b = build_ledger();
        assert_eq!(a.seal(), b.seal());
        b.items[0].value += 1;
        assert_ne!(a.seal(), b.seal());
        let c = Ledger::new();
        assert_ne!(a.seal(), c.seal());
    }

    #[test]
    fn replay_before_freeze_is_refused() {
        let l = build_ledger();
        assert_eq!(l.replay(), Err(VM_ESTATE));
        assert_eq!(l.root(), 0);
    }

    #[test]
    fn ledger_predicate_holds() {
        assert!(ledger_ok());
        assert!(slots_ok());
    }

    #[test]
    fn detail_lines_carry_both_captures() {
        for i in 0..CORPUS {
            let c = case_at(i).unwrap();
            let d = detail(&c);
            assert!(d.contains(c.shape.tag));
            assert!(d.contains("prefix="));
            let g = diagnostic(&c);
            assert!(g.starts_with("[host]"));
            assert!(g.contains("[target]"));
        }
        assert!(detail_ok());
    }

    #[test]
    fn code_names_cover_thirteen_codes() {
        for k in 1..=13u32 {
            assert_ne!(code_name(SEG_BASE + k), "unknown");
        }
        assert_eq!(code_name(0), "identical");
        assert_eq!(code_name(SEG_BASE + 100), "unknown");
    }

    #[test]
    fn number_helpers() {
        assert_eq!(dec_text(0), "0");
        assert_eq!(dec_text(929), "929");
        assert_eq!(dec_text(1000), "1000");
        assert_eq!(hex_text(0x6cf40, 5), "6cf40");
        assert_eq!(hex_text(255, 4), "00ff");
        assert_eq!(i32_text(-1), "-1");
        assert_eq!(u32_text(14), "14");
        assert!(bool_text(true) == "true" && bool_text(false) == "false");
    }

    #[test]
    fn fnv_is_order_sensitive() {
        let h1 = fold_str(fold_str(FNV_OFF, "ab"), "c");
        let h2 = fold_str(fold_str(FNV_OFF, "a"), "bc");
        assert_ne!(h1, h2);
        assert_ne!(fold_word(FNV_OFF, 1), FNV_OFF);
        assert_eq!(fold_byte(FNV_OFF, 0), FNV_OFF.wrapping_mul(FNV_PRIME));
    }

    #[test]
    fn outcome_fold_separates_machines() {
        let o = outcome_at(0).unwrap();
        let h1 = o.fold(FNV_OFF);
        let h2 = o.fold(fold_str(FNV_OFF, "target"));
        assert_ne!(h1, h2);
        let p = outcome_at(1).unwrap();
        assert_ne!(o.fold(FNV_OFF), p.fold(FNV_OFF));
    }

    #[test]
    fn ascii_case_helpers() {
        assert_eq!(ascii_lower(b'A'), b'a');
        assert_eq!(ascii_lower(b'1'), b'1');
        assert_eq!(ascii_upper(b'z'), b'Z');
        assert_eq!(ascii_upper(b'_'), b'_');
    }

    #[test]
    fn drift_only_touches_target_side() {
        for i in 0..CORPUS {
            let sh = shape_at(i).unwrap();
            let (a, sa) = render_slice(&sh, Machine::Host);
            let (b, sb) = render_slice(&sh, Machine::Target);
            let pa = payload_bytes(a.as_str(), sa.label_len);
            let pb = payload_bytes(b.as_str(), sb.label_len);
            if sh.drift == Drift::None {
                assert_eq!(pa, pb);
            } else {
                assert_ne!(pa, pb);
            }
        }
    }

    #[test]
    fn quote_drift_shifts_one_byte_only() {
        let sh = shape_at(5).unwrap();
        let a = msg_render(&sh, Machine::Host);
        let b = msg_render(&sh, Machine::Target);
        assert_eq!(a.len(), b.len());
        let w = walk(payload_bytes(a.as_str(), 9), payload_bytes(b.as_str(), 9));
        assert!(!w.same);
        assert_eq!(w.a_byte, BBT);
        assert_eq!(w.b_byte, BAP);
        assert_eq!(w.prefix, 19);
    }

    #[test]
    fn inject_four_routes_all_green() {
        let v = inject_four();
        assert_eq!(v.len(), 4);
        assert!(v[0] && v[1] && v[2] && v[3]);
    }

    #[test]
    fn checks_are_thirteen_and_all_green() {
        let s = run_unx11659_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{}", s.red_items_text());
    }

    #[test]
    fn check_names_are_ordered_and_unique() {
        let s = run_unx11659_checks();
        let mut prev = String::new();
        for it in s.iter() {
            let name = it.name;
            assert!(name.starts_with("VM"));
            assert_ne!(name, prev.as_str());
            prev = String::from(name);
        }
        assert_eq!(prev, "VM13-冻结重放四注入");
    }
}




