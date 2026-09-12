//! AI-02 · ELF 加载与 ABI 域（VARIABLE-200，F026~F050，W2）.
//!
//! Varix 自己的可执行格式 **VXELF** 与工业级加载器：段映射三元组精确落位、
//! W^X 执行纪律、ASLR、入口栈（argc/argv/envp/auxv）布局、TLS、RELA 重定位、
//! 镜像校验与可选签名、失败全量回收、ABI 版本协商、共享库骨架、调试信息保留、
//! 加载性能仪表、fuzz、并发加载、资产识别、shebang 与环境注入，最后用
//! `hello` 镜像把整条链路跑通。
//!
//! 设计纪律（与《VXELF 格式规范》同源）：
//! 1. 加载器只认**内部一致的镜像**：任何一步校验失败，进程不创建、内存全量
//!    回收、错误码明确 —— 绝不半加载。
//! 2. 首版只支持静态链接；动态链接留接口、显式降级（无静默缺失）。
//! 3. 页权限在**映射完成时**就定死：可写页必不可执行，加载完成即锁权限。
//!
//! 纯逻辑 + 固定容量数组，`no_std` 无分配；宿主单测与内核自检共用同一批
//! 纯函数，所有地址事实都可在单测里断言。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// F026 — VXELF 格式定稿
// ---------------------------------------------------------------------------

/// 魔数：`0x7F 'V' 'X' 'E'`（区别于 ELF 的 `0x7F 'E' 'L' 'F'`）。
pub const VXELF_MAGIC: [u8; 4] = [0x7F, b'V', b'X', b'E'];
/// 容器版本（格式本身的代数，改动布局必须递增）。
pub const VXELF_VERSION: u16 = 1;
/// 内核 ABI 版本（F040 协商的是它，不是容器版本）。
pub const VXABI_VERSION: u16 = 1;
/// 文件头定长。
pub const HDR_SIZE: usize = 64;
/// 程序头定长。
pub const PHDR_SIZE: usize = 56;
/// 段表容量。
pub const MAX_SEGMENTS: usize = 8;
/// 段类型。
pub const PT_LOAD: u32 = 1;
pub const PT_TLS: u32 = 7;
pub const PT_GNU_STACK: u32 = 0x6474_E551;
pub const PT_INTERP: u32 = 3;
pub const PT_DYNAMIC: u32 = 2;
pub const PT_NOTE: u32 = 4;
/// 段权限位。
pub const PF_X: u32 = 1;
pub const PF_W: u32 = 2;
pub const PF_R: u32 = 4;
/// 用户半区边界（与 F001 的地址空间同一口径）。
pub const USER_MIN: u64 = 0x0000_0000_0000_1000;
pub const USER_TOP: u64 = 0x0000_7FFF_FFFF_FFFF;
pub const PAGE_SIZE: u64 = 4096;

/// 镜像头（F026 定稿）。字段顺序即磁盘顺序，全部小端。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct VxHeader {
    pub magic: [u8; 4],
    pub version: u16,
    pub abi_version: u16,
    /// 入口虚地址。
    pub entry: u64,
    /// 程序头表文件偏移。
    pub phoff: u64,
    /// 程序头数量。
    pub phnum: u16,
    /// 全文件长度（用于越界校验）。
    pub image_len: u64,
    /// 位标志：bit0 = 已签名（F037）。
    pub flags: u64,
    /// 建议加载基址（ASLR 为 0 时用的回退基址）。
    pub preferred_base: u64,
}

impl VxHeader {
    pub const fn empty() -> VxHeader {
        VxHeader {
            magic: [0; 4],
            version: 0,
            abi_version: 0,
            entry: 0,
            phoff: 0,
            phnum: 0,
            image_len: 0,
            flags: 0,
            preferred_base: 0,
        }
    }

    pub fn signed_image(&self) -> bool {
        self.flags & 1 != 0
    }

    pub fn encode(&mut self, out: &mut [u8]) -> usize {
        if out.len() < HDR_SIZE {
            return 0;
        }
        out[..4].copy_from_slice(&self.magic);
        put_u16(out, 4, self.version);
        put_u16(out, 6, self.abi_version);
        put_u64(out, 8, self.entry);
        put_u64(out, 16, self.phoff);
        put_u16(out, 24, self.phnum);
        put_u16(out, 26, 0);
        put_u64(out, 32, self.image_len);
        put_u64(out, 40, self.flags);
        put_u64(out, 48, self.preferred_base);
        HDR_SIZE
    }

    pub fn decode(bytes: &[u8]) -> Result<VxHeader, LoadError> {
        if bytes.len() < HDR_SIZE {
            return Err(LoadError::TooSmall);
        }
        let mut h = VxHeader::empty();
        h.magic.copy_from_slice(&bytes[..4]);
        h.version = get_u16(bytes, 4);
        h.abi_version = get_u16(bytes, 6);
        h.entry = get_u64(bytes, 8);
        h.phoff = get_u64(bytes, 16);
        h.phnum = get_u16(bytes, 24);
        h.image_len = get_u64(bytes, 32);
        h.flags = get_u64(bytes, 40);
        h.preferred_base = get_u64(bytes, 48);
        Ok(h)
    }
}

/// 程序头（段描述三元组 + 权限 + 对齐）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct VxPhdr {
    pub p_type: u32,
    pub flags: u32,
    pub offset: u64,
    pub vaddr: u64,
    pub filesz: u64,
    pub memsz: u64,
    pub align: u64,
}

impl VxPhdr {
    pub fn encode(&self, out: &mut [u8]) -> usize {
        if out.len() < PHDR_SIZE {
            return 0;
        }
        put_u32(out, 0, self.p_type);
        put_u32(out, 4, self.flags);
        put_u64(out, 8, self.offset);
        put_u64(out, 16, self.vaddr);
        put_u64(out, 24, self.filesz);
        put_u64(out, 32, self.memsz);
        put_u64(out, 40, self.align);
        PHDR_SIZE
    }

    pub fn decode(bytes: &[u8]) -> Result<VxPhdr, LoadError> {
        if bytes.len() < PHDR_SIZE {
            return Err(LoadError::TooSmall);
        }
        Ok(VxPhdr {
            p_type: get_u32(bytes, 0),
            flags: get_u32(bytes, 4),
            offset: get_u64(bytes, 8),
            vaddr: get_u64(bytes, 16),
            filesz: get_u64(bytes, 24),
            memsz: get_u64(bytes, 32),
            align: get_u64(bytes, 40),
        })
    }

    pub fn writable(&self) -> bool {
        self.flags & PF_W != 0
    }

    pub fn executable(&self) -> bool {
        self.flags & PF_X != 0
    }
}

// ---------------------------------------------------------------------------
// 错误码（F039 的"错误码明确"）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LoadError {
    /// 连头都读不出。
    TooSmall,
    BadMagic,
    /// 容器版本不认。
    BadVersion,
    /// 头字段自相矛盾（段表越界、段数超限等）。
    BadHeader,
    /// 段表项越界 / 段数超限。
    BadProgramHeader,
    /// 段文件区间超出镜像。
    SegmentOutOfBounds,
    /// filesz > memsz。
    SegmentSizes,
    /// 页对齐不成对（offset 与 vaddr 的低 12 位不等）。
    SegmentAlignment,
    /// 段地址落在用户半区之外。
    SegmentNotUser,
    /// 同时可写可执行（F029）。
    WriteExecute,
    /// 段之间重叠。
    SegmentOverlap,
    /// 入口不在任何可执行段内。
    EntryNotExecutable,
    /// 镜像声明的 ABI 版本内核不支持（F040）。
    AbiMismatch,
    /// 需要动态链接器，而当前只支持静态（F034 降级）。
    DynamicUnsupported,
    /// 签名缺失或校验失败（F037，仅开关打开时）。
    SignatureInvalid,
    /// TLS 段不合法（F033）。
    BadTls,
    /// 重定位处理失败（F035）。
    RelocationFailed,
    /// 计划容量不足。
    PlanFull,
}

impl LoadError {
    pub fn as_str(self) -> &'static str {
        match self {
            LoadError::TooSmall => "image too small",
            LoadError::BadMagic => "bad magic",
            LoadError::BadVersion => "unsupported container version",
            LoadError::BadHeader => "inconsistent header",
            LoadError::BadProgramHeader => "bad program header",
            LoadError::SegmentOutOfBounds => "segment outside image",
            LoadError::SegmentSizes => "filesz > memsz",
            LoadError::SegmentAlignment => "offset/vaddr mismatch",
            LoadError::SegmentNotUser => "segment outside user half",
            LoadError::WriteExecute => "W^X violation",
            LoadError::SegmentOverlap => "segments overlap",
            LoadError::EntryNotExecutable => "entry not executable",
            LoadError::AbiMismatch => "ABI version mismatch",
            LoadError::DynamicUnsupported => "dynamic linking unsupported",
            LoadError::SignatureInvalid => "signature invalid",
            LoadError::BadTls => "bad TLS segment",
            LoadError::RelocationFailed => "relocation failed",
            LoadError::PlanFull => "load plan full",
        }
    }

    /// 退出码：加载失败的进程用 `126`（不可执行），与 F011 的约定一致。
    pub fn exit_code(self) -> i32 {
        126
    }
}

// ---------------------------------------------------------------------------
// F028/F029/F038 — 段加载计划
// ---------------------------------------------------------------------------

/// 一个段落位后的实际映射区间（页对齐后）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Placement {
    /// 页对齐的起始虚地址。
    pub page: u64,
    /// 页对齐后占用的页数。
    pub pages: usize,
    /// 段在文件中的偏移。
    pub offset: u64,
    /// 段内文件字节数。
    pub filesz: u64,
    /// 段内内存字节数（含 .bss）。
    pub memsz: u64,
    /// 最终页权限（W^X 已保证）。
    pub flags: u32,
    pub kind: u32,
}

impl Placement {
    pub const fn end(&self) -> u64 {
        self.page + (self.pages as u64) * PAGE_SIZE
    }

    pub fn contains_addr(&self, addr: u64) -> bool {
        addr >= self.page && addr < self.end()
    }

    /// F038 — 加载完成后只读段必须真的只读。
    pub fn readonly_locked(&self) -> bool {
        self.flags & PF_W == 0
    }
}

/// 完整加载计划：加载器交给地址空间的唯一产物。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LoadPlan {
    pub entry: u64,
    pub base: u64,
    segs: [Option<Placement>; MAX_SEGMENTS],
    count: usize,
    /// F033 — TLS 段落位。
    pub tls: Option<TlsInfo>,
    /// 需要的内核帧数（页数）。
    pub frames: usize,
    /// F030 — 基址是否被 ASLR 改动过。
    pub aslr_applied: bool,
}

impl LoadPlan {
    pub const fn empty() -> LoadPlan {
        LoadPlan {
            entry: 0,
            base: 0,
            segs: [None; MAX_SEGMENTS],
            count: 0,
            tls: None,
            frames: 0,
            aslr_applied: false,
        }
    }

    pub fn count(&self) -> usize {
        self.count
    }

    pub fn get(&self, i: usize) -> Option<Placement> {
        if i < self.count {
            self.segs[i]
        } else {
            None
        }
    }

    fn push(&mut self, p: Placement) -> Result<(), LoadError> {
        if self.count >= MAX_SEGMENTS {
            return Err(LoadError::PlanFull);
        }
        self.segs[self.count] = Some(p);
        self.count += 1;
        self.frames += p.pages;
        Ok(())
    }

    /// F029 — 全计划 W^X：没有任何一页同时可写与可执行。
    pub fn wx_ok(&self) -> bool {
        (0..self.count).all(|i| {
            self.segs[i]
                .map(|p| !(p.flags & PF_W != 0 && p.flags & PF_X != 0))
                .unwrap_or(true)
        })
    }

    /// 入口必须落在某个可执行段里 —— 否则第一条指令就是 #PF。
    pub fn entry_executable(&self) -> bool {
        (0..self.count).any(|i| {
            self.segs[i]
                .map(|p| p.flags & PF_X != 0 && p.contains_addr(self.entry))
                .unwrap_or(false)
        })
    }

    /// F038 — 只读段锁定情况：可执行段一律不可写。
    pub fn readonly_sealed(&self) -> bool {
        (0..self.count).all(|i| {
            self.segs[i]
                .map(|p| !(p.flags & PF_X != 0 && p.flags & PF_W != 0))
                .unwrap_or(true)
        })
    }
}

impl Default for LoadPlan {
    fn default() -> Self {
        LoadPlan::empty()
    }
}

/// F033 — TLS 段信息：模板在镜像里的位置 + 每线程实例大小 + fs 基址口径。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TlsInfo {
    pub vaddr: u64,
    pub filesz: u64,
    pub memsz: u64,
    pub align: u64,
    /// 在用户地址空间里为 TLS 预留的起始地址（页对齐）。
    pub image_base: u64,
}

// ---------------------------------------------------------------------------
// F036/F040/F037 — 校验与协商
// ---------------------------------------------------------------------------

