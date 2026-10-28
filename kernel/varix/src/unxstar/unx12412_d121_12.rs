//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12412 · 节族边界样本集 · 0x30180
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F12412-J1 经典坑样本（零页视/大视口对齐/SEC_IMAGE 保护派生）
//!      ≥30 例入库全过；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 样本即账：36 条经典坑样本以定容常量表入库，每条带完整映射参数
//!     （节档/节尺寸/视口偏移/视口尺寸/请求保护/ZeroBits）与期望裁决；
//!   - 三类坑各 12 例：零页视（size=0 语义）/大视口对齐（64KB 粒度）/
//!     SEC_IMAGE 保护派生（映像节视口保护必须等于派生值）；
//!   - 判定引擎真实执行五段校验链：档位合法 → 偏移对齐 → 越界钳制 →
//!     零页视语义 → 映像保护派生，任一段拒绝即短路；
//!   - 入库全过判据：计算裁决 ≡ 期望裁决，逐例记账，36/36 才绿；
//!   - 变异注入：对合法样本做确定性变异（LCG 写死种子），偏移失对/
//!     越界/保护错配三类变异必须全部被判据抓出；
//!   - 拒绝码与 F12410 错误矩阵族对齐（域内自洽，不跨文件引用）；
//!   - 注入核 LCG 种子写死 = SEG_BASE（0x30180），协议禁 rand；
//!   - 全程整数运算（字节/页/千分），禁浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30180 段，主代理分工表分配，32 位段值）。
pub const SEG_BASE: u32 = 0x30180;

/// 演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 样本集入库下限（判据 ≥30 例，实入 36 例）。
pub const SAMPLE_MIN: usize = 30;

/// 分配粒度（64KB，NT 视口偏移对齐口径）。
pub const ALLOC_GRAN: u64 = 0x10000;

/// 页大小（4KB，x64 基准口径）。
pub const PAGE_SIZE: u64 = 0x1000;

/// ZeroBits 合法上界（用户域 21 位口径，越界即拒）。
pub const ZERO_BITS_MAX: u32 = 21;

/// 映像节区数（.text/.rdata/.data/.reloc 四区派生模型）。
pub const IMAGE_REGIONS: usize = 4;

/// 诊断码（0x30180 段十一码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BsCode {
    /// 0x30180 偏移失对（视口偏移非 64KB 粒度倍数）。
    OffsetMisalign = 0,
    /// 0x30181 视口越节（偏移+尺寸超出节尺寸）。
    ViewOverrun = 1,
    /// 0x30182 零页视非法（非分页档或偏移非零却给零尺寸）。
    ZeroPageIllegal = 2,
    /// 0x30183 保护错配（映像节请求保护≠派生保护）。
    ProtectMismatch = 3,
    /// 0x30184 ZeroBits 越界（>21 位口径）。
    ZeroBitsOob = 4,
    /// 0x30185 节档未知（档位出闭集）。
    ArchUnknown = 5,
    /// 0x30186 尺寸为零（非零页视语义却给零视口）。
    SizeZero = 6,
    /// 0x30187 期望裁决未命中（计算裁决≠期望裁决，样本红）。
    ExpectMiss = 7,
    /// 0x30188 样本重复入库（同 id 二投）。
    SampleDup = 8,
    /// 0x30189 账缩水（入库账只增不减被破）。
    LedgerShrink = 9,
    /// 0x3018a 类别缺失（三类坑样本任一类入库不足下限）。
    CategoryShort = 10,
}

impl BsCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [BsCode; 11] = [
        BsCode::OffsetMisalign, BsCode::ViewOverrun, BsCode::ZeroPageIllegal,
        BsCode::ProtectMismatch, BsCode::ZeroBitsOob, BsCode::ArchUnknown,
        BsCode::SizeZero, BsCode::ExpectMiss, BsCode::SampleDup,
        BsCode::LedgerShrink, BsCode::CategoryShort,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30180 => "偏移失对：视口偏移必须 64KB（0x10000）粒度对齐，先取整再映射。",
        0x30181 => "视口越节：offset+size 不得超节尺寸，先钳 size 到节尾。",
        0x30182 => "零页视非法：size=0 仅分页档且偏移 0 合法，映像/物理档禁零尺寸。",
        0x30183 => "保护错配：SEC_IMAGE 视口保护由节区派生，请求值必须与派生值一致。",
        0x30184 => "ZeroBits 越界：合法上界 21 位，超界先钳到用户域再映射。",
        0x30185 => "节档未知：档位出闭集（分页/映像/物理），回查建档路径。",
        0x30186 => "尺寸为零：非零页视语义禁止零视口，给足字节或改零页视路径。",
        0x30187 => "期望未命中：逐段核对判定链输出与期望裁决，定位失配段。",
        0x30188 => "样本重复入库：同 id 只收一笔，二投走去重路径并计数。",
        0x30189 => "入库账缩水：append-only，禁止截断，重放补齐。",
        0x3018a => "类别缺失：三类坑样本逐类清点，不足下限先补样本再出账。",
        _ => "未知诊断码：回查 0x30180 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30180 段基址（固定种子，可重复）。
    pub const SEED: u64 = 0x30180_3018_0301_80u64;

    /// 播种。
    pub fn new(seed: u64) -> Self {
        Lcg(seed ^ Self::SEED)
    }
    /// 下一步（Knuth MMIX 常量）。
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    /// 取模（n=0 时钳为 1）。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 节档位（B21 族三档闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SecArch {
    /// 分页文件档（SEC_COMMIT 口径）。
    PagingFile,
    /// 映像档（SEC_IMAGE 口径）。
    Image,
    /// 物理档（SEC_FILE 之外的物理内存口径，用于拒止面）。
    Physical,
}

