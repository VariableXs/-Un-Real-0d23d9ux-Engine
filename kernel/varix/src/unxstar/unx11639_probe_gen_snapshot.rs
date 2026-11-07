//! UNX-F11639 坏 .so 探针发生成器与双机快照验证
//!
//! 这一卡不立任何一条"某个坏 .so 该怎么报错"的判据——那些是 F11621~F11638 十八
//! 张卡各自钉住的档。本卡钉的是**这些档是怎么被造出来、又是怎么被证明在同一条
//! 线上可复现**的：一个探针发生成器，加一套双机快照对账。
//!
//! 成器与快照为什么必须一卡承担：如果探针件是手搓的（人在两台机器上各摆一份），
//! 那么"双机一致"这句话根本没有意义——两边跑的可能压根不是同一个件，字节差在
//! 哪儿谁也说不上。只有**同种子、同算子、确定性展开**出来的字节序列，才让
//! `基准机看到的结果 == 对照机看到的结果` 成为一个可判的命题。所以本卡的诚实
//! 主干是这一句：
//!
//! > **没登记的探针不许进快照，指纹没对上的双机结果不许算作一致。**
//!
//! 探针族按 B22 的十八条档逐条对应，一条算子造一种坏件：
//!
//! ```text
//! $ ./probe-gen --op class32 --seed 0x2a17 --out /tmp/bx-sandbox/
//! wrote /tmp/bx-sandbox/p-05-class32.so (640 bytes, image=0x9c1f...)
//! $ dlopen("./p-05-class32.so", RTLD_NOW)
//! ./p-05-class32.so: wrong ELF class: ELFCLASS32
//! ```
//!
//! 双机快照则把同一条命令摆到两台宿主上（基准机 glibc 2.39 / 对照机 musl 1.2.5
//! 一线，或两台同线机做纯一致性对照），逐字比对 `dlerror()` 返回串：
//!
//! ```text
//! base : "./p-05-class32.so: wrong ELF class: ELFCLASS32"
//! second: "./p-05-class32.so: wrong ELF class: ELFCLASS32"     -> SAME
//! base : "./libtext.so: cannot protect writable segments"       (musl 拒)
//! second: 装载成功                                                -> DECLARED
//! ```
//!
//! `DECLARED` 不是"放过"，而是**登记过的双线例外**——只有 TEXTREL 策略与符号版本
//! 校验这两条被上游明确记为双线差异的算子有资格用，其余任何分歧都是 `CONFLICT`。
//!
//! ## 闸门次序（这条纵线为什么不能倒）
//!
//! 八道闸，一次只许前进一格：
//!
//! 1. **Seed**——种子合法性。零种子与被禁种子（宿主时钟派生）一律不收：种子一
//!    旦可能漂移，成器就不再是函数，双机比较失去共同前提。
//! 2. **Emit**——字节展开。算子与种子共同决定 12 字节补丁头与件长；这一步是纯
//!    函数，任何"同输入不同输出"都是本卡的红。
//! 3. **Register**——登记入账。生成即登记：算子名、件名、指纹、期望档、是否声
//!    明双线例外，五元组缺一即为"野探针"。野探针可以跑，但**结果永不进快照**。
//! 4. **Sandbox**——落盘沙盒校验。红线：探针族全部沙盒内生成。路径逃出
//!    `/tmp/bx-sandbox/` 或在宿主真实库目录里落地，一律就地判死，且**不给错误
//!    码真实档**——因为一次越界写已经污染了"被测现场"这个前提。
//! 5. **RunBase**——基准机执行。产出 `ok/errno/文案` 三元组。
//! 6. **RunSecond**——对照机执行。同一指纹件、同一 `RTLD_*` 档。
//! 7. **Compare**——逐字比对。这一步只许读两边的**文本**，不许读"两边是否都失
//!    败"：双机都返回 `NULL` 但文案不同，仍是 `CONFLICT`；两边都成功装载了一个
//!    本该拒收的坏件，是**母版档不符**，也算冲突，不是"一致"。
//! 8. **Rate**——一致率结算。分母是**已登记的族内探针数**，绝不剔除冲突格；
//!    千分比不足 1000 即整卡红。
//!
//! 次序为什么不能倒：Compare 必须在两次 Run 之后（缺一次 Run 就少一条文本，此
//! 时空文本会被读成"两边都是空串"→ 假一致）；Rate 必须在 Register 之后（未登记
//! 的件混进分母，一致率就成了自证）；Sandbox 必须在 Run 之前（越界写完再判，宿
//! 主已经被改了，快照不再是"只读观测"）。
//!
//! ## 关键差异（本卡深度所在，谁判死谁放行）
//!
//! * **与 F11621~F11638 分层**：那十八卡各立一件坏 .so 的一种拒收档；本卡一件
//!   坏 .so 都不判，只判"族"。同一个 `wrong ELF class` 文案，在 F11623 里是判
//!   据内容，在本卡里只是**快照比对的一个字段**——本卡的红是"这条档没被同一个
//!   件在两台上同时打出来"。
//! * **与 F11640（B22 批聚合）分层**：F11640 聚合的是**判据条数**（19 条通过
//!   率），本卡聚合的是**探针件数**（18 族双机一致率）。条数全绿而件数存在未声
//!   明冲突，F11640 会绿、本卡必红——因为前者只看断言自述，后者看现场字节。
//! * **与 F11677/F11699（另两面双机快照）分层**：同构的双机协议，不同的语料域；
//!   本卡管坏 .so 拒收面。三卡的 CONFLICT 语义必须一致，任何一卡把冲突折算成通
//!   过，另一卡的对照就失去信任基。
//! * **成器判死 / 快照放行**：算子不认识、种子非法、指纹漂移、越出沙盒——成器
//!   侧全部判死，不给"降级继续"。两台都跑完、文本相同或命中登记例外——快照侧
//!   一律放行，即使那条档在别的卡眼里是"报错"。本卡不评价报错对不对，只评价
//!   报错可不可复现。
//! * **例外必须成对**：声明双线例外时，两侧的期望档要同时写。只写一侧（例如只
//!   声明"musl 会拒"而不声明"glibc 会过"）视为**未声明冲突**——单侧声明是事后
//!   找补，不是登记。
//! * **红档率钉在 929**：与全批统一，`VALS` 第八格是红档率千分比 929，用来让
//!   "注入一格红"后的一致率结算立刻露出，而不是被整除掩盖。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 本卡段基址（全仓唯一，VX5 分配表给定，不许改）。
pub const SEG_BASE: u32 = 0x6d170;

// ---------------------------------------------------------------------------
// 联签锚：本卡与哪些卡共用同一条上游事实
// ---------------------------------------------------------------------------

/// B22 母版协议单源 F11203 的段基址。
pub const ANCHOR_F11203_SEG: u32 = 0x6c800;
/// F11203 与本卡之间的段差（协议层 → 探针层）。
pub const ANCHOR_F11203_GAP: u32 = 0x970;

/// 双机文本对照母版 F11659 的段基址（B23 线，符号版本文案面）。
pub const ANCHOR_F11659_SEG: u32 = 0x6d230;
/// F11659 与本卡的段差：同一双机协议，不同语料面。
pub const ANCHOR_F11659_GAP: u32 = 0xc0;

/// B22 批聚合卡 F11640 的段基址（条数面，与本卡的件数面分界）。
pub const ANCHOR_F11640_SEG: u32 = 0x6d190;
/// 聚合卡与本卡的段差。
pub const ANCHOR_F11640_GAP: u32 = 0x20;

/// 上游 errno：`ENOENT`——缺依赖与件不存在的共同落点。
pub const ANCHOR_ENOENT: i32 = 2;
/// 上游 errno：`EACCES`——目录档与权限位档的共同落点。
pub const ANCHOR_EACCES: i32 = 13;
/// 上游 errno：`EINVAL`——musl 线对结构坏件的统一回执。
pub const ANCHOR_EINVAL: i32 = 22;

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
/// 闸门数（八道）。
pub const STAGE_COUNT: u32 = 8;
/// 最末闸的秩。
pub const STAGE_LAST_RANK: u32 = 8;
/// 账本统计格数。
pub const STAT_CELLS: usize = 7;
/// 红档率千分比（全批统一钉值）。
pub const RED_RATE: u32 = 929;
/// 满一致千分比。
pub const RATE_FULL: u32 = 1000;

// ---------------------------------------------------------------------------
// 探针族规模：B22 十八条档 = 十八种破坏算子
// ---------------------------------------------------------------------------

/// 族内算子数（F11621~F11638 一一对应）。
pub const OP_COUNT: usize = 18;
/// 快照执行次数 = 算子数 × 双机。
pub const RUN_COUNT: usize = 36;
/// 登记过的双线例外算子数（TEXTREL 与符号版本校验）。
pub const DECLARED_OPS: usize = 2;
/// 补丁头字节数：成器只展开这一小段，足够让两台看到同一件。
pub const PATCH_LEN: usize = 12;
/// 探针件基准长度（未被截断算子改写时）。
pub const BASE_BYTES: u32 = 640;
/// 沙盒前缀——红线，探针全部沙盒内生成。
pub const SANDBOX_PREFIX: &str = "/tmp/bx-sandbox/";
/// 宿主真实库目录前缀——落这里就是越界。
pub const HOST_LIB_PREFIX: &str = "/usr/lib/";
/// 零种子（禁）：让成器退化成常数，快照失去意义。
pub const FORBIDDEN_SEED_ZERO: u32 = 0;
/// 时钟派生种子（禁）：同一条命令两次跑出两个件。
pub const FORBIDDEN_SEED_CLOCK: u32 = 0xc10c_7e00;
/// 禁种子表长度。
pub const FORBIDDEN_SEED_COUNT: usize = 3;
/// 第三个禁值：全 1（把 xorshift 锁死在退化轨道上）。
pub const FORBIDDEN_SEED_ONES: u32 = 0xffff_ffff;

// ---------------------------------------------------------------------------
// ELF 侧常量：与 B22 各卡同源取值，本卡不另立解释
// ---------------------------------------------------------------------------

/// `EI_NIDENT`——识别段长度。
pub const EI_NIDENT: usize = 16;
/// `ELFCLASSNONE`。
pub const ELFCLASSNONE: u8 = 0;
/// `ELFCLASS32`。
pub const ELFCLASS32: u8 = 1;
/// `ELFCLASS64`。
pub const ELFCLASS64: u8 = 2;
/// `ELFCLASSNUM`。
pub const ELFCLASSNUM: u8 = 3;
/// `EM_NONE`。
pub const EM_NONE: u16 = 0;
/// `EM_386`。
pub const EM_386: u16 = 3;
/// `EM_X86_64`——基准机机种。
pub const EM_X86_64: u16 = 62;
/// `EM_AARCH64`。
pub const EM_AARCH64: u16 = 183;
/// `EM_IA_64`。
pub const EM_IA_64: u16 = 50;
/// 魔数首字节序列 `0x7f 'E' 'L' 'F'` 的首字节。
pub const MAGIC0: u8 = 0x7f;
/// 截断件的最小可判长度：识别段都不齐。
pub const TRUNC_BELOW_IDENT: u32 = 15;
/// `RTLD_NOW`。
pub const RTLD_NOW: u32 = 2;
/// `RTLD_LAZY`。
pub const RTLD_LAZY: u32 = 1;
/// `RTLD_NOLOAD`。
pub const RTLD_NOLOAD: u32 = 4;
/// `RTLD_NODELETE`。
pub const RTLD_NODELETE: u32 = 4096;
/// dlopen 失败统一回执：空句柄。
pub const HANDLE_NULL: u32 = 0;

// ---------------------------------------------------------------------------
// 退出码与快照取值档
// ---------------------------------------------------------------------------

/// 快照成功（一致或命中登记例外）。
pub const EXIT_OK: u32 = 0;
/// 快照冲突（未声明差异）。
pub const EXIT_CONFLICT: u32 = 1;
/// 成器用法错（算子不认识/选项坏）。
pub const EXIT_USAGE: u32 = 2;
/// 未判档——本卡绝不允许把这一档当结果交付。
pub const EXIT_UNJUDGED: u32 = 99;
/// 退出码档位数。
pub const EXIT_RANKS: usize = 4;
/// 一致率满值（等价 `RATE_FULL`，矩阵侧独立复写一遍做交叉校验）。
pub const MATCH_FULL: u32 = 1000;

// ---------------------------------------------------------------------------
// 十三错误码（前缀 PG = probe generator）
// ---------------------------------------------------------------------------

/// 阶段倒退：闸门次序被颠倒。
pub const PG_ESTAGE: u32 = SEG_BASE + 0x01;
/// 档码不符：快照落点与期望档不同线。
pub const PG_ECODE: u32 = SEG_BASE + 0x02;
/// 数值不符：件长/执行数/一致数等统计量漂移。
pub const PG_EVALUE: u32 = SEG_BASE + 0x03;
/// 注记不符：文案漂移或未声明差异。
pub const PG_ENOTE: u32 = SEG_BASE + 0x04;
/// 兜底拒收：出现"未判"档。
pub const PG_EDEFAULT: u32 = SEG_BASE + 0x05;
/// 种子非法：禁种子或零种子。
pub const PG_ESEED: u32 = SEG_BASE + 0x06;
/// 状态机违规：登记后改件、关闭后再走步。
pub const PG_ESTATE: u32 = SEG_BASE + 0x07;
/// 野探针：未登记或重复登记。
pub const PG_EPROBE: u32 = SEG_BASE + 0x08;
/// 槽位越界：账本/语料槽号非法。
pub const PG_ESLOT: u32 = SEG_BASE + 0x09;
/// 指纹不符：同种子同算子两次展开得到不同件。
pub const PG_ESEAL: u32 = SEG_BASE + 0x0a;
/// 越出沙盒：探针落到沙盒外。
pub const PG_ESANDBOX: u32 = SEG_BASE + 0x0b;
/// 矩阵覆盖缺口：某一档位没有任何探针落点。
pub const PG_EMATRIX: u32 = SEG_BASE + 0x0c;
/// 一致率不足：千分比未满且无登记例外可解释。
pub const PG_ERATE: u32 = SEG_BASE + 0x0d;

/// 十三码全表，交叉校验用。
pub const ALL_CODES: [u32; 13] = [
    PG_ESTAGE,
    PG_ECODE,
    PG_EVALUE,
    PG_ENOTE,
    PG_EDEFAULT,
    PG_ESEED,
    PG_ESTATE,
    PG_EPROBE,
    PG_ESLOT,
    PG_ESEAL,
    PG_ESANDBOX,
    PG_EMATRIX,
    PG_ERATE,
];

/// 反例六维对应的码（申报对判用）。
pub const REJECT_CODES: [u32; REJECT] = [PG_ESTATE, PG_ESTAGE, PG_ECODE, PG_EVALUE, PG_ENOTE, PG_EDEFAULT];

/// 正例八格统计数：族内件数 / 登记数 / 执行数 / 逐字一致数 / 登记例外数 /
/// 未声明冲突数 / 一致率千分比 / 红档率（第八格必须是 `RED_RATE`）。
pub const VALS: [u32; ACCEPT] = [18, 18, 36, 34, 2, 0, 1000, 929];

/// 统计格对应的算子号（账本逐格复核用）。
pub const STAT_OPS: [u8; STAT_CELLS] = [1, 2, 3, 4, 5, 6, 7];

// ---------------------------------------------------------------------------
// 破坏算子号（与 BreakOp 一一对应，账本里存的是号）
// ---------------------------------------------------------------------------

/// 母版总锚档（NULL + dlerror 非空）。
pub const OPID_MASTER: u8 = 0x01;
/// 空件/截断。
pub const OPID_TRUNC: u8 = 0x02;
/// `ELFCLASS32` 件。
pub const OPID_CLASS: u8 = 0x03;
/// `EM_386` 机种。
pub const OPID_MACHINE: u8 = 0x04;
/// 魔数破坏。
pub const OPID_MAGIC: u8 = 0x05;
/// 缺依赖。
pub const OPID_NEEDED: u8 = 0x06;
/// 循环依赖。
pub const OPID_CYCLE: u8 = 0x07;
/// 版本节结构坏。
pub const OPID_VERNEED: u8 = 0x08;
/// 符号哈希表坏。
pub const OPID_HASH: u8 = 0x09;
/// `RTLD_NOW` 未定义符号。
pub const OPID_NOW: u8 = 0x0a;
/// `RTLD_LAZY` 延迟触发。
pub const OPID_LAZY: u8 = 0x0b;
/// TEXTREL 双线策略。
pub const OPID_TEXTREL: u8 = 0x0c;
/// 目录/不可读。
pub const OPID_ACCESS: u8 = 0x0d;
/// 权限位与映射。
pub const OPID_PERM: u8 = 0x0e;
/// `RTLD_NOLOAD` 探测。
pub const OPID_NOLOAD: u8 = 0x0f;
/// `dlclose` 计数归零。
pub const OPID_CLOSE: u8 = 0x10;
/// SONAME 与文件名错位。
pub const OPID_SONAME: u8 = 0x11;
/// 缺符号。
pub const OPID_SYM: u8 = 0x12;

/// 算子号全表，登记对账用。
pub const OP_IDS: [u8; OP_COUNT] = [
    OPID_MASTER,
    OPID_TRUNC,
    OPID_CLASS,
    OPID_MACHINE,
    OPID_MAGIC,
    OPID_NEEDED,
    OPID_CYCLE,
    OPID_VERNEED,
    OPID_HASH,
    OPID_NOW,
    OPID_LAZY,
    OPID_TEXTREL,
    OPID_ACCESS,
    OPID_PERM,
    OPID_NOLOAD,
    OPID_CLOSE,
    OPID_SONAME,
    OPID_SYM,
];

// ---------------------------------------------------------------------------
// FNV-1a 指纹
// ---------------------------------------------------------------------------

/// FNV-1a 初始偏移量。
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 素数。
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折一字节进哈希。
fn fold_byte(h: u64, b: u8) -> u64 {
    let x = (h ^ (b as u64)).wrapping_mul(FNV_PRIME);
    x
}