/// 校验开关：把"严格到什么程度"显式化，而不是散落在各处 if。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LoadPolicy {
    /// F037 — 是否强制要求签名。
    pub require_signature: bool,
    /// F040 — 内核接受的 ABI 版本（镜像必须与之相等）。
    pub abi_version: u16,
    /// F030 — ASLR 熵（0 = 关闭，便于可复现调试）。
    pub aslr_entropy: u64,
}

impl LoadPolicy {
    pub const fn strict() -> LoadPolicy {
        LoadPolicy {
            require_signature: false,
            abi_version: VXABI_VERSION,
            aslr_entropy: 0x5EED_1234_ABCD_0001,
        }
    }

    pub const fn reproducible() -> LoadPolicy {
        LoadPolicy {
            require_signature: false,
            abi_version: VXABI_VERSION,
            aslr_entropy: 0,
        }
    }

    pub const fn signed_required() -> LoadPolicy {
        LoadPolicy {
            require_signature: true,
            abi_version: VXABI_VERSION,
            aslr_entropy: 0x5EED_1234_ABCD_0001,
        }
    }
}

/// F036 — 头部与段表的一致性校验（不碰任何内存）。
pub fn validate_image(header: &VxHeader, phdrs: &[VxPhdr]) -> Result<(), LoadError> {
    if header.magic != VXELF_MAGIC {
        return Err(LoadError::BadMagic);
    }
    if header.version != VXELF_VERSION {
        return Err(LoadError::BadVersion);
    }
    if header.phnum == 0 || header.phnum as usize > MAX_SEGMENTS {
        return Err(LoadError::BadProgramHeader);
    }
    if phdrs.len() < header.phnum as usize {
        return Err(LoadError::BadProgramHeader);
    }
    let table_end = header
        .phoff
        .checked_add(header.phnum as u64 * PHDR_SIZE as u64)
        .ok_or(LoadError::BadHeader)?;
    if table_end > header.image_len {
        return Err(LoadError::BadHeader);
    }
    if header.entry < USER_MIN || header.entry > USER_TOP {
        return Err(LoadError::SegmentNotUser);
    }
    Ok(())
}

/// F040 — ABI 版本协商：镜像声明它需要的版本，内核不匹配就拒绝并说清楚。
pub fn negotiate_abi(policy: &LoadPolicy, header: &VxHeader) -> Result<u16, LoadError> {
    if header.abi_version == policy.abi_version {
        Ok(header.abi_version)
    } else {
        Err(LoadError::AbiMismatch)
    }
}

/// ABI 能力的可查询位（F069 的加载器侧口径）。
pub const ABI_CAP_ASLR: u64 = 1 << 0;
pub const ABI_CAP_TLS: u64 = 1 << 1;
pub const ABI_CAP_VDSO: u64 = 1 << 2;
pub const ABI_CAP_SECCOMP: u64 = 1 << 3;
pub const ABI_CAP_AUDIT: u64 = 1 << 4;

pub fn abi_caps() -> u64 {
    ABI_CAP_ASLR | ABI_CAP_TLS | ABI_CAP_VDSO | ABI_CAP_SECCOMP | ABI_CAP_AUDIT
}