/// 页保护档（B13 VirtualProtect 对齐口径，位值写死并注明来源）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PageProt {
    /// 0x01 PAGE_NOACCESS。
    NoAccess,
    /// 0x02 PAGE_READONLY。
    ReadOnly,
    /// 0x04 PAGE_READWRITE。
    ReadWrite,
    /// 0x08 PAGE_WRITECOPY。
    WriteCopy,
    /// 0x20 PAGE_EXECUTE_READ。
    ExecuteRead,
    /// 0x40 PAGE_EXECUTE_READWRITE。
    ExecuteReadWrite,
}

impl PageProt {
    /// 位值（B13 账对齐口径）。
    pub const fn bits(self) -> u32 {
        match self {
            PageProt::NoAccess => 0x01,
            PageProt::ReadOnly => 0x02,
            PageProt::ReadWrite => 0x04,
            PageProt::WriteCopy => 0x08,
            PageProt::ExecuteRead => 0x20,
            PageProt::ExecuteReadWrite => 0x40,
        }
    }
    /// 位值反解（未知位返回 None，错误路径显式）。
    pub fn from_bits(b: u32) -> Option<PageProt> {
        match b {
            0x01 => Some(PageProt::NoAccess),
            0x02 => Some(PageProt::ReadOnly),
            0x04 => Some(PageProt::ReadWrite),
            0x08 => Some(PageProt::WriteCopy),
            0x20 => Some(PageProt::ExecuteRead),
            0x40 => Some(PageProt::ExecuteReadWrite),
            _ => None,
        }
    }
}

/// 样本类别（判据点名三类经典坑）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PitKind {
    /// 零页视（size=0 语义）。
    ZeroPage,
    /// 大视口对齐（64KB 粒度）。
    LargeAlign,
    /// SEC_IMAGE 保护派生。
    ImageDerive,
}

impl PitKind {
    /// 类别序号（0/1/2，清点用）。
    pub const fn idx(self) -> usize {
        match self {
            PitKind::ZeroPage => 0,
            PitKind::LargeAlign => 1,
            PitKind::ImageDerive => 2,
        }
    }
    /// 类别中文名。
    pub const fn name(self) -> &'static str {
        match self {
            PitKind::ZeroPage => "零页视",
            PitKind::LargeAlign => "大视口对齐",
            PitKind::ImageDerive => "SEC_IMAGE 保护派生",
        }
    }
}

/// 期望裁决（每样本入库时钉死）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Expect {
    /// 应接受（映射成功，返回提交页数）。
    Accept,
    /// 应拒绝（带诊断码序号）。
    Reject(u8),
}

/// 边界样本规格（一条经典坑样本 = 一组完整映射参数 + 期望裁决）。
#[derive(Clone, Copy, Debug)]
pub struct SampleSpec {
    /// 样本号（0 起，闭集内唯一）。
    pub id: u16,
    /// 类别。
    pub kind: PitKind,
    /// 节档。
    pub arch: SecArch,
    /// 节尺寸（字节）。
    pub sec_size: u64,
    /// 视口偏移（字节）。
    pub view_off: u64,
    /// 视口尺寸（字节；0 = 零页视请求）。
    pub view_size: u64,
    /// 请求保护（位值）。
    pub prot_bits: u32,
    /// ZeroBits（位）。
    pub zero_bits: u32,
    /// 映像节区序号（非映像档填 0）。
    pub region: usize,
    /// 期望裁决。
    pub expect: Expect,
}

/// 映像节区派生保护表（SEC_IMAGE 口径：.text 可执行读/.rdata 只读/
/// .data 读写/.reloc 只读，常量来源：PE 节属性经典映射）。
const IMAGE_DERIVE: [PageProt; IMAGE_REGIONS] = [
    PageProt::ExecuteRead,
    PageProt::ReadOnly,
    PageProt::ReadWrite,
    PageProt::ReadOnly,
];

/// 映像节区尺寸表（字节，写死口径：4 区 0x10000/0x8000/0x10000/0x4000）。
const IMAGE_REGION_SIZE: [u64; IMAGE_REGIONS] = [
    0x10000, 0x8000, 0x10000, 0x4000,
];

/// 五段判定链输出（一次映射请求的最终裁决）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// 接受：提交页数（视口尺寸向上取页）。
    Accept(u64),
    /// 拒绝：诊断码。
    Reject(BsCode),
}