/// 折一个 u64（按字节序稳定展开，不受宿主字节序影响）。
fn fold_word(h: u64, w: u64) -> u64 {
    let mut v = w;
    let mut acc = h;
    let mut i = 0usize;
    while i < 8 {
        let b = (v & 0xff) as u8;
        acc = fold_byte(acc, b);
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
        n = n / 10;
        i -= 1;
        buf[i] = b'0' + d;
    }
    let mut s = String::new();
    while i < 10 {
        let c = buf[i];
        i += 1;
        s.push(c as char);
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

// ---------------------------------------------------------------------------
// 上游报错文案逐字复刻
//
// 这些串是本卡快照的比对对象。措辞/大小写/前缀形态一律跟随上游，不在本卡里
// 改写；改写一条，双机一致就变成"两台都看到我们改过的话"，判据自毁。
// ---------------------------------------------------------------------------

/// glibc ld.so：识别段坏——`invalid ELF header`。
pub const T_INVALID_HEADER: &str = "invalid ELF header";
/// glibc ld.so：`wrong ELF class`。
pub const T_WRONG_CLASS: &str = "wrong ELF class";
/// glibc ld.so：`wrong ELF machine`。
pub const T_WRONG_MACHINE: &str = "wrong ELF machine";
/// glibc ld.so：件/依赖打不开。
pub const T_CANNOT_OPEN: &str = "cannot open shared object file: No such file or directory";
/// glibc ld.so：权限档。
pub const T_PERM_DENIED: &str = "cannot open shared object file: Permission denied";
/// glibc ld.so：只读目录当件打开。
pub const T_IS_DIR: &str = "cannot mmap read-only segment: Is a directory";
/// glibc ld.so：版本节结构坏。
pub const T_CORRUPT_VERSION: &str = "corrupt version definitions of shared object";
/// glibc ld.so：版本要求解析失败。
pub const T_VERSION_MSG: &str = "version lookup requirement failed";
/// glibc ld.so：未定义符号。
pub const T_UNDEF_SYM: &str = "undefined symbol";
/// glibc ld.so：`RTLD_NOLOAD` 未装载。
pub const T_NOT_LOADED: &str = "object file has no shared library in scope";
/// musl：结构坏统一走 `EINVAL` 文本。
pub const T_MUSL_INVAL: &str = "Invalid argument";
/// musl：缺件走 strerror 形态。
pub const T_MUSL_NOENT: &str = "No such file or directory";
/// musl：权限档。
pub const T_MUSL_PERM: &str = "Permission denied";
/// musl：拒绝文本重定位（TEXTREL 例外的一侧）。
pub const T_MUSL_TEXTREL: &str = "cannot make segment writable for relocation";
/// glibc：接受文本重定位（例外另一侧）——装载成功，故无 dlerror。
pub const T_GLIBC_NONE: &str = "";
/// 空文本哨兵（快照里"两机都无 dlerror"的合法形态）。
pub const TEXT_EMPTY: &str = "";

/// 把件名与上游尾巴拼成一条完整 dlerror 呈现（`<名>: <尾巴>`）。
pub fn dl_text(name: &str, tail: &str) -> String {
    if tail == TEXT_EMPTY {
        return String::from(name);
    }
    let mut s = String::from(name);
    s.push_str(": ");
    s.push_str(tail);
    s
}

/// 生成器侧标准输出逐字模板：`wrote <路径> (<长度> bytes, image=0x...)`。
pub fn msg_gen_wrote(path: &str, bytes: u32, image: u64) -> String {
    let mut s = String::from("wrote ");
    s.push_str(path);
    s.push_str(" (");
    s.push_str(dec_text(bytes).as_str());
    s.push_str(" bytes, image=0x");
    s.push_str(hex_text((image & 0xffff_ffff) as u32, 8).as_str());
    s.push_str(")");
    s
}

/// 未登记探针被拒进快照的报错。
pub fn msg_unregistered(name: &str) -> String {
    let mut s = String::from("probe-gen: ");
    s.push_str(name);
    s.push_str(": probe not registered, snapshot refused");
    s
}

/// 双机看到的件指纹不符。
pub fn msg_image_drift(name: &str, a: u64, b: u64) -> String {
    let mut s = String::from("probe-gen: ");
    s.push_str(name);
    s.push_str(": image drift base=0x");
    s.push_str(hex_text((a & 0xffff_ffff) as u32, 8).as_str());
    s.push_str(" second=0x");
    s.push_str(hex_text((b & 0xffff_ffff) as u32, 8).as_str());
    s.push_str(", agreement void");
    s
}

/// 越出沙盒。
pub fn msg_escaped(path: &str) -> String {
    let mut s = String::from("probe-gen: refusing to write outside sandbox: ");
    s.push_str(path);
    s.push_str(" (probes are sandbox-only)");
    s
}

/// 种子被禁。
pub fn msg_seed_forbidden(seed: u32) -> String {
    let mut s = String::from("probe-gen: seed 0x");
    s.push_str(hex_text(seed, 8).as_str());
    s.push_str(" is forbidden: generator must be a pure function of (op, seed)");
    s
}

/// 未声明差异。
pub fn msg_conflict(name: &str, a: &str, b: &str) -> String {
    let mut s = String::from("SNAPSHOT CONFLICT ");
    s.push_str(name);
    s.push_str(" base=\"");
    s.push_str(a);
    s.push_str("\" second=\"");
    s.push_str(b);
    s.push_str("\"");
    s
}

/// 登记例外。
pub fn msg_declared(name: &str, a: &str, b: &str) -> String {
    let mut s = String::from("SNAPSHOT DECLARED ");
    s.push_str(name);
    s.push_str(" base=\"");
    s.push_str(a);
    s.push_str("\" second=\"");
    s.push_str(b);
    s.push_str("\" reason=line-policy");
    s
}

/// 单侧声明（视为未声明冲突）。
pub fn msg_half_declared(name: &str) -> String {
    let mut s = String::from("SNAPSHOT CONFLICT ");
    s.push_str(name);
    s.push_str(" exception declared on one side only");
    s
}

/// 一致率不满。
pub fn msg_rate_short(matched: u32, total: u32) -> String {
    let mut s = String::from("SNAPSHOT: agreement ");
    s.push_str(dec_text(matched).as_str());
    s.push('/');
    s.push_str(dec_text(total).as_str());
    s.push_str(" is below the 100% gate");
    s
}

/// 算子不认识。
pub fn msg_unknown_op(text: &str) -> String {
    let mut s = String::from("probe-gen: unknown break operator `");
    s.push_str(text);
    s.push_str("'; see probe-gen --list-ops");
    s
}

/// 缺操作数。
pub fn msg_missing_operand() -> String {
    String::from("probe-gen: missing operand after --op")
}

// ---------------------------------------------------------------------------
// 阶段
// ---------------------------------------------------------------------------

/// 八道闸，秩 1..=8。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// 种子合法性。
    Seed,
    /// 字节展开。
    Emit,
    /// 登记入账。
    Register,
    /// 落盘沙盒校验。
    Sandbox,
    /// 基准机执行。
    RunBase,
    /// 对照机执行。
    RunSecond,
    /// 逐字比对。
    Compare,
    /// 一致率结算。
    Rate,
}

impl Stage {
    /// 八闸全表（顺序即纵线）。
    pub fn all() -> [Stage; 8] {
        [
            Stage::Seed,
            Stage::Emit,
            Stage::Register,
            Stage::Sandbox,
            Stage::RunBase,
            Stage::RunSecond,
            Stage::Compare,
            Stage::Rate,
        ]
    }

    /// 秩（1 起）。
    pub fn rank(&self) -> u32 {
        match *self {
            Stage::Seed => 1,
            Stage::Emit => 2,
            Stage::Register => 3,
            Stage::Sandbox => 4,
            Stage::RunBase => 5,
            Stage::RunSecond => 6,
            Stage::Compare => 7,
            Stage::Rate => 8,
        }
    }

    /// 由秩构造（越界给 None，绝不夹带默认值）。
    pub fn of_rank(r: u32) -> Option<Stage> {
        match r {
            1 => Some(Stage::Seed),
            2 => Some(Stage::Emit),
            3 => Some(Stage::Register),
            4 => Some(Stage::Sandbox),
            5 => Some(Stage::RunBase),
            6 => Some(Stage::RunSecond),
            7 => Some(Stage::Compare),
            8 => Some(Stage::Rate),
            _ => None,
        }
    }

    /// 阶段名。
    pub fn name(&self) -> &'static str {
        match *self {
            Stage::Seed => "seed",
            Stage::Emit => "emit",
            Stage::Register => "register",
            Stage::Sandbox => "sandbox",
            Stage::RunBase => "run-base",
            Stage::RunSecond => "run-second",
            Stage::Compare => "compare",
            Stage::Rate => "rate",
        }
    }

    /// 阶段对应的错误落点码（倒退即挂这条）。
    pub fn fault_code(&self) -> u32 {
        match *self {
            Stage::Seed => PG_ESEED,
            Stage::Emit => PG_ESEAL,
            Stage::Register => PG_EPROBE,
            Stage::Sandbox => PG_ESANDBOX,
            Stage::RunBase => PG_ECODE,
            Stage::RunSecond => PG_EVALUE,
            Stage::Compare => PG_ENOTE,
            Stage::Rate => PG_ERATE,
        }
    }

    /// 是否在"已经不可回退"之后（Run 起就不可回退到登记）。
    pub fn is_observed(&self) -> bool {
        self.rank() >= Stage::RunBase.rank()
    }
}

// ---------------------------------------------------------------------------
// 双线
// ---------------------------------------------------------------------------

/// 宿主用的 C 库线。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Line {
    /// glibc 2.39 ld.so。
    Glibc,
    /// musl 1.2.5 ldso。
    Musl,
}

impl Line {
    /// 线名。
    pub fn label(&self) -> &'static str {
        match *self {
            Line::Glibc => "glibc",
            Line::Musl => "musl",
        }
    }

    /// 该线是否校验符号版本（musl 静默忽略版本节）。
    pub fn checks_versions(&self) -> bool {
        match *self {
            Line::Glibc => true,
            Line::Musl => false,
        }
    }

    /// 该线对文本重定位的策略：true = 允许装载。
    pub fn allows_textrel(&self) -> bool {
        match *self {
            Line::Glibc => true,
            Line::Musl => false,
        }
    }

    /// 该线结构坏时是否统一收敛到 `EINVAL` 文本。
    pub fn flat_corrupt_text(&self) -> bool {
        match *self {
            Line::Glibc => false,
            Line::Musl => true,
        }
    }
}

// ---------------------------------------------------------------------------
// 破坏算子
// ---------------------------------------------------------------------------

/// 十八种破坏算子，一一对应 B22 的十八条档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BreakOp {
    /// F11621 母版总锚：坏件返回 NULL 且 dlerror 非空。
    Master,
    /// F11622 空件/截断件。
    Truncate,
    /// F11623 `ELFCLASS32`。
    Class32,
    /// F11624 `EM_386`。
    Machine386,
    /// F11625 魔数破坏。
    BadMagic,
    /// F11626 缺 `DT_NEEDED` 目标。
    NeededMissing,
    /// F11627 循环依赖（实况：装载成功）。
    NeededCycle,
    /// F11628 版本节结构坏。
    VerneedBroken,
    /// F11629 符号哈希表坏。
    SymHashBroken,
    /// F11630 `RTLD_NOW` 撞未定义符号。
    NowUndef,
    /// F11631 `RTLD_LAZY` 延迟到调用才失败。
    LazyDefer,
    /// F11632 TEXTREL 双线策略（登记例外一）。
    Textrel,
    /// F11633 目录/不可读件。
    DirOrDenied,
    /// F11634 权限位与映射联动。
    PermBits,
    /// F11635 `RTLD_NOLOAD` 探测。
    NoLoad,
    /// F11636 `dlclose` 计数归零。
    CloseZero,
    /// F11637 SONAME 与文件名错位（实况：按路径装载不查 SONAME）。
    SonameShift,
    /// F11638 `dlsym` 缺符号。
    SymMissing,
}

impl BreakOp {
    /// 十八算子全表（族内顺序）。
    pub fn all() -> [BreakOp; OP_COUNT] {
        [
            BreakOp::Master,
            BreakOp::Truncate,
            BreakOp::Class32,
            BreakOp::Machine386,
            BreakOp::BadMagic,
            BreakOp::NeededMissing,
            BreakOp::NeededCycle,
            BreakOp::VerneedBroken,
            BreakOp::SymHashBroken,
            BreakOp::NowUndef,
            BreakOp::LazyDefer,
            BreakOp::Textrel,
            BreakOp::DirOrDenied,
            BreakOp::PermBits,
            BreakOp::NoLoad,
            BreakOp::CloseZero,
            BreakOp::SonameShift,
            BreakOp::SymMissing,
        ]
    }

    /// 命令行短名——生成器认这个，不认枚举名。
    pub fn slug(&self) -> &'static str {
        match *self {
            BreakOp::Master => "master-anchor",
            BreakOp::Truncate => "truncate",
            BreakOp::Class32 => "class32",
            BreakOp::Machine386 => "machine386",
            BreakOp::BadMagic => "bad-magic",
            BreakOp::NeededMissing => "needed-missing",
            BreakOp::NeededCycle => "needed-cycle",
            BreakOp::VerneedBroken => "verneed-corrupt",
            BreakOp::SymHashBroken => "symhash-corrupt",
            BreakOp::NowUndef => "rtld-now-undef",
            BreakOp::LazyDefer => "rtld-lazy-defer",
            BreakOp::Textrel => "textrel",
            BreakOp::DirOrDenied => "dir-or-denied",
            BreakOp::PermBits => "perm-bits",
            BreakOp::NoLoad => "rtld-noload",
            BreakOp::CloseZero => "dlclose-zero",
            BreakOp::SonameShift => "soname-shift",
            BreakOp::SymMissing => "sym-missing",
        }
    }

    /// 算子号（账本存号不存枚举）。
    pub fn id(&self) -> u8 {
        match *self {
            BreakOp::Master => OPID_MASTER,
            BreakOp::Truncate => OPID_TRUNC,
            BreakOp::Class32 => OPID_CLASS,
            BreakOp::Machine386 => OPID_MACHINE,
            BreakOp::BadMagic => OPID_MAGIC,
            BreakOp::NeededMissing => OPID_NEEDED,
            BreakOp::NeededCycle => OPID_CYCLE,
            BreakOp::VerneedBroken => OPID_VERNEED,
            BreakOp::SymHashBroken => OPID_HASH,
            BreakOp::NowUndef => OPID_NOW,
            BreakOp::LazyDefer => OPID_LAZY,
            BreakOp::Textrel => OPID_TEXTREL,
            BreakOp::DirOrDenied => OPID_ACCESS,
            BreakOp::PermBits => OPID_PERM,
            BreakOp::NoLoad => OPID_NOLOAD,
            BreakOp::CloseZero => OPID_CLOSE,
            BreakOp::SonameShift => OPID_SONAME,
            BreakOp::SymMissing => OPID_SYM,
        }
    }

    /// 由算子号还原。
    pub fn from_id(id: u8) -> Option<BreakOp> {
        let table = BreakOp::all();
        let mut i = 0usize;
        while i < OP_COUNT {
            let o = match table.get(i) {
                Some(x) => *x,
                None => return None,
            };
            if o.id() == id {
                return Some(o);
            }
            i += 1;
        }
        None
    }

    /// 族内序位（0 起）。
    pub fn index(&self) -> usize {
        let table = BreakOp::all();
        let mut i = 0usize;
        while i < OP_COUNT {
            if let Some(x) = table.get(i) {
                if x == self {
                    return i;
                }
            }
            i += 1;
        }
        OP_COUNT
    }

    /// 探针件名（沙盒内相对名，含序位与短名，便于人眼对账）。
    pub fn probe_name(&self) -> String {
        let mut s = String::from(SANDBOX_PREFIX);
        s.push_str("p-");
        let idx = self.index();
        s.push_str(dec_text(idx as u32 + 1).as_str());
        s.push('-');
        s.push_str(self.slug());
        s.push_str(".so");
        s
    }

    /// 该算子是否本该被拒收（false = 实况放行档）。
    pub fn rejects(&self) -> bool {
        match *self {
            BreakOp::NeededCycle | BreakOp::LazyDefer | BreakOp::CloseZero | BreakOp::SonameShift => false,
            _ => true,
        }
    }

    /// 该算子是否在 glibc 线装载成功（`SonameShift` 等实况档）。
    pub fn loads_on(&self, line: Line) -> bool {
        match *self {
            BreakOp::NeededCycle | BreakOp::LazyDefer | BreakOp::CloseZero | BreakOp::SonameShift => true,
            BreakOp::Textrel => line.allows_textrel(),
            // glibc 会校版本符号，版本定义坏了它当场拒；musl 整段忽略，照样装载成功。
            BreakOp::VerneedBroken => !line.checks_versions(),
            _ => false,
        }
    }

    /// 该算子是否属于登记过的双线例外（只有两条有资格）。
    pub fn is_line_exception(&self) -> bool {
        match *self {
            BreakOp::Textrel | BreakOp::VerneedBroken => true,
            _ => false,
        }
    }

    /// 例外两侧的期望文本尾巴——成对声明的另一半也从这里出，避免只写一侧。
    pub fn exception_pair(&self) -> Option<(&'static str, &'static str)> {
        match *self {
            BreakOp::Textrel => Some((TEXT_EMPTY, T_MUSL_TEXTREL)),
            BreakOp::VerneedBroken => Some((T_CORRUPT_VERSION, TEXT_EMPTY)),
            _ => None,
        }
    }

    /// 该算子失败时挂的 errno（快照三元组之一）。
    pub fn errno_on(&self, line: Line) -> i32 {
        match *self {
            BreakOp::NeededMissing => ANCHOR_ENOENT,
            BreakOp::DirOrDenied => ANCHOR_EACCES,
            BreakOp::PermBits => ANCHOR_EACCES,
            BreakOp::NoLoad => ANCHOR_ENOENT,
            _ => {
                if line.flat_corrupt_text() {
                    ANCHOR_EINVAL
                } else {
                    0
                }
            }
        }
    }

    /// 该算子在指定线上的 dlerror 尾巴（逐字，本卡不评价它对不对）。
    pub fn tail_on(&self, line: Line) -> &'static str {
        match *self {
            BreakOp::Master => T_INVALID_HEADER,
            BreakOp::Truncate => T_INVALID_HEADER,
            BreakOp::Class32 => "wrong ELF class: ELFCLASS32",
            BreakOp::Machine386 => "wrong ELF machine: EM_386",
            BreakOp::BadMagic => T_INVALID_HEADER,
            BreakOp::NeededMissing => T_CANNOT_OPEN,
            BreakOp::NeededCycle => TEXT_EMPTY,
            BreakOp::VerneedBroken => {
                if line.checks_versions() {
                    T_CORRUPT_VERSION
                } else {
                    TEXT_EMPTY
                }
            }
            BreakOp::SymHashBroken => T_UNDEF_SYM,
            BreakOp::NowUndef => T_UNDEF_SYM,
            BreakOp::LazyDefer => TEXT_EMPTY,
            BreakOp::Textrel => {
                if line.allows_textrel() {
                    TEXT_EMPTY
                } else {
                    T_MUSL_TEXTREL
                }
            }
            BreakOp::DirOrDenied => {
                if line.flat_corrupt_text() {
                    T_MUSL_PERM
                } else {
                    T_PERM_DENIED
                }
            }
            BreakOp::PermBits => T_IS_DIR,
            BreakOp::NoLoad => T_NOT_LOADED,
            BreakOp::CloseZero => TEXT_EMPTY,
            BreakOp::SonameShift => TEXT_EMPTY,
            BreakOp::SymMissing => T_UNDEF_SYM,
        }
    }

    /// 快照该读到的完整呈现（装载成功时为空串，两机都是）。
    pub fn text_on(&self, line: Line, name: &str) -> String {
        let tail = self.tail_on(line);
        if tail == TEXT_EMPTY {
            String::from(TEXT_EMPTY)
        } else {
            dl_text(name, tail)
        }
    }
}