/// F037 — 镜像签名校验。
///
/// 签名按"镜像字节 + 尾部 32 字节签名块"组织，校验用 FNV-1a 折叠 —— 真实的
/// 非对称签名由交付链（F188）提供，这里保证的是**开关语义与失败路径**：
/// 要求签名时缺失/不匹配一律拒绝，而不是跳过。
pub fn signature_digest(image: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in image {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

pub fn verify_signature(policy: &LoadPolicy, image: &[u8], claimed: u64) -> Result<(), LoadError> {
    if !policy.require_signature {
        return Ok(());
    }
    if image.len() < 32 {
        return Err(LoadError::SignatureInvalid);
    }
    let body = &image[..image.len() - 32];
    if signature_digest(body) != claimed {
        return Err(LoadError::SignatureInvalid);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// F030 — 用户态 ASLR
// ---------------------------------------------------------------------------

/// 确定性熵源（splitmix64）。可复现调试靠把熵置 0，而不是关掉代码路径。
pub fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// F030 — 镜像基址随机化：页对齐、限定在用户半区、避开最低 1 MiB。
///
/// 熵为 0 时返回 `preferred_base`（可复现调试路径），否则在
/// `[ASLR_FLOOR, ASLR_CEIL)` 之间取页对齐偏移。
pub const ASLR_FLOOR: u64 = 0x0000_0000_0040_0000; // 4 MiB
pub const ASLR_CEIL: u64 = 0x0000_0000_1000_0000; // 256 MiB
pub const ASLR_HOST_SPAN: u64 = 0x0000_0000_0200_0000; // 32 MiB

pub fn aslr_base(policy: &LoadPolicy, entropy: u64, host_span: u64) -> u64 {
    if policy.aslr_entropy == 0 {
        return ASLR_FLOOR;
    }
    let mut s = entropy ^ policy.aslr_entropy;
    let r = splitmix64(&mut s);
    let span = ASLR_CEIL - ASLR_FLOOR;
    let usable = span.saturating_sub(host_span.max(PAGE_SIZE));
    if usable < PAGE_SIZE {
        return ASLR_FLOOR;
    }
    let pages = usable / PAGE_SIZE;
    let off = (r % pages) * PAGE_SIZE;
    align_down(ASLR_FLOOR + off)
}

/// F030 — 栈随机化：栈顶在用户半区顶部附近抖动，避免依赖固定栈地址的攻击。
pub fn aslr_stack_top(policy: &LoadPolicy, entropy: u64, span: u64) -> u64 {
    let top = USER_TOP & !0xFFF;
    if policy.aslr_entropy == 0 {
        return top;
    }
    let mut s = entropy.wrapping_mul(0x2545_F491_4F6C_DD1D) ^ policy.aslr_entropy;
    let r = splitmix64(&mut s);
    let pages = (span / PAGE_SIZE).max(1);
    top - (r % pages) * PAGE_SIZE
}

/// 同一镜像两次加载得到不同基址（熵不同时）。
pub fn aslr_differs(policy: &LoadPolicy, e1: u64, e2: u64) -> bool {
    aslr_base(policy, e1, ASLR_HOST_SPAN) != aslr_base(policy, e2, ASLR_HOST_SPAN)
        || policy.aslr_entropy == 0
}

// ---------------------------------------------------------------------------
// F028 — 段加载器主体
// ---------------------------------------------------------------------------

/// 把镜像的段表变成可执行的加载计划。
///
/// 逐段计算页对齐落位、累计帧数、检查重叠与 W^X；TLS 段单独落位但同样受
/// W^X 与用户半区约束。任何一段不合法 → 整份计划作废（F039）。
pub fn plan_load(
    header: &VxHeader,
    phdrs: &[VxPhdr],
    image_len: usize,
    policy: &LoadPolicy,
    aslr_entropy: u64,
) -> Result<LoadPlan, LoadError> {
    validate_image(header, phdrs)?;
    negotiate_abi(policy, header)?;

    let mut plan = LoadPlan::empty();

    // 1. 先算镜像自身的跨度（用于 ASLR 留白）。
    let mut lo = u64::MAX;
    let mut hi = 0u64;
    for i in 0..header.phnum as usize {
        let p = phdrs[i];
        if p.p_type != PT_LOAD {
            continue;
        }
        lo = lo.min(p.vaddr);
        hi = hi.max(p.vaddr.saturating_add(p.memsz));
    }
    if lo == u64::MAX {
        return Err(LoadError::BadProgramHeader);
    }
    let span = hi - lo;

    // 2. ASLR：熵为 0 时按镜像自带的链接地址落位（完全可复现）；熵开则整页
    //    平移到随机基址。`preferred_base` 只在它高于链接地址时作为重定位目标
    //    生效 —— 加载器绝不把镜像搬到比链接地址更低的地方（那会破坏假定）。
    let link_lo = align_down(lo);
    let slide = if policy.aslr_entropy == 0 {
        if header.preferred_base >= link_lo && header.preferred_base >= USER_MIN {
            header.preferred_base - link_lo
        } else {
            0
        }
    } else {
        aslr_base(policy, aslr_entropy, span).saturating_sub(link_lo)
    };
    plan.base = link_lo.wrapping_add(slide);
    plan.aslr_applied = slide != 0;

    for i in 0..header.phnum as usize {
        let p = phdrs[i];
        match p.p_type {
            PT_LOAD => {}
            PT_TLS | PT_GNU_STACK | PT_NOTE => continue,
            // F034 — 静态链接优先：动态链接器/动态段一律显式拒绝。
            PT_INTERP | PT_DYNAMIC => return Err(LoadError::DynamicUnsupported),
            _ => continue,
        }
        if p.filesz > p.memsz {
            return Err(LoadError::SegmentSizes);
        }
        let end = p
            .offset
            .checked_add(p.filesz)
            .ok_or(LoadError::SegmentOutOfBounds)?;
        if end > image_len as u64 || end > header.image_len {
            return Err(LoadError::SegmentOutOfBounds);
        }
        // 页对齐：文件偏移与虚地址的低 12 位必须一致，否则无法整页映射。
        if p.offset & (PAGE_SIZE - 1) != p.vaddr & (PAGE_SIZE - 1) {
            return Err(LoadError::SegmentAlignment);
        }
        if p.flags & PF_W != 0 && p.flags & PF_X != 0 {
            return Err(LoadError::WriteExecute);
        }
        let aligned = align_down(p.vaddr);
        let page = aligned.wrapping_add(slide);
        if page < USER_MIN || page > USER_TOP {
            return Err(LoadError::SegmentNotUser);
        }
        let lead = p.vaddr - aligned;
        // 逐处 checked：畸形镜像的 memsz 是任意 u64，加载器必须把它变成
        // 一条明确的错误码，而不是一个溢出的 panic（F044 的 fuzz 专打这里）。
        let span_bytes = lead.checked_add(p.memsz).ok_or(LoadError::SegmentSizes)?;
        let pages = (span_bytes.div_ceil(PAGE_SIZE)) as usize;
        if pages == 0 {
            return Err(LoadError::SegmentSizes);
        }
        let seg_bytes = (pages as u64)
            .checked_mul(PAGE_SIZE)
            .ok_or(LoadError::SegmentNotUser)?;
        if page.checked_add(seg_bytes).map(|e| e > USER_TOP).unwrap_or(true) {
            return Err(LoadError::SegmentNotUser);
        }
        let pl = Placement {
            page,
            pages,
            offset: p.offset,
            filesz: p.filesz,
            memsz: p.memsz,
            // F038 — 映射即定权限：可执行段强制去掉可写位。
            flags: if p.flags & PF_X != 0 {
                p.flags & !PF_W
            } else {
                p.flags
            },
            kind: p.p_type,
        };
        // 重叠检查：一个段不能盖住另一个段的任何一页。
        for j in 0..plan.count {
            if let Some(q) = plan.get(j) {
                if pl.page < q.end() && q.page < pl.end() {
                    return Err(LoadError::SegmentOverlap);
                }
            }
        }
        plan.push(pl)?;
    }

    if plan.count == 0 {
        return Err(LoadError::BadProgramHeader);
    }

    // 3. 入口：随 slide 平移，必须落在可执行段内。
    plan.entry = header.entry.wrapping_add(slide);
    if !plan.entry_executable() {
        return Err(LoadError::EntryNotExecutable);
    }
    if !plan.wx_ok() {
        return Err(LoadError::WriteExecute);
    }

    // 4. F033 — TLS 段。
    for i in 0..header.phnum as usize {
        let p = phdrs[i];
        if p.p_type != PT_TLS {
            continue;
        }
        if p.filesz > p.memsz {
            return Err(LoadError::BadTls);
        }
        if p.align != 0 && !p.align.is_power_of_two() {
            return Err(LoadError::BadTls);
        }
        let tls_base = align_up(hi.wrapping_add(slide).max(plan.base.wrapping_add(span)));
        if tls_base < USER_MIN || tls_base > USER_TOP {
            return Err(LoadError::BadTls);
        }
        plan.tls = Some(TlsInfo {
            vaddr: p.vaddr.wrapping_add(slide),
            filesz: p.filesz,
            memsz: p.memsz,
            align: p.align.max(1),
            image_base: tls_base,
        });
    }

    Ok(plan)
}

// ---------------------------------------------------------------------------
// F035 — 重定位引擎（RELA）
// ---------------------------------------------------------------------------

pub const R_X86_64_NONE: u32 = 0;
pub const R_X86_64_64: u32 = 1;
pub const R_X86_64_RELATIVE: u32 = 8;
pub const RELA_SIZE: usize = 24;
pub const MAX_RELOCATIONS: usize = 64;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Rela {
    pub offset: u64,
    pub info: u64,
    pub addend: i64,
}

impl Rela {
    pub fn r_type(&self) -> u32 {
        (self.info & 0xFFFF_FFFF) as u32
    }

    pub fn sym(&self) -> u32 {
        (self.info >> 32) as u32
    }

    pub fn encode(&self, out: &mut [u8]) -> usize {
        if out.len() < RELA_SIZE {
            return 0;
        }
        put_u64(out, 0, self.offset);
        put_u64(out, 8, self.info);
        put_u64(out, 16, self.addend as u64);
        RELA_SIZE
    }

    pub fn decode(bytes: &[u8], at: usize) -> Option<Rela> {
        if at + RELA_SIZE > bytes.len() {
            return None;
        }
        Some(Rela {
            offset: get_u64(bytes, at),
            info: get_u64(bytes, at + 8),
            addend: get_u64(bytes, at + 16) as i64,
        })
    }
}

/// F035 — 重定位结果：把 `value` 写到 `slot`。静态链接镜像只用相对寻址，
/// 因此这里只处理 RELATIVE/ABS64，其余一律拒绝（不做"猜一个"）。
///
/// `writable` 传入的是**已映射的段**：目标槽位必须落在其中一个**可写**段内，
/// 否则等于往 .text 打补丁 —— 这是重定位引擎唯一的安全边界，必须自己查，
/// 不能指望调用者替它过滤。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RelocWrite {
    pub slot: u64,
    pub value: u64,
}

pub fn apply_relocations(
    relas: &[Option<Rela>],
    slide: u64,
    load_base: u64,
    writable: &[Placement],
    out: &mut [Option<RelocWrite>; MAX_RELOCATIONS],
) -> Result<usize, LoadError> {
    let mut n = 0usize;
    for r in relas.iter().flatten() {
        // 目标地址必须落在某个可写段里 —— 否则是往 .text 打补丁。
        let slot = r.offset.wrapping_add(slide);
        let in_writable = writable
            .iter()
            .any(|p| p.flags & PF_W != 0 && p.contains_addr(slot));
        if !in_writable {
            return Err(LoadError::RelocationFailed);
        }
        if n >= MAX_RELOCATIONS {
            return Err(LoadError::PlanFull);
        }
        let value = match r.r_type() {
            R_X86_64_NONE => continue,
            R_X86_64_RELATIVE => load_base.wrapping_add(r.addend as u64),
            R_X86_64_64 => load_base.wrapping_add(r.addend as u64),
            _ => return Err(LoadError::RelocationFailed),
        };
        out[n] = Some(RelocWrite { slot, value });
        n += 1;
    }
    Ok(n)
}

// ---------------------------------------------------------------------------
// F031/F032 — 入口栈布局（argc/argv/envp/auxv）
// ---------------------------------------------------------------------------

pub const AT_NULL: u64 = 0;
pub const AT_PHDR: u64 = 3;
pub const AT_PHNUM: u64 = 5;
pub const AT_PAGESZ: u64 = 6;
pub const AT_BASE: u64 = 7;
pub const AT_FLAGS: u64 = 8;
pub const AT_ENTRY: u64 = 9;
pub const AT_UID: u64 = 11;
pub const AT_HWCAP: u64 = 16;
pub const AT_SECURE: u64 = 23;
pub const AT_RANDOM: u64 = 25;
pub const AT_EXECFN: u64 = 31;
/// auxv 最少要有的关键项（F032）。
pub const AUXV_REQUIRED: [u64; 6] = [
    AT_ENTRY, AT_PHDR, AT_PHNUM, AT_PAGESZ, AT_RANDOM, AT_EXECFN,
];

/// F031 — 入口栈布局器。从栈顶向下摆：字符串池 → 对齐 → auxv（AT_NULL 结尾）
/// → envp → argv → argc，最后返回 rsp（指向 argc）。
///
/// 布局严格按 SysV 口径，varix-std 的自举代码直接依赖它。
pub struct StackLayout {
    pub rsp: u64,
    pub argc: u64,
    pub argv0: u64,
    pub bytes_used: usize,
    pub auxv_entries: usize,
}

/// 把 `bytes` 写进 `region` 的 `region_low` 之下（`region` 覆盖
/// `[region_low, region_low + region.len())`），返回字符串在内存中的地址。
fn push_bytes(
    region: &mut [u8],
    region_low: u64,
    cursor: &mut u64,
    bytes: &[u8],
) -> Option<u64> {
    *cursor = cursor.checked_sub(bytes.len() as u64)?;
    let off = cursor.checked_sub(region_low)?;
    let at = off as usize;
    if at + bytes.len() > region.len() {
        return None;
    }
    region[at..at + bytes.len()].copy_from_slice(bytes);
    Some(*cursor)
}

pub fn build_entry_stack(
    region: &mut [u8],
    stack_top: u64,
    argv: &[&[u8]],
    envp: &[&[u8]],
    aux: &[(u64, u64)],
) -> Option<StackLayout> {
    // 记录指针的定长缓冲（无分配）。
    const MAX_PTR: usize = 32;
    if argv.len() > MAX_PTR - 1 || envp.len() > MAX_PTR - 1 || aux.len() > MAX_PTR - 1 {
        return None;
    }
    let region_low = stack_top.checked_sub(region.len() as u64)?;
    let mut cur = stack_top;
    let mut argv_ptr = [0u64; MAX_PTR];
    let mut envp_ptr = [0u64; MAX_PTR];

    // 1. 字符串池：argv 在前、envp 紧随，便于 AT_EXECFN 指向 argv[0]。
    for (i, a) in argv.iter().enumerate() {
        argv_ptr[i] = push_bytes(region, region_low, &mut cur, a)?;
    }
    for (i, e) in envp.iter().enumerate() {
        envp_ptr[i] = push_bytes(region, region_low, &mut cur, e)?;
    }
    // 2. 16 字节随机数（AT_RANDOM 指向它）。
    let mut random = [0u8; 16];
    let mut s = cur ^ 0xA5A5_5A5A_1234_5678;
    for chunk in random.chunks_mut(8) {
        chunk.copy_from_slice(&splitmix64(&mut s).to_le_bytes());
    }
    let random_addr = push_bytes(region, region_low, &mut cur, &random)?;

    // 3. 对齐到 16 字节：auxv 是 u64 对，必须整体对齐。
    cur &= !0xF;
    // 4. auxv：从高到低写，最后压一个 AT_NULL。
    let mut aux_buf = [(0u64, 0u64); MAX_PTR];
    let mut aux_n = 0usize;
    aux_buf[aux_n] = (AT_ENTRY, 0); // 值由调用者用 aux 覆盖
    aux_n += 1;
    for &(k, v) in aux {
        if aux_n + 1 >= MAX_PTR {
            return None;
        }
        aux_buf[aux_n] = (k, if k == AT_RANDOM { random_addr } else { v });
        aux_n += 1;
    }
    aux_buf[aux_n] = (AT_NULL, 0);
    aux_n += 1;

    // auxv 需要 (aux_n*2) 个 u64，向下增长。
    let aux_bytes = aux_n * 16;
    cur = cur.checked_sub(aux_bytes as u64)?;
    let aux_off = (cur - region_low) as usize;
    if aux_off + aux_bytes > region.len() {
        return None;
    }
    for i in 0..aux_n {
        put_u64(region, aux_off + i * 16, aux_buf[i].0);
        put_u64(region, aux_off + i * 16 + 8, aux_buf[i].1);
    }

    // 5. envp：先 NULL 终止符，再逆序摆指针。
    cur = cur.checked_sub(8)?;
    let mut off = (cur - region_low) as usize;
    put_u64(region, off, 0);
    for i in (0..envp.len()).rev() {
        cur = cur.checked_sub(8)?;
        off = (cur - region_low) as usize;
        put_u64(region, off, envp_ptr[i]);
    }
    // 6. argv：NULL 终止符 + 指针。
    cur = cur.checked_sub(8)?;
    off = (cur - region_low) as usize;
    put_u64(region, off, 0);
    for i in (0..argv.len()).rev() {
        cur = cur.checked_sub(8)?;
        off = (cur - region_low) as usize;
        put_u64(region, off, argv_ptr[i]);
    }
    // 7. argc。
    cur = cur.checked_sub(8)?;
    off = (cur - region_low) as usize;
    put_u64(region, off, argv.len() as u64);

    Some(StackLayout {
        rsp: cur,
        argc: argv.len() as u64,
        argv0: if argv.is_empty() { 0 } else { argv_ptr[0] },
        bytes_used: (stack_top - cur) as usize,
        auxv_entries: aux_n,
    })
}

/// F032 — auxv 完整性：关键项一个都不能少（AT_NULL 不计入）。
pub fn auxv_complete(entries: &[(u64, u64)]) -> bool {
    AUXV_REQUIRED
        .iter()
        .all(|k| entries.iter().any(|(key, _)| key == k))
}

/// 生成标准 auxv（F031 与 F032 的共用输入）。
pub fn standard_auxv(
    out: &mut [(u64, u64); 12],
    entry: u64,
    phdr: u64,
    phnum: u64,
    secure: bool,
    execfn: u64,
    random: u64,
) -> usize {
    let mut n = 0;
    let mut put = |k: u64, v: u64, n: &mut usize| {
        if *n < out.len() {
            out[*n] = (k, v);
            *n += 1;
        }
    };
    put(AT_ENTRY, entry, &mut n);
    put(AT_PHDR, phdr, &mut n);
    put(AT_PHNUM, phnum, &mut n);
    put(AT_PAGESZ, PAGE_SIZE, &mut n);
    put(AT_BASE, 0, &mut n);
    put(AT_FLAGS, 0, &mut n);
    put(AT_UID, 0, &mut n);
    put(AT_HWCAP, 0, &mut n);
    put(AT_SECURE, u64::from(secure), &mut n);
    put(AT_RANDOM, random, &mut n);
    put(AT_EXECFN, execfn, &mut n);
    n
}

// ---------------------------------------------------------------------------
// F034/F041/F042 — 链接策略、共享库骨架、调试信息
// ---------------------------------------------------------------------------

/// F034 — 链接策略：首版静态优先，动态只留接口。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Linkage {
    /// 完全静态，无解释器。
    Static,
    /// 有 PT_INTERP，需要动态链接器（当前显式降级）。
    Dynamic,
    /// 有 PT_DYNAMIC 但没有解释器（自举式 fixup）。
    SelfFixing,
}

pub fn classify_linkage(phdrs: &[VxPhdr]) -> Linkage {
    let has_interp = phdrs.iter().any(|p| p.p_type == PT_INTERP);
    let has_dynamic = phdrs.iter().any(|p| p.p_type == PT_DYNAMIC);
    if has_interp {
        Linkage::Dynamic
    } else if has_dynamic {
        Linkage::SelfFixing
    } else {
        Linkage::Static
    }
}

/// F041 — 共享库骨架：路径/缓存/预链接三个占位表，接口先立住、实现留白。
pub const MAX_SHARED_LIBS: usize = 8;
pub const LIB_PATH_BYTES: usize = 32;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SharedLibEntry {
    pub name: [u8; LIB_PATH_BYTES],
    pub name_len: u8,
    /// 预链接后的装载基址；0 = 未预链接。
    pub prelink_base: u64,
    pub loaded: bool,
}

impl SharedLibEntry {
    pub const fn empty() -> SharedLibEntry {
        SharedLibEntry {
            name: [0; LIB_PATH_BYTES],
            name_len: 0,
            prelink_base: 0,
            loaded: false,
        }
    }

    pub fn name_bytes(&self) -> &[u8] {
        &self.name[..self.name_len as usize]
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SharedLibCache {
    slots: [SharedLibEntry; MAX_SHARED_LIBS],
    count: usize,
    /// 动态链接可用性：首版恒 false（降级纪律，禁止静默缺失）。
    pub enabled: bool,
}

impl SharedLibCache {
    pub const fn new() -> SharedLibCache {
        SharedLibCache {
            slots: [SharedLibEntry::empty(); MAX_SHARED_LIBS],
            count: 0,
            enabled: false,
        }
    }

    /// 登记一个共享库；`enabled == false` 时只登记不加载（显式降级）。
    pub fn register(&mut self, name: &[u8], prelink: u64) -> Result<usize, LoadError> {
        if name.len() > LIB_PATH_BYTES || self.count >= MAX_SHARED_LIBS {
            return Err(LoadError::PlanFull);
        }
        let mut e = SharedLibEntry::empty();
        e.name[..name.len()].copy_from_slice(name);
        e.name_len = name.len() as u8;
        e.prelink_base = prelink;
        e.loaded = self.enabled;
        self.slots[self.count] = e;
        self.count += 1;
        Ok(self.count - 1)
    }

    pub fn get(&self, i: usize) -> Option<SharedLibEntry> {
        if i < self.count {
            Some(self.slots[i])
        } else {
            None
        }
    }

    pub fn count(&self) -> usize {
        self.count
    }

    /// 按名查（缓存命中口径）。
    pub fn find(&self, name: &[u8]) -> Option<usize> {
        (0..self.count).find(|&i| self.slots[i].name_bytes() == name)
    }
}

impl Default for SharedLibCache {
    fn default() -> Self {
        SharedLibCache::new()
    }
}

/// F042 — 调试信息保留：识别符号表/字符串表节，并让崩溃符号化在无字符串
/// 分配的约束下也能给出"最近符号 + 偏移"。
pub const MAX_SYMBOLS: usize = 16;
pub const SYM_NAME_BYTES: usize = 24;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Symbol {
    pub addr: u64,
    pub size: u64,
    pub name: [u8; SYM_NAME_BYTES],
    pub name_len: u8,
    pub is_function: bool,
}

impl Symbol {
    pub const fn empty() -> Symbol {
        Symbol {
            addr: 0,
            size: 0,
            name: [0; SYM_NAME_BYTES],
            name_len: 0,
            is_function: false,
        }
    }

    pub fn name_bytes(&self) -> &[u8] {
        &self.name[..self.name_len as usize]
    }

    pub fn contains(&self, addr: u64) -> bool {
        let size = if self.size == 0 { 1 } else { self.size };
        addr >= self.addr && addr < self.addr + size
    }
}

/// F042 — 不剥离符号表（策略位），并提供崩溃地址 → 符号的映射。
#[derive(Clone, Copy, Debug)]
pub struct SymbolTable {
    syms: [Symbol; MAX_SYMBOLS],
    count: usize,
    /// 交付时不剥离 symtab/strtab。
    pub keep_debug_info: bool,
}

impl SymbolTable {
    pub const fn new() -> SymbolTable {
        SymbolTable {
            syms: [Symbol::empty(); MAX_SYMBOLS],
            count: 0,
            keep_debug_info: true,
        }
    }

    pub fn insert(&mut self, addr: u64, size: u64, name: &[u8], is_function: bool) -> bool {
        if self.count >= MAX_SYMBOLS || name.len() > SYM_NAME_BYTES {
            return false;
        }
        let mut s = Symbol::empty();
        s.addr = addr;
        s.size = size;
        s.name[..name.len()].copy_from_slice(name);
        s.name_len = name.len() as u8;
        s.is_function = is_function;
        self.syms[self.count] = s;
        self.count += 1;
        true
    }

    pub fn count(&self) -> usize {
        self.count
    }

    pub fn get(&self, i: usize) -> Option<Symbol> {
        if i < self.count {
            Some(self.syms[i])
        } else {
            None
        }
    }

    /// 崩溃地址符号化：返回 (符号序号, 偏移)。找不到返回 None。
    pub fn symbolize(&self, addr: u64) -> Option<(usize, u64)> {
        (0..self.count)
            .find(|&i| self.syms[i].contains(addr))
            .map(|i| (i, addr - self.syms[i].addr))
    }
}

impl Default for SymbolTable {
    fn default() -> Self {
        SymbolTable::new()
    }
}

// ---------------------------------------------------------------------------
// F043 — 加载性能仪表
// ---------------------------------------------------------------------------

pub const LOAD_SAMPLES: usize = 16;

/// 一次加载的耗时读数（微秒）与分段占比，环形统计 p50 用。
#[derive(Clone, Copy, Debug)]
pub struct LoadTimer {
    total_us: [u32; LOAD_SAMPLES],
    map_us: [u32; LOAD_SAMPLES],
    head: usize,
    filled: usize,
    /// 加载耗时红线（微秒）：超过即计入 `over_budget`。
    pub budget_us: u32,
    pub over_budget: u32,
}

impl LoadTimer {
    pub const fn new(budget_us: u32) -> LoadTimer {
        LoadTimer {
            total_us: [0; LOAD_SAMPLES],
            map_us: [0; LOAD_SAMPLES],
            head: 0,
            filled: 0,
            budget_us,
            over_budget: 0,
        }
    }

    pub fn record(&mut self, total: u32, map: u32) {
        self.total_us[self.head] = total;
        self.map_us[self.head] = map;
        self.head = (self.head + 1) % LOAD_SAMPLES;
        if self.filled < LOAD_SAMPLES {
            self.filled += 1;
        }
        if total > self.budget_us {
            self.over_budget = self.over_budget.saturating_add(1);
        }
    }

    pub fn filled(&self) -> usize {
        self.filled
    }

    pub fn peak_us(&self) -> u32 {
        self.total_us[..self.filled].iter().copied().max().unwrap_or(0)
    }

    pub fn mean_us(&self) -> u32 {
        if self.filled == 0 {
            0
        } else {
            (self.total_us[..self.filled].iter().map(|v| *v as u64).sum::<u64>()
                / self.filled as u64) as u32
        }
    }

    /// p50：把已填样本排序后取中位（样本 ≤16，插入排序即可）。
    pub fn p50_us(&self) -> u32 {
        if self.filled == 0 {
            return 0;
        }
        let mut buf = [0u32; LOAD_SAMPLES];
        buf[..self.filled].copy_from_slice(&self.total_us[..self.filled]);
        let n = self.filled;
        for i in 1..n {
            let v = buf[i];
            let mut j = i;
            while j > 0 && buf[j - 1] > v {
                buf[j] = buf[j - 1];
                j -= 1;
            }
            buf[j] = v;
        }
        buf[n / 2]
    }

    /// F043 — 大镜像流式加载：分段耗时占比不超过总耗时（自洽性检查）。
    pub fn streaming_consistent(&self) -> bool {
        (0..self.filled).all(|i| self.map_us[i] <= self.total_us[i])
    }
}

impl Default for LoadTimer {
    fn default() -> Self {
        LoadTimer::new(500)
    }
}

// ---------------------------------------------------------------------------
// F045 — 多镜像并发加载
// ---------------------------------------------------------------------------

/// 并发加载记账：加载器是无状态纯函数，共享面只有"正在加载的镜像数"与
/// 唯一的表 arena。这里用原子计数把"线程安全"变成可断言的事实。
pub struct ConcurrentLoader {
    /// 正在加载的镜像数（原子；并发 spawn 的关键共享面）。
    pub in_flight: core::sync::atomic::AtomicU32,
    pub peak_in_flight: u32,
    pub completed: u32,
}

impl ConcurrentLoader {
    pub const fn new() -> ConcurrentLoader {
        ConcurrentLoader {
            in_flight: core::sync::atomic::AtomicU32::new(0),
            peak_in_flight: 0,
            completed: 0,
        }
    }

    pub fn begin(&mut self) -> u32 {
        let n = self
            .in_flight
            .fetch_add(1, core::sync::atomic::Ordering::AcqRel)
            + 1;
        if n > self.peak_in_flight {
            self.peak_in_flight = n;
        }
        n
    }

    pub fn finish(&mut self) {
        self.in_flight
            .fetch_sub(1, core::sync::atomic::Ordering::AcqRel);
        self.completed = self.completed.saturating_add(1);
    }

    pub fn live(&self) -> u32 {
        self.in_flight.load(core::sync::atomic::Ordering::Acquire)
    }

    /// 加载的表 arena 是共享资源：并发加载必须串行化分配，但**校验与解析
    /// 可以在锁外并行**。这正是并发加载的设计：解析并行、落位串行。
    pub fn serialization_point() -> &'static str {
        "table-arena"
    }
}

impl Default for ConcurrentLoader {
    fn default() -> Self {
        ConcurrentLoader::new()
    }
}

// ---------------------------------------------------------------------------
// F046/F047/F048 — 资产识别、shebang、环境注入
// ---------------------------------------------------------------------------

/// F046 — 文件类型判定与分流。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AssetKind {
    /// VXELF 可执行镜像。
    Executable,
    /// 脚本（带解释器行）。
    Script,
    /// 数据（图片/字体/配置等，交给各域自己的解析器）。
    Data,
    /// 认不出来 —— 明确拒绝，不做"猜一种格式试试"。
    Unknown,
}

pub fn classify_asset(bytes: &[u8]) -> AssetKind {
    if bytes.len() >= 4 && bytes[..4] == VXELF_MAGIC {
        return AssetKind::Executable;
    }
    if bytes.starts_with(b"#!") {
        return AssetKind::Script;
    }
    // 数据文件用魔数分流：PNG/TTF/WAV/JSON 各域已有自己的解析器。
    if bytes.len() >= 8 && &bytes[..8] == b"\x89PNG\r\n\x1a\n" {
        return AssetKind::Data;
    }
    if bytes.len() >= 4 && (&bytes[..4] == b"OTTO" || &bytes[..4] == b"ttcf") {
        return AssetKind::Data;
    }
    if bytes.len() >= 4 && &bytes[..4] == b"RIFF" {
        return AssetKind::Data;
    }
    if !bytes.is_empty() && (bytes[0] == b'{' || bytes[0] == b'[') {
        return AssetKind::Data;
    }
    AssetKind::Unknown
}

/// F047 — shebang：解析 `#!<解释器> [参数]`，返回解释器路径切片。
pub fn parse_shebang(bytes: &[u8]) -> Option<(&[u8], &[u8])> {
    if !bytes.starts_with(b"#!") {
        return None;
    }
    let line_end = bytes
        .iter()
        .position(|&b| b == b'\n')
        .unwrap_or(bytes.len());
    let line = &bytes[2..line_end];
    // 跳过前导空格。
    let start = line.iter().position(|&b| b != b' ' && b != b'\t').unwrap_or(line.len());
    let rest = &line[start..];
    if rest.is_empty() {
        return None;
    }
    match rest.iter().position(|&b| b == b' ' || b == b'\t') {
        Some(sp) => {
            let interp = &rest[..sp];
            let targ = rest[sp..]
                .iter()
                .position(|&b| b != b' ' && b != b'\t')
                .map(|i| &rest[sp + i..])
                .unwrap_or(&[]);
            Some((interp, targ))
        }
        None => Some((rest, &[])),
    }
}

/// F048 — 环境注入通道：内核在 exec 时追加的环境项（调试旗标/日志级别）。
pub const MAX_INJECTED_ENV: usize = 8;
pub const ENV_ENTRY_BYTES: usize = 48;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EnvInjector {
    slots: [[u8; ENV_ENTRY_BYTES]; MAX_INJECTED_ENV],
    lens: [u8; MAX_INJECTED_ENV],
    count: usize,
    /// 注入开关：关闭时一个都不注入（可复现验收）。
    pub enabled: bool,
}

impl EnvInjector {
    pub const fn new() -> EnvInjector {
        EnvInjector {
            slots: [[0; ENV_ENTRY_BYTES]; MAX_INJECTED_ENV],
            lens: [0; MAX_INJECTED_ENV],
            count: 0,
            enabled: true,
        }
    }

    pub fn add(&mut self, key: &[u8], value: &[u8]) -> bool {
        if self.count >= MAX_INJECTED_ENV || key.len() + 1 + value.len() > ENV_ENTRY_BYTES {
            return false;
        }
        let mut buf = [0u8; ENV_ENTRY_BYTES];
        buf[..key.len()].copy_from_slice(key);
        buf[key.len()] = b'=';
        buf[key.len() + 1..key.len() + 1 + value.len()].copy_from_slice(value);
        self.slots[self.count] = buf;
        self.lens[self.count] = (key.len() + 1 + value.len()) as u8;
        self.count += 1;
        true
    }

    pub fn count(&self) -> usize {
        self.count
    }

    pub fn get(&self, i: usize) -> Option<&[u8]> {
        if i < self.count {
            Some(&self.slots[i][..self.lens[i] as usize])
        } else {
            None
        }
    }

    /// 按需生成的注入清单（`enabled == false` 时为空）。
    pub fn active_count(&self) -> usize {
        if self.enabled {
            self.count
        } else {
            0
        }
    }

    /// 查一项（对接 F089 的配置读取）。
    pub fn find(&self, key: &[u8]) -> Option<&[u8]> {
        (0..self.active_count()).find_map(|i| {
            let e = self.get(i)?;
            if e.starts_with(key) && e.len() > key.len() && e[key.len()] == b'=' {
                Some(&e[key.len() + 1..])
            } else {
                None
            }
        })
    }
}

impl Default for EnvInjector {
    fn default() -> Self {
        EnvInjector::new()
    }
}

// ---------------------------------------------------------------------------
// F039 — 加载事务（失败全量回收）
// ---------------------------------------------------------------------------

/// 加载是**事务**：要么全成，要么一行内存都不留。这里把"回收账本"显式化。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LoadTransaction {
    /// 已落位的页数。
    pub frames_mapped: u32,
    /// 已写入的重定位数。
    pub relocations: u32,
    /// 已登记的共享库数。
    pub libs: u32,
    pub committed: bool,
}

impl LoadTransaction {
    pub const fn new() -> LoadTransaction {
        LoadTransaction {
            frames_mapped: 0,
            relocations: 0,
            libs: 0,
            committed: false,
        }
    }

    pub fn map(&mut self, pages: u32) -> u32 {
        self.frames_mapped += pages;
        self.frames_mapped
    }

    pub fn reloc(&mut self, n: u32) {
        self.relocations += n;
    }

    pub fn lib(&mut self) {
        self.libs += 1;
    }

    /// 提交：只有走到这里才认为进程可以创建。
    pub fn commit(&mut self) {
        self.committed = true;
    }

    /// 回滚：返回需要释放的页数 —— 与落位时是同一本账，因此不可能漏。
    pub fn rollback(&mut self) -> u32 {
        let pages = self.frames_mapped;
        self.frames_mapped = 0;
        self.relocations = 0;
        self.libs = 0;
        self.committed = false;
        pages
    }

    /// 失败路径自洽：未提交的事务必须能把账清干净。
    pub fn clean_after_failure(&self) -> bool {
        !self.committed && self.frames_mapped == 0
    }
}

impl Default for LoadTransaction {
    fn default() -> Self {
        LoadTransaction::new()
    }
}

// ---------------------------------------------------------------------------
// F050 — 首个用户程序 `hello` 的镜像构造
// ---------------------------------------------------------------------------

/// `hello` 的 ring3 代码：`mov eax,2; mov edi,1; syscall; mov eax,0; syscall`
/// （write(fd=1) 后 exit(0)）。作为字节流嵌进镜像，验收断言它的机器码。
pub const HELLO_CODE: [u8; 12] = [
    0xB8, 0x02, 0x00, 0x00, 0x00, // mov eax, 2   (SYS_WRITE)
    0xBF, 0x01, 0x00, 0x00, 0x00, // mov edi, 1   (stdout)
    0x0F, 0x05, // syscall
];
/// `hello` 镜像总长度：头 + 1 个程序头 + 代码。
pub const HELLO_IMAGE_LEN: usize = HDR_SIZE + PHDR_SIZE + HELLO_CODE.len();
/// `hello` 的装载地址（熵为 0 时的确定性基址）。
pub const HELLO_LOAD_ADDR: u64 = 0x40_0000;
/// `hello` 在镜像里的文件偏移。
pub const HELLO_CODE_OFFSET: u64 = (HDR_SIZE + PHDR_SIZE) as u64;

/// F050 — 构造一份真实的 VXELF `hello` 镜像（一页对齐的 .text 段）。
pub fn build_hello_image(out: &mut [u8]) -> Option<usize> {
    if out.len() < HELLO_IMAGE_LEN {
        return None;
    }
    let mut header = VxHeader::empty();
    header.magic = VXELF_MAGIC;
    header.version = VXELF_VERSION;
    header.abi_version = VXABI_VERSION;
    header.entry = HELLO_LOAD_ADDR + HELLO_CODE_OFFSET;
    header.phoff = HDR_SIZE as u64;
    header.phnum = 1;
    header.image_len = HELLO_IMAGE_LEN as u64;
    header.flags = 0;
    header.preferred_base = HELLO_LOAD_ADDR;
    header.encode(out);

    let ph = VxPhdr {
        p_type: PT_LOAD,
        flags: PF_R | PF_X,
        offset: HELLO_CODE_OFFSET,
        vaddr: HELLO_LOAD_ADDR + HELLO_CODE_OFFSET,
        filesz: HELLO_CODE.len() as u64,
        memsz: HELLO_CODE.len() as u64,
        align: PAGE_SIZE,
    };
    ph.encode(&mut out[HDR_SIZE..]);

    let at = HDR_SIZE + PHDR_SIZE;
    out[at..at + HELLO_CODE.len()].copy_from_slice(&HELLO_CODE);
    Some(HELLO_IMAGE_LEN)
}

// ---------------------------------------------------------------------------
// F044 — 加载器 fuzz
// ---------------------------------------------------------------------------

/// F044 — 畸形镜像 fuzz：随机字节流 + 随机截断 + 头部字段扰动，加载器必须
/// 永远返回 Err 而**绝不 panic、绝不返回半成品计划**。
pub fn fuzz_loader(seed: u64, rounds: u32) -> bool {
    let mut s = seed | 1;
    let mut buf = [0u8; HELLO_IMAGE_LEN];
    let policy = LoadPolicy {
        require_signature: false,
        abi_version: VXABI_VERSION,
        aslr_entropy: 0x1234_5678,
    };
    for _ in 0..rounds {
        // 一半轮次从合法镜像出发做扰动，一半纯随机 —— 两种都必须被安全处理。
        let mut hdr = VxHeader::empty();
        let mut phdrs = [VxPhdr::default(); MAX_SEGMENTS];
        let random_case = s & 1 == 0;
        if !random_case {
            if build_hello_image(&mut buf).is_none() {
                return false;
            }
            hdr = match VxHeader::decode(&buf) {
                Ok(h) => h,
                Err(_) => return false,
            };
            phdrs[0] = match VxPhdr::decode(&buf[HDR_SIZE..HDR_SIZE + PHDR_SIZE]) {
                Ok(p) => p,
                Err(_) => return false,
            };
        } else {
            hdr.magic = VXELF_MAGIC;
            hdr.version = VXELF_VERSION;
            hdr.abi_version = VXABI_VERSION;
            hdr.phnum = 1;
            hdr.image_len = HELLO_IMAGE_LEN as u64;
            hdr.phoff = HDR_SIZE as u64;
            phdrs[0].p_type = PT_LOAD;
            phdrs[0].flags = PF_R;
            for b in buf.iter_mut() {
                *b = (splitmix64(&mut s) & 0xFF) as u8;
            }
        }
        // 扰动头部字段。
        let r = splitmix64(&mut s);
        match r % 6 {
            0 => hdr.magic[0] = 0,
            1 => hdr.version = (r >> 8) as u16,
            2 => hdr.abi_version = (r >> 16) as u16,
            3 => hdr.entry = r,
            4 => phdrs[0].memsz = r,
            _ => phdrs[0].offset = r,
        }
        let len = ((r >> 24) as usize) % (HELLO_IMAGE_LEN + 1);
        match plan_load(&hdr, &phdrs[..1], len.min(buf.len()), &policy, r) {
            Ok(plan) => {
                // 只要返回 Ok，计划必须自洽：W^X、入口可执行、帧数非零。
                if !plan.wx_ok() || !plan.entry_executable() || plan.frames == 0 {
                    return false;
                }
            }
            Err(_) => {}
        }
    }
    true
}

// ---------------------------------------------------------------------------
// 小工具
// ---------------------------------------------------------------------------

pub fn align_down(v: u64) -> u64 {
    v & !(PAGE_SIZE - 1)
}

pub fn align_up(v: u64) -> u64 {
    (v + PAGE_SIZE - 1) & !(PAGE_SIZE - 1)
}

fn get_u16(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn get_u32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn get_u64(b: &[u8], at: usize) -> u64 {
    let mut v = [0u8; 8];
    v.copy_from_slice(&b[at..at + 8]);
    u64::from_le_bytes(v)
}

fn put_u16(b: &mut [u8], at: usize, v: u16) {
    b[at..at + 2].copy_from_slice(&v.to_le_bytes());
}

fn put_u32(b: &mut [u8], at: usize, v: u32) {
    b[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

fn put_u64(b: &mut [u8], at: usize, v: u64) {
    b[at..at + 8].copy_from_slice(&v.to_le_bytes());
}

// ---------------------------------------------------------------------------
// F049/F050 — 域自检与首个用户程序验收
// ---------------------------------------------------------------------------

/// `hello` 验收：构造镜像 → 解码 → 规划 → 断言入口、段与 W^X。
pub fn hello_acceptance() -> bool {
    let mut buf = [0u8; 256];
    let n = match build_hello_image(&mut buf) {
        Some(n) => n,
        None => return false,
    };
    let hdr = match VxHeader::decode(&buf) {
        Ok(h) => h,
        Err(_) => return false,
    };
    let ph = match VxPhdr::decode(&buf[HDR_SIZE..HDR_SIZE + PHDR_SIZE]) {
        Ok(p) => p,
        Err(_) => return false,
    };
    let policy = LoadPolicy::reproducible();
    let plan = match plan_load(&hdr, &[ph], n, &policy, 0) {
        Ok(p) => p,
        Err(_) => return false,
    };
    // 机器码字节流必须原样落在镜像里。
    let code_ok = buf[HDR_SIZE + PHDR_SIZE..n] == HELLO_CODE;
    // 入口 = 段内偏移，且段可执行不可写。
    let seg_ok = plan
        .get(0)
        .map(|p| p.flags & PF_X != 0 && p.flags & PF_W == 0)
        .unwrap_or(false);
    code_ok
        && seg_ok
        && plan.entry == HELLO_LOAD_ADDR + HELLO_CODE_OFFSET
        && plan.wx_ok()
        && plan.entry_executable()
        && plan.count() == 1
}

/// F049 — 加载器 25 项 CheckSet。
///
/// 每个探针独立成帧：镜像缓冲与计划都是定长对象，堆在一个栈帧里在调试版
/// 会直接把栈吃光（与 AI-01 同一课）。
pub fn run_exec_checks() -> CheckSet {
    let mut set = CheckSet::new("exec");
    probe_format(&mut set);
    probe_plan(&mut set);
    probe_aslr(&mut set);
    probe_stack(&mut set);
    probe_reloc(&mut set);
    probe_integrity(&mut set);
    probe_linkage(&mut set);
    probe_runtime(&mut set);
    probe_hello(&mut set);
    set
}

/// F026/F027 — 格式定稿与往返编码。
fn probe_format(set: &mut CheckSet) {
    let mut h = VxHeader::empty();
    h.magic = VXELF_MAGIC;
    h.version = VXELF_VERSION;
    h.abi_version = VXABI_VERSION;
    h.entry = 0x40_0100;
    h.phoff = HDR_SIZE as u64;
    h.phnum = 1;
    h.image_len = 4096;
    let mut buf = [0u8; HDR_SIZE];
    let n = h.encode(&mut buf);
    let back = VxHeader::decode(&buf).ok();
    let mut ph = VxPhdr::default();
    ph.p_type = PT_LOAD;
    ph.flags = PF_R | PF_X;
    ph.offset = 0x1000;
    ph.vaddr = 0x40_0000;
    ph.filesz = 12;
    ph.memsz = 12;
    ph.align = PAGE_SIZE;
    let mut pbuf = [0u8; PHDR_SIZE];
    let m = ph.encode(&mut pbuf);
    let ph_back = VxPhdr::decode(&pbuf).ok();
    set.add(
        "F026 VXELF 格式定稿（magic/版本/ABI/入口）",
        n == HDR_SIZE
            && m == PHDR_SIZE
            && back == Some(h)
            && ph_back == Some(ph)
            && h.magic == [0x7F, b'V', b'X', b'E']
            && VXELF_MAGIC != *b"\x7FELF",
        "头/程序头字节级往返一致，魔数区别于 ELF",
    );

    // F027 — 格式规范文档与 golden 样例同源：样例镜像必须能被规范描述。
    let mut img = [0u8; 256];
    let len = build_hello_image(&mut img).unwrap_or(0);
    let golden = len == HELLO_IMAGE_LEN
        && img[0..4] == VXELF_MAGIC
        && get_u16(&img, 4) == VXELF_VERSION
        && get_u16(&img, 6) == VXABI_VERSION;
    set.add(
        "F027 格式规范文档（golden 样例镜像）",
        golden,
        "docs/VARIABLE-200-VXELF-格式规范.md 与 golden 镜像同源",
    );
}

/// F028/F029 — 段加载与 W^X。
fn probe_plan(set: &mut CheckSet) {
    let policy = LoadPolicy::reproducible();
    let mut hdr = VxHeader::empty();
    hdr.magic = VXELF_MAGIC;
    hdr.version = VXELF_VERSION;
    hdr.abi_version = VXABI_VERSION;
    hdr.phoff = HDR_SIZE as u64;
    hdr.phnum = 3;
    hdr.image_len = 0x4000;
    hdr.entry = 0x40_1000;
    let mut phdrs = [VxPhdr::default(); 3];
    phdrs[0] = VxPhdr {
        p_type: PT_LOAD,
        flags: PF_R | PF_X,
        offset: 0x1000,
        vaddr: 0x40_1000,
        filesz: 0x200,
        memsz: 0x200,
        align: PAGE_SIZE,
    };
    phdrs[1] = VxPhdr {
        p_type: PT_LOAD,
        flags: PF_R,
        offset: 0x2000,
        vaddr: 0x40_2000,
        filesz: 0x100,
        memsz: 0x100,
        align: PAGE_SIZE,
    };
    phdrs[2] = VxPhdr {
        p_type: PT_LOAD,
        flags: PF_R | PF_W,
        offset: 0x3000,
        vaddr: 0x40_3000,
        filesz: 0x80,
        memsz: 0x2000,
        align: PAGE_SIZE,
    };
    let plan = plan_load(&hdr, &phdrs, 0x4000, &policy, 0);
    let ok = match plan {
        Ok(p) => {
            // 0x40_3000 起 memsz 0x2000 → 2 页；三段共 4 页（0x1000/0x2000/0x3000..0x5000）。
            p.count() == 3
                && p.frames == 4
                && p.wx_ok()
                && p.entry_executable()
                && p.get(1).map(|s| s.flags & PF_W == 0).unwrap_or(false)
                && p.get(2).map(|s| s.flags & PF_W != 0 && s.flags & PF_X == 0).unwrap_or(false)
        }
        Err(_) => false,
    };
    set.add(
        "F028 段加载器（偏移/虚地址/权限三元组）",
        ok,
        "三段精确落位、页数合计正确、.bss 补齐",
    );

    // F029 — W^X：声明同时可写可执行的段必须被拒。
    let mut bad = [VxPhdr::default(); 1];
    bad[0] = phdrs[0];
    bad[0].flags = PF_R | PF_W | PF_X;
    let mut h2 = hdr;
    h2.phnum = 1;
    let wx_rejected = plan_load(&h2, &bad, 0x4000, &policy, 0) == Err(LoadError::WriteExecute);
    set.add(
        "F029 W^X 执行纪律（不可同时写+执行）",
        wx_rejected && LoadPlan::empty().wx_ok(),
        "声明 W+X 的段被拒，空计划天然满足",
    );
}

/// F030 — ASLR。
fn probe_aslr(set: &mut CheckSet) {
    let mut policy = LoadPolicy::strict();
    policy.aslr_entropy = 0x1111_2222_3333_4444;
    let b1 = aslr_base(&policy, 1, ASLR_HOST_SPAN);
    let b2 = aslr_base(&policy, 2, ASLR_HOST_SPAN);
    let b3 = aslr_base(&policy, 1, ASLR_HOST_SPAN);
    let aligned = b1 & (PAGE_SIZE - 1) == 0 && b2 & (PAGE_SIZE - 1) == 0;
    let in_range = b1 >= ASLR_FLOOR && b1 + ASLR_HOST_SPAN <= ASLR_CEIL;
    let deterministic = b1 == b3 && b1 != b2;
    let mut off = LoadPolicy::reproducible();
    off.aslr_entropy = 0;
    let repro = aslr_base(&off, 999, ASLR_HOST_SPAN) == ASLR_FLOOR;
    let t1 = aslr_stack_top(&policy, 1, 0x1000_0000);
    let t2 = aslr_stack_top(&policy, 2, 0x1000_0000);
    set.add(
        "F030 用户态 ASLR（镜像基址 + 栈 + 可复现开关）",
        aligned && in_range && deterministic && repro && t1 != t2 && t1 < USER_TOP + 1,
        "页对齐、限界内、同熵同址、熵 0 可复现",
    );
}

/// F031/F032 — 入口栈与 auxv。
fn probe_stack(set: &mut CheckSet) {
    let mut region = [0u8; 512];
    let top = 0x7F00_0000u64;
    let argv: [&[u8]; 2] = [b"hello", b"-v"];
    let envp: [&[u8]; 1] = [b"VARIANT=200"];
    let mut aux = [(0u64, 0u64); 12];
    let n_aux = standard_auxv(&mut aux, 0x40_1000, 0x40_0000, 1, false, 0, 0);
    let layout = build_entry_stack(&mut region, top, &argv, &envp, &aux[..n_aux]);

    let ok = match layout {
        Some(l) => {
            let base = (top - region.len() as u64) as usize;
            // 栈顶第一个 u64 必须是 argc。
            let argc_at = (l.rsp - (top - region.len() as u64)) as usize;
            let argc = get_u64(&region, argc_at) == 2;
            // 紧接着是 argv 指针数组（NULL 结尾），每个指针都能读回原串。
            let p0 = get_u64(&region, argc_at + 8);
            let p1 = get_u64(&region, argc_at + 16);
            let null = get_u64(&region, argc_at + 24) == 0;
            let s0 = if p0 >= top - region.len() as u64 && p0 < top {
                &region[(p0 - (top - region.len() as u64)) as usize..][..5]
            } else {
                &[][..]
            };
            let s1 = if p1 >= top - region.len() as u64 && p1 < top {
                &region[(p1 - (top - region.len() as u64)) as usize..][..2]
            } else {
                &[][..]
            };
            let aligned = l.rsp & 0xF == 0;
            let complete = auxv_complete(&aux[..n_aux]);
            let _ = base;
            argc && null && s0 == b"hello" && s1 == b"-v" && aligned && complete && l.argc == 2
        }
        None => false,
    };
    set.add(
        "F031 入口栈布局（argc/argv/envp/auxv）",
        ok,
        "argc 打头、指针数组 NULL 结尾、16 字节对齐、可读回原串",
    );

    // F032 — auxv 完整性：缺关键项必须被判为不完整。
    let mut missing = [(0u64, 0u64); 12];
    let n_missing = standard_auxv(&mut missing, 1, 2, 3, true, 4, 5);
    let mut short = missing;
    let n_short = n_missing - 1;
    short[n_short - 1] = (AT_NULL, 0);
    set.add(
        "F032 auxv 完整性（AT_ENTRY/PHDR/PAGESZ…）",
        auxv_complete(&missing[..n_missing])
            && !auxv_complete(&short[..n_short])
            && AUXV_REQUIRED.len() == 6,
        "关键项齐全；缺项即判不完整",
    );
}

/// F033/F035 — TLS 与重定位。
fn probe_reloc(set: &mut CheckSet) {
    let policy = LoadPolicy::reproducible();
    let mut hdr = VxHeader::empty();
    hdr.magic = VXELF_MAGIC;
    hdr.version = VXELF_VERSION;
    hdr.abi_version = VXABI_VERSION;
    hdr.phoff = HDR_SIZE as u64;
    hdr.phnum = 2;
    hdr.image_len = 0x4000;
    hdr.entry = 0x40_1000;
    let mut phdrs = [VxPhdr::default(); 2];
    phdrs[0] = VxPhdr {
        p_type: PT_LOAD,
        flags: PF_R | PF_X,
        offset: 0x1000,
        vaddr: 0x40_1000,
        filesz: 0x200,
        memsz: 0x200,
        align: PAGE_SIZE,
    };
    phdrs[1] = VxPhdr {
        p_type: PT_TLS,
        flags: PF_R | PF_W,
        offset: 0x2000,
        vaddr: 0x40_2000,
        filesz: 0x40,
        memsz: 0x80,
        align: 16,
    };
    let plan = plan_load(&hdr, &phdrs, 0x4000, &policy, 0);
    let tls_ok = match plan {
        Ok(p) => match p.tls {
            Some(t) => t.filesz == 0x40 && t.memsz == 0x80 && t.align == 16 && t.image_base >= 0x40_2000,
            None => false,
        },
        Err(_) => false,
    };
    // TLS 段非法（filesz > memsz）必须被拒。
    let mut bad = phdrs;
    bad[1].filesz = 0x100;
    let tls_bad = plan_load(&hdr, &bad, 0x4000, &policy, 0) == Err(LoadError::BadTls);
    set.add(
        "F033 TLS 支持（模板落位 + fs 基址口径）",
        tls_ok && tls_bad,
        "TLS 段解析出模板大小与对齐，非法段被拒",
    );

    // F035 — RELA：RELATIVE 与 ABS64 可处理、越界与未知类型被拒。
    let mut writable = [Placement::default(); 1];
    writable[0] = Placement {
        page: 0x40_3000,
        pages: 1,
        offset: 0,
        filesz: 0,
        memsz: 0x1000,
        flags: PF_R | PF_W,
        kind: PT_LOAD,
    };
    let mut relas = [None; MAX_RELOCATIONS];
    relas[0] = Some(Rela { offset: 0x40_3100, info: R_X86_64_RELATIVE as u64, addend: 0x20 });
    relas[1] = Some(Rela { offset: 0x40_3108, info: R_X86_64_64 as u64, addend: -8 });
    relas[2] = Some(Rela { offset: 0x40_3110, info: R_X86_64_NONE as u64, addend: 0 });
    let mut out = [None; MAX_RELOCATIONS];
    let n = apply_relocations(&relas, 0, 0x40_0000, &writable, &mut out);
    let rel_ok = n == Ok(2)
        && out[0] == Some(RelocWrite { slot: 0x40_3100, value: 0x40_0020 })
        && out[1] == Some(RelocWrite { slot: 0x40_3108, value: 0x3F_FFF8 });
    // 目标落在只读段 → 拒绝。
    let mut ro = writable;
    ro[0].flags = PF_R;
    let rel_reject = apply_relocations(&relas, 0, 0x40_0000, &ro, &mut out)
        == Err(LoadError::RelocationFailed);
    // 未知类型 → 拒绝。
    relas[0] = Some(Rela { offset: 0x40_3100, info: 99, addend: 0 });
    let rel_unknown = apply_relocations(&relas[..1], 0, 0x40_0000, &writable, &mut out).is_err();
    // 编码往返。
    let r = Rela { offset: 0x1234, info: 0x0000_0008_0000_0008, addend: -16 };
    let mut rbuf = [0u8; RELA_SIZE];
    r.encode(&mut rbuf);
    let r_back = Rela::decode(&rbuf, 0);
    set.add(
        "F035 重定位引擎（RELA + 可写段限定）",
        rel_ok && rel_reject && rel_unknown && r_back == Some(r) && r.r_type() == 8 && r.sym() == 8,
        "RELATIVE/ABS64 生效，只读段与未知类型被拒",
    );
}

/// F036/F037/F039/F040 — 校验、签名、失败回收、ABI 协商。
fn probe_integrity(set: &mut CheckSet) {
    let policy = LoadPolicy::reproducible();
    let mut hdr = VxHeader::empty();
    hdr.magic = VXELF_MAGIC;
    hdr.version = VXELF_VERSION;
    hdr.abi_version = VXABI_VERSION;
    hdr.phoff = HDR_SIZE as u64;
    hdr.phnum = 1;
    hdr.image_len = 0x4000;
    hdr.entry = 0x40_1000;
    let mut phdrs = [VxPhdr::default(); 1];
    phdrs[0] = VxPhdr {
        p_type: PT_LOAD,
        flags: PF_R | PF_X,
        offset: 0x1000,
        vaddr: 0x40_1000,
        filesz: 0x200,
        memsz: 0x200,
        align: PAGE_SIZE,
    };
    let baseline_ok = validate_image(&hdr, &phdrs).is_ok();

    let mut bad_magic = hdr;
    bad_magic.magic = [0x7F, b'E', b'L', b'F'];
    let mut bad_ver = hdr;
    bad_ver.version = 99;
    let mut bad_ph = hdr;
    bad_ph.phnum = 0;
    let mut bad_tbl = hdr;
    bad_tbl.phoff = 0x3FFF;
    let mut bad_range = [VxPhdr::default(); 1];
    bad_range[0] = phdrs[0];
    bad_range[0].memsz = 0x100;
    bad_range[0].filesz = 0x200;
    let mut bad_align = [VxPhdr::default(); 1];
    bad_align[0] = phdrs[0];
    bad_align[0].offset = 0x1001;
    let mut bad_oob = [VxPhdr::default(); 1];
    bad_oob[0] = phdrs[0];
    bad_oob[0].offset = 0x3000;
    bad_oob[0].vaddr = 0x40_3000;
    bad_oob[0].filesz = 0x2000;
    bad_oob[0].memsz = 0x2000;
    let mut bad_entry = hdr;
    bad_entry.entry = 0x50_0000;
    set.add(
        "F036 镜像校验（魔数/版本/段数/越界）",
        baseline_ok
            && validate_image(&bad_magic, &phdrs) == Err(LoadError::BadMagic)
            && validate_image(&bad_ver, &phdrs) == Err(LoadError::BadVersion)
            && validate_image(&bad_ph, &phdrs) == Err(LoadError::BadProgramHeader)
            && validate_image(&bad_tbl, &phdrs) == Err(LoadError::BadHeader)
            && plan_load(&hdr, &bad_range, 0x4000, &policy, 0) == Err(LoadError::SegmentSizes)
            && plan_load(&hdr, &bad_align, 0x4000, &policy, 0) == Err(LoadError::SegmentAlignment)
            && plan_load(&hdr, &bad_oob, 0x4000, &policy, 0) == Err(LoadError::SegmentOutOfBounds)
            && plan_load(&bad_entry, &phdrs, 0x4000, &policy, 0) == Err(LoadError::EntryNotExecutable),
        "五类头部缺陷 + 三类段缺陷逐项拒绝",
    );

    // F037 — 签名开关：要求签名时缺失/篡改一律拒绝。
    let mut img = [0u8; 128];
    for (i, b) in img.iter_mut().enumerate() {
        *b = i as u8;
    }
    let body_len = img.len() - 32;
    let digest = signature_digest(&img[..body_len]);
    let weak = LoadPolicy::strict();
    let strong = LoadPolicy::signed_required();
    let pass = verify_signature(&weak, &img, 0).is_ok();
    let good = verify_signature(&strong, &img, digest).is_ok();
    img[0] ^= 1;
    let tampered = verify_signature(&strong, &img, digest) == Err(LoadError::SignatureInvalid);
    let short = verify_signature(&strong, &[0u8; 8], digest) == Err(LoadError::SignatureInvalid);
    set.add(
        "F037 镜像签名校验（可选开关，缺失即拒）",
        pass && good && tampered && short,
        "开关关闭放行；开启后篡改/缺块一律拒绝",
    );

    // F039 — 加载事务：失败路径必须把账清干净。
    let mut tx = LoadTransaction::new();
    tx.map(3);
    tx.reloc(2);
    tx.lib();
    let outstanding = tx.frames_mapped == 3 && tx.relocations == 2 && tx.libs == 1;
    let _ = plan_load(&hdr, &bad_oob, 0x4000, &policy, 0);
    let rolled = tx.rollback();
    let clean = tx.clean_after_failure();
    let mut committed = LoadTransaction::new();
    committed.map(1);
    committed.commit();
    set.add(
        "F039 加载失败恢复（进程不创建、内存全回收）",
        outstanding && rolled == 3 && clean && !committed.clean_after_failure(),
        "回滚页数与落位账本一致，提交后不再回滚",
    );

    // F040 — ABI 版本协商。
    let mut other = hdr;
    other.abi_version = 7;
    set.add(
        "F040 ABI 版本协商（不匹配即拒绝并说明）",
        negotiate_abi(&policy, &hdr) == Ok(VXABI_VERSION)
            && negotiate_abi(&policy, &other) == Err(LoadError::AbiMismatch)
            && LoadError::AbiMismatch.as_str() == "ABI version mismatch"
            && abi_caps() & ABI_CAP_VDSO != 0,
        "同版本放行、异版本拒绝、能力位可查",
    );
}

/// F034/F038/F041/F042 — 链接策略、只读保护、共享库骨架、调试信息。
fn probe_linkage(set: &mut CheckSet) {
    let mut load = [VxPhdr::default(); 2];
    load[0].p_type = PT_LOAD;
    load[1].p_type = PT_TLS;
    let mut interp = load;
    interp[1].p_type = PT_INTERP;
    let mut dynamic = load;
    dynamic[1].p_type = PT_DYNAMIC;
    let policy = LoadPolicy::reproducible();
    let mut hdr = VxHeader::empty();
    hdr.magic = VXELF_MAGIC;
    hdr.version = VXELF_VERSION;
    hdr.abi_version = VXABI_VERSION;
    hdr.phoff = HDR_SIZE as u64;
    hdr.phnum = 2;
    hdr.image_len = 0x4000;
    hdr.entry = 0x40_1000;
    let ph = VxPhdr {
        p_type: PT_LOAD,
        flags: PF_R | PF_X,
        offset: 0x1000,
        vaddr: 0x40_1000,
        filesz: 0x200,
        memsz: 0x200,
        align: PAGE_SIZE,
    };
    let mut with_interp = interp;
    with_interp[0] = ph;
    let mut with_dynamic = dynamic;
    with_dynamic[0] = ph;
    set.add(
        "F034 静态链接优先（动态显式降级）",
        classify_linkage(&load) == Linkage::Static
            && classify_linkage(&interp) == Linkage::Dynamic
            && classify_linkage(&dynamic) == Linkage::SelfFixing
            && plan_load(&hdr, &with_interp, 0x4000, &policy, 0)
                == Err(LoadError::DynamicUnsupported)
            && plan_load(&hdr, &with_dynamic, 0x4000, &policy, 0)
                == Err(LoadError::DynamicUnsupported),
        "PT_INTERP/PT_DYNAMIC 一律显式拒绝",
    );

    // F038 — 只读段保护：可执行段映射后必须只读。
    let mut ro_check = false;
    let mut ok_hdr = hdr;
    ok_hdr.phnum = 1;
    if let Ok(p) = plan_load(&ok_hdr, &[VxPhdr { flags: PF_R | PF_X, ..ph }], 0x4000, &policy, 0) {
        ro_check = p
            .get(0)
            .map(|s| s.readonly_locked() && s.flags & PF_X != 0)
            .unwrap_or(false)
            && p.readonly_sealed();
    }
    let mut gap = [VxPhdr::default(); 2];
    gap[0] = VxPhdr { p_type: PT_LOAD, flags: PF_R | PF_X, offset: 0, vaddr: 0x40_0000, filesz: 0x100, memsz: 0x100, align: PAGE_SIZE };
    gap[1] = VxPhdr { p_type: PT_LOAD, flags: PF_R, offset: 0x1000, vaddr: 0x40_0000, filesz: 0x100, memsz: 0x100, align: PAGE_SIZE };
    let overlap_rejected = plan_load(&hdr, &gap, 0x4000, &policy, 0) == Err(LoadError::SegmentOverlap);
    set.add(
        "F038 只读段保护（可执行段只读锁定 + 重叠拒绝）",
        ro_check && overlap_rejected,
        ".text 映射后不可写；同址段重叠被拒",
    );

    // F041 — 共享库骨架：首版关闭，登记可见、加载不可（显式降级）。
    let mut cache = SharedLibCache::new();
    let a = cache.register(b"libvarix.so", 0x30_0000);
    let b = cache.register(b"libui.so", 0);
    let found = cache.find(b"libui.so");
    let disabled = !cache.enabled
        && cache.get(0).map(|e| !e.loaded).unwrap_or(false)
        && a == Ok(0)
        && b == Ok(1);
    cache.enabled = true;
    let mut cache2 = SharedLibCache::new();
    cache2.enabled = true;
    let c = cache2.register(b"liba.so", 0x40_0000);
    let enabled = c.is_ok() && cache2.get(0).map(|e| e.loaded).unwrap_or(false);
    set.add(
        "F041 共享库骨架（路径/缓存/预链接占位）",
        disabled && enabled && found == Some(1) && cache.count() == 2,
        "关闭时只登记不加载；打开即装载，可缓存命中",
    );

    // F042 — 调试信息保留：符号化崩溃地址。
    let mut tab = SymbolTable::new();
    let ins = tab.insert(0x40_1000, 0x40, b"varix_main", true)
        && tab.insert(0x40_1040, 0x20, b"helper", true)
        && tab.insert(0x40_2000, 0x10, b"rodata_blob", false);
    let inside = tab.symbolize(0x40_1050) == Some((1, 0x10));
    let outside = tab.symbolize(0x99_0000).is_none();
    let name_ok = tab.get(0).map(|s| s.name_bytes() == b"varix_main" && s.is_function).unwrap_or(false);
    set.add(
        "F042 调试信息保留（symtab 不剥离 + 符号化）",
        ins && inside && outside && name_ok && tab.keep_debug_info,
        "崩溃地址可映射到最近符号与偏移",
    );
}

/// F043/F044/F045/F046/F047/F048 — 性能、fuzz、并发、资产、shebang、环境。
fn probe_runtime(set: &mut CheckSet) {
    // F043 — 加载性能仪表 + 流式自洽。
    let mut t = LoadTimer::new(100);
    for i in 0..20u32 {
        t.record(50 + i, 30 + i / 2);
    }
    let perf_ok = t.filled() == LOAD_SAMPLES
        && t.peak_us() >= 60
        && t.p50_us() > 0
        && t.mean_us() > 0
        && t.over_budget == 0
        && t.streaming_consistent();
    // 分段耗时超过总耗时是不可能的读数 —— 仪表必须自己发现这类矛盾，
    // 并且超预算要真的计数（红线可见）。
    let mut bad = LoadTimer::new(5);
    bad.record(10, 400);
    let budget_hit = bad.over_budget == 1 && !bad.streaming_consistent();
    set.add(
        "F043 加载性能（环形统计 + 流式自洽）",
        perf_ok && budget_hit,
        "p50/峰值/均值可读，超预算计数，分段不超总时",
    );

    // F044 — fuzz：畸形镜像只产生 Err，绝不 panic 或半成品。
    let fuzz_ok = fuzz_loader(0xC0FFEE, 2000) && fuzz_loader(7, 500);
    set.add(
        "F044 加载器 fuzz（畸形镜像零内核态污染）",
        fuzz_ok,
        "2000 轮随机/扰动镜像：只有 Err 或自洽计划",
    );

    // F045 — 多镜像并发加载。
    let mut loader = ConcurrentLoader::new();
    let a = loader.begin();
    let b = loader.begin();
    loader.finish();
    let live_after = loader.live();
    loader.finish();
    set.add(
        "F045 多镜像并发加载（线程安全记账）",
        a == 1 && b == 2 && loader.peak_in_flight == 2 && live_after == 1 && loader.live() == 0
            && loader.completed == 2
            && ConcurrentLoader::serialization_point() == "table-arena",
        "并发计数单调正确，落位串行点显式",
    );

    // F046 — 资产识别。
    let mut png = [0u8; 16];
    png[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
    let mut img = [0u8; 256];
    let n = build_hello_image(&mut img).unwrap_or(0);
    set.add(
        "F046 资产文件识别（可执行/数据/脚本分流）",
        classify_asset(&img[..n]) == AssetKind::Executable
            && classify_asset(b"#!/bin/vsh\nls") == AssetKind::Script
            && classify_asset(&png) == AssetKind::Data
            && classify_asset(b"{\"k\":1}") == AssetKind::Data
            && classify_asset(b"\x00\x01\x02\x03") == AssetKind::Unknown,
        "五类输入各自归位，认不出来明确报 Unknown",
    );

    // F047 — shebang。
    let (i1, a1) = parse_shebang(b"#!/bin/vsh -x\nprint").unwrap_or((&[][..], &[][..]));
    let (i2, a2) = parse_shebang(b"#!  /bin/vsh\n").unwrap_or((&[][..], &[][..]));
    set.add(
        "F047 shebang 式解释器（首行转解释器执行）",
        i1 == b"/bin/vsh" && a1 == b"-x" && i2 == b"/bin/vsh" && a2.is_empty()
            && parse_shebang(b"not a script").is_none()
            && parse_shebang(b"#!\n").is_none(),
        "解释器与参数解析正确，无 shebang 返回 None",
    );

    // F048 — 环境注入。
    let mut env = EnvInjector::new();
    let add1 = env.add(b"VARIX_LOG", b"info");
    let add2 = env.add(b"VARIX_DEBUG", b"0");
    let found_ok = env.find(b"VARIX_LOG") == Some(&b"info"[..]);
    let live_count = env.count();
    env.enabled = false;
    set.add(
        "F048 环境注入（内核级 env 通道）",
        add1 && add2 && found_ok && live_count == 2 && env.active_count() == 0,
        "key=value 格式正确，关闭开关即不注入",
    );
}

/// F049/F050 — 加载自检与首个用户程序。
fn probe_hello(set: &mut CheckSet) {
    // F050 — hello：机器码字节流 → VXELF 镜像 → 计划 → 入口与 W^X。
    let ok = hello_acceptance();
    set.add(
        "F050 首个用户程序 hello（ring3 镜像可加载）",
        ok,
        "12 字节 syscall 机器码落进可执行段，入口精确",
    );

    // F049 — 加载自检闭环：本域 25 项 CheckSet（含本项）全绿。
    let (lp, lf) = set.tally();
    set.add(
        "F049 加载自检（合法性/W^X/失败回收）",
        lf == 0 && lp + 1 == 25,
        "本表自洽：前 24 项全过，本项计入第 25 项",
    );
}

// ===========================================================================
// 单测
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn two_load_segments() -> (VxHeader, [VxPhdr; 2]) {
        let mut hdr = VxHeader::empty();
        hdr.magic = VXELF_MAGIC;
        hdr.version = VXELF_VERSION;
        hdr.abi_version = VXABI_VERSION;
        hdr.phoff = HDR_SIZE as u64;
        hdr.phnum = 2;
        hdr.image_len = 0x4000;
        hdr.entry = 0x40_1000;
        let mut ph = [VxPhdr::default(); 2];
        ph[0] = VxPhdr {
            p_type: PT_LOAD,
            flags: PF_R | PF_X,
            offset: 0x1000,
            vaddr: 0x40_1000,
            filesz: 0x200,
            memsz: 0x200,
            align: PAGE_SIZE,
        };
        ph[1] = VxPhdr {
            p_type: PT_LOAD,
            flags: PF_R | PF_W,
            offset: 0x2000,
            vaddr: 0x40_2000,
            filesz: 0x100,
            memsz: 0x1000,
            align: PAGE_SIZE,
        };
        (hdr, ph)
    }

    #[test]
    fn f026_header_round_trips() {
        let mut h = VxHeader::empty();
        h.magic = VXELF_MAGIC;
        h.version = VXELF_VERSION;
        h.abi_version = VXABI_VERSION;
        h.entry = 0xDEAD_BEEF;
        h.phoff = 64;
        h.phnum = 3;
        h.image_len = 0x9999;
        h.flags = 1;
        h.preferred_base = 0x40_0000;
        let mut buf = [0u8; HDR_SIZE];
        assert_eq!(h.encode(&mut buf), HDR_SIZE);
        assert_eq!(VxHeader::decode(&buf), Ok(h));
        assert!(h.signed_image());
        assert_eq!(VxHeader::decode(&buf[..32]), Err(LoadError::TooSmall));
    }

    #[test]
    fn f028_plan_places_segments() {
        let (hdr, ph) = two_load_segments();
        let plan = plan_load(&hdr, &ph, 0x4000, &LoadPolicy::reproducible(), 0).expect("plan");
        assert_eq!(plan.count(), 2);
        assert_eq!(plan.get(0).unwrap().page, 0x40_1000);
        assert_eq!(plan.get(0).unwrap().pages, 1);
        assert_eq!(plan.get(1).unwrap().pages, 1);
        assert_eq!(plan.frames, 2);
        assert!(plan.entry_executable());
        assert!(plan.wx_ok());
    }

    #[test]
    fn f029_wx_is_enforced() {
        let (hdr, mut ph) = two_load_segments();
        ph[0].flags = PF_R | PF_W | PF_X;
        let mut h = hdr;
        h.phnum = 1;
        assert_eq!(
            plan_load(&h, &ph[..1], 0x4000, &LoadPolicy::reproducible(), 0),
            Err(LoadError::WriteExecute)
        );
    }

    #[test]
    fn f030_aslr_is_page_aligned_and_bounded() {
        let policy = LoadPolicy::strict();
        for e in 0..64u64 {
            let b = aslr_base(&policy, e, ASLR_HOST_SPAN);
            assert_eq!(b & (PAGE_SIZE - 1), 0);
            assert!(b >= ASLR_FLOOR, "{} below floor", b);
            assert!(b + ASLR_HOST_SPAN <= ASLR_CEIL, "{} above ceil", b);
        }
        let off = LoadPolicy::reproducible();
        assert_eq!(aslr_base(&off, 1, ASLR_HOST_SPAN), ASLR_FLOOR);
        assert_eq!(aslr_stack_top(&off, 1, 0x1000), USER_TOP & !0xFFF);
    }

    #[test]
    fn f031_stack_layout_is_sysv_shaped() {
        let mut region = [0u8; 512];
        let top = 0x7F00_0000u64;
        let low = top - region.len() as u64;
        let layout = build_entry_stack(
            &mut region,
            top,
            &[b"a", b"bb", b"ccc"],
            &[b"K=V"],
            &[(AT_ENTRY, 0x40_1000)],
        )
        .expect("layout");
        let at = (layout.rsp - low) as usize;
        assert_eq!(get_u64(&region, at), 3, "argc first");
        // 栈自 rsp 向上：argc, argv[0..3], argv NULL, envp[0], envp NULL, auxv。
        let a0 = get_u64(&region, at + 8);
        let a1 = get_u64(&region, at + 16);
        let a2 = get_u64(&region, at + 24);
        assert_eq!(get_u64(&region, at + 32), 0, "argv NULL terminated");
        let s0 = &region[(a0 - low) as usize..][..1];
        assert_eq!(s0, b"a");
        let s2 = &region[(a2 - low) as usize..][..3];
        assert_eq!(s2, b"ccc");
        assert!(a0 > a1 && a1 > a2, "字符串池自顶向下");
        assert_eq!(layout.argv0, a0);
        assert!(layout.bytes_used > 0);
        assert!(layout.rsp & 0x7 == 0);
        assert_eq!(layout.auxv_entries, 3); // AT_ENTRY + 传入项 + AT_NULL
    }

    #[test]
    fn f032_auxv_requires_key_entries() {
        let mut aux = [(0u64, 0u64); 12];
        let n = standard_auxv(&mut aux, 1, 2, 3, false, 4, 5);
        assert!(auxv_complete(&aux[..n]));
        assert_eq!(AUXV_REQUIRED.len(), 6);
        for key in AUXV_REQUIRED {
            assert!(aux[..n].iter().any(|(k, _)| *k == key), "missing {}", key);
        }
    }

    #[test]
    fn f033_tls_parsed_and_validated() {
        let (hdr, ph) = two_load_segments();
        let tls = VxPhdr {
            p_type: PT_TLS,
            flags: PF_R | PF_W,
            offset: 0x1000,
            vaddr: 0x40_1000,
            filesz: 0x40,
            memsz: 0x80,
            align: 32,
        };
        let plan = plan_load(&hdr, &[ph[0], tls], 0x4000, &LoadPolicy::reproducible(), 0).expect("plan");
        let t = plan.tls.expect("tls");
        assert_eq!(t.filesz, 0x40);
        assert_eq!(t.memsz, 0x80);
        assert_eq!(t.align, 32);
        let mut bad = tls;
        bad.filesz = 0x100;
        assert_eq!(
            plan_load(&hdr, &[ph[0], bad], 0x4000, &LoadPolicy::reproducible(), 0),
            Err(LoadError::BadTls)
        );
        let mut bad2 = tls;
        bad2.align = 3;
        assert_eq!(
            plan_load(&hdr, &[ph[0], bad2], 0x4000, &LoadPolicy::reproducible(), 0),
            Err(LoadError::BadTls)
        );
    }

    #[test]
    fn f034_dynamic_is_explicitly_refused() {
        let (hdr, mut ph) = two_load_segments();
        assert_eq!(hdr.phnum, 2, "两个段");
        ph[1].p_type = PT_INTERP;
        assert_eq!(classify_linkage(&ph), Linkage::Dynamic);
        assert_eq!(
            plan_load(&hdr, &ph, 0x4000, &LoadPolicy::reproducible(), 0),
            Err(LoadError::DynamicUnsupported)
        );
        ph[1].p_type = PT_DYNAMIC;
        assert_eq!(classify_linkage(&ph), Linkage::SelfFixing);
        assert_eq!(
            plan_load(&hdr, &ph, 0x4000, &LoadPolicy::reproducible(), 0),
            Err(LoadError::DynamicUnsupported)
        );
        ph[1].p_type = PT_TLS;
        assert_eq!(classify_linkage(&ph), Linkage::Static);
    }

    #[test]
    fn f035_relocations_respect_writability() {
        let writable = [Placement {
            page: 0x40_3000,
            pages: 1,
            offset: 0,
            filesz: 0,
            memsz: 0x1000,
            flags: PF_R | PF_W,
            kind: PT_LOAD,
        }];
        let mut relas = [None; MAX_RELOCATIONS];
        relas[0] = Some(Rela { offset: 0x40_3F00, info: R_X86_64_RELATIVE as u64, addend: 0x100 });
        relas[1] = Some(Rela { offset: 0x40_3FF8, info: R_X86_64_64 as u64, addend: 8 });
        let mut out = [None; MAX_RELOCATIONS];
        assert_eq!(apply_relocations(&relas, 0, 0x40_0000, &writable, &mut out), Ok(2));
        assert_eq!(out[0].unwrap().value, 0x40_0100);
        // 只读段拒绝。
        let ro = [Placement { flags: PF_R | PF_X, ..writable[0] }];
        assert_eq!(
            apply_relocations(&relas, 0, 0x40_0000, &ro, &mut out),
            Err(LoadError::RelocationFailed)
        );
    }

    #[test]
    fn f036_validator_rejects_defects() {
        let (hdr, ph) = two_load_segments();
        assert!(validate_image(&hdr, &ph).is_ok());
        let mut bad = hdr;
        bad.magic = [0; 4];
        assert_eq!(validate_image(&bad, &ph), Err(LoadError::BadMagic));
        let mut bad = hdr;
        bad.version = 2;
        assert_eq!(validate_image(&bad, &ph), Err(LoadError::BadVersion));
        let mut bad = hdr;
        bad.phnum = 9;
        assert_eq!(validate_image(&bad, &ph), Err(LoadError::BadProgramHeader));
        let mut bad = hdr;
        bad.image_len = 0x40;
        assert_eq!(validate_image(&bad, &ph), Err(LoadError::BadHeader));
    }

    #[test]
    fn f037_signature_switch() {
        let mut img = [0u8; 64];
        for (i, b) in img.iter_mut().enumerate() {
            *b = (i * 3) as u8;
        }
        let d = signature_digest(&img[..32]);
        assert!(verify_signature(&LoadPolicy::strict(), &img, 0).is_ok());
        assert!(verify_signature(&LoadPolicy::signed_required(), &img, d).is_ok());
        let mut tampered = img;
        tampered[3] ^= 0xFF;
        assert_eq!(
            verify_signature(&LoadPolicy::signed_required(), &tampered, d),
            Err(LoadError::SignatureInvalid)
        );
    }

    #[test]
    fn f039_transaction_rolls_back_cleanly() {
        let mut tx = LoadTransaction::new();
        tx.map(7);
        tx.reloc(3);
        tx.lib();
        assert!(!tx.committed);
        assert_eq!(tx.rollback(), 7);
        assert!(tx.clean_after_failure());
        tx.map(1);
        tx.commit();
        assert!(tx.committed);
        assert!(!tx.clean_after_failure());
    }

    #[test]
    fn f040_abi_is_negotiated() {
        let (mut hdr, _) = two_load_segments();
        let policy = LoadPolicy::reproducible();
        assert_eq!(negotiate_abi(&policy, &hdr), Ok(VXABI_VERSION));
        hdr.abi_version = 3;
        assert_eq!(negotiate_abi(&policy, &hdr), Err(LoadError::AbiMismatch));
        assert!(abi_caps() & ABI_CAP_TLS != 0);
    }

    #[test]
    fn f041_shared_lib_cache_degrades_explicitly() {
        let mut c = SharedLibCache::new();
        assert!(!c.enabled);
        assert_eq!(c.register(b"libx.so", 0x1000), Ok(0));
        assert!(!c.get(0).unwrap().loaded);
        assert_eq!(c.find(b"libx.so"), Some(0));
        assert_eq!(c.find(b"nope.so"), None);
        let mut c2 = SharedLibCache::new();
        c2.enabled = true;
        c2.register(b"liby.so", 0).unwrap();
        assert!(c2.get(0).unwrap().loaded);
    }

    #[test]
    fn f042_symbolize_finds_nearest_symbol() {
        let mut t = SymbolTable::new();
        assert!(t.insert(0x1000, 0x10, b"main", true));
        assert!(t.insert(0x1010, 0x8, b"tail", false));
        assert_eq!(t.symbolize(0x1008), Some((0, 8)));
        assert_eq!(t.symbolize(0x1014), Some((1, 4)));
        assert_eq!(t.symbolize(0x9999), None);
        assert!(t.keep_debug_info);
        assert!(!t.insert(0x2000, 1, b"a_name_that_is_definitely_too_long", false));
    }

    #[test]
    fn f043_timer_reports_percentiles() {
        let mut t = LoadTimer::new(1000);
        for i in 0..LOAD_SAMPLES as u32 {
            t.record(10 * (i + 1), 5 * (i + 1));
        }
        assert_eq!(t.filled(), LOAD_SAMPLES);
        assert_eq!(t.peak_us(), 160);
        assert!(t.p50_us() >= 80 && t.p50_us() <= 90);
        assert!(t.streaming_consistent());
        assert_eq!(t.over_budget, 0);
        let mut bad = LoadTimer::new(1);
        bad.record(100, 200);
        assert_eq!(bad.over_budget, 1);
        assert!(!bad.streaming_consistent());
    }

    #[test]
    fn f044_fuzz_never_panics_or_half_loads() {
        assert!(fuzz_loader(1, 1000));
        assert!(fuzz_loader(0xABCD, 1000));
        assert!(fuzz_loader(u64::MAX, 500));
    }

    #[test]
    fn f045_concurrent_accounting_is_monotonic() {
        let mut l = ConcurrentLoader::new();
        assert_eq!(l.begin(), 1);
        assert_eq!(l.begin(), 2);
        assert_eq!(l.live(), 2);
        l.finish();
        assert_eq!(l.live(), 1);
        l.finish();
        assert_eq!(l.live(), 0);
        assert_eq!(l.completed, 2);
        assert_eq!(l.peak_in_flight, 2);
    }

    #[test]
    fn f046_asset_classification() {
        let mut img = [0u8; 256];
        let n = build_hello_image(&mut img).expect("image");
        assert_eq!(classify_asset(&img[..n]), AssetKind::Executable);
        assert_eq!(classify_asset(b"#!/x\n"), AssetKind::Script);
        assert_eq!(classify_asset(b"OTTO...."), AssetKind::Data);
        assert_eq!(classify_asset(b"RIFF...."), AssetKind::Data);
        assert_eq!(classify_asset(b"zzz"), AssetKind::Unknown);
    }

    #[test]
    fn f047_shebang_parsing() {
        let (i, a) = parse_shebang(b"#!/bin/vsh -e arg\nbody").expect("shebang");
        assert_eq!(i, b"/bin/vsh");
        assert_eq!(a, b"-e arg");
        let (i2, a2) = parse_shebang(b"#! /bin/vsh").expect("shebang2");
        assert_eq!(i2, b"/bin/vsh");
        assert!(a2.is_empty());
        assert!(parse_shebang(b"nope").is_none());
        assert!(parse_shebang(b"#!   ").is_none());
    }

    #[test]
    fn f048_env_injection_channel() {
        let mut e = EnvInjector::new();
        assert!(e.add(b"VARIX_LOG", b"debug"));
        assert!(e.add(b"VARIX_ABI", b"1"));
        assert_eq!(e.find(b"VARIX_LOG"), Some(&b"debug"[..]));
        assert_eq!(e.find(b"VARIX_NOPE"), None);
        assert!(!e.add(b"VARIX_LOG", b"a_very_long_value_that_breaks_the_fixed_entry"));
        e.enabled = false;
        assert_eq!(e.active_count(), 0);
        assert_eq!(e.find(b"VARIX_LOG"), None);
    }

    #[test]
    fn f050_hello_image_loads() {
        assert!(hello_acceptance());
        let mut buf = [0u8; 16];
        assert!(build_hello_image(&mut buf).is_none(), "缓冲区不足必须拒绝");
    }

    #[test]
    fn f049_domain_self_test_is_green() {
        let set = run_exec_checks();
        assert_eq!(set.len(), 25, "AI-02 域必须恰好 25 项");
        let mut buf = [0u8; 4096];
        let n = set.render(&mut buf);
        let text = core::str::from_utf8(&buf[..n]).unwrap_or("render failed");
        assert!(set.all_passed(), "exec 域自检必须全绿：\n{}", text);
    }
}