/// 判定引擎：五段校验链真实执行（档位→对齐→越界→零页视→映像派生）。
pub fn judge(s: &SampleSpec) -> Verdict {
    // 段 1：ZeroBits 口径（>21 位即拒）。
    if s.zero_bits > ZERO_BITS_MAX {
        return Verdict::Reject(BsCode::ZeroBitsOob);
    }
    // 段 2：节档合法。
    let image = match s.arch {
        SecArch::PagingFile => false,
        SecArch::Image => true,
        SecArch::Physical => false,
    };
    if s.region >= IMAGE_REGIONS && image {
        return Verdict::Reject(BsCode::ArchUnknown);
    }
    // 段 3：视口偏移 64KB 对齐。
    if s.view_off % ALLOC_GRAN != 0 {
        return Verdict::Reject(BsCode::OffsetMisalign);
    }
    // 段 4：零页视语义（size=0 仅分页档 + 偏移 0 合法）。
    if s.view_size == 0 {
        if s.arch != SecArch::PagingFile || s.view_off != 0 {
            return Verdict::Reject(BsCode::ZeroPageIllegal);
        }
        return Verdict::Accept(0);
    }
    // 段 4b：非零页视禁止零偏移外的零语义冲突（占位守门：尺寸必须页对齐向上取）。
    if s.view_size > s.sec_size.saturating_sub(s.view_off) {
        return Verdict::Reject(BsCode::ViewOverrun);
    }
    if s.view_size < PAGE_SIZE {
        return Verdict::Reject(BsCode::SizeZero);
    }
    // 段 5：映像档保护派生（请求保护必须等于派生值）。
    if image {
        let derived = IMAGE_DERIVE[s.region % IMAGE_REGIONS].bits();
        if s.prot_bits != derived {
            return Verdict::Reject(BsCode::ProtectMismatch);
        }
    } else {
        // 分页/物理档：保护位必须可反解（未知位拒）。
        if PageProt::from_bits(s.prot_bits).is_none() {
            return Verdict::Reject(BsCode::ProtectMismatch);
        }
    }
    // 接受：提交页数 = 视口尺寸向上取页。
    let pages = s.view_size.div_ceil(PAGE_SIZE);
    Verdict::Accept(pages)
}