// ---------------------------------------------------------------------------
// 判决注记 / 缺陷维
// ---------------------------------------------------------------------------

/// 快照比对判决。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// 两机文本逐字相同。
    Same,
    /// 两机文本不同，但命中登记过的双线例外。
    Declared,
    /// 两机文本不同且没登记——红。
    Conflict,
}

impl Verdict {
    /// 判决名。
    pub fn name(&self) -> &'static str {
        match *self {
            Verdict::Same => "same",
            Verdict::Declared => "declared",
            Verdict::Conflict => "conflict",
        }
    }

    /// 只有冲突是红；`Declared` 不是"放过"，是"登记过的事实"。
    pub fn is_red(&self) -> bool {
        match *self {
            Verdict::Conflict => true,
            _ => false,
        }
    }

    /// 计入一致分子的判决（Same 与 Declared 都计，Conflict 一律不计）。
    pub fn counts_toward_agreement(&self) -> bool {
        match *self {
            Verdict::Same | Verdict::Declared => true,
            Verdict::Conflict => false,
        }
    }
}

/// 注记——快照的"为什么"，逐字可追。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Note {
    /// 无注记（正例默认）。
    Nil,
    /// 文本逐字对上。
    TextVerbatim,
    /// 文本漂移（同一算子两机尾巴不同）。
    TextDrift,
    /// 登记过的双线例外。
    LineException,
    /// 例外只声明了单侧。
    HalfDeclared,
    /// 野探针（未登记）。
    WildProbe,
    /// 探针重复登记。
    DuplicateProbe,
    /// 双机看到的件指纹不同。
    ImageDrift,
    /// 落盘越出沙盒。
    EscapedSandbox,
    /// 一次 Run 缺失。
    RunMissing,
    /// 种子被禁。
    SeedForbidden,
    /// 用法错。
    UsageLine,
    /// 不属于本卡（跨面）。
    OutOfScope,
}

impl Note {
    /// 注记名。
    pub fn name(&self) -> &'static str {
        match *self {
            Note::Nil => "nil",
            Note::TextVerbatim => "text-verbatim",
            Note::TextDrift => "text-drift",
            Note::LineException => "line-exception",
            Note::HalfDeclared => "half-declared",
            Note::WildProbe => "wild-probe",
            Note::DuplicateProbe => "duplicate-probe",
            Note::ImageDrift => "image-drift",
            Note::EscapedSandbox => "escaped-sandbox",
            Note::RunMissing => "run-missing",
            Note::SeedForbidden => "seed-forbidden",
            Note::UsageLine => "usage-line",
            Note::OutOfScope => "out-of-scope",
        }
    }

    /// 该注记是否让快照结果整体作废。
    pub fn voids_snapshot(&self) -> bool {
        match *self {
            Note::WildProbe | Note::ImageDrift | Note::EscapedSandbox | Note::RunMissing => true,
            _ => false,
        }
    }

    /// 该注记是否属于"跨面"——本卡不判，必须显式拒收而不是默认放行。
    pub fn out_of_scope(&self) -> bool {
        match *self {
            Note::OutOfScope => true,
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

    /// 缺陷维对应的错误码。
    pub fn code(&self) -> u32 {
        match *self {
            Fault::Estate => PG_ESTATE,
            Fault::Stage => PG_ESTAGE,
            Fault::Code => PG_ECODE,
            Fault::Value => PG_EVALUE,
            Fault::Note => PG_ENOTE,
            Fault::Default => PG_EDEFAULT,
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

/// 反例六维表（复写一份，和 `Fault::reject_faults()` 交叉校验）。
pub const REJECT_FAULTS: [Fault; REJECT] = [
    Fault::Estate,
    Fault::Stage,
    Fault::Code,
    Fault::Value,
    Fault::Note,
    Fault::Default,
];

// ---------------------------------------------------------------------------
// 确定性成器
// ---------------------------------------------------------------------------

/// xorshift32 流——成器的全部随机性来源；种子一确定，序列就确定。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Stream {
    state: u32,
}

impl Stream {
    /// 建流。禁种子一律拒绝，返回 None（不夹带"退化成 1"的默认值）。
    pub fn new(seed: u32) -> Option<Stream> {
        if seed == FORBIDDEN_SEED_ZERO || seed == FORBIDDEN_SEED_CLOCK || seed == FORBIDDEN_SEED_ONES {
            return None;
        }
        Some(Stream { state: seed })
    }

    /// 当前状态（快照登记要把它写进台账）。
    pub fn state(&self) -> u32 {
        self.state
    }

    /// 下一个 u32。
    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.state = x;
        x
    }

    /// 下一个字节（取高位，避开低位的短周期结构）。
    pub fn next_byte(&mut self) -> u8 {
        let v = self.next_u32();
        ((v >> 24) & 0xff) as u8
    }
}

/// 一个展开出来的探针件。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Artifact {
    /// 算子。
    pub op: BreakOp,
    /// 种子。
    pub seed: u32,
    /// 补丁头。
    pub head: [u8; PATCH_LEN],
    /// 件长。
    pub bytes: u32,
    /// 件指纹。
    pub image: u64,
}

impl Artifact {
    /// 指纹（把算子号、种子、头、件长全折进去）。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_byte(h, self.op.id());
        h = fold_word(h, self.seed as u64);
        h = fold_word(h, self.bytes as u64);
        let mut i = 0usize;
        while i < PATCH_LEN {
            if let Some(b) = self.head.get(i) {
                h = fold_byte(h, *b);
            }
            i += 1;
        }
        h
    }

    /// 头部的十六进制呈现（台账里人眼对账用）。
    pub fn head_text(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < PATCH_LEN {
            if let Some(b) = self.head.get(i) {
                s.push_str(hex_text(*b as u32, 2).as_str());
            }
            i += 1;
        }
        s
    }
}

/// 该算子把件长改写成多少——截断档必须真的比识别段短，否则测不到那条码。
fn artifact_len(op: BreakOp, seed: u32) -> u32 {
    match op {
        BreakOp::Truncate => {
            let n = seed % TRUNC_BELOW_IDENT;
            n + 1
        }
        BreakOp::Class32 => BASE_BYTES / 2,
        BreakOp::Machine386 => BASE_BYTES / 2,
        BreakOp::NoLoad => BASE_BYTES,
        BreakOp::CloseZero => BASE_BYTES,
        // 参照件是"健康基线"：一件未被破坏的 .so 长度就是底座长度，
        // 多出来的那些字节是坏件补坏节表的痕迹，参照件不该有。
        BreakOp::Master => BASE_BYTES,
        _ => {
            let extra = seed % 64;
            BASE_BYTES + extra
        }
    }
}

/// 该算子改写识别段的哪一格——决定头部模板字节，保证同算子同种子的头确定。
fn patch_head(op: BreakOp, seed: u32) -> [u8; PATCH_LEN] {
    let mut head = [0u8; PATCH_LEN];
    let mut i = 0usize;
    while i < PATCH_LEN {
        if let Some(slot) = head.get_mut(i) {
            let base = MAGIC0.wrapping_add(i as u8);
            *slot = base;
        }
        i += 1;
    }
    // 头一格写 `0x7f`、次三格写 `ELF`，让"截断到识别段以内"这一档可判。
    if let Some(s0) = head.get_mut(0) {
        *s0 = MAGIC0;
    }
    if let Some(s1) = head.get_mut(1) {
        *s1 = b'E';
    }
    if let Some(s2) = head.get_mut(2) {
        *s2 = b'L';
    }
    if let Some(s3) = head.get_mut(3) {
        *s3 = b'F';
    }
    // 识别段之后的格位由算子决定：类/机种档要写进固定的 EI_CLASS/EI_MACHINE 槽。
    match op {
        BreakOp::Class32 => {
            if let Some(s4) = head.get_mut(4) {
                *s4 = ELFCLASS32;
            }
        }
        BreakOp::Machine386 => {
            if let Some(s4) = head.get_mut(4) {
                *s4 = ELFCLASS64;
            }
            if let Some(s6) = head.get_mut(6) {
                *s6 = (EM_386 & 0xff) as u8;
            }
            if let Some(s7) = head.get_mut(7) {
                *s7 = (EM_386 >> 8) as u8;
            }
        }
        BreakOp::BadMagic => {
            if let Some(s0) = head.get_mut(0) {
                *s0 = MAGIC0.wrapping_add(1);
            }
        }
        BreakOp::Master => {
            if let Some(s4) = head.get_mut(4) {
                *s4 = ELFCLASSNUM;
            }
        }
        _ => {}
    }
    // 尾部四格由种子流决定，且**只由种子流决定**——这是确定性的锚。
    let mut st = match Stream::new(seed) {
        Some(s) => s,
        None => return head,
    };
    let mut k = 8usize;
    while k < PATCH_LEN {
        if let Some(slot) = head.get_mut(k) {
            *slot = st.next_byte();
        }
        k += 1;
    }
    head
}

/// 成器主函数：`emit(op, seed)` 是纯函数——同输入必同输出。
pub fn emit(op: BreakOp, seed: u32) -> Option<Artifact> {
    if Stream::new(seed).is_none() {
        return None;
    }
    let head = patch_head(op, seed);
    let bytes = artifact_len(op, seed);
    let a = Artifact {
        op,
        seed,
        head,
        bytes,
        image: 0,
    };
    let image = a.seal();
    Some(Artifact {
        op,
        seed,
        head,
        bytes,
        image,
    })
}

/// 用短名找算子（生成器命令行入口的查表）。
pub fn op_by_slug(text: &str) -> Option<BreakOp> {
    let table = BreakOp::all();
    let mut i = 0usize;
    while i < OP_COUNT {
        if let Some(o) = table.get(i) {
            if o.slug() == text {
                return Some(*o);
            }
        }
        i += 1;
    }
    None
}

// ---------------------------------------------------------------------------
// 登记台账
// ---------------------------------------------------------------------------

/// 一条登记：算子、期望件指纹、双线例外声明（成对）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Entry {
    /// 算子号。
    pub opid: u8,
    /// 登记时锁定的件指纹。
    pub image: u64,
    /// 是否声明双线例外。
    pub declared: bool,
    /// 例外声明是否成对（两侧期望文本都写了）。
    pub paired: bool,
    /// 基准侧期望文本尾巴。
    pub want_a: &'static str,
    /// 对照侧期望文本尾巴。
    pub want_b: &'static str,
}

impl Entry {
    /// 登记一条探针（期望尾巴从算子在线上逐字取，避免手抄漂移）。
    pub fn bind(op: BreakOp, image: u64, line_a: Line, line_b: Line, declared: bool) -> Entry {
        let paired = if declared { op.is_line_exception() } else { true };
        Entry {
            opid: op.id(),
            image,
            declared,
            paired,
            want_a: op.tail_on(line_a),
            want_b: op.tail_on(line_b),
        }
    }

    /// 登记指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_byte(h, self.opid);
        h = fold_word(h, self.image);
        h = fold_bool(h, self.declared);
        h = fold_bool(h, self.paired);
        h = fold_str(h, self.want_a);
        h = fold_str(h, self.want_b);
        h
    }

    /// 台账呈现（一行一条，人眼对账）。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str("reg op=");
        s.push_str(hex_text(self.opid as u32, 2).as_str());
        s.push_str(" image=");
        s.push_str(hex_text((self.image & 0xffff_ffff) as u32, 8).as_str());
        s.push_str(" declared=");
        s.push_str(bool_text(self.declared));
        s.push_str(" paired=");
        s.push_str(bool_text(self.paired));
        s
    }
}

/// 族内登记台账：十八格一一对应算子，未登记即 `None`。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Registry {
    slots: [Option<Entry>; OP_COUNT],
    count: usize,
    dup: bool,
}

impl Registry {
    /// 空台账。
    pub fn new() -> Registry {
        Registry {
            slots: [None; OP_COUNT],
            count: 0,
            dup: false,
        }
    }

    /// 登记一条；重复登记同一算子会被记下并拒绝（野探针与双登记是两种红）。
    pub fn register(&mut self, op: BreakOp, e: Entry) -> Result<(), u32> {
        let idx = op.index();
        if idx >= OP_COUNT {
            return Err(PG_ESLOT);
        }
        let occupied = match self.slots.get(idx) {
            Some(x) => x.is_some(),
            None => return Err(PG_ESLOT),
        };
        if occupied {
            self.dup = true;
            return Err(PG_EPROBE);
        }
        if let Some(slot) = self.slots.get_mut(idx) {
            *slot = Some(e);
        }
        self.count += 1;
        Ok(())
    }

    /// 取一条登记。
    pub fn entry(&self, op: BreakOp) -> Option<Entry> {
        let idx = op.index();
        match self.slots.get(idx) {
            Some(x) => *x,
            None => None,
        }
    }

    /// 已登记条数。
    pub fn len(&self) -> usize {
        self.count
    }

    /// 是否空台账。
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// 是否发生过重复登记。
    pub fn duplicated(&self) -> bool {
        self.dup
    }

    /// 台账指纹（顺序无关——按算子号排序折入，保证族重排不改指纹）。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut ids = [0u8; OP_COUNT];
        let mut i = 0usize;
        while i < OP_COUNT {
            if let Some(o) = BreakOp::all().get(i) {
                if let Some(e) = self.entry(*o) {
                    if let Some(slot) = ids.get_mut(i) {
                        *slot = e.opid;
                    }
                }
            }
            i += 1;
        }
        let mut n = 0usize;
        while n < OP_COUNT {
            if let Some(id) = ids.get(n) {
                if *id != 0 {
                    h = fold_byte(h, *id);
                    if let Some(o) = BreakOp::from_id(*id) {
                        if let Some(e) = self.entry(o) {
                            h = fold_word(h, e.seal());
                        }
                    }
                }
            }
            n += 1;
        }
        h
    }

    /// 族是否登记齐（十八条全在账上）。
    pub fn family_complete(&self) -> bool {
        self.count == OP_COUNT
    }

    /// 逐条复核：登记指纹必须等于 `emit(op, seed)` 的指纹，否则是"登记后改件"。
    pub fn verify_against(&self, op: BreakOp, seed: u32) -> Result<(), u32> {
        let e = match self.entry(op) {
            Some(x) => x,
            None => return Err(PG_EPROBE),
        };
        let a = match emit(op, seed) {
            Some(x) => x,
            None => return Err(PG_ESEED),
        };
        if a.image != e.image {
            return Err(PG_ESEAL);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 单机执行结果
// ---------------------------------------------------------------------------

/// 一台宿主上跑一条探针的三元组。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Run {
    /// 是否装载成功。
    pub ok: bool,
    /// errno。
    pub errno: i32,
    /// dlerror 全文尾巴（成功时为空）。
    pub tail: &'static str,
    /// 看到的件指纹。
    pub image: u64,
    /// 句柄（0 = NULL）。
    pub handle: u32,
}

impl Run {
    /// 按算子与线构造一次真实执行结果（跟随上游实况，不做本卡加工）。
    pub fn of(op: BreakOp, line: Line, image: u64) -> Run {
        let loads = op.loads_on(line);
        Run {
            ok: loads,
            errno: if loads { 0 } else { op.errno_on(line) },
            tail: op.tail_on(line),
            image,
            handle: if loads { 1 } else { HANDLE_NULL },
        }
    }

    /// 执行结果指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_bool(h, self.ok);
        h = fold_i32(h, self.errno);
        h = fold_str(h, self.tail);
        h = fold_word(h, self.image);
        h = fold_word(h, self.handle as u64);
        h
    }

    /// 呈现一行快照（`<线>: <文本>`，成功走 `(loaded)`）。
    pub fn render(&self, line: Line, name: &str) -> String {
        let mut s = String::from(line.label());
        s.push_str(": ");
        if self.ok {
            s.push_str("(loaded) ");
            s.push_str(name);
        } else if self.tail == TEXT_EMPTY {
            s.push_str(name);
        } else {
            s.push_str(dl_text(name, self.tail).as_str());
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 快照对
// ---------------------------------------------------------------------------

/// 一次双机快照。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Snap {
    /// 基准机结果。
    pub base: Run,
    /// 对照机结果。
    pub second: Run,
    /// 判决。
    pub verdict: Verdict,
}

impl Snap {
    /// 比对：文本逐字相同即 Same；否则只有"登记例外且算子确有例外资格且成对"
    /// 才是 Declared，其余一律 Conflict。**两边都失败但文本不同也是 Conflict**。
    pub fn compare(op: BreakOp, base: Run, second: Run, declared: bool, paired: bool) -> Snap {
        let verdict = if base.tail == second.tail && base.ok == second.ok {
            Verdict::Same
        } else if declared && paired && op.is_line_exception() {
            Verdict::Declared
        } else {
            Verdict::Conflict
        };
        Snap {
            base,
            second,
            verdict,
        }
    }

    /// 快照指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.base.seal());
        h = fold_word(h, self.second.seal());
        h = fold_str(h, self.verdict.name());
        h
    }

    /// 呈现（`SAME` / `DECLARED` / `CONFLICT` 三态逐字）。
    pub fn render(&self, op: BreakOp, name: &str) -> String {
        let mut s = String::from(self.verdict.name());
        s.push(' ');
        s.push_str(op.slug());
        s.push(' ');
        s.push_str(name);
        s
    }
}

// ---------------------------------------------------------------------------
// 域模型：Shape / Outcome / Claim / Case
// ---------------------------------------------------------------------------

/// 本卡输入的全部维度——一条探针快照的完整现场。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Shape {
    /// 破坏算子。
    pub op: BreakOp,
    /// 种子。
    pub seed: u32,
    /// 基准机线。
    pub line_a: Line,
    /// 对照机线。
    pub line_b: Line,
    /// 已登记。
    pub registered: bool,
    /// 发生过重复登记。
    pub dup_register: bool,
    /// 落盘在沙盒内。
    pub sandbox_ok: bool,
    /// 两机看到的件指纹与登记一致。
    pub image_ok: bool,
    /// 基准机跑完。
    pub run_a_done: bool,
    /// 对照机跑完。
    pub run_b_done: bool,
    /// 两机文本不同。
    pub text_drift: bool,
    /// 登记了双线例外。
    pub declared: bool,
    /// 例外只声明了单侧。
    pub declared_half: bool,
    /// 用法错档（选项缺失/算子不认识）。
    pub usage_err: bool,
    /// 跨面（不属于坏 .so 拒收面）。
    pub out_of_scope: bool,
    /// 结算前没有任何判决（未判档）。
    pub unjudged: bool,
    /// 统计值（本条计入的分子）。
    pub value: u32,
    /// 兜底档被当成结论。
    pub default: bool,
}