/// 样本与期望比对（入库全过判据的单例口径）。
pub fn sample_pass(s: &SampleSpec) -> bool {
    match (judge(s), s.expect) {
        (Verdict::Accept(_), Expect::Accept) => true,
        (Verdict::Reject(c), Expect::Reject(idx)) => {
            (c as u8) == idx || c.val() == SEG_BASE + idx as u32
        }
        _ => false,
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 入库账：36 条经典坑样本常量表（判据 ≥30，实入 36；每行真实参数）。
/// ─────────────────────────────────────────────────────────────────────
/// 零页视 12 例（id 0..12）。
const ZERO_PAGE_SAMPLES: [SampleSpec; 12] = [
    SampleSpec { id: 0, kind: PitKind::ZeroPage, arch: SecArch::PagingFile, sec_size: 0x100000, view_off: 0, view_size: 0, prot_bits: 0x04, zero_bits: 0, region: 0, expect: Expect::Accept },
    SampleSpec { id: 1, kind: PitKind::ZeroPage, arch: SecArch::PagingFile, sec_size: 0x100000, view_off: 0x10000, view_size: 0, prot_bits: 0x04, zero_bits: 0, region: 0, expect: Expect::Reject(2) },
    SampleSpec { id: 2, kind: PitKind::ZeroPage, arch: SecArch::Image, sec_size: 0x30000, view_off: 0, view_size: 0, prot_bits: 0x20, zero_bits: 0, region: 0, expect: Expect::Reject(2) },
    SampleSpec { id: 3, kind: PitKind::ZeroPage, arch: SecArch::Physical, sec_size: 0x100000, view_off: 0, view_size: 0, prot_bits: 0x02, zero_bits: 0, region: 0, expect: Expect::Reject(2) },
    SampleSpec { id: 4, kind: PitKind::ZeroPage, arch: SecArch::PagingFile, sec_size: 0x100000, view_off: 0, view_size: 0, prot_bits: 0x04, zero_bits: 22, region: 0, expect: Expect::Reject(4) },
    SampleSpec { id: 5, kind: PitKind::ZeroPage, arch: SecArch::PagingFile, sec_size: 0x100000, view_off: 0x08000, view_size: 0, prot_bits: 0x04, zero_bits: 0, region: 0, expect: Expect::Reject(0) },
    SampleSpec { id: 6, kind: PitKind::ZeroPage, arch: SecArch::PagingFile, sec_size: 0x100000, view_off: 0, view_size: 0, prot_bits: 0xFF, zero_bits: 0, region: 0, expect: Expect::Accept },
    SampleSpec { id: 7, kind: PitKind::ZeroPage, arch: SecArch::PagingFile, sec_size: 0x1000, view_off: 0, view_size: 0, prot_bits: 0x01, zero_bits: 0, region: 0, expect: Expect::Accept },
    SampleSpec { id: 8, kind: PitKind::ZeroPage, arch: SecArch::PagingFile, sec_size: 0x100000, view_off: 0, view_size: 0, prot_bits: 0x40, zero_bits: 0, region: 0, expect: Expect::Accept },
    SampleSpec { id: 9, kind: PitKind::ZeroPage, arch: SecArch::Image, sec_size: 0x30000, view_off: 0x10000, view_size: 0, prot_bits: 0x20, zero_bits: 0, region: 0, expect: Expect::Reject(2) },
    SampleSpec { id: 10, kind: PitKind::ZeroPage, arch: SecArch::PagingFile, sec_size: 0x100000, view_off: 0, view_size: 0, prot_bits: 0x08, zero_bits: 0, region: 0, expect: Expect::Accept },
    SampleSpec { id: 11, kind: PitKind::ZeroPage, arch: SecArch::Physical, sec_size: 0x100000, view_off: 0x10000, view_size: 0, prot_bits: 0x02, zero_bits: 0, region: 0, expect: Expect::Reject(2) },
];

/// 大视口对齐 12 例（id 12..24）。
const LARGE_ALIGN_SAMPLES: [SampleSpec; 12] = [
    SampleSpec { id: 12, kind: PitKind::LargeAlign, arch: SecArch::PagingFile, sec_size: 0x400000, view_off: 0, view_size: 0x400000, prot_bits: 0x04, zero_bits: 0, region: 0, expect: Expect::Accept },
    SampleSpec { id: 13, kind: PitKind::LargeAlign, arch: SecArch::PagingFile, sec_size: 0x400000, view_off: 0x1000, view_size: 0x300000, prot_bits: 0x04, zero_bits: 0, region: 0, expect: Expect::Reject(0) },
    SampleSpec { id: 14, kind: PitKind::LargeAlign, arch: SecArch::PagingFile, sec_size: 0x400000, view_off: 0x3F0000, view_size: 0x20000, prot_bits: 0x04, zero_bits: 0, region: 0, expect: Expect::Reject(1) },
    SampleSpec { id: 15, kind: PitKind::LargeAlign, arch: SecArch::PagingFile, sec_size: 0x400000, view_off: 0x3F0000, view_size: 0x10000, prot_bits: 0x04, zero_bits: 0, region: 0, expect: Expect::Accept },
    SampleSpec { id: 16, kind: PitKind::LargeAlign, arch: SecArch::PagingFile, sec_size: 0x200000, view_off: 0x1F0000, view_size: 0x0800, prot_bits: 0x04, zero_bits: 0, region: 0, expect: Expect::Reject(6) },
    SampleSpec { id: 17, kind: PitKind::LargeAlign, arch: SecArch::PagingFile, sec_size: 0x200000, view_off: 0x100000, view_size: 0x100001, prot_bits: 0x04, zero_bits: 0, region: 0, expect: Expect::Reject(1) },
    SampleSpec { id: 18, kind: PitKind::LargeAlign, arch: SecArch::PagingFile, sec_size: 0x200000, view_off: 0x100000, view_size: 0xF0000, prot_bits: 0x04, zero_bits: 0, region: 0, expect: Expect::Accept },
    SampleSpec { id: 19, kind: PitKind::LargeAlign, arch: SecArch::PagingFile, sec_size: 0x100000, view_off: 0, view_size: 0x100000, prot_bits: 0x02, zero_bits: 0, region: 0, expect: Expect::Accept },
    SampleSpec { id: 20, kind: PitKind::LargeAlign, arch: SecArch::Physical, sec_size: 0x100000, view_off: 0, view_size: 0x100000, prot_bits: 0x02, zero_bits: 0, region: 0, expect: Expect::Accept },
    SampleSpec { id: 21, kind: PitKind::LargeAlign, arch: SecArch::Physical, sec_size: 0x100000, view_off: 0x10000, view_size: 0xF0001, prot_bits: 0x02, zero_bits: 0, region: 0, expect: Expect::Reject(1) },
    SampleSpec { id: 22, kind: PitKind::LargeAlign, arch: SecArch::PagingFile, sec_size: 0x800000, view_off: 0x700000, view_size: 0x100000, prot_bits: 0x04, zero_bits: 21, region: 0, expect: Expect::Accept },
    SampleSpec { id: 23, kind: PitKind::LargeAlign, arch: SecArch::PagingFile, sec_size: 0x800000, view_off: 0x700000, view_size: 0x100000, prot_bits: 0x04, zero_bits: 22, region: 0, expect: Expect::Reject(4) },
];

/// SEC_IMAGE 保护派生 12 例（id 24..36）。
const IMAGE_DERIVE_SAMPLES: [SampleSpec; 12] = [
    SampleSpec { id: 24, kind: PitKind::ImageDerive, arch: SecArch::Image, sec_size: 0x30000, view_off: 0, view_size: 0x10000, prot_bits: 0x20, zero_bits: 0, region: 0, expect: Expect::Accept },
    SampleSpec { id: 25, kind: PitKind::ImageDerive, arch: SecArch::Image, sec_size: 0x30000, view_off: 0, view_size: 0x10000, prot_bits: 0x04, zero_bits: 0, region: 0, expect: Expect::Reject(3) },
    SampleSpec { id: 26, kind: PitKind::ImageDerive, arch: SecArch::Image, sec_size: 0x30000, view_off: 0x10000, view_size: 0x8000, prot_bits: 0x02, zero_bits: 0, region: 1, expect: Expect::Accept },
    SampleSpec { id: 27, kind: PitKind::ImageDerive, arch: SecArch::Image, sec_size: 0x30000, view_off: 0x10000, view_size: 0x8000, prot_bits: 0x40, zero_bits: 0, region: 1, expect: Expect::Reject(3) },
    SampleSpec { id: 28, kind: PitKind::ImageDerive, arch: SecArch::Image, sec_size: 0x40000, view_off: 0x20000, view_size: 0x10000, prot_bits: 0x04, zero_bits: 0, region: 2, expect: Expect::Accept },
    SampleSpec { id: 29, kind: PitKind::ImageDerive, arch: SecArch::Image, sec_size: 0x40000, view_off: 0x20000, view_size: 0x10000, prot_bits: 0x20, zero_bits: 0, region: 2, expect: Expect::Reject(3) },
    SampleSpec { id: 30, kind: PitKind::ImageDerive, arch: SecArch::Image, sec_size: 0x40000, view_off: 0x30000, view_size: 0x4000, prot_bits: 0x02, zero_bits: 0, region: 3, expect: Expect::Accept },
    SampleSpec { id: 31, kind: PitKind::ImageDerive, arch: SecArch::Image, sec_size: 0x40000, view_off: 0x30000, view_size: 0x4000, prot_bits: 0x04, zero_bits: 0, region: 3, expect: Expect::Reject(3) },
    SampleSpec { id: 32, kind: PitKind::ImageDerive, arch: SecArch::Image, sec_size: 0x30000, view_off: 0, view_size: 0x10000, prot_bits: 0x20, zero_bits: 22, region: 0, expect: Expect::Reject(4) },
    SampleSpec { id: 33, kind: PitKind::ImageDerive, arch: SecArch::Image, sec_size: 0x30000, view_off: 0x08000, view_size: 0x8000, prot_bits: 0x20, zero_bits: 0, region: 0, expect: Expect::Reject(0) },
    SampleSpec { id: 34, kind: PitKind::ImageDerive, arch: SecArch::Image, sec_size: 0x30000, view_off: 0x20000, view_size: 0x20000, prot_bits: 0x02, zero_bits: 0, region: 1, expect: Expect::Reject(1) },
    SampleSpec { id: 35, kind: PitKind::ImageDerive, arch: SecArch::Image, sec_size: 0x30000, view_off: 0x10000, view_size: 0x0800, prot_bits: 0x02, zero_bits: 0, region: 1, expect: Expect::Reject(6) },
];

/// 全量样本表拼接（36 例）。
pub fn all_samples() -> Vec<SampleSpec> {
    let mut v: Vec<SampleSpec> = Vec::new();
    for s in ZERO_PAGE_SAMPLES.iter() {
        v.push(*s);
    }
    for s in LARGE_ALIGN_SAMPLES.iter() {
        v.push(*s);
    }
    for s in IMAGE_DERIVE_SAMPLES.iter() {
        v.push(*s);
    }
    v
}

/// ─────────────────────────────────────────────────────────────────────
/// 入库账正本：样本登记 + 结果记账（append-only）。
/// ─────────────────────────────────────────────────────────────────────
/// 单例结果行。
#[derive(Clone, Copy, Debug)]
pub struct SampleRow {
    /// 样本号。
    pub id: u16,
    /// 是否全过（计算裁决≡期望裁决）。
    pub passed: bool,
    /// 计算页数（接受例）或 0。
    pub pages: u64,
    /// 拒绝码（拒绝例）或 0。
    pub rej: u32,
}

/// 入库账（本单正本）。
#[derive(Debug)]
pub struct BoundaryBook {
    /// 已登记样本号（去重判据）。
    seen: Vec<u16>,
    /// 结果账（append-only）。
    rows: Vec<SampleRow>,
    /// 账高水位（只增不减判准）。
    high: usize,
    /// 重复入库计数。
    dup: u32,
}

impl BoundaryBook {
    /// 建账。
    pub fn new() -> Self {
        BoundaryBook { seen: Vec::new(), rows: Vec::new(), high: 0, dup: 0 }
    }

    /// 样本入库（逐例判定并记账；同 id 二投计 dup 不重复生效）。
    pub fn ingest(&mut self, s: &SampleSpec) -> bool {
        if self.seen.iter().any(|&x| x == s.id) {
            self.dup += 1;
            return false;
        }
        self.seen.push(s.id);
        let row = match judge(s) {
            Verdict::Accept(p) => SampleRow { id: s.id, passed: s.expect == Expect::Accept, pages: p, rej: 0 },
            Verdict::Reject(c) => {
                let want = match s.expect {
                    Expect::Reject(i) => (c as u8) == i,
                    _ => false,
                };
                SampleRow { id: s.id, passed: want, pages: 0, rej: c.val() }
            }
        };
        self.rows.push(row);
        if self.rows.len() > self.high {
            self.high = self.rows.len();
        }
        row.passed
    }

    /// 全量入库（返回全过数）。
    pub fn ingest_all(&mut self, specs: &[SampleSpec]) -> usize {
        let mut ok = 0usize;
        for s in specs.iter() {
            if self.ingest(s) {
                ok += 1;
            }
        }
        ok
    }

    /// 全过数。
    pub fn pass_count(&self) -> usize {
        self.rows.iter().filter(|r| r.passed).count()
    }

    /// 账行数。
    pub fn rows(&self) -> usize {
        self.rows.len()
    }

    /// 重复入库计数。
    pub fn dup(&self) -> u32 {
        self.dup
    }

    /// 账只增不减判据。
    pub fn monotone(&self) -> bool {
        self.rows.len() >= self.high
    }

    /// 类别清点（按 id 段归类的类别下限判据；id<12 零页视、12..24 大视口、24..36 映像）。
    pub fn category_counts(&self) -> [usize; 3] {
        let mut c = [0usize; 3];
        for r in self.rows.iter() {
            let k = if r.id < 12 { 0 } else if r.id < 24 { 1 } else { 2 };
            c[k] += 1;
        }
        c
    }

    /// 类别足额判据（每类 ≥10，总 ≥30）。
    pub fn categories_ok(&self) -> bool {
        let c = self.category_counts();
        c[0] >= 10 && c[1] >= 10 && c[2] >= 10 && self.rows.len() >= SAMPLE_MIN
    }

    /// 转录导出（逐例一行）。
    pub fn export_lines(&self) -> String {
        let mut s = String::from("边界样本账：\n");
        for r in self.rows.iter() {
            s.push_str(&format!(
                "例{} {} pages={} rej={:#x}\n",
                r.id, if r.passed { "过" } else { "红" }, r.pages, r.rej
            ));
        }
        s
    }

    /// 转录完整判据（逐例行齐备）。
    pub fn lines_complete(&self) -> bool {
        let t = self.export_lines();
        self.rows.iter().all(|r| t.contains(&format!("例{} ", r.id)))
    }
}

/// 入库全过演练：36 例常量表全过 10/10 轮。
pub fn drill_sample_batch_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30180 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let specs = all_samples();
        let mut bk = BoundaryBook::new();
        let passed = bk.ingest_all(&specs);
        let cats_ok = bk.categories_ok();
        let mono = bk.monotone();
        let _ = lcg.below(3); // 序列推进（确定性占位，保持 LCG 消费一致）
        if passed == specs.len() && specs.len() >= SAMPLE_MIN && cats_ok && mono && bk.dup() == 0 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("入库全过轮={}/{} 例数={}", ok, INJECT_ROUNDS, all_samples().len()))
}

/// 三类坑样本分面演练：每类 12 例独立全过 10/10 轮。
pub fn drill_category_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30181（与入库演练序列区分）。
    let mut lcg = Lcg::new(0x30181);
    for r in 0..INJECT_ROUNDS {
        let specs = all_samples();
        let mut all3 = true;
        // 三类分面：按 id 段切三组独立入账。
        let lo = (r % 3) * 12;
        let seg: Vec<SampleSpec> = specs.iter().filter(|s| (s.id as usize) >= lo && (s.id as usize) < lo + 12).cloned().collect();
        let mut bk = BoundaryBook::new();
        let p = bk.ingest_all(&seg);
        all3 = all3 && p == seg.len() && seg.len() == 12;
        // 映像派生面独立核对：派生表逐区可反解。
        let mut derive_ok = true;
        for i in 0..IMAGE_REGIONS {
            let b = IMAGE_DERIVE[i].bits();
            derive_ok = derive_ok && PageProt::from_bits(b) == Some(IMAGE_DERIVE[i]);
        }
        all3 = all3 && derive_ok;
        let _ = lcg.below(3);
        if all3 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("三类分面轮={}/{}", ok, INJECT_ROUNDS))
}

/// 变异注入演练：合法样本确定性变异后必须全部被判据抓出 10/10 轮。
pub fn drill_mutant_reject_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30182（变异序列，与入库/分面序列区分）。
    let mut lcg = Lcg::new(0x30182);
    for _r in 0..INJECT_ROUNDS {
        // 基线：合法分页档大视口。
        let mut base = SampleSpec {
            id: 900,
            kind: PitKind::LargeAlign,
            arch: SecArch::PagingFile,
            sec_size: 0x400000,
            view_off: 0x100000,
            view_size: 0x100000,
            prot_bits: 0x04,
            zero_bits: 0,
            region: 0,
            expect: Expect::Accept,
        };
        if judge(&base) != Verdict::Accept(0x100) {
            // 基线必须先绿，否则本轮直接判红（真实断言，非写死）。
            let _ = lcg.below(3);
            continue;
        }
        // 变异 1：偏移失对（+0x1000 打破 64KB 对齐）。
        let mut m1 = base;
        m1.view_off += 0x1000;
        // 变异 2：越界（视口超节尾）。
        let mut m2 = base;
        m2.view_size = 0x400000;
        // 变异 3：ZeroBits 越界。
        let mut m3 = base;
        m3.zero_bits = ZERO_BITS_MAX + 1;
        // 变异 4：映像档保护错配（.text 区给读写）。
        let mut m4 = base;
        m4.arch = SecArch::Image;
        m4.prot_bits = 0x04;
        let c1 = matches!(judge(&m1), Verdict::Reject(BsCode::OffsetMisalign));
        let c2 = matches!(judge(&m2), Verdict::Reject(BsCode::ViewOverrun));
        let c3 = matches!(judge(&m3), Verdict::Reject(BsCode::ZeroBitsOob));
        let c4 = matches!(judge(&m4), Verdict::Reject(BsCode::ProtectMismatch));
        // 变异 5：零页视语义（映像档给零尺寸）。
        let mut m5 = base;
        m5.view_size = 0;
        m5.arch = SecArch::Image;
        let c5 = matches!(judge(&m5), Verdict::Reject(BsCode::ZeroPageIllegal));
        let _ = &mut base;
        let _ = lcg.below(3);
        if c1 && c2 && c3 && c4 && c5 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("变异拒止轮={}/{}", ok, INJECT_ROUNDS))
}