impl Shape {
    /// 一条活探针的缺省形状：登记齐、沙盒内、两机都跑完、文本一致。
    pub fn live(op: BreakOp, seed: u32) -> Shape {
        Shape {
            op,
            seed,
            line_a: Line::Glibc,
            line_b: Line::Glibc,
            registered: true,
            dup_register: false,
            sandbox_ok: true,
            image_ok: true,
            run_a_done: true,
            run_b_done: true,
            text_drift: false,
            declared: false,
            declared_half: false,
            usage_err: false,
            out_of_scope: false,
            unjudged: false,
            value: 2,
            default: false,
        }
    }

    /// 一条跨线例外探针：基准 glibc、对照 musl。
    pub fn cross_line(op: BreakOp, seed: u32) -> Shape {
        let mut s = Shape::live(op, seed);
        s.line_b = Line::Musl;
        s.text_drift = true;
        s.declared = true;
        s
    }

    /// 换算子。
    pub fn with_op(&self, op: BreakOp) -> Shape {
        Shape { op, ..*self }
    }

    /// 换文本漂移位。
    pub fn with_drift(&self, d: bool) -> Shape {
        Shape {
            text_drift: d,
            ..*self
        }
    }

    /// 换登记位。
    pub fn with_registered(&self, r: bool) -> Shape {
        Shape {
            registered: r,
            ..*self
        }
    }

    /// 换沙盒位。
    pub fn with_sandbox(&self, ok: bool) -> Shape {
        Shape {
            sandbox_ok: ok,
            ..*self
        }
    }

    /// 换某侧 Run 完成位。
    pub fn with_run(&self, which: u8, done: bool) -> Shape {
        if which == 0 {
            Shape {
                run_a_done: done,
                ..*self
            }
        } else {
            Shape {
                run_b_done: done,
                ..*self
            }
        }
    }

    /// 换件指纹一致位。
    pub fn with_image(&self, ok: bool) -> Shape {
        Shape {
            image_ok: ok,
            ..*self
        }
    }

    /// 形状指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_byte(h, self.op.id());
        h = fold_word(h, self.seed as u64);
        h = fold_str(h, self.line_a.label());
        h = fold_str(h, self.line_b.label());
        h = fold_bool(h, self.registered);
        h = fold_bool(h, self.dup_register);
        h = fold_bool(h, self.sandbox_ok);
        h = fold_bool(h, self.image_ok);
        h = fold_bool(h, self.run_a_done);
        h = fold_bool(h, self.run_b_done);
        h = fold_bool(h, self.text_drift);
        h = fold_bool(h, self.declared);
        h = fold_bool(h, self.declared_half);
        h = fold_bool(h, self.usage_err);
        h = fold_bool(h, self.out_of_scope);
        h = fold_bool(h, self.unjudged);
        h = fold_word(h, self.value as u64);
        h = fold_bool(h, self.default);
        h
    }
}

/// `define` 的输出：走到了哪一闸、落在哪条码、记了什么注。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Outcome {
    /// 最后成立的闸。
    pub stage: Stage,
    /// 错误码（0 = 无）。
    pub code: u32,
    /// 统计值。
    pub value: u32,
    /// 注记。
    pub note: Note,
    /// 兜底档。
    pub default: bool,
    /// 本条是否可作为一致分子。
    pub ok: bool,
    /// 快照判决。
    pub verdict: Verdict,
}

impl Outcome {
    /// 一条正例结果。
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

    /// 一条失败结果。
    fn fail(stage: Stage, code: u32, note: Note, value: u32) -> Outcome {
        Outcome {
            stage,
            code,
            value,
            note,
            default: false,
            ok: false,
            verdict: Verdict::Conflict,
        }
    }

    /// 是否成功。
    pub fn ok(&self) -> bool {
        self.ok
    }

    /// 是否失败。
    pub fn failed(&self) -> bool {
        !self.ok
    }

    /// 是否进入快照分子。
    pub fn anchored(&self) -> bool {
        self.ok && self.verdict.counts_toward_agreement()
    }

    /// 是否被成器侧判死（根本不该被观测）。
    pub fn refused(&self) -> bool {
        self.code == PG_ESEED || self.code == PG_EPROBE || self.code == PG_ESANDBOX || self.code == PG_ESEAL
    }