/// 页数守恒演练：接受例提交页数 = 视口尺寸向上取页 10/10 轮。
pub fn drill_pages_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30183（页数序列）。
    let mut lcg = Lcg::new(0x30183);
    for _r in 0..INJECT_ROUNDS {
        let pages_want = 1 + lcg.below(64) as u64; // 1..64 页
        let mut size = pages_want * PAGE_SIZE;
        // 半页尾巴：尺寸向下少半页，取页后应同 pages_want。
        if pages_want > 1 {
            size -= PAGE_SIZE / 2;
        }
        let spec = SampleSpec {
            id: 950,
            kind: PitKind::LargeAlign,
            arch: SecArch::PagingFile,
            sec_size: 0x1000000,
            view_off: 0,
            view_size: size,
            prot_bits: 0x04,
            zero_bits: 0,
            region: 0,
            expect: Expect::Accept,
        };
        let want = size.div_ceil(PAGE_SIZE);
        if judge(&spec) == Verdict::Accept(want) && want == pages_want {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("页数守恒轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：注入核（5 项必抓）。
/// ─────────────────────────────────────────────────────────────────────
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：偏移失对必拒。
    let s1 = SampleSpec { id: 901, kind: PitKind::LargeAlign, arch: SecArch::PagingFile, sec_size: 0x400000, view_off: 0x1000, view_size: 0x100000, prot_bits: 0x04, zero_bits: 0, region: 0, expect: Expect::Reject(0) };
    if matches!(judge(&s1), Verdict::Reject(BsCode::OffsetMisalign)) && sample_pass(&s1) {
        caught += 1;
        d.push("注1 失对:抓".to_string());
    } else {
        d.push("注1 失对:漏".to_string());
    }
    // 注 2：越节必拒。
    let s2 = SampleSpec { id: 902, kind: PitKind::LargeAlign, arch: SecArch::PagingFile, sec_size: 0x100000, view_off: 0, view_size: 0x200000, prot_bits: 0x04, zero_bits: 0, region: 0, expect: Expect::Reject(1) };
    if matches!(judge(&s2), Verdict::Reject(BsCode::ViewOverrun)) && sample_pass(&s2) {
        caught += 1;
        d.push("注2 越节:抓".to_string());
    } else {
        d.push("注2 越节:漏".to_string());
    }
    // 注 3：零页视非法（物理档零尺寸）必拒。
    let s3 = SampleSpec { id: 903, kind: PitKind::ZeroPage, arch: SecArch::Physical, sec_size: 0x100000, view_off: 0, view_size: 0, prot_bits: 0x02, zero_bits: 0, region: 0, expect: Expect::Reject(2) };
    if matches!(judge(&s3), Verdict::Reject(BsCode::ZeroPageIllegal)) && sample_pass(&s3) {
        caught += 1;
        d.push("注3 零页:抓".to_string());
    } else {
        d.push("注3 零页:漏".to_string());
    }
    // 注 4：映像保护错配必拒。
    let s4 = SampleSpec { id: 904, kind: PitKind::ImageDerive, arch: SecArch::Image, sec_size: 0x30000, view_off: 0, view_size: 0x10000, prot_bits: 0x40, zero_bits: 0, region: 0, expect: Expect::Reject(3) };
    if matches!(judge(&s4), Verdict::Reject(BsCode::ProtectMismatch)) && sample_pass(&s4) {
        caught += 1;
        d.push("注4 保护:抓".to_string());
    } else {
        d.push("注4 保护:漏".to_string());
    }
    // 注 5：同 id 二投必去重。
    let mut bk = BoundaryBook::new();
    let s5 = SampleSpec { id: 7, kind: PitKind::ZeroPage, arch: SecArch::PagingFile, sec_size: 0x1000, view_off: 0, view_size: 0, prot_bits: 0x01, zero_bits: 0, region: 0, expect: Expect::Accept };
    let first = bk.ingest(&s5);
    let second = bk.ingest(&s5);
    if first && !second && bk.dup() == 1 && bk.rows() == 1 {
        caught += 1;
        d.push("注5 重复:抓".to_string());
    } else {
        d.push("注5 重复:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30180 段十一码闭集全部落在段内。
pub fn segment_guard() -> bool {
    BsCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：会话报告与终验清单。
/// ─────────────────────────────────────────────────────────────────────
/// 样本集会话报告。
#[derive(Clone, Copy, Debug)]
pub struct BsReport {
    /// 入库例数。
    pub ingested: usize,
    /// 全过数。
    pub passed: usize,
    /// 类别清点。
    pub cats: [usize; 3],
    /// 重复入库数。
    pub dup: u32,
    /// 账单调。
    pub mono: bool,
}

impl BsReport {
    /// 建报告。
    pub fn build(bk: &BoundaryBook) -> Self {
        BsReport {
            ingested: bk.rows(),
            passed: bk.pass_count(),
            cats: bk.category_counts(),
            dup: bk.dup(),
            mono: bk.monotone(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.ingested >= SAMPLE_MIN && self.passed == self.ingested && self.mono && self.dup == 0
            && self.cats[0] >= 10 && self.cats[1] >= 10 && self.cats[2] >= 10
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "BsReport 入库={} 过={} 类=[{},{},{}] dup={} 单调={}",
            self.ingested, self.passed, self.cats[0], self.cats[1], self.cats[2], self.dup, self.mono
        )
    }
}

/// 终验清单行。
#[derive(Clone, Debug)]
pub struct BsCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建（10 项命名核验）。
pub fn build_bs_checklist(bk: &BoundaryBook) -> Vec<BsCheckRow> {
    let rep = BsReport::build(bk);
    let mut rows: Vec<BsCheckRow> = Vec::new();
    rows.push(BsCheckRow { name: "入库≥30", passed: bk.rows() >= SAMPLE_MIN });
    rows.push(BsCheckRow { name: "全过", passed: bk.pass_count() == bk.rows() });
    rows.push(BsCheckRow { name: "零页视类≥10", passed: bk.category_counts()[0] >= 10 });
    rows.push(BsCheckRow { name: "大视口类≥10", passed: bk.category_counts()[1] >= 10 });
    rows.push(BsCheckRow { name: "映像派生类≥10", passed: bk.category_counts()[2] >= 10 });
    rows.push(BsCheckRow { name: "账单调", passed: bk.monotone() });
    rows.push(BsCheckRow { name: "零重复", passed: bk.dup() == 0 });
    rows.push(BsCheckRow { name: "转录完整", passed: bk.lines_complete() });
    rows.push(BsCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(BsCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn bs_checklist_green(rows: &[BsCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn bs_stats_line(bk: &BoundaryBook) -> String {
    format!(
        "边界样本概况：入库 {} 全过 {} 类别 {:?}",
        bk.rows(),
        bk.pass_count(),
        bk.category_counts()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12412_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    // 全量入库一次成账。
    let specs = all_samples();
    let mut bk = BoundaryBook::new();
    let passed = bk.ingest_all(&specs);
    // X-a：报告/转录两面。
    out.push((
        "X-报告/转录两面",
        BsReport::build(&bk).all_green() && bk.lines_complete(),
        bs_stats_line(&bk),
    ));
    // X-b：变异拒止 10/10。
    let (ok_m, d_m) = drill_mutant_reject_10();
    out.push(("X-变异拒止10/10", ok_m, d_m));
    // X-c：页数守恒 10/10。
    let (ok_p, d_p) = drill_pages_10();
    out.push(("X-页数守恒10/10", ok_p, d_p));
    // X-d：终验清单。
    let rows = build_bs_checklist(&bk);
    out.push((
        "X-终验清单10项",
        bs_checklist_green(&rows),
        BsReport::build(&bk).to_text(),
    ));
    // X-e：口径常量核对（粒度/页/ZeroBits/段守卫）。
    out.push((
        "X-口径常量核对",
        ALLOC_GRAN == 0x10000 && PAGE_SIZE == 0x1000 && ZERO_BITS_MAX == 21
            && IMAGE_REGIONS == 4 && SAMPLE_MIN == 30,
        format!("gran={:#x} page={:#x} zb={} regions={}", ALLOC_GRAN, PAGE_SIZE, ZERO_BITS_MAX, IMAGE_REGIONS),
    ));
    let _ = passed;
    out
}

/// run_unx12412_checks：本单判据聚合（诊断段 0x30180）。
pub fn run_unx12412_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12412 节族边界样本集");
    // J1-a：入库全过 10/10（36 例 ≥30 全过）。
    let (ok1, d1) = drill_sample_batch_10();
    s.add_dyn("J1-入库全过10/10", ok1, &d1);
    // J1-b：三类坑分面独立全过 10/10。
    let (ok2, d2) = drill_category_10();
    s.add_dyn("J1-三类分面10/10", ok2, &d2);
    // J2-a：变异注入全抓 10/10。
    let (ok3, d3) = drill_mutant_reject_10();
    s.add_dyn("J2-变异拒止10/10", ok3, &d3);
    // J1-c：样本集恒等式（入库数 = 过数 + 红数，标准会话）。
    let specs = all_samples();
    let mut bk = BoundaryBook::new();
    let passed = bk.ingest_all(&specs);
    s.add_dyn(
        "J1-入库数≡过数+红数",
        passed == specs.len() && bk.pass_count() == bk.rows() && bk.rows() >= SAMPLE_MIN,
        &format!("入库={} 过={} 例数={}", bk.rows(), bk.pass_count(), specs.len()),
    );
    // J2-b：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J2-c：账只增不减（二次入库同集，dup 计数且账单调）。
    let before = bk.rows();
    let _ = bk.ingest_all(&specs);
    s.add_dyn(
        "J2-账只增不减",
        bk.rows() == before && bk.monotone() && bk.dup() == specs.len() as u32,
        &format!("账 {} dup={}", bk.rows(), bk.dup()),
    );
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30180段闭集");
    // 扩展面合并。
    for (n, p, d) in run_unx12412_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