    /// 该结果对应的退出码档。
    pub fn exit(&self) -> u32 {
        if self.default {
            return EXIT_UNJUDGED;
        }
        if self.code == PG_ESLOT || self.note == Note::UsageLine {
            return EXIT_USAGE;
        }
        if self.ok {
            return EXIT_OK;
        }
        if self.note == Note::TextDrift || self.note == Note::HalfDeclared {
            return EXIT_CONFLICT;
        }
        EXIT_CONFLICT
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
    /// 声称成功。
    pub success: bool,
    /// 声称到达的闸。
    pub stage: Stage,
    /// 声称的错误码。
    pub code: u32,
    /// 声称的统计值。
    pub value: u32,
    /// 声称的注记。
    pub note: Note,
    /// 声称的判决。
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

    /// 反例申报。
    pub fn bad(stage: Stage, code: u32, note: Note, value: u32, tag: &'static str) -> Claim {
        Claim {
            success: false,
            stage,
            code,
            value,
            note,
            verdict: Verdict::Conflict,
            default: false,
            tag,
        }
    }

    /// 申报是否声称成功。
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

    /// 缺陷标签。
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
// 八闸纵线
// ---------------------------------------------------------------------------

/// 种子是否被禁。
pub fn seed_forbidden(seed: u32) -> bool {
    seed == FORBIDDEN_SEED_ZERO || seed == FORBIDDEN_SEED_CLOCK || seed == FORBIDDEN_SEED_ONES
}

/// 禁种子表（复写一份，和 `seed_forbidden` 交叉校验）。
pub const FORBIDDEN_SEEDS: [u32; FORBIDDEN_SEED_COUNT] =
    [FORBIDDEN_SEED_ZERO, FORBIDDEN_SEED_CLOCK, FORBIDDEN_SEED_ONES];

/// 主判据纵线：一条现场如何一步步走到结论。
///
/// 顺序不可调：任何一闸的"过"都只表示"这一维没坏"，不表示后面可以跳。
pub fn define(s: &Shape) -> Outcome {
    // 1 Seed
    if seed_forbidden(s.seed) {
        return Outcome::fail(Stage::Seed, PG_ESEED, Note::SeedForbidden, 0);
    }
    // 用法错档在种子合法之后判：`--op` 缺失/不认识属于成器入口，不进快照。
    if s.usage_err {
        return Outcome::fail(Stage::Seed, PG_ESLOT, Note::UsageLine, 0);
    }
    // 2 Emit
    let art = match emit(s.op, s.seed) {
        Some(a) => a,
        None => return Outcome::fail(Stage::Emit, PG_ESEAL, Note::Nil, 0),
    };
    // 跨面现场：本卡显式拒收，不给真实档。
    if s.out_of_scope {
        return Outcome {
            stage: Stage::Emit,
            code: PG_ECODE,
            value: 0,
            note: Note::OutOfScope,
            default: false,
            ok: false,
            verdict: Verdict::Conflict,
        };
    }
    // 3 Register
    if s.dup_register {
        return Outcome::fail(Stage::Register, PG_EPROBE, Note::DuplicateProbe, 0);
    }
    if !s.registered {
        return Outcome::fail(Stage::Register, PG_EPROBE, Note::WildProbe, 0);
    }
    // 4 Sandbox
    if !s.sandbox_ok {
        return Outcome::fail(Stage::Sandbox, PG_ESANDBOX, Note::EscapedSandbox, 0);
    }
    // 5/6 件指纹：登记了但两机看到的件不同——快照作废
    if !s.image_ok {
        return Outcome::fail(Stage::Register, PG_ESEAL, Note::ImageDrift, 0);
    }
    if !s.run_a_done {
        return Outcome::fail(Stage::RunBase, PG_ECODE, Note::RunMissing, 0);
    }
    if !s.run_b_done {
        return Outcome::fail(Stage::RunSecond, PG_EVALUE, Note::RunMissing, 0);
    }
    // 8 Rate：两次 Run 都在，却没有任何判决落账——这就是"未判档"，本卡绝不交付。
    if s.unjudged {
        return Outcome {
            stage: Stage::Rate,
            code: PG_EDEFAULT,
            value: 0,
            note: Note::Nil,
            default: true,
            ok: false,
            verdict: Verdict::Conflict,
        };
    }
    // 7 Compare
    let base = Run::of(s.op, s.line_a, art.image);
    let second = Run::of(s.op, s.line_b, art.image);
    let drift = s.text_drift || (base.tail != second.tail) || (base.ok != second.ok);
    if !drift {
        let snap = Snap::compare(s.op, base, second, s.declared, true);
        if snap.verdict == Verdict::Same {
            return Outcome::pass(Stage::Compare, Note::TextVerbatim, 2, Verdict::Same);
        }
        return Outcome::fail(Stage::Compare, PG_ENOTE, Note::TextDrift, 0);
    }
    // 文本确有分歧——只有登记过、成对声明、且算子真有例外资格，才是 DECLARED。
    if s.declared_half {
        return Outcome::fail(Stage::Compare, PG_ENOTE, Note::HalfDeclared, 0);
    }
    if s.declared && s.op.is_line_exception() {
        if let Some((a, b)) = s.op.exception_pair() {
            if a == s.op.tail_on(s.line_a) && b == s.op.tail_on(s.line_b) {
                return Outcome::pass(Stage::Compare, Note::LineException, 2, Verdict::Declared);
            }
        }
        return Outcome::fail(Stage::Compare, PG_ENOTE, Note::TextDrift, 0);
    }
    if s.declared && !s.op.is_line_exception() {
        // 把非例外档申报成例外——比未声明更严重，注记仍走漂移维。
        return Outcome::fail(Stage::Compare, PG_ENOTE, Note::TextDrift, 0);
    }
    Outcome::fail(Stage::Compare, PG_ENOTE, Note::TextDrift, 0)
    // 8 Rate 由 `run_with` 结算；单条现场不产生分母。
}

/// 快照文本呈现。
pub fn message(s: &Shape, o: &Outcome) -> String {
    let name = s.op.probe_name();
    match o.note {
        Note::TextVerbatim => {
            let art = match emit(s.op, s.seed) {
                Some(a) => a,
                None => return msg_unknown_op(s.op.slug()),
            };
            let base = Run::of(s.op, s.line_a, art.image);
            base.render(s.line_a, name.as_str())
        }
        Note::LineException => {
            let art = match emit(s.op, s.seed) {
                Some(a) => a,
                None => return msg_unknown_op(s.op.slug()),
            };
            let base = Run::of(s.op, s.line_a, art.image);
            let second = Run::of(s.op, s.line_b, art.image);
            msg_declared(
                name.as_str(),
                base.render(s.line_a, name.as_str()).as_str(),
                second.render(s.line_b, name.as_str()).as_str(),
            )
        }
        Note::TextDrift => {
            let art = match emit(s.op, s.seed) {
                Some(a) => a,
                None => return msg_unknown_op(s.op.slug()),
            };
            let base = Run::of(s.op, s.line_a, art.image);
            let second = Run::of(s.op, s.line_b, art.image);
            msg_conflict(
                name.as_str(),
                base.render(s.line_a, name.as_str()).as_str(),
                second.render(s.line_b, name.as_str()).as_str(),
            )
        }
        Note::HalfDeclared => msg_half_declared(name.as_str()),
        Note::WildProbe | Note::DuplicateProbe => msg_unregistered(name.as_str()),
        Note::ImageDrift => msg_image_drift(name.as_str(), 0, 1),
        Note::EscapedSandbox => msg_escaped(HOST_LIB_PREFIX),
        Note::SeedForbidden => msg_seed_forbidden(s.seed),
        Note::UsageLine => msg_missing_operand(),
        Note::RunMissing => {
            let mut t = String::from("SNAPSHOT: missing run for ");
            t.push_str(s.op.slug());
            t.push_str(" — agreement cannot be computed against a one-sided site");
            t
        }
        Note::OutOfScope => msg_unknown_op(s.op.slug()),
        Note::Nil => {
            let mut t = String::from("probe-gen: ");
            t.push_str(s.op.slug());
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
    s.push_str(" op=");
    s.push_str(c.shape.op.slug());
    s.push_str(" seed=");
    s.push_str(hex_text(c.shape.seed, 8).as_str());
    s.push_str(" line=");
    s.push_str(c.shape.line_a.label());
    s.push('/');
    s.push_str(c.shape.line_b.label());
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

/// 语料基型：一条完全合法的活探针。
const BASE: Shape = Shape {
    op: BreakOp::Master,
    seed: 0x2a17_0001,
    line_a: Line::Glibc,
    line_b: Line::Glibc,
    registered: true,
    dup_register: false,
    sandbox_ok: true,
    image_ok: true,
    run_a_done: true,
    run_b_done: true,
    text_drift: false,
    declared: false,
    declared_half: false,
    usage_err: false,
    out_of_scope: false,
    unjudged: false,
    value: 2,
    default: false,
};

/// 跨线基型：基准 glibc、对照 musl，且例外已在台账里成对声明。
const CROSS: Shape = Shape {
    line_b: Line::Musl,
    text_drift: true,
    declared: true,
    ..BASE
};

/// 语料现场表——八活六死，逐槽手写。
pub const SHAPES: [Shape; CORPUS] = [
    // 0 母版总锚：同线双机，结构坏文本逐字相同。
    Shape {
        seed: 0x2a17_0001,
        ..BASE
    },
    // 1 class32：两机都报 wrong ELF class: ELFCLASS32。
    Shape {
        op: BreakOp::Class32,
        seed: 0x2a17_0002,
        ..BASE
    },
    // 2 machine386：两机都报 wrong ELF machine: EM_386。
    Shape {
        op: BreakOp::Machine386,
        seed: 0x2a17_0003,
        ..BASE
    },
    // 3 缺依赖：两机都报 ENOENT 文案，含名呈现。
    Shape {
        op: BreakOp::NeededMissing,
        seed: 0x2a17_0004,
        ..BASE
    },
    // 4 实况放行档：SONAME 错位两机都装载成功，空文本也是逐字一致。
    Shape {
        op: BreakOp::SonameShift,
        seed: 0x2a17_0005,
        ..BASE
    },
    // 5 TEXTREL 跨线例外：glibc 过、musl 拒，登记成对。
    Shape {
        op: BreakOp::Textrel,
        seed: 0x2a17_0006,
        ..CROSS
    },
    // 6 版本校验跨线例外：glibc 拒、musl 静默忽略，登记成对。
    Shape {
        op: BreakOp::VerneedBroken,
        seed: 0x2a17_0007,
        ..CROSS
    },
    // 7 dlsym 缺符号：两机都报 undefined symbol。
    Shape {
        op: BreakOp::SymMissing,
        seed: 0x2a17_0008,
        ..BASE
    },
    // 8 Estate：现场是未声明冲突，申报却声称一致。
    Shape {
        op: BreakOp::Truncate,
        seed: 0x2a17_0009,
        line_b: Line::Musl,
        text_drift: true,
        declared: false,
        value: 0,
        ..BASE
    },
    // 9 Stage：对照机根本没跑，申报却声称已到 Compare。
    Shape {
        op: BreakOp::DirOrDenied,
        seed: 0x2a17_000a,
        run_b_done: false,
        value: 0,
        ..BASE
    },
    // 10 Code：探针未登记（野件），申报给了真实档码。
    Shape {
        op: BreakOp::BadMagic,
        seed: 0x2a17_000b,
        registered: false,
        value: 0,
        ..BASE
    },
    // 11 Value：件指纹漂移，申报的统计值按两机都跑齐给。
    Shape {
        op: BreakOp::SymHashBroken,
        seed: 0x2a17_000c,
        image_ok: false,
        value: 0,
        ..BASE
    },
    // 12 Note：例外只声明单侧，申报注成登记例外。
    Shape {
        op: BreakOp::Textrel,
        seed: 0x2a17_000d,
        declared_half: true,
        value: 0,
        ..CROSS
    },
    // 13 Default：两次 Run 都在却没有任何判决落账（未判档）。
    Shape {
        op: BreakOp::NoLoad,
        seed: 0x2a17_000e,
        unjudged: true,
        value: 0,
        ..BASE
    },
];

/// 申报基型：与活探针的实际结论逐格相同。
const GOOD_CLAIM: Claim = Claim {
    success: true,
    stage: Stage::Compare,
    code: 0,
    value: 2,
    note: Note::TextVerbatim,
    verdict: Verdict::Same,
    default: false,
    tag: "",
};

/// 跨线例外的申报基型。
const DECLARED_CLAIM: Claim = Claim {
    note: Note::LineException,
    verdict: Verdict::Declared,
    ..GOOD_CLAIM
};

/// 十四格申报。前八格与 `define(SHAPES[i])` 逐格相同，后六格各坏一维。
pub const CLAIMS: [Claim; CORPUS] = [
    Claim {
        tag: "f11639-master-verbatim",
        ..GOOD_CLAIM
    },
    Claim {
        tag: "f11639-class-verbatim",
        ..GOOD_CLAIM
    },
    Claim {
        tag: "f11639-machine-verbatim",
        ..GOOD_CLAIM
    },
    Claim {
        tag: "f11639-needed-verbatim",
        ..GOOD_CLAIM
    },
    Claim {
        tag: "f11639-loaded-both-verbatim",
        ..GOOD_CLAIM
    },
    Claim {
        tag: "f11639-textrel-declared",
        ..DECLARED_CLAIM
    },
    Claim {
        tag: "f11639-verneed-declared",
        ..DECLARED_CLAIM
    },
    Claim {
        tag: "f11639-sym-verbatim",
        ..GOOD_CLAIM
    },
    // 8 成败互报：其余五维照实抄，只把"冲突"这一行申报成一致。
    Claim {
        success: true,
        stage: Stage::Compare,
        code: PG_ENOTE,
        value: 0,
        note: Note::TextDrift,
        verdict: Verdict::Conflict,
        default: false,
        tag: "f11639-estate-swap",
    },
    // 9 阶段不符：对照机没跑，申报已到比对。
    Claim {
        success: false,
        stage: Stage::Compare,
        code: PG_EVALUE,
        value: 0,
        note: Note::RunMissing,
        verdict: Verdict::Conflict,
        default: false,
        tag: "f11639-stage-skip",
    },
    // 10 码档不符：野件申报成真实档码。
    Claim {
        success: false,
        stage: Stage::Register,
        code: PG_ECODE,
        value: 0,
        note: Note::WildProbe,
        verdict: Verdict::Conflict,
        default: false,
        tag: "f11639-code-wild",
    },
    // 11 数值不符：指纹漂移仍按两机跑齐计。
    Claim {
        success: false,
        stage: Stage::Register,
        code: PG_ESEAL,
        value: 2,
        note: Note::ImageDrift,
        verdict: Verdict::Conflict,
        default: false,
        tag: "f11639-value-drift",
    },
    // 12 注记不符：单侧声明申报成登记例外。
    Claim {
        success: false,
        stage: Stage::Compare,
        code: PG_ENOTE,
        value: 0,
        note: Note::LineException,
        verdict: Verdict::Conflict,
        default: false,
        tag: "f11639-note-halfdeclared",
    },
    // 13 兜底档：未判现场申报成"没有兜底"。
    Claim {
        success: false,
        stage: Stage::Rate,
        code: PG_EDEFAULT,
        value: 0,
        note: Note::Nil,
        verdict: Verdict::Conflict,
        default: false,
        tag: "f11639-default-unjudged",
    },
];

// ---------------------------------------------------------------------------
// 期望表（申报表的第二种形式，任一处笔误都要露出来）
// ---------------------------------------------------------------------------

/// 期望阶段。
/// 期望阶段（第二形式：现场**真实**落到的阶段，不是申报那一栏）。
pub const WANT_STAGE: [Stage; CORPUS] = [
    Stage::Compare,
    Stage::Compare,
    Stage::Compare,
    Stage::Compare,
    Stage::Compare,
    Stage::Compare,
    Stage::Compare,
    Stage::Compare,
    Stage::Compare,
    Stage::RunSecond,
    Stage::Register,
    Stage::Register,
    Stage::Compare,
    Stage::Rate,
];

/// 期望错误码（0 = 无码；第二形式，现场真实挂的码）。
pub const WANT_CODE: [u32; CORPUS] = [
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    PG_ENOTE,
    PG_EVALUE,
    PG_EPROBE,
    PG_ESEAL,
    PG_ENOTE,
    PG_EDEFAULT,
];

/// 期望缺陷维序位（0..5 六维，6 = 无缺陷）。
pub const WANT_NDX: [usize; CORPUS] = [6, 6, 6, 6, 6, 6, 6, 6, 0, 1, 2, 3, 4, 5];

/// 期望注记（第二形式，现场真实注记）。
pub const WANT_NOTE: [Note; CORPUS] = [
    Note::TextVerbatim,
    Note::TextVerbatim,
    Note::TextVerbatim,
    Note::TextVerbatim,
    Note::TextVerbatim,
    Note::LineException,
    Note::LineException,
    Note::TextVerbatim,
    Note::TextDrift,
    Note::RunMissing,
    Note::WildProbe,
    Note::ImageDrift,
    Note::HalfDeclared,
    Note::Nil,
];

/// 期望兜底位。
pub const WANT_DEFAULT: [bool; CORPUS] = [
    false, false, false, false, false, false, false, false, false, false, false, false, false, true,
];

/// 期望短名（标签必须挂本卡号）。
pub const WANT_TAGS: [&str; CORPUS] = [
    "f11639-master-verbatim",
    "f11639-class-verbatim",
    "f11639-machine-verbatim",
    "f11639-needed-verbatim",
    "f11639-loaded-both-verbatim",
    "f11639-textrel-declared",
    "f11639-verneed-declared",
    "f11639-sym-verbatim",
    "f11639-estate-swap",
    "f11639-stage-skip",
    "f11639-code-wild",
    "f11639-value-drift",
    "f11639-note-halfdeclared",
    "f11639-default-unjudged",
];

/// 期望统计值（第二形式，现场真实统计值）。
pub const WANT_VALUE: [u32; CORPUS] = [2, 2, 2, 2, 2, 2, 2, 2, 0, 0, 0, 0, 0, 0];

/// 期望成败（第二形式，现场真实成败）。
pub const WANT_SUCCESS: [bool; CORPUS] = [
    true, true, true, true, true, true, true, true, false, false, false, false, false, false,
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
/// 成败互报优先：一条被申报成"一致"的冲突，无论后面几维对不对，都是最严重的
/// 那类谎——快照的整个价值就是"这台说的和那台说的一样"。
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
    // 兜底档双向判死：现场真落了未判档，无论怎么申报都不许当结论；
    // 现场没落未判档却申报"落了"，同样是把结论塞进了兜底——两边都得挂。
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
        return Err(PG_ESLOT);
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
// 场景族：十八条算子各造一条活探针
// ---------------------------------------------------------------------------

/// 族内场景数（= 算子数）。
pub const SCENE_COUNT: usize = OP_COUNT;

/// 族内统一种子：证明成器是纯函数（同种子同算子 = 同件）。
pub const FAMILY_SEED: u32 = 0x2a17_b5e3;

/// 构造一条场景：算子决定线，双例外算子走跨线，其余同线。
pub fn scene_of(op: BreakOp) -> Shape {
    if op.is_line_exception() {
        Shape {
            op,
            seed: FAMILY_SEED,
            ..CROSS
        }
    } else {
        Shape {
            op,
            seed: FAMILY_SEED,
            ..BASE
        }
    }
}

/// 全族场景。
pub fn scenes() -> [Shape; SCENE_COUNT] {
    let mut out = [BASE; SCENE_COUNT];
    let table = BreakOp::all();
    let mut i = 0usize;
    while i < SCENE_COUNT {
        if let Some(o) = table.get(i) {
            if let Some(slot) = out.get_mut(i) {
                *slot = scene_of(*o);
            }
        }
        i += 1;
    }
    out
}

/// 取一条场景。
pub fn scene_at(i: usize) -> Option<Shape> {
    match scenes().get(i) {
        Some(s) => Some(*s),
        None => None,
    }
}

/// 该场景是否落进快照分子。
pub fn scene_anchored(i: usize) -> Option<bool> {
    match scene_at(i) {
        Some(s) => Some(define(&s).anchored()),
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
    Some(Case {
        shape: s,
        claim: Claim {
            success: o.ok(),
            stage: o.stage,
            code: o.code,
            value: o.value,
            note: o.note,
            verdict: o.verdict,
            default: o.default,
            tag: head_tag(),
        },
    })
}

/// 族内每条场景都必须落进快照分子，并且自申报自洽（判据绿的前提）。
pub fn scenes_agree() -> bool {
    let mut i = 0usize;
    while i < SCENE_COUNT {
        let c = match scene_case(i) {
            Some(x) => x,
            None => return false,
        };
        if !c.actual().anchored() {
            return false;
        }
        if judge(&c) != Fault::Ok {
            return false;
        }
        i += 1;
    }
    true
}

/// 同种子重放两次，件指纹必须逐格相同——成器的确定性契约。
pub fn emit_is_pure() -> bool {
    let table = BreakOp::all();
    let mut i = 0usize;
    while i < OP_COUNT {
        let o = match table.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let a = match emit(o, FAMILY_SEED) {
            Some(x) => x,
            None => return false,
        };
        let b = match emit(o, FAMILY_SEED) {
            Some(x) => x,
            None => return false,
        };
        if a.image != b.image || a.head != b.head || a.bytes != b.bytes {
            return false;
        }
        i += 1;
    }
    true
}

/// 不同算子必须产出不同件（否则"族"只是同一件换个名字）。
pub fn ops_diverge() -> bool {
    let table = BreakOp::all();
    let mut i = 0usize;
    while i < OP_COUNT {
        let a = match table.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if a == BreakOp::Class32 || a == BreakOp::Machine386 || a == BreakOp::Master {
            i += 1;
            continue;
        }
        let sa = match emit(a, FAMILY_SEED) {
            Some(x) => x.image,
            None => return false,
        };
        let mut j = i + 1usize;
        while j < OP_COUNT {
            let b = match table.get(j) {
                Some(x) => *x,
                None => return false,
            };
            if b == BreakOp::Class32 || b == BreakOp::Machine386 || b == BreakOp::Master {
                j += 1;
                continue;
            }
            let sb = match emit(b, FAMILY_SEED) {
                Some(x) => x.image,
                None => return false,
            };
            if sa == sb {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 族登记：把十八条件按指纹入账，例外成对声明。
pub fn build_registry() -> Result<Registry, u32> {
    let mut r = Registry::new();
    let table = BreakOp::all();
    let mut i = 0usize;
    while i < OP_COUNT {
        let o = match table.get(i) {
            Some(x) => *x,
            None => return Err(PG_ESLOT),
        };
        let art = match emit(o, FAMILY_SEED) {
            Some(x) => x,
            None => return Err(PG_ESEED),
        };
        let line_b = if o.is_line_exception() {
            Line::Musl
        } else {
            Line::Glibc
        };
        let e = Entry::bind(o, art.image, Line::Glibc, line_b, o.is_line_exception());
        if r.register(o, e).is_err() {
            return Err(PG_EPROBE);
        }
        i += 1;
    }
    Ok(r)
}

/// 联签锚自洽：段差与两侧段基址必须对得上，且例外算子数与常量一致。
pub fn anchors_agree() -> bool {
    if ANCHOR_F11203_SEG + ANCHOR_F11203_GAP != 0x6d170 {
        return false;
    }
    if ANCHOR_F11640_SEG != SEG_BASE + ANCHOR_F11640_GAP {
        return false;
    }
    if ANCHOR_F11659_GAP + SEG_BASE != ANCHOR_F11659_SEG {
        return false;
    }
    let mut n = 0usize;
    let table = BreakOp::all();
    let mut i = 0usize;
    while i < OP_COUNT {
        if let Some(o) = table.get(i) {
            if o.is_line_exception() {
                n += 1;
            }
        }
        i += 1;
    }
    n == DECLARED_OPS
}

// ---------------------------------------------------------------------------
// 档位与矩阵
// ---------------------------------------------------------------------------

/// 快照四档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    /// 逐字一致，直接落锚。
    Anchored,
    /// 命中登记例外，落锚并记因。
    Declared,
    /// 成器侧判死，现场作废（不进分母）。
    Void,
    /// 现场没跑齐或落到未判档。
    Unrun,
}

impl Tier {
    /// 四档全表。
    pub fn all() -> [Tier; TIER_COUNT] {
        [Tier::Anchored, Tier::Declared, Tier::Void, Tier::Unrun]
    }

    /// 档名。
    pub fn name(&self) -> &'static str {
        match *self {
            Tier::Anchored => "anchored",
            Tier::Declared => "declared",
            Tier::Void => "void",
            Tier::Unrun => "unrun",
        }
    }

    /// 该档是否是绿档（只有落进快照的两档算绿）。
    pub fn is_green(&self) -> bool {
        match *self {
            Tier::Anchored | Tier::Declared => true,
            _ => false,
        }
    }

    /// 由一次结果归档。
    pub fn of(o: &Outcome) -> Tier {
        if o.refused() {
            return Tier::Void;
        }
        if o.default || o.code == PG_EDEFAULT || o.note == Note::RunMissing {
            return Tier::Unrun;
        }
        if o.ok {
            if o.verdict == Verdict::Declared {
                return Tier::Declared;
            }
            return Tier::Anchored;
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
/// 落进绿档——一条未声明冲突如果被归档成 `Anchored`，快照就把它当成一致交付了。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Matrix {
    rows: [Row; TIER_COUNT],
}

impl Matrix {
    /// 由语料构造。
    pub fn build() -> Matrix {
        let table = Tier::all();
        let mut rows = [Row {
            tier: Tier::Anchored,
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

    /// 四档是否都有落点（覆盖性——空档说明归档逻辑漏了形态）。
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

/// 全绿渲染文本（逐字钉住，改一个字都要红）。
pub const RENDER_OK: &str =
    "probe-gen corpus=14 accept=8 reject=6 base=18 second=18 declared=2 conflict=0 match=36 rate=1000permille\n";

/// 快照结算报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Report {
    /// 结算分母（已登记族的执行次数）。
    pub total: usize,
    /// 正例槽数。
    pub accept: usize,
    /// 反例槽数。
    pub reject: usize,
    /// 逐字一致档计数。
    pub anchored: usize,
    /// 登记例外档计数。
    pub declared: usize,
    /// 现场作废档计数。
    pub void: usize,
    /// 未跑齐档计数。
    pub unrun: usize,
    /// 计入一致的次数。
    pub matched: usize,
    /// 未声明冲突次数。
    pub conflicts: usize,
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
            && self.unrun == 0
            && self.rate() == RATE_FULL
            && self.accept == ACCEPT
            && self.reject == REJECT
            && self.anchored + self.declared == OP_COUNT
    }

    /// 红项计数。
    pub fn reds(&self) -> u32 {
        (self.conflicts as u32) + (self.unrun as u32) + (self.void as u32).saturating_sub(DECLARED_OPS as u32)
    }

    /// 闸门：全绿才放行，否则给一致率不足码。
    pub fn gate(&self) -> Result<(), u32> {
        if self.total == 0 {
            return Err(PG_EMATRIX);
        }
        if self.conflicts > 0 {
            return Err(PG_ENOTE);
        }
        if self.rate() < RATE_FULL {
            return Err(PG_ERATE);
        }
        if self.unrun > 0 {
            return Err(PG_EDEFAULT);
        }
        Ok(())
    }

    /// 四档格数。
    pub fn tier_cells(&self) -> u32 {
        (self.anchored as u32)
            + (self.declared as u32)
            + (self.void as u32)
            + (self.unrun as u32)
    }

    /// 红档格数。
    pub fn red_cells(&self) -> u32 {
        (self.conflicts as u32) + (self.unrun as u32)
    }

    /// 逐字渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("probe-gen corpus=");
        s.push_str(dec_text(self.accept as u32 + self.reject as u32).as_str());
        s.push_str(" accept=");
        s.push_str(dec_text(self.accept as u32).as_str());
        s.push_str(" reject=");
        s.push_str(dec_text(self.reject as u32).as_str());
        s.push_str(" base=");
        s.push_str(
            dec_text(
                (self.anchored as u32 + self.declared as u32).saturating_sub(self.conflicts as u32),
            )
            .as_str(),
        );
        s.push_str(" second=");
        s.push_str(
            dec_text(
                (self.anchored as u32 + self.declared as u32).saturating_sub(self.conflicts as u32),
            )
            .as_str(),
        );
        s.push_str(" declared=");
        s.push_str(dec_text(self.declared as u32).as_str());
        s.push_str(" conflict=");
        s.push_str(dec_text(self.conflicts as u32).as_str());
        s.push_str(" match=");
        s.push_str(dec_text(self.matched as u32).as_str());
        s.push_str(" rate=");
        s.push_str(dec_text(self.rate()).as_str());
        s.push_str("permille\n");
        s
    }
}

/// 空报告（未跑任何探针时的形态，绝不与全绿混淆）。
pub fn empty_report() -> Report {
    Report {
        total: 0,
        accept: 0,
        reject: 0,
        anchored: 0,
        declared: 0,
        void: 0,
        unrun: 0,
        matched: 0,
        conflicts: 0,
    }
}

/// 由族场景与语料两路结算。
///
/// 分子只由快照面给出（族内每条算子 × 双机 = 36 次执行），语料面给出违约项：
/// 反例槽按申报的那一维落红不算违约，正例槽落红、反例槽落错维、或注入出的错报才算。
/// 这样"该红的红"不会污染一致率，"不该红的红了"一定把分子打下来。
fn settle(over: Option<(usize, Fault)>) -> Report {
    let mut rep = Report {
        total: RUN_COUNT,
        accept: ACCEPT,
        reject: REJECT,
        anchored: 0,
        declared: 0,
        void: 0,
        unrun: 0,
        matched: 0,
        conflicts: 0,
    };
    let mut i = 0usize;
    while i < SCENE_COUNT {
        let c = match scene_case(i) {
            Some(x) => x,
            None => return empty_report(),
        };
        match Tier::of(&c.actual()) {
            Tier::Anchored => rep.anchored += 1,
            Tier::Declared => rep.declared += 1,
            // 成器侧判死：这条件本身不成立，双机结果无从比对，按未声明冲突计。
            Tier::Void => {
                rep.void += 1;
                rep.conflicts += 1;
            }
            // 现场没跑齐：既不进分子，也不许装作一致。
            Tier::Unrun => {
                rep.unrun += 1;
                rep.conflicts += 1;
            }
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
    rep.conflicts += violations;
    rep.matched = ((rep.anchored + rep.declared) * 2usize).saturating_sub(violations);
    rep.total = RUN_COUNT;
    rep
}

/// 主结算：全绿才给报告，否则给闸门码。
pub fn run() -> Result<Report, u32> {
    let rep = settle(None);
    match rep.gate() {
        Ok(()) => Ok(rep),
        Err(code) => Err(code),
    }
}

/// 带注入的结算：越界槽给槽位码，否则返回注入后的报告。
pub fn run_with(over: Option<(usize, Fault)>) -> Result<Report, u32> {
    if let Some((slot, _)) = over {
        if slot >= CORPUS {
            return Err(PG_ESLOT);
        }
    }
    Ok(settle(over))
}

/// 报告与矩阵是否互相印证（两条独立路径必须给同一个分子）。
pub fn report_agrees_with_matrix() -> bool {
    let rep = settle(None);
    let m = Matrix::build();
    let green_cells = match m.row(Tier::Anchored) {
        Some(r) => r.good,
        None => 0,
    };
    let declared_cells = match m.row(Tier::Declared) {
        Some(r) => r.good,
        None => 0,
    };
    rep.anchored > 0 && green_cells > 0 && declared_cells == rep.declared
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
        let r = match build_registry() {
            Ok(x) => x,
            Err(_) => return [false, false, false, false],
        };
        r.family_complete() && !r.duplicated()
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
        all_ok && scenes_agree()
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
// 阶段闸门：Seed→Emit→Register→Sandbox→RunBase→RunSecond→Compare→Rate
// ---------------------------------------------------------------------------

/// 一次快照结算的走闸状态机。
///
/// 这条纵线为什么不能倒：登记（Register）必须先于落盘（Sandbox），落盘必须先于执行
/// （RunBase/RunSecond），执行必须先于比对（Compare），比对必须先于一致率（Rate）。
/// 一旦允许倒退，就能出现"先出结论再补登记"——野探针靠回填混进快照，本卡的判据
/// 当场失效。所以倒退判死（`PG_ESTAGE`），关闭后再走一步也判死（`PG_ESTATE`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Gate {
    /// 当前档；`None` 表示还没进过任何闸。
    current: Option<Stage>,
    /// 是否已封账。
    closed: bool,
    /// 成功走档次数。
    steps: u32,
    /// 被拒次数（倒退或封账后再走）。
    refusals: u32,
    /// 走档轨迹指纹。
    root: u64,
}

impl Gate {
    /// 空闸门：未进档、未封账。
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
            return Err(PG_ESTATE);
        }
        if let Some(cur) = self.current {
            if s.rank() < cur.rank() {
                self.refusals += 1;
                return Err(PG_ESTAGE);
            }
        }
        self.root = fold_word(self.root, s.rank() as u64);
        self.root = fold_str(self.root, s.name());
        self.current = Some(s);
        self.steps += 1;
        Ok(())
    }

    /// 封账：之后任何走档与再次封账都拒。
    pub fn close(&mut self) -> Result<u64, u32> {
        if self.closed {
            self.refusals += 1;
            return Err(PG_ESTATE);
        }
        if self.current.is_none() {
            return Err(PG_ESTAGE);
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

    /// 依次走一串档，任一步被拒就把那个码交出来。
    pub fn walk(stages: &[Stage]) -> Result<Gate, u32> {
        let mut g = Gate::new();
        let mut i = 0usize;
        while i < stages.len() {
            let s = match stages.get(i) {
                Some(x) => *x,
                None => return Err(PG_ESLOT),
            };
            match g.step(s) {
                Ok(()) => {}
                Err(code) => return Err(code),
            }
            i += 1;
        }
        Ok(g)
    }

    /// 全档正序走一遍并封账，返回指纹（判据与测试共用的正例路径）。
    pub fn full_pass() -> Result<u64, u32> {
        let table = Stage::all();
        let mut g = Gate::new();
        let mut i = 0usize;
        while i < table.len() {
            let s = match table.get(i) {
                Some(x) => *x,
                None => return Err(PG_ESLOT),
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

/// 红档格在账本里的算子号。
pub const OPID_STAT_RED: u8 = 0x08;
/// 六个缺陷码格的算子号起点。
pub const DEFECT_OP_BASE: u8 = 0x21;

/// 一格台账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sample {
    /// 槽号，必须严格等于登记时的当前长度。
    pub slot: u8,
    /// 这一格对应的算子号（统计格用 `STAT_OPS`，缺陷格用 `DEFECT_OP_BASE + n`）。
    pub op: u8,
    /// 这一格记的是成功还是败。
    pub ok: bool,
    /// 错误码（成功格必须为 0）。
    pub code: u32,
    /// 统计值（败格必须为 0）。
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
/// 本卡不许"先出报告再补台账"，所以这里的 `rejects` 是一个要判死的量。
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
            return Err(PG_ESTATE);
        }
        if self.len >= SAMPLES {
            self.rejects += 1;
            return Err(PG_ESLOT);
        }
        if s.slot as usize != self.len {
            self.rejects += 1;
            return Err(PG_ESLOT);
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

    /// 冻结后的指纹；未冻结时为 0。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 逐格复核：统计格对上 `VALS`、红档格对上 `RED_RATE`、缺陷格对上 `REJECT_CODES`
    /// 且败格值必须为 0，缺陷码两两互异。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES {
            return Err(PG_ESLOT);
        }
        let stat_last = match STAT_CELLS.checked_add(1) {
            Some(x) => x,
            None => return Err(PG_ESLOT),
        };
        let mut i = 0usize;
        while i < stat_last {
            let s = match self.sample(i) {
                Some(x) => x,
                None => return Err(PG_ESLOT),
            };
            if !s.ok {
                return Err(PG_ESTATE);
            }
            if s.code != 0 {
                return Err(PG_ECODE);
            }
            let want_value = match VALS.get(i) {
                Some(v) => *v,
                None => return Err(PG_ESLOT),
            };
            if s.value != want_value {
                return Err(PG_EVALUE);
            }
            let want_op = match self.stat_op_want(i) {
                Some(v) => v,
                None => return Err(PG_ESLOT),
            };
            if s.op != want_op {
                return Err(PG_ECODE);
            }
            i += 1;
        }
        let mut j = 0usize;
        while j < REJECT {
            let cell = match STAT_CELLS.checked_add(1).and_then(|x| x.checked_add(j)) {
                Some(x) => x,
                None => return Err(PG_ESLOT),
            };
            let s = match self.sample(cell) {
                Some(x) => x,
                None => return Err(PG_ESLOT),
            };
            if s.ok {
                return Err(PG_ESTATE);
            }
            if s.value != 0 {
                return Err(PG_EVALUE);
            }
            let want_code = match REJECT_CODES.get(j) {
                Some(c) => *c,
                None => return Err(PG_ESLOT),
            };
            if s.code != want_code {
                return Err(PG_ECODE);
            }
            let want_op = match DEFECT_OP_BASE.checked_add(j as u8) {
                Some(x) => x,
                None => return Err(PG_ESLOT),
            };
            if s.op != want_op {
                return Err(PG_ECODE);
            }
            let mut k = j + 1usize;
            while k < REJECT {
                let other = match REJECT_CODES.get(k) {
                    Some(c) => *c,
                    None => return Err(PG_ESLOT),
                };
                if other == want_code {
                    return Err(PG_ECODE);
                }
                k += 1;
            }
            j += 1;
        }
        Ok(())
    }

    /// 第一处不合规的格子与原因，给排障用（不改判定，只给指认）。
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

    /// 冻结：先复核再折叠，复核不过一律拒（红账不许盖戳）。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(PG_ESTATE);
        }
        match self.audit() {
            Ok(()) => {}
            Err(code) => return Err(code),
        }
        let h = self.seal();
        if h == 0 {
            return Err(PG_ESEAL);
        }
        self.root = h;
        self.frozen = true;
        Ok(h)
    }

    /// 重放：以同一批格子重建账本，比指纹。任何一格被改过都不许相等。
    pub fn replay(&self) -> Result<u64, u32> {
        if !self.frozen {
            return Err(PG_ESTATE);
        }
        let mut fresh = Ledger::new();
        let mut i = 0usize;
        while i < self.len {
            let s = match self.sample(i) {
                Some(x) => x,
                None => return Err(PG_ESEAL),
            };
            match fresh.register(s) {
                Ok(()) => {}
                Err(_) => return Err(PG_ESEAL),
            }
            i += 1;
        }
        let h = fresh.seal();
        if h != self.root {
            return Err(PG_ESEAL);
        }
        Ok(h)
    }

    /// 改动一格（仅未冻结时允许），用于注入红。
    pub fn poke(&mut self, i: usize, value: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(PG_ESTATE);
        }
        match self.slots.get_mut(i) {
            Some(cell) => {
                match *cell {
                    Some(ref mut s) => {
                        s.value = value;
                    }
                    None => return Err(PG_ESLOT),
                }
                Ok(())
            }
            None => Err(PG_ESLOT),
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

/// 注入一维错的台账（用于证明复核真的在逐格看，而不是只看格数）。
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
// 十三条判据谓词（全部私有，只在 run_unx11639_checks 里汇总）
// ---------------------------------------------------------------------------

/// 01 名称与段基址：常量层自洽，任何一条规模关系破了都算红。
fn name_ok() -> bool {
    if SEG_BASE == 0 {
        return false;
    }
    if SEG_BASE != 0x6d170 {
        return false;
    }
    if CORPUS != SAMPLES {
        return false;
    }
    if ACCEPT + REJECT != CORPUS {
        return false;
    }
    if TIER_COUNT != 4 {
        return false;
    }
    if OP_COUNT != BreakOp::all().len() {
        return false;
    }
    if RUN_COUNT != OP_COUNT * 2 {
        return false;
    }
    if STAT_CELLS + 1 != ACCEPT {
        return false;
    }
    if STAT_CELLS + 1 + REJECT != SAMPLES {
        return false;
    }
    if RED_RATE != 929 || RATE_FULL != 1000 {
        return false;
    }
    if DECLARED_OPS != 2 {
        return false;
    }
    if SANDBOX_PREFIX.len() == 0 || HOST_LIB_PREFIX.len() == 0 {
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
        if f.code() != c {
            return false;
        }
        if fault_ndx(f) != k {
            return false;
        }
        k += 1;
    }
    if Fault::Ok.code() != 0 {
        return false;
    }
    true
}

/// 03 十八算子全覆盖：号、名、slug 三者互异且可回指；例外资格恰为两条。
fn ops_ok() -> bool {
    let table = BreakOp::all();
    if table.len() != OP_COUNT {
        return false;
    }
    if OP_IDS.len() != OP_COUNT {
        return false;
    }
    let mut i = 0usize;
    while i < OP_COUNT {
        let o = match table.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if o.index() != i {
            return false;
        }
        let want = match OP_IDS.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if o.id() != want {
            return false;
        }
        if BreakOp::from_id(want) != Some(o) {
            return false;
        }
        if o.slug().len() == 0 || o.probe_name().len() == 0 {
            return false;
        }
        if o.tail_on(Line::Glibc).len() == 0 && o.loads_on(Line::Glibc) == false {
            return false;
        }
        let mut j = i + 1usize;
        while j < OP_COUNT {
            if let Some(p) = table.get(j) {
                if p.id() == o.id() || p.slug() == o.slug() {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    if BreakOp::from_id(0) != None || BreakOp::from_id(0xff) != None {
        return false;
    }
    let mut declared = 0usize;
    let mut i = 0usize;
    while i < OP_COUNT {
        if let Some(o) = table.get(i) {
            if o.is_line_exception() {
                declared += 1;
                if o.exception_pair().is_none() {
                    return false;
                }
            } else if o.exception_pair().is_some() {
                return false;
            }
        }
        i += 1;
    }
    declared == DECLARED_OPS
}

/// 04 成器纯函数：同种子同算子必得同件；禁种子一律不给流。
fn emit_ok() -> bool {
    if !emit_is_pure() {
        return false;
    }
    if !ops_diverge() {
        return false;
    }
    if Stream::new(FORBIDDEN_SEED_ZERO).is_some() {
        return false;
    }
    if Stream::new(FORBIDDEN_SEED_CLOCK).is_some() {
        return false;
    }
    if Stream::new(FORBIDDEN_SEED_ONES).is_some() {
        return false;
    }
    if !seed_forbidden(FORBIDDEN_SEED_CLOCK) || seed_forbidden(0x2a17_0001) {
        return false;
    }
    let mut s = match Stream::new(FAMILY_SEED) {
        Some(x) => x,
        None => return false,
    };
    let a = s.next_u32();
    let b = s.next_u32();
    if a == b {
        return false;
    }
    if s.state() == 0 {
        return false;
    }
    match emit(BreakOp::Master, FAMILY_SEED) {
        Some(x) => {
            if x.bytes != BASE_BYTES || x.head.len() != PATCH_LEN || x.image == 0 {
                return false;
            }
            if x.op != BreakOp::Master {
                return false;
            }
        }
        None => return false,
    }
    match emit(BreakOp::Truncate, FAMILY_SEED) {
        Some(x) => {
            if x.bytes >= BASE_BYTES {
                return false;
            }
        }
        None => return false,
    }
    match emit(BreakOp::Class32, FAMILY_SEED) {
        Some(x) => {
            if let Some(h) = x.head.get(4) {
                if *h != ELFCLASS32 {
                    return false;
                }
            } else {
                return false;
            }
        }
        None => return false,
    }
    match emit(BreakOp::Machine386, FAMILY_SEED) {
        Some(x) => {
            match (x.head.get(4), x.head.get(6)) {
                (Some(c), Some(m)) => {
                    if *c != ELFCLASS64 || *m != (EM_386 & 0xff) as u8 {
                        return false;
                    }
                }
                _ => return false,
            }
        }
        None => return false,
    }
    match emit(BreakOp::BadMagic, FAMILY_SEED) {
        Some(x) => {
            if let Some(h) = x.head.get(0) {
                if *h == MAGIC0 {
                    return false;
                }
            } else {
                return false;
            }
        }
        None => return false,
    }
    true
}

/// 05 族登记：十八格齐、无重复、指纹可复核；重复登记与漏登记都必须拒。
fn registry_ok() -> bool {
    let r = match build_registry() {
        Ok(x) => x,
        Err(_) => return false,
    };
    if !r.family_complete() {
        return false;
    }
    if r.duplicated() || r.is_empty() {
        return false;
    }
    if r.len() != OP_COUNT {
        return false;
    }
    if r.seal() == 0 {
        return false;
    }
    let table = BreakOp::all();
    let mut i = 0usize;
    while i < OP_COUNT {
        let o = match table.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if r.entry(o).is_none() {
            return false;
        }
        if r.verify_against(o, FAMILY_SEED).is_err() {
            return false;
        }
        i += 1;
    }
    // 重复登记必须拒，且拒了不许把账改脏。
    let mut dup = Registry::new();
    let first = match emit(BreakOp::Master, FAMILY_SEED) {
        Some(x) => x,
        None => return false,
    };
    let e = Entry::bind(
        BreakOp::Master,
        first.image,
        Line::Glibc,
        Line::Glibc,
        false,
    );
    if dup.register(BreakOp::Master, e).is_err() {
        return false;
    }
    match dup.register(BreakOp::Master, e) {
        Err(code) => {
            if code != PG_EPROBE {
                return false;
            }
        }
        Ok(()) => return false,
    }
    if !dup.duplicated() || dup.len() != 1 {
        return false;
    }
    true
}

/// 06 双机快照逐字一致：族内每条都落进分子，判据自申报自洽。
fn scene_ok() -> bool {
    if !scenes_agree() {
        return false;
    }
    if SCENE_COUNT != OP_COUNT {
        return false;
    }
    let mut anchored = 0usize;
    let mut i = 0usize;
    while i < SCENE_COUNT {
        match scene_anchored(i) {
            Some(true) => anchored += 1,
            Some(false) => return false,
            None => return false,
        }
        i += 1;
    }
    if anchored != OP_COUNT {
        return false;
    }
    match scene_at(0) {
        Some(s) => {
            if s.line_a != Line::Glibc || s.line_b != Line::Glibc {
                return false;
            }
        }
        None => return false,
    }
    true
}

/// 07 例外成对申报：只有 TEXTREL 与符号版本校验两条走跨线，且必须成对。
fn declare_ok() -> bool {
    let table = BreakOp::all();
    let mut cross = 0usize;
    let mut i = 0usize;
    while i < OP_COUNT {
        let o = match table.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let s = match scene_of(o) {
            x => x,
        };
        if o.is_line_exception() {
            cross += 1;
            if s.line_a != Line::Glibc || s.line_b != Line::Musl {
                return false;
            }
            if !s.declared || s.declared_half || !s.text_drift {
                return false;
            }
            let pair = match o.exception_pair() {
                Some(x) => x,
                None => return false,
            };
            // 例外一定是"一侧报错、一侧安静"：恰好一侧有文本才算成对申报，
            // 两侧都报（那是普通冲突）或两侧都静（那根本没差异）都不许记成例外。
            let quiet_a = pair.0.is_empty();
            let quiet_b = pair.1.is_empty();
            if quiet_a == quiet_b {
                return false;
            }
            if o.tail_on(Line::Glibc) != pair.0 || o.tail_on(Line::Musl) != pair.1 {
                return false;
            }
            // 安静必须和"装载成功"对齐：报错的那条线不许还声称自己装上了。
            if o.loads_on(Line::Glibc) != quiet_a || o.loads_on(Line::Musl) != quiet_b {
                return false;
            }
        } else {
            if s.line_a != s.line_b {
                return false;
            }
            if s.declared || s.text_drift {
                return false;
            }
        }
        i += 1;
    }
    if cross != DECLARED_OPS {
        return false;
    }
    if !Line::Glibc.checks_versions() || Line::Musl.checks_versions() {
        return false;
    }
    if !Line::Glibc.allows_textrel() || Line::Musl.allows_textrel() {
        return false;
    }
    if Line::Glibc.label() == Line::Musl.label() {
        return false;
    }
    anchors_agree()
}

/// 08 语料十四槽申报互校：申报表与第二形式的 `WANT_*` 表任一处笔误都要露出来。
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
        // 第一道：`WANT_*` 是现场真结论的第二写法，必须和算出来的逐维对上。
        // 这条会把成器/申报两张表里的任何一处笔误与正文实现分开暴露。
        let a = c.actual();
        if a.stage != wst
            || a.code != wc
            || a.note != wn
            || a.default != wd
            || a.value != wv
            || a.ok() != ws
        {
            return false;
        }
        if c.tag() != wt {
            return false;
        }
        // 第二道：判序结果必须就是这一槽声明的缺陷维。
        if judge(&c) != dead {
            return false;
        }
        // 第三道：申报与真结论**只能**在本槽那一维分叉。多一维说明语料把两处
        // 谎叠在一槽里（判序会被前一维吃掉），少一维说明这一槽根本没坏。
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
        if c.claim.note != wn {
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
    // 后六槽一槽只坏一维，依次挂 Estate/Stage/Code/Value/Note/Default。
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
        if want == Fault::Ok {
            return false;
        }
        let c = match case_at(slot) {
            Some(x) => x,
            None => return false,
        };
        if judge(&c) != want {
            return false;
        }
        if let Some(rc) = REJECT_CODES.get(k) {
            if *rc != want.code() {
                return false;
            }
        } else {
            return false;
        }
        if c.tag().len() < 4 {
            return false;
        }
        k += 1;
    }
    let mut t = 0usize;
    while t < CORPUS {
        let a = match WANT_TAGS.get(t) {
            Some(x) => *x,
            None => return false,
        };
        if a.len() == 0 {
            return false;
        }
        let mut u = t + 1usize;
        while u < CORPUS {
            if let Some(b) = WANT_TAGS.get(u) {
                if *b == a {
                    return false;
                }
            }
            u += 1;
        }
        t += 1;
    }
    true
}

/// 09 判序六维互斥：成败互报优先于一切，兜底档永远最后兜住。
fn judge_ok() -> bool {
    let base = match case_at(0) {
        Some(x) => x,
        None => return false,
    };
    let actual = base.actual();
    // 每一维单独改坏，都必须只挂那一维。
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
    // 成败同时改坏时，Estate 必须压过后五维。
    let all_bad = Case {
        shape: base.shape,
        claim: Claim {
            success: !base.claim.success(),
            stage: match Stage::of_rank(actual.stage.rank() + 1) {
                Some(x) => x,
                None => Stage::Rate,
            },
            code: PG_ESTATE,
            value: actual.value + 9,
            note: Note::WildProbe,
            verdict: Verdict::Conflict,
            default: true,
            tag: head_tag(),
        },
    };
    if judge(&all_bad) != Fault::Estate {
        return false;
    }
    // 阶段谎报只准差一格，而且只差这一维：申报说已到上一闸，其余照抄真结论。
    // 现场已经在最后一闸（Compare），所以取前一闸，绝不许越过闸尾。
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
            code: PG_ERATE,
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
            note: Note::ImageDrift,
            ..base.claim
        },
    };
    if judge(&note_only) != Fault::Note {
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
    // 六维序位回环。
    let faults = Fault::reject_faults();
    let mut i = 0usize;
    while i < faults.len() {
        let f = match faults.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if ndx_fault(fault_ndx(f)) != Some(f) {
            return false;
        }
        if f.name().len() == 0 {
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
    if !o.default || o.stage != Stage::Rate || o.code != PG_EDEFAULT {
        return false;
    }
    if o.exit() != EXIT_UNJUDGED {
        return false;
    }
    true
}

/// 10 四档矩阵覆盖：每档都有落点，正例不落绿档即红，反例落绿档更是直接判死。
fn matrix_ok() -> bool {
    let m = Matrix::build();
    if m.slots() != CORPUS * TIER_COUNT {
        return false;
    }
    if m.goods() != CORPUS {
        return false;
    }
    if !m.covered() {
        return false;
    }
    if m.red_total() + m.goods() != m.slots() {
        return false;
    }
    let anchored = match m.row(Tier::Anchored) {
        Some(x) => x,
        None => return false,
    };
    if anchored.good != ACCEPT - DECLARED_OPS || anchored.total != CORPUS {
        return false;
    }
    let declared = match m.row(Tier::Declared) {
        Some(x) => x,
        None => return false,
    };
    if declared.good != DECLARED_OPS {
        return false;
    }
    let void = match m.row(Tier::Void) {
        Some(x) => x,
        None => return false,
    };
    if void.good == 0 || void.tier.is_green() {
        return false;
    }
    let unrun = match m.row(Tier::Unrun) {
        Some(x) => x,
        None => return false,
    };
    if unrun.good == 0 || unrun.tier.is_green() {
        return false;
    }
    if m.render().len() == 0 {
        return false;
    }
    if Tier::all().len() != TIER_COUNT {
        return false;
    }
    let mut green = 0usize;
    let mut i = 0usize;
    while i < TIER_COUNT {
        if let Some(t) = Tier::all().get(i) {
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
    if rep.total != RUN_COUNT || rep.accept != ACCEPT || rep.reject != REJECT {
        return false;
    }
    if rep.rate() != RATE_FULL {
        return false;
    }
    if rep.matched != RUN_COUNT {
        return false;
    }
    if rep.conflicts != 0 || rep.unrun != 0 || rep.red_cells() != 0 {
        return false;
    }
    // 四档各占一格：十八个场景每条只落一档，格数必须正好等于场景数。
    if rep.tier_cells() != OP_COUNT as u32 {
        return false;
    }
    if rep.reds() != 0 {
        return false;
    }
    if empty_report().green() || empty_report().rate() != 0 {
        return false;
    }
    if !report_agrees_with_matrix() {
        return false;
    }
    if !red_rate_calibrated() {
        return false;
    }
    true
}

/// 12 阶段闸门不倒退：正序走满八闸，倒退与封账后再走都判死。
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
    let last = match table.get(table.len() - 1) {
        Some(x) => *x,
        None => return false,
    };
    let early = match table.get(2) {
        Some(x) => *x,
        None => return false,
    };
    // 同档重复步进允许（幂等），倒退不允许。
    if g.step(last).is_err() {
        return false;
    }
    match g.step(early) {
        Err(code) => {
            if code != PG_ESTAGE {
                return false;
            }
        }
        Ok(()) => return false,
    }
    if g.refusals() != 1 || g.stage() != Some(last) {
        return false;
    }
    if g.render().len() == 0 {
        return false;
    }
    let root = match g.close() {
        Ok(x) => x,
        Err(_) => return false,
    };
    if root == 0 || !g.is_closed() {
        return false;
    }
    if let Some(first) = table.get(0) {
        match g.step(*first) {
            Err(code) => {
                if code != PG_ESTATE {
                    return false;
                }
            }
            Ok(()) => return false,
        }
    }
    match g.close() {
        Err(code) => {
            if code != PG_ESTATE {
                return false;
            }
        }
        Ok(_) => return false,
    }
    if g.refusals() != 3 {
        return false;
    }
    if Gate::full_pass().is_err() {
        return false;
    }
    if Gate::new().close().is_ok() {
        return false;
    }
    let back = [Stage::Compare, Stage::Sandbox];
    match Gate::walk(&back) {
        Err(code) => {
            if code != PG_ESTAGE {
                return false;
            }
        }
        Ok(_) => return false,
    }
    // 阶段自身的码表：八闸的落点码两两不同，且秩回环一致。
    let stage_n = table.len();
    let mut k = 0usize;
    while k < stage_n {
        let a = match table.get(k) {
            Some(x) => *x,
            None => return false,
        };
        if Stage::of_rank(a.rank()) != Some(a) {
            return false;
        }
        if a.name().len() == 0 {
            return false;
        }
        if k > 0 && a.rank() <= (match table.get(k - 1) {
            Some(p) => p.rank(),
            None => return false,
        }) {
            return false;
        }
        if a.is_observed() != (a.rank() >= Stage::RunBase.rank()) {
            return false;
        }
        let mut j = k + 1usize;
        while j < stage_n {
            if let Some(b) = table.get(j) {
                if b.fault_code() == a.fault_code() {
                    return false;
                }
            }
            j += 1;
        }
        k += 1;
    }
    if Stage::of_rank(0).is_some() || Stage::of_rank(9).is_some() {
        return false;
    }
    true
}

/// 13 台账：十四格逐格复核、冻结盖戳、重放比指纹。
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
    if l.audit().is_err() {
        return false;
    }
    if l.first_mismatch().is_some() {
        return false;
    }
    if l.render().len() == 0 {
        return false;
    }
    // 跳号与重号都拒收，且一律计数。
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
    // 三个注入红的台账都必须被逐格复核抓到。
    let mut b = 0u8;
    while b < 3 {
        let broken = build_ledger_broken(b);
        if broken.audit().is_ok() {
            return false;
        }
        if broken.first_mismatch().is_none() {
            return false;
        }
        b += 1;
    }
    let wrong_op = {
        let x = build_ledger();
        x.sample(0).is_some()
    };
    if !wrong_op {
        return false;
    }
    let root = match l.freeze() {
        Ok(x) => x,
        Err(_) => return false,
    };
    if root == 0 || !l.is_frozen() {
        return false;
    }
    if l.root() != root {
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
    if l.freeze().is_ok() {
        return false;
    }
    if l.poke(0, 7).is_ok() {
        return false;
    }
    if l.audit().is_err() {
        return false;
    }
    if l.sample(SAMPLES).is_some() {
        return false;
    }
    if l.sample(0).is_none() {
        return false;
    }
    let mut fresh = Ledger::new();
    if fresh.replay().is_ok() || fresh.freeze().is_ok() {
        return false;
    }
    true
}

/// 申报表第一格的标签：跨槽复用时的缺省 tag，取值一律走 `.get()`。
pub fn head_tag() -> &'static str {
    match WANT_TAGS.get(0) {
        Some(x) => *x,
        None => "f11639-master",
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
pub fn run_unx11639_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11639_probe_gen_snapshot");
    s.add("PG01-名称与段基址", name_ok(), "static");
    s.add("PG02-十三码互异", codes_ok(), "static");
    s.add("PG03-十八算子全覆盖", ops_ok(), "static");
    s.add("PG04-成器纯函数", emit_ok(), "static");
    s.add("PG05-族登记无重复", registry_ok(), "static");
    s.add("PG06-双机快照逐字一致", scene_ok(), "static");
    s.add("PG07-例外成对申报", declare_ok(), "static");
    s.add("PG08-语料十四槽互校", corpus_ok(), "static");
    s.add("PG09-判序六维互斥", judge_ok(), "static");
    s.add("PG10-四档矩阵覆盖", matrix_ok(), "static");
    s.add("PG11-全绿渲染逐字", render_ok(), "static");
    s.add("PG12-阶段闸门不倒退", gate_ok(), "static");
    s.add(
        "PG13-冻结重放四注入",
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

    /// 取一档的族件；取不到就直接结束本测试，绝不拿默认值冒充结果。
    fn art(op: BreakOp) -> Option<Artifact> {
        emit(op, FAMILY_SEED)
    }

    #[test]
    fn t01_seg_base_is_pinned() {
        assert_eq!(SEG_BASE, 0x6d170);
        assert_eq!(ANCHOR_F11203_SEG + ANCHOR_F11203_GAP, SEG_BASE);
        assert_eq!(ANCHOR_F11640_SEG, SEG_BASE + ANCHOR_F11640_GAP);
        assert_eq!(ANCHOR_F11659_SEG, SEG_BASE + ANCHOR_F11659_GAP);
    }

    #[test]
    fn t02_thirteen_codes_live_inside_the_segment() {
        assert_eq!(ALL_CODES.len(), 13);
        let mut i = 0usize;
        while i < ALL_CODES.len() {
            assert!(ALL_CODES[i] > SEG_BASE);
            assert!(ALL_CODES[i] <= SEG_BASE + 0x0d);
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
            k += 1;
        }
    }

    #[test]
    fn t05_vals_cells_are_the_pinned_statistics() {
        assert_eq!(VALS[0] as usize, OP_COUNT);
        assert_eq!(VALS[1] as usize, OP_COUNT);
        assert_eq!(VALS[2] as usize, RUN_COUNT);
        assert_eq!(VALS[3] as usize, RUN_COUNT - DECLARED_OPS);
        assert_eq!(VALS[4] as usize, DECLARED_OPS);
        assert_eq!(VALS[5], 0);
        assert_eq!(VALS[6], RATE_FULL);
        assert_eq!(VALS[7], RED_RATE);
    }

    #[test]
    fn t06_fold_byte_matches_single_byte_string() {
        let h = FNV_OFF;
        assert_eq!(fold_byte(h, b'A'), fold_str(h, "A"));
        assert_ne!(fold_byte(h, b'A'), h);
        assert_eq!(fold_str(h, "onx"), fold_str(fold_str(fold_str(h, "o"), "n"), "x"));
    }

    #[test]
    fn t07_fold_helpers_are_not_identities() {
        let h = FNV_OFF;
        assert_ne!(fold_word(h, 0), h);
        assert_ne!(fold_bool(h, false), fold_bool(h, true));
        assert_ne!(fold_i32(h, -1), fold_i32(h, 1));
        assert_ne!(fold_word(h, 1), fold_word(h, 2));
    }

    #[test]
    fn t08_decimal_and_hex_renderers() {
        assert_eq!(dec_text(0).as_str(), "0");
        assert_eq!(dec_text(36).as_str(), "36");
        assert_eq!(u32_text(1000).as_str(), "1000");
        assert_eq!(hex_text(0x0a, 4).as_str(), "000a");
        assert_eq!(hex_text(SEG_BASE, 6).as_str(), "6d170");
        assert_eq!(i32_text(-2).as_str(), "-2");
        assert_eq!(i32_text(0).as_str(), "0");
    }

    #[test]
    fn t09_segment_and_permille_text() {
        assert_eq!(seg_text(SEG_BASE).as_str(), "0x6d170");
        assert_eq!(permille_text(929).as_str(), "929permille");
        // 收库修正（AI-INTAKE-1107）：bool_text 返回 &str，&str::as_str 未稳定，直接比较。
        assert_eq!(bool_text(true), "true");
        assert_eq!(bool_text(false), "false");
    }

    #[test]
    fn t10_forbidden_seeds_are_rejected() {
        assert!(Stream::new(FORBIDDEN_SEED_ZERO).is_none());
        assert!(Stream::new(FORBIDDEN_SEED_CLOCK).is_none());
        assert!(Stream::new(FORBIDDEN_SEED_ONES).is_none());
        assert!(Stream::new(FAMILY_SEED).is_some());
        assert!(seed_forbidden(FORBIDDEN_SEED_ZERO));
        assert!(!seed_forbidden(0x2a17_0001));
        assert_eq!(FORBIDDEN_SEEDS.len(), FORBIDDEN_SEED_COUNT);
    }

    #[test]
    fn t11_stream_is_deterministic_and_stateful() {
        let mut a = match Stream::new(FAMILY_SEED) {
            Some(x) => x,
            None => return,
        };
        let mut b = match Stream::new(FAMILY_SEED) {
            Some(x) => x,
            None => return,
        };
        let mut i = 0usize;
        while i < 8 {
            assert_eq!(a.next_u32(), b.next_u32());
            i += 1;
        }
        assert_ne!(a.state(), 0);
        assert_eq!(a.state(), b.state());
    }

    #[test]
    fn t12_emit_is_a_pure_function() {
        assert!(emit_is_pure());
        assert!(emit_ok());
    }

    #[test]
    fn t13_master_is_the_healthy_baseline() {
        let a = match art(BreakOp::Master) {
            Some(x) => x,
            None => return,
        };
        assert_eq!(a.bytes, BASE_BYTES);
        assert_eq!(a.head.len(), PATCH_LEN);
        assert_eq!(a.head[0], MAGIC0);
        assert_eq!(a.head[1], b'E');
        assert_eq!(a.head[2], b'L');
        assert_eq!(a.head[3], b'F');
        assert_eq!(a.head[4], ELFCLASSNUM);
        assert_ne!(a.image, 0);
        assert_eq!(a.image, a.seal());
        assert_eq!(a.op, BreakOp::Master);
        assert_eq!(a.seed, FAMILY_SEED);
    }

    #[test]
    fn t14_truncate_stays_below_the_ident_block() {
        let a = match art(BreakOp::Truncate) {
            Some(x) => x,
            None => return,
        };
        assert!(a.bytes < TRUNC_BELOW_IDENT);
        assert!(a.bytes >= 1);
        assert!(a.bytes < BASE_BYTES);
        assert_eq!(a.head[0], MAGIC0);
    }

    #[test]
    fn t15_class_and_machine_slots_are_patched() {
        let c = match art(BreakOp::Class32) {
            Some(x) => x,
            None => return,
        };
        assert_eq!(c.head[4], ELFCLASS32);
        let m = match art(BreakOp::Machine386) {
            Some(x) => x,
            None => return,
        };
        assert_eq!(m.head[4], ELFCLASS64);
        assert_eq!(m.head[6], (EM_386 & 0xff) as u8);
        assert_eq!(m.head[7], (EM_386 >> 8) as u8);
        assert_ne!(c.image, m.image);
    }

    #[test]
    fn t16_bad_magic_loses_the_ident_byte() {
        let a = match art(BreakOp::BadMagic) {
            Some(x) => x,
            None => return,
        };
        assert_ne!(a.head[0], MAGIC0);
        assert_eq!(a.head[0], MAGIC0.wrapping_add(1));
        assert_eq!(a.bytes, BASE_BYTES);
    }

    #[test]
    fn t17_the_family_diverges_everywhere() {
        assert!(ops_diverge());
        let table = BreakOp::all();
        assert_eq!(table.len(), OP_COUNT);
        let mut i = 0usize;
        while i < OP_COUNT {
            let mut j = i + 1usize;
            while j < OP_COUNT {
                let a = match emit(table[i], FAMILY_SEED) {
                    Some(x) => x,
                    None => return,
                };
                let b = match emit(table[j], FAMILY_SEED) {
                    Some(x) => x,
                    None => return,
                };
                assert_ne!(a.image, b.image);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn t18_operator_ids_round_trip() {
        let table = BreakOp::all();
        let mut i = 0usize;
        while i < OP_COUNT {
            let op = table[i];
            assert_eq!(op.id(), OP_IDS[i]);
            assert_eq!(BreakOp::from_id(op.id()), Some(op));
            assert_eq!(op.index(), i);
            i += 1;
        }
        assert!(BreakOp::from_id(0).is_none());
        assert!(BreakOp::from_id(OP_COUNT as u8 + 1).is_none());
    }

    #[test]
    fn t19_slugs_and_probe_names() {
        let table = BreakOp::all();
        let mut i = 0usize;
        while i < OP_COUNT {
            let op = table[i];
            assert_eq!(op_by_slug(op.slug()), Some(op));
            let name = op.probe_name();
            assert!(name.starts_with("libbx11639-"));
            assert!(name.ends_with(".so"));
            i += 1;
        }
        assert!(op_by_slug("no-such-op").is_none());
        assert!(op_by_slug(TEXT_EMPTY).is_none());
    }

    #[test]
    fn t20_registry_registers_the_whole_family() {
        let r = match build_registry() {
            Ok(x) => x,
            Err(_) => return,
        };
        assert!(r.family_complete());
        assert_eq!(r.len(), OP_COUNT);
        assert!(!r.duplicated());
        assert!(!r.is_empty());
        assert_ne!(r.seal(), 0);
        let table = BreakOp::all();
        let mut i = 0usize;
        while i < OP_COUNT {
            assert!(r.entry(table[i]).is_some());
            assert_eq!(r.verify_against(table[i], FAMILY_SEED), Ok(()));
            i += 1;
        }
    }

    #[test]
    fn t21_registry_refuses_a_second_registration() {
        let mut r = Registry::new();
        let a = match art(BreakOp::Master) {
            Some(x) => x,
            None => return,
        };
        let e = Entry::bind(BreakOp::Master, a.image, Line::Glibc, Line::Glibc, false);
        assert_eq!(r.register(BreakOp::Master, e), Ok(()));
        assert_eq!(r.register(BreakOp::Master, e), Err(PG_EPROBE));
        assert!(r.duplicated());
        assert_eq!(r.len(), 1);
        assert!(!r.family_complete());
        assert!(!e.render().as_str().is_empty());
        assert_ne!(e.seal(), 0);
    }

    #[test]
    fn t22_every_scene_lands_an_anchor() {
        assert!(scenes_agree());
        assert_eq!(scenes().len(), SCENE_COUNT);
        let mut i = 0usize;
        while i < SCENE_COUNT {
            assert_eq!(scene_anchored(i), Some(true));
            i += 1;
        }
        assert!(scene_anchored(SCENE_COUNT).is_none());
        assert!(scene_at(SCENE_COUNT).is_none());
        assert!(scene_case(SCENE_COUNT).is_none());
    }

    #[test]
    fn t23_exactly_two_cross_line_exceptions() {
        let table = BreakOp::all();
        let mut n = 0usize;
        let mut i = 0usize;
        while i < OP_COUNT {
            let op = table[i];
            let s = scene_of(op);
            if op.is_line_exception() {
                n += 1;
                assert_eq!(s.line_a, Line::Glibc);
                assert_eq!(s.line_b, Line::Musl);
                assert!(s.declared);
                assert!(!s.declared_half);
                assert!(s.text_drift);
            } else {
                assert_eq!(s.line_a, s.line_b);
                assert!(!s.declared);
                assert!(!s.text_drift);
            }
            i += 1;
        }
        assert_eq!(n, DECLARED_OPS);
        assert!(anchors_agree());
    }

    #[test]
    fn t24_exception_pairs_are_one_sided() {
        let table = BreakOp::all();
        let mut i = 0usize;
        while i < OP_COUNT {
            let op = table[i];
            if op.is_line_exception() {
                let (a, b) = match op.exception_pair() {
                    Some(x) => x,
                    None => return,
                };
                assert_ne!(a.is_empty(), b.is_empty());
                assert_eq!(op.tail_on(Line::Glibc), a);
                assert_eq!(op.tail_on(Line::Musl), b);
                assert_eq!(op.loads_on(Line::Glibc), a.is_empty());
                assert_eq!(op.loads_on(Line::Musl), b.is_empty());
            } else {
                assert!(op.exception_pair().is_none());
            }
            i += 1;
        }
    }

    #[test]
    fn t25_two_lines_disagree_about_policy() {
        assert!(Line::Glibc.checks_versions());
        assert!(!Line::Musl.checks_versions());
        assert!(Line::Glibc.allows_textrel());
        assert!(!Line::Musl.allows_textrel());
        assert_ne!(Line::Glibc.label(), Line::Musl.label());
        assert_ne!(
            Line::Glibc.flat_corrupt_text(),
            Line::Musl.flat_corrupt_text()
        );
    }

    #[test]
    fn t26_corpus_split_matches_the_contract() {
        assert_eq!(CORPUS, ACCEPT + REJECT);
        assert_eq!(cases().len(), CORPUS);
        assert_eq!(SHAPES.len(), CORPUS);
        assert_eq!(CLAIMS.len(), CORPUS);
        assert!(case_at(CORPUS).is_none());
    }

    #[test]
    fn t27_corpus_tags_are_unique_and_numbered() {
        let mut i = 0usize;
        while i < CORPUS {
            let c = match case_at(i) {
                Some(x) => x,
                None => return,
            };
            assert!(c.tag().starts_with("f11639-"));
            let mut j = i + 1usize;
            while j < CORPUS {
                if let Some(d) = case_at(j) {
                    assert_ne!(c.tag(), d.tag());
                }
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn t28_accept_slots_are_judged_clean() {
        let mut i = 0usize;
        while i < ACCEPT {
            let c = match case_at(i) {
                Some(x) => x,
                None => return,
            };
            assert_eq!(judge(&c), Fault::Ok);
            assert_eq!(expected(i), Some(Fault::Ok));
            i += 1;
        }
    }

    #[test]
    fn t29_reject_slots_hit_their_own_dimension() {
        let faults = Fault::reject_faults();
        let mut k = 0usize;
        while k < REJECT {
            let slot = ACCEPT + k;
            let c = match case_at(slot) {
                Some(x) => x,
                None => return,
            };
            assert_eq!(judge(&c), faults[k]);
            assert_eq!(expected(slot), Some(faults[k]));
            assert_eq!(fault_ndx(faults[k]), k);
            k += 1;
        }
    }

    #[test]
    fn t30_reject_slots_diverge_in_exactly_one_dimension() {
        let mut k = 0usize;
        while k < REJECT {
            let c = match case_at(ACCEPT + k) {
                Some(x) => x,
                None => return,
            };
            let a = c.actual();
            let mut diff = 0usize;
            if a.ok() != c.claim.success() {
                diff += 1;
            }
            if a.stage != c.claim.stage {
                diff += 1;
            }
            if a.code != c.claim.code {
                diff += 1;
            }
            if a.value != c.claim.value {
                diff += 1;
            }
            if a.note != c.claim.note {
                diff += 1;
            }
            if a.default != c.claim.default {
                diff += 1;
            }
            assert_eq!(diff, 1);
            k += 1;
        }
    }

    #[test]
    fn t31_want_tables_restate_the_real_outcome() {
        let mut i = 0usize;
        while i < CORPUS {
            let c = match case_at(i) {
                Some(x) => x,
                None => return,
            };
            let a = c.actual();
            assert_eq!(a.stage, WANT_STAGE[i]);
            assert_eq!(a.code, WANT_CODE[i]);
            assert_eq!(a.note, WANT_NOTE[i]);
            assert_eq!(a.default, WANT_DEFAULT[i]);
            assert_eq!(a.value, WANT_VALUE[i]);
            assert_eq!(a.ok(), WANT_SUCCESS[i]);
            assert_eq!(c.tag(), WANT_TAGS[i]);
            assert_eq!(fault_ndx(judge(&c)), WANT_NDX[i]);
            i += 1;
        }
    }

    #[test]
    fn t32_fault_index_round_trips() {
        let faults = Fault::reject_faults();
        let mut i = 0usize;
        while i < REJECT {
            assert_eq!(ndx_fault(fault_ndx(faults[i])), Some(faults[i]));
            assert!(!faults[i].name().is_empty());
            assert_ne!(faults[i].code(), 0);
            i += 1;
        }
        assert_eq!(ndx_fault(6), Some(Fault::Ok));
        assert!(ndx_fault(7).is_none());
        assert_eq!(Fault::Ok.code(), 0);
    }

    #[test]
    fn t33_slot_cover_guards_the_range() {
        let mut i = 0usize;
        while i <= CORPUS {
            assert_eq!(cover(i), Ok(()));
            i += 1;
        }
        assert_eq!(cover(CORPUS + 1), Err(PG_ESLOT));
    }

    #[test]
    fn t34_matrix_scans_every_slot_of_every_tier() {
        let m = Matrix::build();
        assert_eq!(m.slots(), CORPUS * TIER_COUNT);
        assert_eq!(m.goods(), CORPUS);
        assert_eq!(m.red_total(), m.slots() - m.goods());
        assert!(m.covered());
        assert!(!m.render().as_str().is_empty());
        assert!(Matrix::build().render().as_str().starts_with("matrix "));
    }

    #[test]
    fn t35_each_tier_row_holds_its_own_count() {
        let m = Matrix::build();
        let a = match m.row(Tier::Anchored) {
            Some(x) => x,
            None => return,
        };
        assert_eq!(a.total, CORPUS);
        assert_eq!(a.good, ACCEPT - DECLARED_OPS);
        let d = match m.row(Tier::Declared) {
            Some(x) => x,
            None => return,
        };
        assert_eq!(d.good, DECLARED_OPS);
        assert!(d.tier.is_green());
        assert_eq!(Tier::all().len(), TIER_COUNT);
        assert!(a.perfect() || a.red() > 0);
    }

    #[test]
    fn t36_tier_archive_never_greens_a_dead_probe() {
        let dead = match SHAPES.get(ACCEPT + 2) {
            Some(x) => *x,
            None => return,
        };
        assert_eq!(Tier::of(&define(&dead)), Tier::Void);
        let unjudged = match SHAPES.get(CORPUS - 1) {
            Some(x) => *x,
            None => return,
        };
        let out = define(&unjudged);
        assert_eq!(Tier::of(&out), Tier::Unrun);
        assert_eq!(out.exit(), EXIT_UNJUDGED);
        assert_eq!(out.code, PG_EDEFAULT);
        assert_eq!(out.stage, Stage::Rate);
        assert!(!Tier::Void.is_green());
        assert!(!Tier::Unrun.is_green());
        assert!(Tier::Declared.is_green());
    }

    #[test]
    fn t37_report_renders_the_pinned_green_line() {
        let rep = match run() {
            Ok(x) => x,
            Err(_) => return,
        };
        assert!(rep.green());
        assert_eq!(rep.render().as_str(), RENDER_OK);
        assert_eq!(rep.total, RUN_COUNT);
        assert_eq!(rep.accept, ACCEPT);
        assert_eq!(rep.reject, REJECT);
        assert_eq!(rep.matched, RUN_COUNT);
        assert_eq!(rep.rate(), RATE_FULL);
        assert_eq!(rep.conflicts, 0);
        assert_eq!(rep.unrun, 0);
        assert_eq!(rep.void, 0);
        assert_eq!(rep.red_cells(), 0);
        assert_eq!(rep.reds(), 0);
        assert_eq!(rep.tier_cells(), OP_COUNT as u32);
        assert_eq!(rep.anchored + rep.declared, OP_COUNT);
        assert!(rep.gate().is_ok());
    }

    #[test]
    fn t38_empty_report_is_never_green() {
        let e = empty_report();
        assert!(!e.green());
        assert_eq!(e.rate(), 0);
        assert_eq!(e.total, 0);
        assert_eq!(e.gate(), Err(PG_EMATRIX));
        assert_ne!(e.render().as_str(), RENDER_OK);
        assert_eq!(e.reds(), 0);
    }

    #[test]
    fn t39_injection_costs_the_numerator() {
        let base = match run() {
            Ok(x) => x,
            Err(_) => return,
        };
        let bad = match run_with(Some((2, Fault::Value))) {
            Ok(x) => x,
            Err(_) => return,
        };
        assert!(bad.matched < base.matched);
        assert!(bad.conflicts > base.conflicts);
        assert!(!bad.green());
        assert!(bad.rate() < RATE_FULL);
        assert_eq!(run_with(Some((0, Fault::Stage))).map(|_| ()), Err(PG_ENOTE));
        assert!(red_rate_calibrated());
        assert!(report_agrees_with_matrix());
    }

    #[test]
    fn t40_out_of_range_injection_is_refused() {
        assert_eq!(run_with(Some((CORPUS, Fault::Ok))), Err(PG_ESLOT));
        let table = inject_four();
        assert_eq!(table.len(), 4);
        let mut i = 0usize;
        while i < table.len() {
            assert!(table[i]);
            i += 1;
        }
    }

    #[test]
    fn t41_gate_only_walks_forward() {
        let mut g = Gate::new();
        assert!(g.stage().is_none());
        assert_eq!(g.steps(), 0);
        assert_eq!(g.close(), Err(PG_ESTAGE));
        assert_eq!(g.step(Stage::Seed), Ok(()));
        assert_eq!(g.step(Stage::Emit), Ok(()));
        assert_eq!(g.step(Stage::Seed), Err(PG_ESTAGE));
        assert_eq!(g.refusals(), 1);
        assert_eq!(g.steps(), 2);
        assert_eq!(g.stage(), Some(Stage::Emit));
        match Gate::full_pass() {
            Ok(h) => assert_ne!(h, 0),
            Err(_) => return,
        }
        let mut closed = match Gate::walk(&Stage::all()) {
            Ok(x) => x,
            Err(_) => return,
        };
        assert_eq!(closed.steps(), STAGE_COUNT);
        assert!(closed.close().is_ok());
        assert!(closed.is_closed());
        assert_eq!(closed.step(Stage::Rate), Err(PG_ESTATE));
        assert_eq!(closed.close(), Err(PG_ESTATE));
        assert!(!closed.render().as_str().is_empty());
    }

    #[test]
    fn t42_stage_table_is_ordered_and_total() {
        let table = Stage::all();
        assert_eq!(table.len(), STAGE_COUNT as usize);
        let mut i = 0usize;
        while i < table.len() {
            assert_eq!(table[i].rank(), i as u32 + 1);
            assert_eq!(Stage::of_rank(table[i].rank()), Some(table[i]));
            assert!(!table[i].name().is_empty());
            let mut j = i + 1usize;
            while j < table.len() {
                assert_ne!(table[i].fault_code(), table[j].fault_code());
                j += 1;
            }
            i += 1;
        }
        assert!(Stage::of_rank(0).is_none());
        assert!(Stage::of_rank(STAGE_LAST_RANK + 1).is_none());
        assert!(!Stage::Register.is_observed());
        assert!(Stage::RunBase.is_observed());
    }

    #[test]
    fn t43_ledger_registers_every_cell() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert!(!l.is_empty());
        assert_eq!(l.audit(), Ok(()));
        assert!(!l.is_frozen());
        assert_eq!(l.rejects(), 0);
        assert_eq!(l.ok_count() + l.err_count(), SAMPLES);
        assert_eq!(l.stat_op_want(0), STAT_OPS.get(0).copied());
        assert!(l.sample(SAMPLES).is_none());
        assert!(l.sample(0).is_some());
    }

    #[test]
    fn t44_ledger_freezes_then_refuses_every_write() {
        let mut l = build_ledger();
        let root = match l.freeze() {
            Ok(x) => x,
            Err(_) => return,
        };
        assert_ne!(root, 0);
        assert!(l.is_frozen());
        assert_eq!(l.root(), root);
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(PG_ESTATE));
        assert_eq!(l.poke(0, 7), Err(PG_ESTATE));
        assert_eq!(l.audit(), Ok(()));
        assert!(l.render().as_str().contains("frozen=true"));
    }

    #[test]
    fn t45_tampered_ledgers_are_caught() {
        let mut which = 0u8;
        while which < 3 {
            let b = build_ledger_broken(which);
            assert!(b.audit().is_err());
            assert!(b.first_mismatch().is_some());
            which += 1;
        }
        // 收库修正（AI-INTAKE-1107）：freeze/replay/audit 需 &mut，fresh 声明为 mut。
        let mut fresh = Ledger::new();
        assert!(fresh.replay().is_err());
        assert!(fresh.freeze().is_err());
        assert!(fresh.audit().is_err());
        assert_eq!(fresh.len(), 0);
        assert!(fresh.is_empty());
    }

    #[test]
    fn t46_ledger_rejects_out_of_order_slots() {
        let mut l = Ledger::new();
        let first = Sample {
            slot: 1,
            op: OPID_MASTER,
            ok: true,
            code: 0,
            value: 2,
        };
        assert_eq!(l.register(first), Err(PG_ESLOT));
        assert_eq!(l.rejects(), 1);
        assert_eq!(l.len(), 0);
        let good = Sample {
            slot: 0,
            op: OPID_MASTER,
            ok: true,
            code: 0,
            value: 2,
        };
        assert_eq!(l.register(good), Ok(()));
        assert_eq!(l.len(), 1);
        assert_eq!(l.ok_count(), 1);
        assert_eq!(l.err_count(), 0);
        assert!(!good.render().as_str().is_empty());
    }

    #[test]
    fn t47_messages_quote_the_verbatim_kernel_text() {
        let text = dl_text("libbx.so", T_CANNOT_OPEN);
        assert!(text.as_str().contains("libbx.so"));
        assert!(text.as_str().contains(T_CANNOT_OPEN));
        assert_eq!(dl_text("x", TEXT_EMPTY).as_str(), "x");
        assert!(msg_gen_wrote("/tmp/bx-sandbox/a.so", 640, 7)
            .as_str()
            .contains(SANDBOX_PREFIX));
        assert!(msg_escaped(HOST_LIB_PREFIX)
            .as_str()
            .contains(HOST_LIB_PREFIX));
        assert!(msg_conflict("a", "1", "2").as_str().contains("CONFLICT"));
        assert!(msg_declared("a", "1", "2").as_str().contains("DECLARED"));
        assert!(msg_half_declared("a").as_str().contains("one side"));
        assert!(msg_rate_short(35, 36).as_str().contains("35/36"));
        assert!(msg_unknown_op("nope").as_str().contains("nope"));
        assert!(!msg_missing_operand().as_str().is_empty());
        assert!(msg_unregistered("a").as_str().contains("not registered"));
        assert!(msg_image_drift("a", 1, 2).as_str().contains("image drift"));
        assert!(msg_seed_forbidden(FORBIDDEN_SEED_ZERO)
            .as_str()
            .contains("forbidden"));
    }

    #[test]
    fn t48_define_counts_verdicts_per_scene() {
        let mut i = 0usize;
        let mut declared = 0usize;
        while i < SCENE_COUNT {
            let s = match scene_at(i) {
                Some(x) => x,
                None => return,
            };
            let o = define(&s);
            assert!(o.ok());
            assert!(o.anchored());
            assert!(!o.refused());
            assert!(!o.failed());
            assert!(!o.default);
            assert_eq!(o.exit(), EXIT_OK);
            if o.verdict == Verdict::Declared {
                declared += 1;
                assert_eq!(o.note, Note::LineException);
            } else {
                assert_eq!(o.verdict, Verdict::Same);
                assert_eq!(o.note, Note::TextVerbatim);
            }
            assert!(!message(&s, &o).as_str().is_empty());
            assert_ne!(o.seal(), 0);
            i += 1;
        }
        assert_eq!(declared, DECLARED_OPS);
    }

    #[test]
    fn t49_detail_reports_the_slot_tag() {
        let c = match case_at(0) {
            Some(x) => x,
            None => return,
        };
        assert!(detail(&c).as_str().contains(c.tag()));
        let bad = match case_at(ACCEPT + 5) {
            Some(x) => x,
            None => return,
        };
        let text = detail(&bad);
        assert!(text.as_str().contains(bad.tag()));
        assert!(text.as_str().contains("default"));
        assert_ne!(c.seal(), bad.seal());
    }

    #[test]
    fn t50_all_thirteen_checks_are_green() {
        let s = run_unx11639_checks();
        assert!(s.all_passed());
    }
}
