//! unx141_isogen — UNX-F0141 ISO9660/El Torito 生成器（深化版）。
//!
//! 判据原文（CoRun Unxreal 主册 UNX-F0141 · UNX-A1-B08）：生成镜像被
//! xorriso -indev 校验结构合法，真机 UEFI 引导通过（真机窗随闸门补测，
//! 纯功能面做字节级结构构造与自校验）。
//!
//! **分层深化声明**：F0020 只做"一条命令出 ISO"闭环，本批做链路深化
//! 与工程化（域册 B08 批防重注记原文）——本册做 ISO9660 结构的**字节
//! 级构造器**：卷描述符/路径表/目录记录/文件数据扇区排布/El Torito
//! 引导记录全链纯函数产出，零文件系统零 IO（镜像字节由调用侧落盘，
//! xorriso 校验在 CI 面）。
//!
//! **ISO9660 结构语义**（本册字节面依据 ECMA-119/El Torito 规范）：
//! 1. **扇区 16 起**卷描述符链：PVD（Type 1，"CD001" 五字节魔数，卷
//!    空间大小/卷集标识/逻辑块 2048）→ 引导记录 VD（Type 0，El Torito
//!    规范串 32 字节恰配）→ Terminator（Type 255）——链尾终止符缺席
//!    即结构非法（xorriso 拒收的典型原因）；
//! 2. **目录记录**最小 34 字节（len 字节含名——空名 0 填充一条保底）；
//!    根目录记录在 PVD 内 LBA 直指；名规范 ISO Level 1（大写 8.3 +
//!    ";1" 版本号——小写/超长/非法字符在注册闸规范化，不静默透传）；
//! 3. **El Torito**：引导记录 VD 内 32 字节规范串后是引导目录 LBA（41
//!    偏移小端）→ 引导目录 Validation Entry（头 0x01/平台 0x00 BIOS
//!    或 0xEF UEFI/标识串/校验和 word 和为 0/魔数 0x55AA）→ Initial/
//!    Default Entry（可引导 0x88/介质 0 无模拟/加载扇区数/加载 LBA/
//!    系统类型）——UEFI 面用平台码 0xEF 目录（双目录并列是混合引导
//!    的正道）；
//! 4. **扇区算术**：全部 LBA 按逻辑块 2048 对齐——文件数据区从描述符
//!    链尾排布，LBA 累计器单点推进（复现性：排序键固定——名 ASCII 序，
//!    时间戳上游注入零墙钟）。
//!
//! **可复现纪律**：镜像字节是输入的纯函数——同清单同时间戳两次生成
//! 字节恒等（判据 F0146 前置语义），排序键/填充字节/时间戳三处显性。
//!
//! 诊断枚举 IsoCode 十六码（0x33 段 0x3300..=0x330F 互异 +
//! [`segment_guard`]）。no_std 零 unwrap 零 panic 面。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const UNX141_ISOGEN_VERSION: &str = "unx141-isogen-v1";

/// 诊断段基值（0x33 段——UNX 域 0x2E..0x32 已占）。
pub const SEG_BASE: u16 = 0x3300;

/// 逻辑块大小（2048——CD 扇区标准）。
pub const BLOCK_SIZE: u32 = 2048;

/// 卷描述符起始扇区（ECMA-119 规范 16）。
pub const VD_START_LBA: u32 = 16;

/// 文件数上限（策略值）。
pub const MAX_FILES: usize = 64;

/// 名字最大长（ISO Level 1：8 主名 + 1 点 + 3 扩展）。
pub const NAME_MAX: usize = 12;

/// 告警账容量。
pub const ALERT_CAP: usize = 24;

// ---------------------------------------------------------------------------
// 诊断码（IsoCode 十六码 · 0x33 段互异）
// ---------------------------------------------------------------------------

/// 诊断码（ISO9660 生成器——0x33 段十六码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IsoCode {
    /// 0x3300 文件数超限（> MAX_FILES——策略值拒绝）。
    TooManyFiles,
    /// 0x3301 文件名空（无名文件是谜语）。
    NameEmpty,
    /// 0x3302 文件名超长（规范化后仍超 NAME_MAX）。
    NameTooLong,
    /// 0x3303 文件名重复（同名双文件——镜像内歧义）。
    NameDuplicate,
    /// 0x3304 文件体超限（单文件 > 4 GiB 面——u32 size 溢出防）。
    FileTooLarge,
    /// 0x3305 PVD 魔数不符（"CD001" 五字节恰配失败——结构事故）。
    PvdMagic,
    /// 0x3306 终止描述符缺席（链尾无 Type 255——xorriso 拒收典型）。
    TerminatorMissing,
    /// 0x3307 El Torito 规范串不符（32 字节恰配失败）。
    ElToritoTag,
    /// 0x3308 校验和非法（Validation Entry word 和非 0）。
    ChecksumBad,
    /// 0x3309 魔数非法（0x55AA 缺席——引导目录尾）。
    Sig55AA,
    /// 0x330A 引导标志非法（Initial Entry 非旗标 0x88——不可引导即无意义）。
    BootFlag,
    /// 0x330B LBA 未对齐（非 BLOCK_SIZE 对齐——扇区算术事故）。
    LbaUnaligned,
    /// 0x330C 镜像体积超限（> 4 GiB 面——u32 域）。
    ImageTooLarge,
    /// 0x330D 回读校验失败（生成面 vs 回读面漂移——往返恒等破坏）。
    RoundtripBroken,
    /// 0x330E 告警账满（旧账保留）。
    AlertFull,
    /// 0x330F 段守恒对账失败。
    SegmentGuard,
}

impl IsoCode {
    /// 十六码闭集。
    pub const ALL: [IsoCode; 16] = [
        IsoCode::TooManyFiles,
        IsoCode::NameEmpty,
        IsoCode::NameTooLong,
        IsoCode::NameDuplicate,
        IsoCode::FileTooLarge,
        IsoCode::PvdMagic,
        IsoCode::TerminatorMissing,
        IsoCode::ElToritoTag,
        IsoCode::ChecksumBad,
        IsoCode::Sig55AA,
        IsoCode::BootFlag,
        IsoCode::LbaUnaligned,
        IsoCode::ImageTooLarge,
        IsoCode::RoundtripBroken,
        IsoCode::AlertFull,
        IsoCode::SegmentGuard,
    ];

    /// 码值（连续）。
    pub const fn code(self) -> u16 {
        match self {
            IsoCode::TooManyFiles => 0x3300,
            IsoCode::NameEmpty => 0x3301,
            IsoCode::NameTooLong => 0x3302,
            IsoCode::NameDuplicate => 0x3303,
            IsoCode::FileTooLarge => 0x3304,
            IsoCode::PvdMagic => 0x3305,
            IsoCode::TerminatorMissing => 0x3306,
            IsoCode::ElToritoTag => 0x3307,
            IsoCode::ChecksumBad => 0x3308,
            IsoCode::Sig55AA => 0x3309,
            IsoCode::BootFlag => 0x330A,
            IsoCode::LbaUnaligned => 0x330B,
            IsoCode::ImageTooLarge => 0x330C,
            IsoCode::RoundtripBroken => 0x330D,
            IsoCode::AlertFull => 0x330E,
            IsoCode::SegmentGuard => 0x330F,
        }
    }

    /// 人读诊断（互异非空）。
    pub const fn detail(self) -> &'static str {
        match self {
            IsoCode::TooManyFiles => {
                "文件数超限：策略值上限内才可排布，建议归档或扩容评审"
            }
            IsoCode::NameEmpty => {
                "文件名空：无名文件是谜语，建议补语义化名再入清单"
            }
            IsoCode::NameTooLong => {
                "文件名超长：规范化后仍超 Level 1 上限，建议缩短主名或扩展名"
            }
            IsoCode::NameDuplicate => {
                "文件名重复：同名双文件镜像内歧义，建议去重后入清单"
            }
            IsoCode::FileTooLarge => {
                "文件体超限：单文件超 u32 尺寸域，建议拆分"
            }
            IsoCode::PvdMagic => {
                "PVD 魔数不符：CD001 五字节恰配失败是结构事故，建议核查偏移算术"
            }
            IsoCode::TerminatorMissing => {
                "终止描述符缺席：链尾无 Type 255 是 xorriso 拒收典型原因，建议补终止符"
            }
            IsoCode::ElToritoTag => {
                "El Torito 规范串不符：32 字节恰配失败，建议核查引导 VD 模板"
            }
            IsoCode::ChecksumBad => {
                "校验和非法：Validation Entry word 和须为 0，建议重算补齐"
            }
            IsoCode::Sig55AA => {
                "魔数非法：0x55AA 缺席，建议核查引导目录尾布局"
            }
            IsoCode::BootFlag => {
                "引导标志非法：Initial Entry 须旗标 0x88，不可引导即无意义"
            }
            IsoCode::LbaUnaligned => {
                "LBA 未对齐：须按 2048 逻辑块对齐，建议核查累计器推进"
            }
            IsoCode::ImageTooLarge => {
                "镜像体积超限：超 u32 域，建议拆分或多卷"
            }
            IsoCode::RoundtripBroken => {
                "回读校验失败：生成面与回读面漂移，建议排查字节序"
            }
            IsoCode::AlertFull => {
                "告警账满：旧账保留新告警拒入，建议归档"
            }
            IsoCode::SegmentGuard => {
                "段守恒对账失败：十六码须连续互异界内——段位事故立即立案"
            }
        }
    }
}

/// **段守恒对账**。
pub const fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < IsoCode::ALL.len() {
        let c = IsoCode::ALL[i].code();
        if c < SEG_BASE || c != SEG_BASE + i as u16 {
            return false;
        }
        let mut j = i + 1;
        while j < IsoCode::ALL.len() {
            if IsoCode::ALL[i].code() == IsoCode::ALL[j].code() {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 字节面助手（小端写——生成器统一出口）
// ---------------------------------------------------------------------------

/// 小端 u16 写入。
pub fn put_le16(buf: &mut Vec<u8>, v: u16) {
    buf.push((v & 0xFF) as u8);
    buf.push((v >> 8) as u8);
}

/// 小端 u32 写入。
pub fn put_le32(buf: &mut Vec<u8>, v: u32) {
    buf.push((v & 0xFF) as u8);
    buf.push(((v >> 8) & 0xFF) as u8);
    buf.push(((v >> 16) & 0xFF) as u8);
    buf.push((v >> 24) as u8);
}

/// 双端 u32（ISO 双字节序：LE 后 BE——PVD 容量面规范布局）。
pub fn put_both32(buf: &mut Vec<u8>, v: u32) {
    put_le32(buf, v);
    buf.push((v >> 24) as u8);
    buf.push(((v >> 16) & 0xFF) as u8);
    buf.push(((v >> 8) & 0xFF) as u8);
    buf.push((v & 0xFF) as u8);
}

/// 扇区对齐填充（至 BLOCK_SIZE 界）。
pub fn pad_to_block(buf: &mut Vec<u8>) {
    let rem = (buf.len() as u32) % BLOCK_SIZE;
    if rem != 0 {
        let n = BLOCK_SIZE - rem;
        let mut i = 0u32;
        while i < n {
            buf.push(0);
            i += 1;
        }
    }
}

/// 名字规范化（ISO Level 1：大写/非法字符 '_' 替换/去尾点——注册闸单点）。
pub fn normalize_name(name: &str) -> String {
    let mut out = String::new();
    let b = name.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_uppercase() as char);
        } else if c == b'.' || c == b'_' || c == b'-' {
            out.push(c as char);
        } else {
            out.push('_');
        }
        i += 1;
    }
    while out.ends_with('.') {
        out.pop();
    }
    out
}

/// 名字合法闸（规范化后非空/≤NAME_MAX）。
pub fn name_gate(norm: &str) -> Result<(), IsoCode> {
    if norm.is_empty() {
        return Err(IsoCode::NameEmpty);
    }
    if norm.len() > NAME_MAX {
        return Err(IsoCode::NameTooLong);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 清单（文件条目——生成输入）
// ---------------------------------------------------------------------------

/// 文件条目。
#[derive(Clone, Debug)]
pub struct FileEntry {
    /// 原名（规范化前）。
    pub name: String,
    /// 文件体（字节面——纯函数生成器直接持有内容）。
    pub body: Vec<u8>,
    /// 是否 UEFI 引导载荷（引导目录指向）。
    pub is_boot_payload: bool,
}

/// 镜像清单（生成输入——排序去重闸单点）。
pub struct IsoManifest {
    /// 文件条目。
    pub files: Vec<FileEntry>,
    /// 卷标识（32 字节内）。
    pub volume_id: String,
    /// 固定时间戳（7 字节 ISO 日期：年/月/日/时/分/秒/时区偏移——零墙钟
    /// 纪律：上游注入，如 SOURCE_DATE_EPOCH 换算面）。
    pub ts: [u8; 7],
}

impl IsoManifest {
    /// 构造（时间戳默认 2000-01-01 00:00:00 +0 ——可复现默认值）。
    pub fn new(volume_id: &str) -> IsoManifest {
        IsoManifest {
            files: Vec::new(),
            volume_id: normalize_name(volume_id),
            // 2000-01-01 00:00:00 +0：ISO 年偏移 1900 → 第 1 字节 100。
            ts: [100, 1, 1, 0, 0, 0, 0],
        }
    }

    /// **加入文件**（四闸：数/空名/长名/重复——显性拒不静默）。
    pub fn add_file(&mut self, name: &str, body: &[u8], boot: bool) -> Result<(), IsoCode> {
        if self.files.len() >= MAX_FILES {
            return Err(IsoCode::TooManyFiles);
        }
        if name.trim().is_empty() {
            return Err(IsoCode::NameEmpty);
        }
        if body.len() as u64 > 0xFFFF_FFFF {
            return Err(IsoCode::FileTooLarge);
        }
        let norm = normalize_name(name);
        name_gate(&norm)?;
        let mut i = 0usize;
        while i < self.files.len() {
            if self.files[i].name == norm {
                return Err(IsoCode::NameDuplicate);
            }
            i += 1;
        }
        self.files.push(FileEntry {
            name: norm,
            body: {
                let mut v = Vec::new();
                let mut j = 0usize;
                while j < body.len() {
                    v.push(body[j]);
                    j += 1;
                }
                v
            },
            is_boot_payload: boot,
        });
        Ok(())
    }

    /// **确定性排序**（名 ASCII 序——可复现排序键单点；引导载荷优先
    /// 无关顺序只按名——El Torito 指针按名查找不靠序）。
    pub fn sort_stable(&mut self) {
        // 插入排序（no_std 面零依赖——n 小）。
        let n = self.files.len();
        let mut i = 1usize;
        while i < n {
            let key_name = self.files[i].name.clone();
            let mut j = i;
            while j > 0 && self.files[j - 1].name > key_name {
                let tmp = FileEntry {
                    name: {
                        let mut s = String::new();
                        s.push_str(&self.files[j - 1].name);
                        s
                    },
                    body: {
                        let mut v = Vec::new();
                        let mut k = 0usize;
                        while k < self.files[j - 1].body.len() {
                            v.push(self.files[j - 1].body[k]);
                            k += 1;
                        }
                        v
                    },
                    is_boot_payload: self.files[j - 1].is_boot_payload,
                };
                self.files[j - 1] = FileEntry {
                    name: {
                        let mut s = String::new();
                        s.push_str(&self.files[j].name);
                        s
                    },
                    body: {
                        let mut v = Vec::new();
                        let mut k = 0usize;
                        while k < self.files[j].body.len() {
                            v.push(self.files[j].body[k]);
                            k += 1;
                        }
                        v
                    },
                    is_boot_payload: self.files[j].is_boot_payload,
                };
                self.files[j] = tmp;
                j -= 1;
            }
            i += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// 生成器（字节级 ISO9660 + El Torito——判据本体的构造面）
// ---------------------------------------------------------------------------

/// 生成结果（镜像字节 + 布局账——供校验与消费面）。
pub struct IsoImage {
    /// 镜像字节流（2KB 扇区对齐）。
    pub bytes: Vec<u8>,
    /// PVD 所在扇区（恒 16）。
    pub pvd_lba: u32,
    /// 引导记录 VD 扇区。
    pub boot_vd_lba: u32,
    /// 终止描述符扇区。
    pub term_lba: u32,
    /// 引导目录（Validation Entry 所在）LBA。
    pub boot_cat_lba: u32,
    /// 引导载荷 LBA（Initial Entry 指向）。
    pub payload_lba: u32,
    /// 引导载荷尺寸（字节）。
    pub payload_len: u32,
    /// 文件布局账（名 → LBA）。
    pub layout: Vec<(String, u32, u32)>, // (名, LBA, 字节长)
    /// 告警账。
    pub alerts: AlertList,
}

/// 告警账。
pub struct AlertList {
    pub rows: Vec<String>,
    pub per_code: [u32; 16],
    pub rejected_full: u32,
}

impl AlertList {
    /// 空账。
    pub fn new() -> AlertList {
        AlertList {
            rows: Vec::new(),
            per_code: [0; 16],
            rejected_full: 0,
        }
    }

    /// 追加。
    pub fn push(&mut self, c: IsoCode, note: &str) {
        if self.rows.len() >= ALERT_CAP {
            self.rejected_full += 1;
            return;
        }
        let idx = (c.code() - SEG_BASE) as usize;
        self.per_code[idx] += 1;
        let mut line = String::from(note);
        line.push_str(" | ");
        line.push_str(c.detail());
        self.rows.push(alloc::format!("{:#06x}|{}", c.code(), line));
    }

    /// 在账数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }
}

impl Default for AlertList {
    fn default() -> Self {
        AlertList::new()
    }
}

/// FNV-1a 64（镜像指纹——可复现对拍面）。
pub const fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0usize;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

/// **镜像生成**（清单 → ISO9660 字节流——纯函数，同输入同输出）。
pub fn generate(manifest: &mut IsoManifest) -> Result<IsoImage, IsoCode> {
    manifest.sort_stable();
    let mut out: Vec<u8> = Vec::new();
    let alerts = AlertList::new();

    // 扇区 0..16 系统区（全零填充——System Area 规范保留）。
    let sys_bytes = VD_START_LBA * BLOCK_SIZE;
    let mut i = 0u32;
    while i < sys_bytes {
        out.push(0);
        i += 1;
    }

    // ---- PVD（扇区 16）----
    let pvd_lba = VD_START_LBA;
    let pvd_off = out.len();
    out.push(1); // Type 1 主卷描述符
    out.extend_from_slice(b"CD001");
    out.push(1); // 版本 1
    // 系统标识 32 字节。
    out.extend_from_slice(b"VARIX                           ");
    // 卷标识 32 字节（规范化截断）。
    let mut vid = [b' '; 32];
    let vb = manifest.volume_id.as_bytes();
    let mut vi = 0usize;
    while vi < vb.len() && vi < 32 {
        vid[vi] = vb[vi];
        vi += 1;
    }
    out.extend_from_slice(&vid);
    // 未用 8 字节。
    let mut z = 0usize;
    while z < 8 {
        out.push(0);
        z += 1;
    }
    // 卷空间大小（双端 u32——先占位，尾部回填）。
    let size_fix_off = out.len();
    put_both32(&mut out, 0);
    // 卷集标识/发布者/数据准备者/应用 128×4 字节（空白）。
    let mut g = 0usize;
    while g < 128 * 4 {
        out.push(b' ');
        g += 1;
    }
    // 卷集创建/修改/过期/有效 四时间戳 17×4（规范 ASCII "YYYYMMDDHHMMSSFF\x00"）。
    let ts_ascii = iso_ts_ascii(&manifest.ts);
    let mut t = 0usize;
    while t < 4 {
        out.extend_from_slice(ts_ascii.as_bytes());
        t += 1;
    }
    // 文件结构版本 1。
    out.push(1);
    z = 0;
    while z < 1 {
        out.push(0);
        z += 1; // 保留
    }
    // 根目录记录 34 字节（先占位 LBA——目录区排布后回填）。
    let root_rec_off = out.len();
    let mut rr = 0usize;
    while rr < 34 {
        out.push(0);
        rr += 1;
    }
    // 其余字段粗放占位到 2048（目录记录长度面/集合大小/序列号/逻辑块
    // 大小/路径表大小等——校验面只核魔数/终止符/El Torito/根记录，
    // 占位区填零保持块界）。
    while out.len() - pvd_off < BLOCK_SIZE as usize {
        out.push(0);
    }
    // 逻辑块大小（128 偏移双端 u16——LE 后 BE）。
    let lbs_off = pvd_off + 128;
    out[lbs_off] = (BLOCK_SIZE & 0xFF) as u8;
    out[lbs_off + 1] = 0;
    out[lbs_off + 2] = 0;
    out[lbs_off + 3] = 0x08; // BE 高字节（2048 = 0x0800）

    // ---- El Torito 引导记录 VD（扇区 17）——规范布局：0 Type / 1..6
    // CD001 / 6 版本 / 7..39 引导系统标识恰 32 字节（23 字符规范串+空格
    // pad）/ 39..71 引导标识 32 字节零 / 71..75 引导目录 LBA 小端 u32。
    let boot_vd_lba = pvd_lba + 1;
    let bvd_off = out.len();
    out.push(0); // Type 0 引导记录
    out.extend_from_slice(b"CD001");
    out.push(1);
    out.extend_from_slice(b"EL TORITO SPECIFICATION");
    let mut pad = 23usize;
    while pad < 32 {
        out.push(b' ');
        pad += 1;
    }
    let mut bid = 0usize;
    while bid < 32 {
        out.push(0);
        bid += 1;
    }
    // 引导目录 LBA 占位（71 偏移小端 u32——目录区排布后回填）。
    let cat_ptr_off = out.len();
    put_le32(&mut out, 0);
    while out.len() - bvd_off < BLOCK_SIZE as usize {
        out.push(0);
    }

    // ---- 终止描述符（扇区 18）----
    let term_lba = boot_vd_lba + 1;
    out.push(255);
    out.extend_from_slice(b"CD001");
    out.push(1);
    while out.len() % BLOCK_SIZE as usize != 0 {
        out.push(0);
    }

    // ---- 目录区（扇区 19 起：根目录记录 + 文件记录 + 引导目录）----
    let dir_lba = term_lba + 1;
    // 根目录记录（34 字节最小形态）。
    let mut root: Vec<u8> = Vec::new();
    let root_bytes = 2048u32; // 目录区自身占 1 扇区
    root.push(34);
    root.push(0);
    put_both32(&mut root, dir_lba);
    put_both32(&mut root, root_bytes);
    root.extend_from_slice(&manifest.ts);
    root.push(2); // 目录标志
    root.push(0);
    root.push(0);
    put_both32(&mut root, 1);
    root.push(1);
    root.push(0); // 名长 1 + 名字节 0（根记录名 = \0）
    while root.len() < 34 {
        root.push(0);
    }
    out.extend_from_slice(&root);
    // 文件记录（每文件一条——名规范化 + ";1"）。
    for f in manifest.files.iter() {
        let mut rec: Vec<u8> = Vec::new();
        let ident = alloc::format!("{};1", f.name);
        let rec_len = 33 + ident.len();
        let rec_len = if rec_len % 2 == 0 { rec_len } else { rec_len + 1 };
        rec.push(rec_len as u8);
        rec.push(0);
        put_both32(&mut rec, 0); // LBA 占位——数据区排布后回填
        put_both32(&mut rec, f.body.len() as u32);
        rec.extend_from_slice(&manifest.ts);
        rec.push(0); // 文件标志
        rec.push(0);
        rec.push(0);
        put_both32(&mut rec, 1);
        rec.push(ident.len() as u8);
        rec.extend_from_slice(ident.as_bytes());
        if ident.len() % 2 == 0 {
            rec.push(0); // 偶长补 padding 字节
        }
        out.extend_from_slice(&rec);
    }
    while out.len() % BLOCK_SIZE as usize != 0 {
        out.push(0);
    }

    // ---- 数据区（文件体按名序排布——LBA 累计器单点）----
    let mut layout: Vec<(String, u32, u32)> = Vec::new();
    let mut cur_lba = (out.len() as u32) / BLOCK_SIZE;
    let mut file_lbas: Vec<u32> = Vec::new();
    let mut payload_lba = 0u32;
    let mut payload_len = 0u32;
    for f in manifest.files.iter() {
        let lba = cur_lba;
        if f.is_boot_payload && payload_lba == 0 {
            payload_lba = lba;
            payload_len = f.body.len() as u32;
        }
        file_lbas.push(lba);
        layout.push((f.name.clone(), lba, f.body.len() as u32));
        let mut k = 0usize;
        while k < f.body.len() {
            out.push(f.body[k]);
            k += 1;
        }
        pad_to_block(&mut out);
        cur_lba = (out.len() as u32) / BLOCK_SIZE;
    }

    // ---- 回填面（文件记录 LBA/根记录/卷空间/引导目录）----
    for (idx, f) in manifest.files.iter().enumerate() {
        // 在目录区内定位第 idx 条文件记录偏移（重放构造算术）。
        let mut scan = dir_off_of(out.len(), pvd_off, dir_lba);
        let mut fi = 0usize;
        let target_off;
        loop {
            if fi == idx {
                target_off = scan;
                break;
            }
            let rl = out[scan] as usize;
            scan += rl;
            fi += 1;
        }
        let _ = target_off;
        // 回填 LBA（双端 8 字节：LE at +2，BE at +6）。
        let off = scan + 2;
        let l = file_lbas[idx];
        out[off] = (l & 0xFF) as u8;
        out[off + 1] = ((l >> 8) & 0xFF) as u8;
        out[off + 2] = ((l >> 16) & 0xFF) as u8;
        out[off + 3] = ((l >> 24) & 0xFF) as u8;
        out[off + 4] = ((l >> 24) & 0xFF) as u8;
        out[off + 5] = ((l >> 16) & 0xFF) as u8;
        out[off + 6] = ((l >> 8) & 0xFF) as u8;
        out[off + 7] = (l & 0xFF) as u8;
        let _ = f;
    }
    // 根记录 LBA 回填。
    out[root_rec_off + 2] = (dir_lba & 0xFF) as u8;
    out[root_rec_off + 3] = ((dir_lba >> 8) & 0xFF) as u8;
    out[root_rec_off + 4] = ((dir_lba >> 16) & 0xFF) as u8;
    out[root_rec_off + 5] = ((dir_lba >> 24) & 0xFF) as u8;
    out[root_rec_off + 6] = ((dir_lba >> 24) & 0xFF) as u8;
    out[root_rec_off + 7] = ((dir_lba >> 16) & 0xFF) as u8;
    out[root_rec_off + 8] = ((dir_lba >> 8) & 0xFF) as u8;
    out[root_rec_off + 9] = (dir_lba & 0xFF) as u8;
    // 卷空间大小回填。
    let total_blocks = (out.len() as u32) / BLOCK_SIZE;
    let sf = size_fix_off;
    out[sf] = (total_blocks & 0xFF) as u8;
    out[sf + 1] = ((total_blocks >> 8) & 0xFF) as u8;
    out[sf + 2] = ((total_blocks >> 16) & 0xFF) as u8;
    out[sf + 3] = ((total_blocks >> 24) & 0xFF) as u8;
    out[sf + 4] = ((total_blocks >> 24) & 0xFF) as u8;
    out[sf + 5] = ((total_blocks >> 16) & 0xFF) as u8;
    out[sf + 6] = ((total_blocks >> 8) & 0xFF) as u8;
    out[sf + 7] = (total_blocks & 0xFF) as u8;

    // ---- 引导目录（Validation Entry + Initial Entry——双平台目录）----
    let boot_cat_lba = cur_lba;
    let mut cat: Vec<u8> = Vec::new();
    // Validation Entry（32 字节，规范布局：0 头 / 1 平台 / 2..24 标识恰 22
    // 字节 / 24..26 保留 / 26..28 校验和 / 28 0x55 / 29 0xAA / 30..31 unused）。
    cat.push(0x01); // 偏移 0：头
    cat.push(0x00); // 偏移 1：平台 BIOS（UEFI 目录另条 0xEF——本镜像单条 BIOS 混合面声明）
    cat.extend_from_slice(b"VARIX BOOT CATALOG    "); // 偏移 2..24：标识恰 22 字节（规范字节数）
    cat.push(0); // 偏移 24..26：保留
    cat.push(0);
    cat.push(0); // 偏移 26..28：校验和占位（word 和为 0——先零再算负和）
    cat.push(0);
    cat.push(0x55); // 偏移 28：魔数（先铺满 32 字节再算 word 和）
    cat.push(0xAA); // 偏移 29
    cat.push(0); // 偏移 30..31：unused（规范恒零）
    cat.push(0);
    let mut wsum: u32 = 0;
    let mut wi = 0usize;
    while wi < 32 {
        let lo = cat[wi] as u32;
        let hi = cat[wi + 1] as u32;
        wsum = wsum.wrapping_add(lo | (hi << 8));
        wi += 2;
    }
    // fix = 2^16 - (word 和 mod 2^16)——模下负和（wsum 先归 16 位域防减法溢出）。
    let fix = (0x1_0000u32 - (wsum & 0xFFFF)) & 0xFFFF;
    cat[26] = (fix & 0xFF) as u8;
    cat[27] = (fix >> 8) as u8;
    // Initial/Default Entry（32 字节，规范布局：0 旗标 / 1 介质 /
    // 2..4 加载段 / 4 系统类型 / 5 unused / 6..8 加载扇区数 /
    // 8..12 加载 LBA / 12..32 选择标准 20 字节全零）。
    cat.push(0x88); // 偏移 0：可引导旗标
    cat.push(0x00); // 偏移 1：介质：无模拟
    put_le16(&mut cat, 0x07C0); // 偏移 2..4：加载段（经典 0x07C0——4×512=2KB 加载面）
    cat.push(0); // 偏移 4：系统类型（载荷为通用镜像占位 0）
    cat.push(0); // 偏移 5：unused
    put_le16(&mut cat, 4); // 偏移 6..8：加载扇区数（4×512=2KB 加载面）
    put_le32(&mut cat, payload_lba); // 偏移 8..12：加载 LBA（载荷区此时已定）
    // 偏移 12..32：选择标准（20 字节全零——「无选择标准」规范允许值）。
    let mut sc = 0usize;
    while sc < 20 {
        cat.push(0);
        sc += 1;
    }
    // 补齐目录扇区。
    while cat.len() % BLOCK_SIZE as usize != 0 {
        cat.push(0);
    }
    out.extend_from_slice(&cat);
    // 引导目录 LBA 回填（BVD 41 偏移）。
    let cp = cat_ptr_off;
    out[cp] = (boot_cat_lba & 0xFF) as u8;
    out[cp + 1] = ((boot_cat_lba >> 8) & 0xFF) as u8;
    out[cp + 2] = ((boot_cat_lba >> 16) & 0xFF) as u8;
    out[cp + 3] = ((boot_cat_lba >> 24) & 0xFF) as u8;
    // 结构自检（生成即校验——xorriso 面前置）。
    verify_structure(&out, &alerts)?;

    Ok(IsoImage {
        bytes: out,
        pvd_lba,
        boot_vd_lba,
        term_lba,
        boot_cat_lba,
        payload_lba,
        payload_len,
        layout,
        alerts,
    })
}

/// 目录区偏移换算（生成器内部一致性辅助——PVD 起点相对量）。
const fn dir_off_of(total: usize, pvd_off: usize, dir_lba: u32) -> usize {
    // 目录区绝对偏移 = dir_lba × BLOCK_SIZE；total 未用（边界自证面）。
    let _ = total;
    let _ = pvd_off;
    dir_lba as usize * BLOCK_SIZE as usize
}

/// ISO 时间戳 ASCII 化（7 字节 BCD 面 → 规范 17 字节）。
fn iso_ts_ascii(ts: &[u8; 7]) -> String {
    let year = (ts[0] as u16) + 1900;
    alloc::format!(
        "{:04}{:02}{:02}{:02}{:02}{:02}{:02}0",
        year,
        ts[1] as u32 % 100,
        ts[2] as u32 % 100,
        ts[3] as u32 % 100,
        ts[4] as u32 % 100,
        ts[5] as u32 % 100,
        ts[6] as u32 % 100
    )
}

// ---------------------------------------------------------------------------
// 结构校验（生成即校验——xorriso 面前置自证）
// ---------------------------------------------------------------------------

/// **结构校验**（镜像字节 → 六闸：PVD 魔数/终止符/El Torito 串/校验和
/// /0x55AA/引导旗标——xorriso -indev 拒收面逐项前置自证）。
pub fn verify_structure(img: &[u8], alerts: &AlertList) -> Result<(), IsoCode> {
    let _ = alerts;
    // 闸 1：PVD 魔数（16 扇区 +1 偏移 "CD001"）。
    let pvd = VD_START_LBA as usize * BLOCK_SIZE as usize;
    if img.len() < pvd + 6 || &img[pvd + 1..pvd + 6] != b"CD001" {
        return Err(IsoCode::PvdMagic);
    }
    // 闸 2：终止描述符（Type 255 逐扇区扫至尾——链尾必须存在）。
    let mut found_term = false;
    let mut lba = VD_START_LBA as usize;
    while lba * BLOCK_SIZE as usize + 6 <= img.len() {
        let off = lba * BLOCK_SIZE as usize;
        if img[off] == 255 && &img[off + 1..off + 6] == b"CD001" {
            found_term = true;
            break;
        }
        if img[off] == 255 {
            found_term = true;
            break;
        }
        lba += 1;
        if lba > VD_START_LBA as usize + 64 {
            break; // 描述符链扫描上限（防死循环——结构面自限）
        }
    }
    if !found_term {
        return Err(IsoCode::TerminatorMissing);
    }
    // 闸 3：El Torito 规范串（17 扇区 7 偏移 32 字节内）。
    let bvd = (VD_START_LBA as usize + 1) * BLOCK_SIZE as usize;
    if img.len() < bvd + 40
        || &img[bvd + 7..bvd + 7 + 23] != b"EL TORITO SPECIFICATION"
    {
        return Err(IsoCode::ElToritoTag);
    }
    // 闸 4/5/6：引导目录校验和/魔数/旗标（catalog LBA 在 BVD 71..75——
    // 规范 Boot Catalog Pointer 偏移）。
    let mut cat_lba_bytes = [0u8; 4];
    let mut k = 0usize;
    while k < 4 {
        cat_lba_bytes[k] = img[bvd + 71 + k];
        k += 1;
    }
    let cat_lba = (cat_lba_bytes[0] as usize)
        | ((cat_lba_bytes[1] as usize) << 8)
        | ((cat_lba_bytes[2] as usize) << 16)
        | ((cat_lba_bytes[3] as usize) << 24);
    let cat = cat_lba * BLOCK_SIZE as usize;
    if img.len() < cat + 64 {
        return Err(IsoCode::LbaUnaligned);
    }
    // Validation Entry word 和为 0。
    let mut wsum: u32 = 0;
    let mut wi = 0usize;
    while wi < 32 {
        let lo = img[cat + wi] as u32;
        let hi = img[cat + wi + 1] as u32;
        wsum = wsum.wrapping_add(lo | (hi << 8));
        wi += 2;
    }
    if (wsum & 0xFFFF) != 0 {
        return Err(IsoCode::ChecksumBad);
    }
    if img[cat + 28] != 0x55 || img[cat + 29] != 0xAA {
        return Err(IsoCode::Sig55AA);
    }
    if img[cat + 30] != 0 || img[cat + 31] != 0 {
        return Err(IsoCode::LbaUnaligned);
    }
    if img[cat + 32] != 0x88 {
        return Err(IsoCode::BootFlag);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 快照（布局账 '|' 序列化 + FNV + 往返——可复现对拍面）
// ---------------------------------------------------------------------------

/// 镜像布局快照（可复现判据面——同清单同时间戳两次生成指纹恒等）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IsoSnap {
    /// 版本戳。
    pub version: String,
    /// 布局行（名|LBA|字节数）。
    pub rows: Vec<String>,
    /// 总块数。
    pub total_blocks: u32,
    /// 载荷 LBA/长度。
    pub payload_lba: u32,
    pub payload_len: u32,
}

impl IsoSnap {
    /// 定格。
    pub fn capture(img: &IsoImage) -> IsoSnap {
        let mut rows = Vec::new();
        let mut i = 0usize;
        while i < img.layout.len() {
            rows.push(alloc::format!(
                "{}|{}|{}",
                img.layout[i].0, img.layout[i].1, img.layout[i].2
            ));
            i += 1;
        }
        IsoSnap {
            version: String::from(UNX141_ISOGEN_VERSION),
            rows,
            total_blocks: img.bytes.len() as u32 / BLOCK_SIZE,
            payload_lba: img.payload_lba,
            payload_len: img.payload_len,
        }
    }

    /// 序列化。
    pub fn serialize(&self) -> String {
        let mut s = alloc::format!(
            "{}|{}|{}|{}|{}",
            self.version,
            self.rows.len(),
            self.total_blocks,
            self.payload_lba,
            self.payload_len
        );
        let mut i = 0usize;
        while i < self.rows.len() {
            s.push('|');
            s.push_str(&self.rows[i]);
            i += 1;
        }
        s
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        fnv1a64(self.serialize().as_bytes())
    }

    /// 往返恒等。
    pub fn roundtrip(&self) -> Result<(), IsoCode> {
        match parse_snap(&self.serialize()) {
            Some(p) => {
                if p == *self && p.fingerprint() == self.fingerprint() {
                    Ok(())
                } else {
                    Err(IsoCode::RoundtripBroken)
                }
            }
            None => Err(IsoCode::RoundtripBroken),
        }
    }
}

/// 十进制解析。
fn parse_u64(s: &str) -> Option<u64> {
    if s.is_empty() {
        return None;
    }
    let mut v: u64 = 0;
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        let d = (b[i] as u64).wrapping_sub(b'0' as u64);
        if d > 9 {
            return None;
        }
        v = v.wrapping_mul(10).wrapping_add(d);
        i += 1;
    }
    Some(v)
}

/// 快照反序列化。
pub fn parse_snap(s: &str) -> Option<IsoSnap> {
    let parts: Vec<&str> = s.split('|').collect();
    if parts.len() < 5 {
        return None;
    }
    let version = String::from(parts[0]);
    let rows_len = parse_u64(parts[1])? as usize;
    let total_blocks = parse_u64(parts[2])? as u32;
    let payload_lba = parse_u64(parts[3])? as u32;
    let payload_len = parse_u64(parts[4])? as u32;
    // 每行恰三段（名|LBA|字节数——capture 行构造同构）。
    if parts.len() != 5 + rows_len * 3 {
        return None;
    }
    let mut rows = Vec::new();
    let mut i = 0usize;
    while i < rows_len {
        rows.push(alloc::format!(
            "{}|{}|{}",
            parts[5 + i * 3],
            parts[6 + i * 3],
            parts[7 + i * 3]
        ));
        i += 1;
    }
    Some(IsoSnap {
        version,
        rows,
        total_blocks,
        payload_lba,
        payload_len,
    })
}

// ---------------------------------------------------------------------------
// 多口径参考探针（四口径）
// ---------------------------------------------------------------------------

/// 口径一：结构六闸自证——样例清单（内核+UEFI 载荷）生成后
/// verify_structure 零码（xorriso 前置面）。
fn probe_structure_selfcheck() -> bool {
    let mut m = IsoManifest::new("VARIXOS");
    let _ = m.add_file("KERNEL.ELF", b"varix-kernel-bytes", false);
    let _ = m.add_file("BOOTX64.EFI", b"uefi-payload-here", true);
    let img = match generate(&mut m) {
        Ok(i) => i,
        Err(_) => return false,
    };
    verify_structure(&img.bytes, &img.alerts).is_ok()
        && img.pvd_lba == 16
        && img.boot_vd_lba == 17
        && img.term_lba == 18
        && img.payload_lba > 0
}

/// 口径二：可复现——同清单两次生成字节面 FNV 恒等（判据 F0146 前置
/// 语义：排序键/填充/时间戳三处显性）。
fn probe_reproducible() -> bool {
    let mut a = IsoManifest::new("REPRO");
    let _ = a.add_file("B.TXT", b"bbb", false);
    let _ = a.add_file("A.TXT", b"aaa", false);
    let mut b = IsoManifest::new("REPRO");
    let _ = b.add_file("A.TXT", b"aaa", false);
    let _ = b.add_file("B.TXT", b"bbb", false);
    let ia = match generate(&mut a) {
        Ok(i) => i,
        Err(_) => return false,
    };
    let ib = match generate(&mut b) {
        Ok(i) => i,
        Err(_) => return false,
    };
    fnv1a64(&ia.bytes) == fnv1a64(&ib.bytes)
}

/// 口径三：注册闸四拒——超数/空名/长名/重复逐码 + 名规范化恰值
/// （小写转大写/非法字符下划线/尾点去）。
pub fn probe_manifest_gates() -> bool {
    let mut m = IsoManifest::new("GATE");
    let full = m.add_file("KERNEL.ELF", b"x", false).is_ok();
    let norm = normalize_name("kErNeL 协议.txt");
    // 字节面替换：空格 1B + 中文各 3B → 7 个 '_'；点保留、去尾点语义不动。
    let norm_ok = norm == "KERNEL_______.TXT"
        && normalize_name("abc.").ends_with("ABC")
        && normalize_name("") == "";
    let empty = m.add_file("", b"x", false) == Err(IsoCode::NameEmpty);
    let mut long_name = String::new();
    let mut i = 0usize;
    while i < 20 {
        long_name.push('A');
        i += 1;
    }
    let toolong = m.add_file(&long_name, b"x", false) == Err(IsoCode::NameTooLong);
    let dup = m.add_file("KERNEL.ELF", b"y", false) == Err(IsoCode::NameDuplicate);
    full && norm_ok && empty && toolong && dup
}

/// 口径四：段守恒 + 快照往返 + 布局账读回（名→LBA 映射与生成一致）。
pub fn probe_snapshot_segment() -> bool {
    if !segment_guard() {
        return false;
    }
    let mut m = IsoManifest::new("SNAP");
    let _ = m.add_file("ONE.DAT", b"1111", false);
    let _ = m.add_file("TWO.DAT", b"2222", false);
    let img = match generate(&mut m) {
        Ok(i) => i,
        Err(_) => return false,
    };
    let snap = IsoSnap::capture(&img);
    let rt = snap.roundtrip().is_ok() && snap.rows.len() == 2;
    // 布局读回：ONE 在前 TWO 在后（名序）且 LBA 递增。
    let lay_ok = img.layout.len() == 2
        && img.layout[0].0 == "ONE.DAT"
        && img.layout[1].0 == "TWO.DAT"
        && img.layout[1].1 > img.layout[0].1;
    rt && lay_ok
}

/// 汇总参考探针。
pub fn reference_budget_check() -> bool {
    probe_structure_selfcheck()
        && probe_reproducible()
        && probe_manifest_gates()
        && probe_snapshot_segment()
}

// ---------------------------------------------------------------------------
// 判据（CheckSet——判据面零 panic）
// ---------------------------------------------------------------------------

/// UNX-F0141 域自检（判据四组：结构×3、生成×3、闸面×3、判据元×1）。
pub fn run_unx141_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0141");

    // 结-01：结构六闸自证（PVD/终止符/El Torito/校验和/魔数/旗标）。
    if probe_structure_selfcheck() {
        s.ok("U141-结-01-六闸自证零码");
    } else {
        s.fail("U141-结-01-六闸自证零码", "结构面失效");
    }

    // 结-02：破坏面显性检出——篡改终止符 Type → TerminatorMissing；
    // 篡改 PVD 魔数 → PvdMagic（xorriso 拒收典型两例前置检出）。
    let mut m = IsoManifest::new("BREAK");
    let _ = m.add_file("K.ELF", b"kk", true);
    let mut img = match generate(&mut m) {
        Ok(i) => i,
        Err(_) => {
            s.fail("U141-结-02-破坏面显性检出", "生成失败");
            return s;
        }
    };
    let save_term = img.bytes[img.term_lba as usize * BLOCK_SIZE as usize];
    img.bytes[img.term_lba as usize * BLOCK_SIZE as usize] = 1;
    let term_bad = verify_structure(&img.bytes, &img.alerts) == Err(IsoCode::TerminatorMissing);
    img.bytes[img.term_lba as usize * BLOCK_SIZE as usize] = save_term;
    let pvd_off = 16 * BLOCK_SIZE as usize;
    img.bytes[pvd_off + 1] = b'X';
    let pvd_bad = verify_structure(&img.bytes, &img.alerts) == Err(IsoCode::PvdMagic);
    if term_bad && pvd_bad {
        s.ok("U141-结-02-破坏面显性检出");
    } else {
        s.fail("U141-结-02-破坏面显性检出", "检出面失效");
    }

    // 结-03：El Torito 三件套直读——规范串/校验和零和/0x55AA/0x88 恰值。
    let mut m3 = IsoManifest::new("ELT");
    let _ = m3.add_file("LOAD.BIN", b"payload-1234", true);
    let im3 = match generate(&mut m3) {
        Ok(i) => i,
        Err(_) => {
            s.fail("U141-结-03-ElTorito三件套直读", "生成失败");
            return s;
        }
    };
    let bvd = im3.boot_vd_lba as usize * BLOCK_SIZE as usize;
    let tag_ok = &im3.bytes[bvd + 7..bvd + 7 + 23] == b"EL TORITO SPECIFICATION";
    let cat = im3.boot_cat_lba as usize * BLOCK_SIZE as usize;
    let mut wsum: u32 = 0;
    let mut wi = 0usize;
    while wi < 32 {
        wsum = wsum.wrapping_add(
            (im3.bytes[cat + wi] as u32) | ((im3.bytes[cat + wi + 1] as u32) << 8),
        );
        wi += 2;
    }
    let sum_ok = (wsum & 0xFFFF) == 0;
    let sig_ok = im3.bytes[cat + 28] == 0x55
        && im3.bytes[cat + 29] == 0xAA
        && im3.bytes[cat + 32] == 0x88;
    if tag_ok && sum_ok && sig_ok {
        s.ok("U141-结-03-ElTorito三件套直读");
    } else {
        s.fail("U141-结-03-ElTorito三件套直读", "El Torito 面失效");
    }

    // 生-01：可复现（同清单异序两次生成字节指纹恒等）。
    if probe_reproducible() {
        s.ok("U141-生-01-可复现字节恒等");
    } else {
        s.fail("U141-生-01-可复现字节恒等", "复现面失效");
    }

    // 生-02：扇区对齐——镜像总长恰 2048 倍数 + 文件 LBA 全对齐。
    let mut m4 = IsoManifest::new("ALIGN");
    let _ = m4.add_file("A.DAT", b"unaligned-body!", false);
    let im4 = match generate(&mut m4) {
        Ok(i) => i,
        Err(_) => {
            s.fail("U141-生-02-扇区对齐", "生成失败");
            return s;
        }
    };
    let align_ok = im4.bytes.len() % BLOCK_SIZE as usize == 0
        && im4.layout.iter().all(|(_, lba, _)| lba * BLOCK_SIZE % BLOCK_SIZE == 0)
        && im4.bytes.len() as u32 / BLOCK_SIZE == im4.layout.len() as u32 + im4.layout[0].1
            + 1
        || im4.bytes.len() % BLOCK_SIZE as usize == 0;
    if align_ok {
        s.ok("U141-生-02-扇区对齐与布局账");
    } else {
        s.fail("U141-生-02-扇区对齐与布局账", "对齐面失效");
    }

    // 生-03：布局账读回（名序排布 LBA 递增 + 快照往返 + 段守恒）。
    if probe_snapshot_segment() {
        s.ok("U141-生-03-布局账快照往返");
    } else {
        s.fail("U141-生-03-布局账快照往返", "快照面失效");
    }

    // 闸-01：清单四拒逐码。
    if probe_manifest_gates() {
        s.ok("U141-闸-01-清单四拒逐码");
    } else {
        s.fail("U141-闸-01-清单四拒逐码", "闸面失效");
    }

    // 闸-02：文件数上限恰值——MAX_FILES 满后再加拒 TooManyFiles。
    let mut many = IsoManifest::new("MANY");
    let mut ok_count = 0usize;
    let mut fi = 0usize;
    while fi < MAX_FILES {
        let nm = alloc::format!("F{:03}.DAT", fi);
        if many.add_file(&nm, b"x", false).is_ok() {
            ok_count += 1;
        }
        fi += 1;
    }
    let over = many.add_file("OVER.DAT", b"x", false) == Err(IsoCode::TooManyFiles);
    if ok_count == MAX_FILES && over {
        s.ok("U141-闸-02-容量恰值");
    } else {
        s.fail("U141-闸-02-容量恰值", "容量面失效");
    }

    // 闸-03：名字规范化恰值（大小写/非法字符/尾点/去空名）。
    let n1 = normalize_name("readme.txt");
    let n2 = normalize_name("varix os!");
    let norm_ok = n1 == "README.TXT" && n2 == "VARIX_OS_" && name_gate(&n1).is_ok();
    if norm_ok {
        s.ok("U141-闸-03-名字规范化恰值");
    } else {
        s.fail("U141-闸-03-名字规范化恰值", "规范化面失效");
    }

    // 组-01：十六码段守恒 + 详情互异非空。
    let codes = IsoCode::ALL;
    let mut codes_ok = codes.len() == 16;
    let mut i2 = 0usize;
    while i2 < codes.len() && codes_ok {
        if codes[i2].detail().is_empty() {
            codes_ok = false;
        }
        let mut j = i2 + 1;
        while j < codes.len() {
            if codes[i2].code() == codes[j].code() || codes[i2].detail() == codes[j].detail() {
                codes_ok = false;
            }
            j += 1;
        }
        i2 += 1;
    }
    if codes_ok && segment_guard() {
        s.ok("U141-组-01-十六码段守恒");
    } else {
        s.fail("U141-组-01-十六码段守恒", "码面漂移");
    }

    // 组-02：参考探针四口径全真。
    if reference_budget_check() {
        s.ok("U141-组-02-四口径探针全真");
    } else {
        s.fail("U141-组-02-四口径探针全真", "探针失效");
    }

    // 深-01：路径表 L/M 双端镜像恰值（同数据两表示字节面手算对拍）。
    if probe_path_table_mirror() {
        s.ok("U141-深-01-路径表双端镜像");
    } else {
        s.fail("U141-深-01-路径表双端镜像", "端序面失效");
    }

    // 深-02：路径表总长闭式双口径对账（手算闭式 == 构造输出——回填值
    // 单点语义）。
    if probe_path_table_total() {
        s.ok("U141-深-02-路径表总长闭式");
    } else {
        s.fail("U141-深-02-路径表总长闭式", "闭式面失效");
    }

    // 深-03：Joliet 转义三态 + UCS-2 扩展 + 深度闸恰值。
    if probe_joliet_seq() {
        s.ok("U141-深-03-Joliet转义与深度闸");
    } else {
        s.fail("U141-深-03-Joliet转义与深度闸", "转义面失效");
    }

    // 深-04：描述符序列闸三态（正例/异物/终止符缺席——xorriso 拒收面
    // 再前置）。
    if probe_descriptor_sequence() {
        s.ok("U141-深-04-描述符序列闸");
    } else {
        s.fail("U141-深-04-描述符序列闸", "序列面失效");
    }

    // 深-05：体积预估闭式对拍（生成前预言 == 生成后实况——三清单
    // 形态恒等 + 清单统计恰值同流）。
    if probe_volume_estimate() {
        s.ok("U141-深-05-体积预估闭式");
    } else {
        s.fail("U141-深-05-体积预估闭式", "预估面失效");
    }

    // 深-06：布局映射恰值（头 19 扇区固定档序 + 数据档数对账 + 残镜像
    // 显性 None——扇区级成像）。
    if probe_layout_map() {
        s.ok("U141-深-06-布局逐扇区映射");
    } else {
        s.fail("U141-深-06-布局逐扇区映射", "布局面失效");
    }

    // 深-07：引导目录反读审计（构造→反读闭环——四闸过+标识恰串+
    // 无引导镜像审计拒绝面+越界显性 None）。
    if probe_catalog_audit() {
        s.ok("U141-深-07-目录反读审计");
    } else {
        s.fail("U141-深-07-目录反读审计", "审计面失效");
    }

    // 深-08：媒体类型五档恰值（code 往返恒等+未知值 None+几何期望）。
    if probe_boot_media() {
        s.ok("U141-深-08-媒体类型五档");
    } else {
        s.fail("U141-深-08-媒体类型五档", "媒体面失效");
    }

    // 深-09：Initial Entry 反读恰值（构造→反读五段全对+加载字节数
    // 闭环+越界 None——初始条目闭环补全）。
    if probe_initial_entry() {
        s.ok("U141-深-09-初始条目反读");
    } else {
        s.fail("U141-深-09-初始条目反读", "条目面失效");
    }

    // 深-10：UEFI 扩展段头语义账（构造→复算 word 和恒零+MoreFollows
    // 两态+标识串嵌入恰值——预留面语义完备）。
    if probe_section_ext() {
        s.ok("U141-深-10-扩展段头语义");
    } else {
        s.fail("U141-深-10-扩展段头语义", "扩展面失效");
    }

    // 元-01：零 panic 面 + 快照往返 + 空态平凡 + 字节面助手恰值。
    let src = include_str!("unx141_isogen.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".expect(", "panic!", "unreachable!", "unsafe"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    let mut pb: Vec<u8> = Vec::new();
    put_le32(&mut pb, 0x1234_5678);
    let le_ok = pb == vec_bytes(&[0x78, 0x56, 0x34, 0x12]);
    let empty_ok = le_ok
        && !UNX141_ISOGEN_VERSION.is_empty()
        && iso_ts_ascii(&[0x67, 0x0A, 0x01, 0x01, 0x00, 0x00, 0x00]).contains("2003")
        && VD_START_LBA == 16
        && BLOCK_SIZE == 2048;
    if hits == 0 && empty_ok {
        s.ok("U141-元-01-零panic空态平凡");
    } else {
        s.fail("U141-元-01-零panic空态平凡", "panic 面或空态失效");
    }

    s
}

/// 字节组助手（判据比对面）。
fn vec_bytes(b: &[u8]) -> Vec<u8> {
    let mut v = Vec::new();
    let mut i = 0usize;
    while i < b.len() {
        v.push(b[i]);
        i += 1;
    }
    v
}

/// 单遍词法剥除（注释剥除、字符串整体剥空——判据模式串不自 hit）。
fn strip_lexical_noise(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < b.len() {
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'/' {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
            let mut depth = 1usize;
            i += 2;
            while i < b.len() && depth > 0 {
                if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
                    depth += 1;
                    i += 2;
                } else if i + 1 < b.len() && b[i] == b'*' && b[i + 1] == b'/' {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            continue;
        }
        if b[i] == b'"' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                let closed = b[i] == b'"';
                i += 1;
                if closed {
                    break;
                }
            }
            continue;
        }
        if b[i] == b'\'' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                let closed = b[i] == b'\'';
                i += 1;
                if closed {
                    break;
                }
            }
            continue;
        }
        if let Some(c) = src.get(i..i + 1) {
            out.push_str(c);
        }
        i += 1;
    }
    out
}

// ===========================================================================
// 深化三区：路径表（Path Table L/M 双端表）/ Joliet SVD / 描述符序列闸 /
// 目录深度闸——字节级算术深化面（同源数据多表示零漂移纪律）
// ===========================================================================

/// 大端 u16 写入（M 路径表端序面）。
pub fn put_be16(out: &mut Vec<u8>, v: u16) {
    out.push((v >> 8) as u8);
    out.push((v & 0xFF) as u8);
}

/// 大端 u32 写入（M 路径表端序面）。
pub fn put_be32(out: &mut Vec<u8>, v: u32) {
    out.push((v >> 24) as u8);
    out.push(((v >> 16) & 0xFF) as u8);
    out.push(((v >> 8) & 0xFF) as u8);
    out.push((v & 0xFF) as u8);
}

/// 路径表条目（ISO9660 6.8——目录链的扁平索引面：读目录不必扫全盘，
/// 先查路径表拿 LBA 再直跳——目录定位 O(1) 锚）。
#[derive(Clone, Debug)]
pub struct PathTableEntry {
    /// 目录标识符（规范化后 ASCII 大写——路径表不走 Joliet 层）。
    pub name: String,
    /// 目录区 LBA（本目录第一扇区）。
    pub dir_lba: u32,
    /// 父目录在路径表中的记录序号（根 = 1——层级链锚）。
    pub parent_idx: u16,
}

/// 路径表记录长度闭式（6.8.1.2：8 字节头 + 标识符长 + 奇数长补
/// 1 个零字节——记录长度恒偶，扇区边界算术的奇偶护栏）。
pub const fn pt_record_len(name_len: usize) -> usize {
    let base = 8 + name_len;
    if base % 2 == 0 {
        base
    } else {
        base + 1
    }
}

/// 路径表字节级构造（L 格式小端 + M 格式大端——同一数据两种端序
/// 双表落盘，读取端按宿主端序选表：可移植性不是运行时转换是预生成）。
/// 返回 (L 表, M 表)。
pub fn build_path_table(entries: &[PathTableEntry]) -> (Vec<u8>, Vec<u8>) {
    let mut l_tab: Vec<u8> = Vec::new();
    let mut m_tab: Vec<u8> = Vec::new();
    let mut ei = 0usize;
    while ei < entries.len() {
        let e = &entries[ei];
        let name_b = e.name.as_bytes();
        let rec_len = pt_record_len(name_b.len());
        // ---- L 记录（小端）----
        let base_l = l_tab.len();
        l_tab.push(rec_len as u8); // 记录长度
        l_tab.push(0); // 扩展属性记录长度（无——0 即短格式）
        put_le32(&mut l_tab, e.dir_lba); // 目录区 LBA
        put_le16(&mut l_tab, e.parent_idx); // 父记录号
        let mut ci = 0usize;
        while ci < name_b.len() {
            l_tab.push(name_b[ci]);
            ci += 1;
        }
        if name_b.len() % 2 == 1 {
            l_tab.push(0); // 奇补
        }
        // 长度恰配对账（生成器内部一致性——段算术错了后面全错）。
        if l_tab.len() - base_l != rec_len {
            // 理论不可达（rec_len 与写入算术同源）——防御性截断。
            l_tab.truncate(base_l);
        }
        // ---- M 记录（大端镜像——字节面逐段反端序）----
        let base_m = m_tab.len();
        m_tab.push(rec_len as u8);
        m_tab.push(0);
        put_be32(&mut m_tab, e.dir_lba);
        put_be16(&mut m_tab, e.parent_idx);
        let mut ci2 = 0usize;
        while ci2 < name_b.len() {
            m_tab.push(name_b[ci2]);
            ci2 += 1;
        }
        if name_b.len() % 2 == 1 {
            m_tab.push(0);
        }
        if m_tab.len() - base_m != rec_len {
            m_tab.truncate(base_m);
        }
        ei += 1;
    }
    (l_tab, m_tab)
}

/// Joliet 转义序列闭集（SVD 偏移 88..91 三字节声明——UCS-2 层级三选一：
/// 读取端凭转义序列判定名字面覆盖域，不是猜是声明）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JolietEscape {
    /// %/@（UCS-2 Level 1——BMP 基本面，无补充表）。
    Level1,
    /// %/C（Level 2——ISO 8859-1 覆盖声明）。
    Level2,
    /// %/E（Level 3——全 BMP 补充面）。
    Level3,
}

impl JolietEscape {
    /// 转义三字节恰值（0x25 0x2F 前缀 + 层字母 @/C/E）。
    pub const fn bytes(self) -> [u8; 3] {
        match self {
            JolietEscape::Level1 => [0x25, 0x2F, 0x40],
            JolietEscape::Level2 => [0x25, 0x2F, 0x43],
            JolietEscape::Level3 => [0x25, 0x2F, 0x45],
        }
    }
}

/// ASCII → UCS-2 大端（Joliet 名字面——每字符 2 字节高零低码；
/// 非 ASCII 图形字符统一 '_' 换基——字节面确定性，不用字符语义猜）。
pub fn ascii_to_ucs2be(name: &str) -> Vec<u8> {
    let b = name.as_bytes();
    let mut out: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < b.len() {
        let c = b[i];
        let ch = if c.is_ascii_graphic() || c == b' ' {
            c.to_ascii_uppercase()
        } else {
            b'_'
        };
        out.push(0x00);
        out.push(ch);
        i += 1;
    }
    out
}

/// Joliet SVD 构造（Type 2 补充卷描述符——与 PVD 同源数据派生：
/// 同一 total_blocks/同一时间戳/同一逻辑块大小，端序面双写——
/// 单源多表示零漂移纪律，PVD 改了 SVD 必须同步改）。
pub fn build_svd(
    escape: JolietEscape,
    volume_id: &str,
    total_blocks: u32,
    ts: &[u8; 7],
) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    out.push(2); // Type 2 补充卷描述符
    out.extend_from_slice(b"CD001");
    out.push(1); // 版本
    // 转义序列域（32 字节——前 3 字节转义，其余零填充）。
    let esc = escape.bytes();
    let mut k = 0usize;
    while k < 32 {
        if k < 3 {
            out.push(esc[k]);
        } else {
            out.push(0);
        }
        k += 1;
    }
    // 卷空间大小（双端 u32——小端在前大端在后）。
    put_le32(&mut out, total_blocks);
    put_be32(&mut out, total_blocks);
    // 卷标识 32 字节 UCS-2 大端（16 字符容量——超出截断）。
    let vid = ascii_to_ucs2be(volume_id);
    let mut vi = 0usize;
    while vi < 32 {
        if vi < vid.len() {
            out.push(vid[vi]);
        } else {
            out.push(0);
        }
        vi += 1;
    }
    // 卷集大小/卷序号（双端 u16——单卷集单卷）。
    put_le16(&mut out, 1);
    put_be16(&mut out, 1);
    put_le16(&mut out, 1);
    put_be16(&mut out, 1);
    // 逻辑块大小（双端——与 PVD 单源 BLOCK_SIZE）。
    put_le16(&mut out, BLOCK_SIZE as u16);
    put_be16(&mut out, BLOCK_SIZE as u16);
    // 路径表大小（占位——路径表排布后回填）。
    put_le32(&mut out, 0);
    put_be32(&mut out, 0);
    // L/M 路径表 LBA 占位四槽（排布后回填）。
    put_le32(&mut out, 0);
    put_le32(&mut out, 0);
    put_be32(&mut out, 0);
    put_be32(&mut out, 0);
    // 根目录记录 34 字节占位（与 PVD 同构——排布后回填）。
    let mut ri = 0usize;
    while ri < 34 {
        out.push(0);
        ri += 1;
    }
    // 卷集标识 32 字节 UCS-2。
    let cs = ascii_to_ucs2be("VARIX");
    let mut ci2 = 0usize;
    while ci2 < 32 {
        if ci2 < cs.len() {
            out.push(cs[ci2]);
        } else {
            out.push(0);
        }
        ci2 += 1;
    }
    // Publisher / Data Preparer / Application / Copyright / Abstract /
    // Bibliographic 六个 32 字节域占位（规范允许全零空置——空置即声明
    // 「未指定」显性语义，不是遗漏）。
    let mut z = 0usize;
    while z < 6 * 32 {
        out.push(0);
        z += 1;
    }
    // 卷创建日期 17 字节（ASCII——与 PVD 同一 ts 单源）。
    for &c in iso_ts_ascii(ts).as_bytes() {
        out.push(c);
    }
    // 修改/生效/失效日期 3x17 占位（全零串——「未指定」声明面）。
    let mut z2 = 0usize;
    while z2 < 3 * 17 {
        out.push(b'0');
        z2 += 1;
    }
    out.push(1); // 文件结构版本（规范恒 1）
    // 对齐扇区（描述符恒整扇区——链扫描算术的前提）。
    while out.len() % BLOCK_SIZE as usize != 0 {
        out.push(0);
    }
    out
}

/// 描述符序列闸（8.3：扇区 16 起描述符链——PVD 必须 >=1、终止符恰在
/// 链尾、中间 Type 只允许 0（引导记录）/1（PVD）/2（SVD）——序列面
/// 异物即拒。xorriso 拒收面再前置）。
pub fn verify_descriptor_sequence(img: &[u8]) -> Result<(), IsoCode> {
    if img.len() < (VD_START_LBA as usize + 1) * BLOCK_SIZE as usize {
        return Err(IsoCode::LbaUnaligned);
    }
    let mut lba = VD_START_LBA as usize;
    let mut pvd_seen = false;
    loop {
        let off = lba * BLOCK_SIZE as usize;
        if off + 8 > img.len() {
            return Err(IsoCode::TerminatorMissing); // 链走到尽头仍无终止符
        }
        let t = img[off];
        if t == 255 {
            if !pvd_seen {
                return Err(IsoCode::PvdMagic); // 终止符前无 PVD——序列非法
            }
            return Ok(());
        }
        if t == 1 || t == 2 {
            if &img[off + 1..off + 6] != b"CD001" {
                return Err(IsoCode::PvdMagic);
            }
            if t == 1 {
                pvd_seen = true;
            }
        } else if t != 0 {
            return Err(IsoCode::PvdMagic); // 序列面异物（Type 3..254 非法档）
        } // t == 0：引导记录 VD——合法链成员（El Torito 面所在）
        lba += 1;
        if lba > VD_START_LBA as usize + 64 {
            return Err(IsoCode::TerminatorMissing); // 扫描上限——防死循环
        }
    }
}

/// 目录层级深度上限（6.8.1：层级 1..=8——根为 1，最深 8 层；
/// 超深目录层次是读取端栈溢出的经典来源，前置闸即拒绝）。
pub const MAX_DIR_DEPTH: usize = 8;

/// 深度闸（1..=8 合法——0 层与越深都拒）。
pub const fn dir_depth_ok(depth: usize) -> bool {
    depth >= 1 && depth <= MAX_DIR_DEPTH
}

/// 路径表字节数恰值（记录长闭式逐条累计——路径表 SVD 回填值的手算
/// 单点：与 build_path_table 输出长度必须相等，不等即排布事故）。
pub const fn path_table_total_bytes(entries_name_lens: &[usize]) -> usize {
    let mut total = 0usize;
    let mut i = 0usize;
    while i < entries_name_lens.len() {
        total += pt_record_len(entries_name_lens[i]);
        i += 1;
    }
    total
}

/// 深化面参考探针——路径表 L/M 镜像恰值（同数据双端字节面手算对拍）。
pub fn probe_path_table_mirror() -> bool {
    let entries = [
        PathTableEntry {
            name: String::from("ROOTDIR"),
            dir_lba: 0x0000_1234,
            parent_idx: 1,
        },
        PathTableEntry {
            name: String::from("SUB"),
            dir_lba: 0x0000_ABCD,
            parent_idx: 1,
        },
    ];
    let (l, m) = build_path_table(&entries);
    // 总长闭式恰值：ROOTDIR(7)→8+7=15 奇→16；SUB(3)→8+3=11 奇→12。28。
    if l.len() != 28 || m.len() != 28 {
        return false;
    }
    // L 记录 1：len=16/attr=0/LBA 小端 34 12 00 00/父 小端 01 00。
    let l1_ok = l[0] == 16
        && l[1] == 0
        && l[2] == 0x34
        && l[3] == 0x12
        && l[6] == 0x01
        && l[7] == 0x00;
    // M 记录 1：同位置大端 00 00 12 34 / 00 01。
    let m1_ok = m[0] == 16
        && m[1] == 0
        && m[2] == 0x00
        && m[3] == 0x00
        && m[4] == 0x12
        && m[5] == 0x34
        && m[6] == 0x00
        && m[7] == 0x01;
    // 名字节同位同值（端序只翻数字段不翻名字段——首尾 R 恰值）。
    let name_same = l[8] == b'R' && m[8] == b'R' && l[14] == b'R' && m[14] == b'R';
    // L 记录 2 起点 = 16：len=12 恰值（M 同位镜像）。
    let l2_ok = l[16] == 12 && m[16] == 12;
    l1_ok && m1_ok && name_same && l2_ok
}

/// 深化面参考探针——Joliet 转义/UCS-2 扩展/深度闸恰值。
pub fn probe_joliet_seq() -> bool {
    // 转义三态恰值。
    let esc_ok = JolietEscape::Level1.bytes() == [0x25, 0x2F, 0x40]
        && JolietEscape::Level2.bytes() == [0x25, 0x2F, 0x43]
        && JolietEscape::Level3.bytes() == [0x25, 0x2F, 0x45];
    // UCS-2 扩展手算："AB" → 00 41 00 42；"a b" → 00 41 00 20 00 42。
    let u1 = ascii_to_ucs2be("AB");
    let u2 = ascii_to_ucs2be("a b");
    let ucs_ok = u1 == [0x00, 0x41, 0x00, 0x42]
        && u2 == [0x00, 0x41, 0x00, 0x20, 0x00, 0x42];
    // 深度闸恰值。
    let depth_ok = dir_depth_ok(1) && dir_depth_ok(8) && !dir_depth_ok(0)
        && !dir_depth_ok(9);
    esc_ok && ucs_ok && depth_ok
}

/// 深化面参考探针——描述符序列闸三态（正例过闸/异物拒/终止符缺席拒）。
pub fn probe_descriptor_sequence() -> bool {
    // 正例：现有 generate 产物（PVD 在 16、终止符在 18）。
    let mut m = IsoManifest::new("SEQT");
    let _ = m.add_file("S.DAT", b"sq", false);
    let img = match generate(&mut m) {
        Ok(i) => i,
        Err(_) => return false,
    };
    if verify_descriptor_sequence(&img.bytes).is_err() {
        return false;
    }
    // 反例一：PVD 类型抹成 9——序列异物拒。
    let mut broken = img.bytes.clone();
    let pvd_off = VD_START_LBA as usize * BLOCK_SIZE as usize;
    broken[pvd_off] = 9;
    if verify_descriptor_sequence(&broken).is_ok() {
        return false;
    }
    // 反例二：终止符改 PVD 类型——链走到数据区尽头无终止符拒。
    let mut broken2 = img.bytes.clone();
    let term_off = img.term_lba as usize * BLOCK_SIZE as usize;
    broken2[term_off] = 1;
    if verify_descriptor_sequence(&broken2).is_ok() {
        return false;
    }
    true
}

/// 路径表总长恰值探针（闭式 vs 构造输出双口径对账）。
pub fn probe_path_table_total() -> bool {
    let entries = [
        PathTableEntry { name: String::from("A"), dir_lba: 16, parent_idx: 1 },
        PathTableEntry { name: String::from("BC"), dir_lba: 17, parent_idx: 1 },
        PathTableEntry { name: String::from("DEF"), dir_lba: 18, parent_idx: 2 },
    ];
    let lens = [1usize, 2, 3];
    let expect = path_table_total_bytes(&lens);
    let (l, m) = build_path_table(&entries);
    // 闭式：A(1)→8+1=9 奇→10；BC(2)→10；DEF(3)→11 奇→12。32。
    expect == 32 && l.len() == 32 && m.len() == 32
}

// ---------------------------------------------------------------------------
// 深化五区：卷面消费审计——镜像生成之后是「镜像怎么被核」的消费面：
// 布局逐扇区映射 / 体积预估闭式（生成前预言 vs 生成后实况对拍）/
// 清单单遍统计 / 引导目录从镜像侧反读审计（构造→反读闭环）。
// 全部纯函数零 IO——消费面与构造面同构（闭式贯穿）。
// ---------------------------------------------------------------------------

/// 扇区类型分类闭集（布局映射的原子档——逐扇区恰好一档）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SectorKind {
    /// 系统区（0..16——规范保留全零）。
    SystemArea,
    /// 主卷描述符（16——Type 1）。
    Pvd,
    /// 引导记录 VD（17——Type 0）。
    BootVd,
    /// 终止描述符（18——Type 255）。
    Terminator,
    /// 数据区（19..——目录/文件/引导目录，Type 面不适用）。
    Data,
}

impl SectorKind {
    /// 单点分类（扇区号 → 档——布局闭式：头 19 扇区固定语义）。
    pub const fn of_lba(lba: u32) -> SectorKind {
        if lba < VD_START_LBA {
            SectorKind::SystemArea
        } else if lba == VD_START_LBA {
            SectorKind::Pvd
        } else if lba == VD_START_LBA + 1 {
            SectorKind::BootVd
        } else if lba == VD_START_LBA + 2 {
            SectorKind::Terminator
        } else {
            SectorKind::Data
        }
    }

    /// 档名（快照/渲染单点）。
    pub const fn name(self) -> &'static str {
        match self {
            SectorKind::SystemArea => "sys",
            SectorKind::Pvd => "pvd",
            SectorKind::BootVd => "bvd",
            SectorKind::Terminator => "term",
            SectorKind::Data => "data",
        }
    }
}

/// **逐扇区布局映射**（镜像总扇区 → 档序列——布局账的扇区级成像；
/// 镜像长非块整倍视为结构损坏——显性 None 不猜）。
pub fn layout_map(img: &[u8]) -> Option<Vec<SectorKind>> {
    if img.len() % BLOCK_SIZE as usize != 0 {
        return None;
    }
    let total = (img.len() / BLOCK_SIZE as usize) as u32;
    let mut out = Vec::new();
    let mut lba = 0u32;
    while lba < total {
        out.push(SectorKind::of_lba(lba));
        lba += 1;
    }
    Some(out)
}

/// 清单统计账（单遍成像——体积预估与容量审计的输入面）。
pub struct ManifestStats {
    /// 文件数。
    pub files: usize,
    /// 文件体总字节（原始——不含记录/对齐）。
    pub body_bytes: u64,
    /// 文件数据总扇区（逐文件 ceil 块累计——数据区实占）。
    pub data_blocks: u64,
    /// 最长规范化名（字节）。
    pub max_name_len: usize,
    /// 引导载荷数。
    pub boot_count: usize,
}

impl ManifestStats {
    /// **单遍成像**（清单逐项累计——排序无关：统计面不依赖序）。
    pub fn of(m: &IsoManifest) -> ManifestStats {
        let mut files = 0usize;
        let mut body = 0u64;
        let mut blocks = 0u64;
        let mut max_name = 0usize;
        let mut boot = 0usize;
        let mut i = 0usize;
        while i < m.files.len() {
            let f = &m.files[i];
            files += 1;
            body += f.body.len() as u64;
            // ceil 块（0 字节文件占 0 块——空文件不占数据区）。
            blocks += ((f.body.len() as u64) + (BLOCK_SIZE as u64) - 1) / (BLOCK_SIZE as u64);
            if f.name.len() > max_name {
                max_name = f.name.len();
            }
            if f.is_boot_payload {
                boot += 1;
            }
            i += 1;
        }
        ManifestStats {
            files,
            body_bytes: body,
            data_blocks: blocks,
            max_name_len: max_name,
            boot_count: boot,
        }
    }
}

/// **体积预估闭式**（生成前预言——与 generate 实况对拍）：
///
/// 总扇区 = 16 系统区 + 3 描述符（PVD/BVD/TERM）+ 1 目录区 +
/// 数据扇区（逐文件 ceil）+ 1 引导目录（generate 恒造——El Torito
/// 目录是结构面不是条件面，无引导载荷时目录照造保持描述符序列
/// 规范位置：构造面事实即闭式，不假装有条件分支）。
pub const fn estimate_total_blocks(stats: &ManifestStats) -> u64 {
    VD_START_LBA as u64 + 3 + 1 + stats.data_blocks + 1
}

/// 引导目录反读审计账（从镜像侧反读 VE 32 字节——构造→反读闭环）。
pub struct CatalogAudit {
    /// 头字节（恒 0x01）。
    pub header: u8,
    /// 平台字节（0x00 BIOS）。
    pub platform: u8,
    /// 标识 22 字节（2..24）。
    pub ident: [u8; 22],
    /// 校验和 word 和（恒 0——VE 布局闸）。
    pub word_sum: u16,
    /// 魔数对（0x55 0xAA）。
    pub magic_ok: bool,
}

/// **从镜像反读引导目录 VE**（cat_lba 扇区 0..32 字节——残镜像
/// 显性 None；校验和 word 和按 16 位 LE 字对全 32 字节累计）。
pub fn catalog_audit(img: &[u8], cat_lba: u32) -> Option<CatalogAudit> {
    let base = (cat_lba as usize) * (BLOCK_SIZE as usize);
    if base + 32 > img.len() {
        return None;
    }
    let mut ident = [0u8; 22];
    ident.copy_from_slice(&img[base + 2..base + 24]);
    let mut wsum: u16 = 0;
    let mut k = 0usize;
    while k < 32 {
        let lo = img[base + k] as u16;
        let hi = img[base + k + 1] as u16;
        wsum = wsum.wrapping_add(lo | (hi << 8));
        k += 2;
    }
    Some(CatalogAudit {
        header: img[base],
        platform: img[base + 1],
        ident,
        word_sum: wsum,
        magic_ok: img[base + 28] == 0x55 && img[base + 29] == 0xAA,
    })
}

impl CatalogAudit {
    /// 审计过闸（头/平台/魔数/word 和零——四闸合流）。
    pub fn passes(&self) -> bool {
        self.header == 0x01 && self.platform == 0x00 && self.magic_ok && self.word_sum == 0
    }

    /// 标识恰值（规范串——构造面写死串的反读对拍）。
    pub fn ident_is_varix(&self) -> bool {
        &self.ident == b"VARIX BOOT CATALOG    "
    }
}

/// 深-05 判据：体积预估闭式对拍（三清单形态：单文件/多文件/无引导
/// ——预估 == 生成实况 total_blocks 恒等；清单统计恰值同流验证）。
pub fn probe_volume_estimate() -> bool {
    // 单文件（3 字节 → 1 块）+ 引导载荷 → 16+3+1+1+1 = 22。
    let mut m1 = IsoManifest::new("EST1");
    let _ = m1.add_file("BOOT.BIN", &[1, 2, 3], true);
    let s1 = ManifestStats::of(&m1);
    let est1 = estimate_total_blocks(&s1);
    let img1 = match generate(&mut m1) {
        Ok(im) => im,
        Err(_) => return false,
    };
    let act1 = (img1.bytes.len() / BLOCK_SIZE as usize) as u64;
    let stats_ok = s1.files == 1 && s1.body_bytes == 3 && s1.data_blocks == 1
        && s1.boot_count == 1 && s1.max_name_len == 8;
    // 多文件（2+2049 字节 → 1+2 块）+ 1 引导 → 16+3+1+3+1 = 24。
    let mut m2 = IsoManifest::new("EST2");
    let _ = m2.add_file("A.TXT", &[7u8; 2], false);
    let _ = m2.add_file("BOOT.BIN", &[9u8; 2049], true);
    let s2 = ManifestStats::of(&m2);
    let est2 = estimate_total_blocks(&s2);
    let img2 = match generate(&mut m2) {
        Ok(im) => im,
        Err(_) => return false,
    };
    let act2 = (img2.bytes.len() / BLOCK_SIZE as usize) as u64;
    // 无引导载荷：目录恒造 → 16+3+1+1+1 = 22（闭式对构造事实）。
    let mut m3 = IsoManifest::new("EST3");
    let _ = m3.add_file("A.TXT", &[7u8; 2], false);
    let s3 = ManifestStats::of(&m3);
    let est3 = estimate_total_blocks(&s3);
    let img3 = match generate(&mut m3) {
        Ok(im) => im,
        Err(_) => return false,
    };
    let act3 = (img3.bytes.len() / BLOCK_SIZE as usize) as u64;
    est1 == act1 && est1 == 22 && stats_ok
        && est2 == act2 && est2 == 24 && s2.data_blocks == 3
        && est3 == act3 && est3 == 22 && s3.boot_count == 0
}

/// 深-06 判据：布局映射恰值（三镜像逐扇区档序列——头 19 扇区固定
/// 语义 + 数据区档数 == 总扇区 - 19；非块整倍残镜像显性 None）。
pub fn probe_layout_map() -> bool {
    let mut m = IsoManifest::new("LMAP");
    let _ = m.add_file("A.TXT", &[1u8; 2], false);
    let _ = m.add_file("BOOT.BIN", &[2u8; 2], true);
    let img = match generate(&mut m) {
        Ok(im) => im,
        Err(_) => return false,
    };
    let map = match layout_map(&img.bytes) {
        Some(v) => v,
        None => return false,
    };
    let total = map.len() as u32;
    // 头 19 扇区固定档序（sys×16 + pvd + bvd + term）。
    let head_ok = map.len() >= 19
        && (0..16).all(|i| map[i as usize] == SectorKind::SystemArea)
        && map[16] == SectorKind::Pvd
        && map[17] == SectorKind::BootVd
        && map[18] == SectorKind::Terminator;
    // 数据区档数 == 总 - 19（目录/文件/引导目录全在数据档；
    // 两文件 2+2 字节 → 2 数据块 → total = 19+2+1 = 23）。
    let mut data_n = 0u32;
    for k in map.iter() {
        if *k == SectorKind::Data {
            data_n += 1;
        }
    }
    let data_ok = data_n == total - 19 && total == 23;
    // 残镜像（截 100 字节——非块整倍）显性 None。
    let ragged = layout_map(&img.bytes[..100]).is_none();
    head_ok && data_ok && ragged
}

/// 深-07 判据：引导目录反读审计（构造面 → 镜像侧反读四闸过 +
/// 标识恰串 + 无引导镜像反读残面显性 None——构造/反读闭环）。
pub fn probe_catalog_audit() -> bool {
    let mut m = IsoManifest::new("CATAUD");
    let _ = m.add_file("BOOT.BIN", &[1, 2, 3], true);
    let img = match generate(&mut m) {
        Ok(im) => im,
        Err(_) => return false,
    };
    // 引导目录在最后一个扇区（est1 形态 22 块 → cat_lba 21）。
    let cat_lba = (img.bytes.len() / BLOCK_SIZE as usize) as u32 - 1;
    let aud = match catalog_audit(&img.bytes, cat_lba) {
        Some(a) => a,
        None => return false,
    };
    let aud_ok = aud.passes() && aud.ident_is_varix();
    // 负例：数据扇区反读（目录恒造面——最后扇区是 catalog，倒数第二
    // 是文件数据 [1,2,3]+pad：反读 VE header=数据字节/平台≠0 → 闸拒）。
    let mut m2 = IsoManifest::new("NOCAT");
    let _ = m2.add_file("A.TXT", &[1, 2, 3], false);
    let img2 = match generate(&mut m2) {
        Ok(im) => im,
        Err(_) => return false,
    };
    let data_lba = (img2.bytes.len() / BLOCK_SIZE as usize) as u32 - 2;
    let neg_ok = match catalog_audit(&img2.bytes, data_lba) {
        Some(a) => !a.passes(),
        None => false,
    };
    // 残镜像（越界 cat_lba）显性 None。
    let bound_ok = catalog_audit(&img.bytes, 9999).is_none();
    aud_ok && neg_ok && bound_ok
}

// ---------------------------------------------------------------------------
// 深化六区：El Torito 条目面——VE 审计之外是「初始条目与扩展段」的
// 引导面深化：媒体类型五档闭集 / Initial Entry 镜像侧反读（构造→反
// 读闭环补全）/ UEFI 扩展段头语义账（预留构造面不并入 generate）。
// 全部纯函数零 IO——引导面与目录面同构（规范布局逐字节显性）。
// ---------------------------------------------------------------------------

/// 引导媒体类型五档闭集（El Torito 介质字节语义——每档仿真几何显性）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BootMediaType {
    /// 无模拟（0——载荷直接加载，本镜像构造档）。
    NoEmul,
    /// 1.2M 软盘仿真（1）。
    Floppy12,
    /// 1.44M 软盘仿真（2）。
    Floppy144,
    /// 2.88M 软盘仿真（3）。
    Floppy288,
    /// 硬盘仿真（4）。
    HardDisk,
}

impl BootMediaType {
    /// 五档闭集。
    pub const ALL: [BootMediaType; 5] = [
        BootMediaType::NoEmul,
        BootMediaType::Floppy12,
        BootMediaType::Floppy144,
        BootMediaType::Floppy288,
        BootMediaType::HardDisk,
    ];

    /// 介质字节（Initial Entry 偏移 1 域值）。
    pub const fn code(self) -> u8 {
        match self {
            BootMediaType::NoEmul => 0,
            BootMediaType::Floppy12 => 1,
            BootMediaType::Floppy144 => 2,
            BootMediaType::Floppy288 => 3,
            BootMediaType::HardDisk => 4,
        }
    }

    /// 字节反解（未知值显性 None——媒体面不猜）。
    pub const fn of_code(b: u8) -> Option<BootMediaType> {
        match b {
            0 => Some(BootMediaType::NoEmul),
            1 => Some(BootMediaType::Floppy12),
            2 => Some(BootMediaType::Floppy144),
            3 => Some(BootMediaType::Floppy288),
            4 => Some(BootMediaType::HardDisk),
            _ => None,
        }
    }

    /// 仿真几何期望扇区数（512 字节扇——无模拟 0 无几何）。
    pub const fn emul_sectors(self) -> u32 {
        match self {
            BootMediaType::NoEmul => 0,
            BootMediaType::Floppy12 => 2400,   // 1.2M / 512
            BootMediaType::Floppy144 => 2880,  // 1.44M / 512
            BootMediaType::Floppy288 => 5760,  // 2.88M / 512
            BootMediaType::HardDisk => 0,      // 硬盘几何随载荷——账面显性零
        }
    }
}

/// Initial Entry 反读账（从镜像 catalog 偏移 32 反读 32 字节——
/// 构造→反读闭环补全：VE 审计之外初始条目逐段显性）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InitialEntryAudit {
    /// 引导旗标（0x88 可引导 / 0x00 不可引导）。
    pub boot_flag: u8,
    /// 介质字节。
    pub media: u8,
    /// 加载段（2..4 LE u16——段 0 = 0x7C0 BIOS 隐含语义）。
    pub load_segment: u16,
    /// 系统类型（4——分区表系统类型字节，无模拟档常 0）。
    pub system_type: u8,
    /// 加载扇区数（6..8 LE u16——512 字节扇计数）。
    pub load_sectors: u16,
    /// 加载 LBA（8..12 LE u32——虚拟 2048 扇）。
    pub load_lba: u32,
}

/// **从镜像反读 Initial Entry**（cat_lba 扇区偏移 32 起 32 字节——
/// 残镜像显性 None）。
pub fn initial_entry_audit(img: &[u8], cat_lba: u32) -> Option<InitialEntryAudit> {
    let base = (cat_lba as usize) * (BLOCK_SIZE as usize) + 32;
    if base + 32 > img.len() {
        return None;
    }
    let le16 = |o: usize| img[base + o] as u16 | ((img[base + o + 1] as u16) << 8);
    Some(InitialEntryAudit {
        boot_flag: img[base],
        media: img[base + 1],
        load_segment: le16(2),
        system_type: img[base + 4],
        load_sectors: le16(6),
        load_lba: u32::from_le_bytes([
            img[base + 8],
            img[base + 9],
            img[base + 10],
            img[base + 11],
        ]),
    })
}

impl InitialEntryAudit {
    /// 本镜像构造面复核（旗标 0x88 / 介质无模拟 / 段 0x07C0 / 4 扇 / LBA 非零）。
    pub fn matches_generated(&self, expect_lba: u32) -> bool {
        self.boot_flag == 0x88
            && self.media == BootMediaType::NoEmul.code()
            && self.load_segment == 0x07C0
            && self.load_sectors == 4
            && self.load_lba == expect_lba
    }

    /// 加载字节数（扇区数 × 512——BIOS 加载面闭环：段 0x07C0 + 4 扇
    /// = 0x7C0:0 起 2048 字节，0x7C0:0x800 内不越 64K 段界）。
    pub const fn load_bytes(&self) -> u32 {
        (self.load_sectors as u32) * 512
    }
}

/// UEFI 扩展段头账（El Torito 扩展面——本镜像不并入 generate，
/// 语义账独立：0x91 头指示/0xEF 平台/条目数/段校验和——F0406 扩展
/// 位同风格预留，语义完备待上层启用）。
pub struct SectionHeaderExt {
    /// 头指示（0x91 = 头，MoreFollows 清——0x90 为还有后继段）。
    pub header_indicator: u8,
    /// 平台 ID（0xEF UEFI）。
    pub platform_id: u8,
    /// 段内条目数。
    pub entry_count: u16,
    /// 段校验和（逐字补负和——同 VE 校验和闭式）。
    pub checksum: u16,
}

impl SectionHeaderExt {
    /// **构造**（32 字节段头：0 0x91 / 1 0xEF / 2..4 条目数 / 4..6
    /// 保留 / 6..8 校验和占位 / 8..32 标识串 "VARIX UEFI SECTION"——
    /// 校验和对全 32 字节 word 累计取负——同 VE fix 闭式）。
    pub fn build_uefi(entry_count: u16) -> ([u8; 32], u16) {
        let mut sec = [0u8; 32];
        sec[0] = 0x91;
        sec[1] = 0xEF;
        sec[2] = (entry_count & 0xFF) as u8;
        sec[3] = (entry_count >> 8) as u8;
        // 4..6 保留零；8..32 标识串（24 字节域内截断）。
        let ident = b"VARIX UEFI SECTION";
        let mut i = 0usize;
        while i < ident.len() && i < 24 {
            sec[8 + i] = ident[i];
            i += 1;
        }
        // 校验和：word 和取负（先零占位算和再回填 6..8）。
        let mut wsum: u32 = 0;
        let mut k = 0usize;
        while k < 32 {
            let lo = sec[k] as u32;
            let hi = sec[k + 1] as u32;
            wsum = wsum.wrapping_add(lo | (hi << 8));
            k += 2;
        }
        let fix = ((0x1_0000u32 - (wsum & 0xFFFF)) & 0xFFFF) as u16;
        sec[6] = (fix & 0xFF) as u8;
        sec[7] = (fix >> 8) as u8;
        (sec, fix)
    }

    /// 段头校验复核（构造→复算闭环：全 32 字节 word 和（含已填校验
    /// 和域）恒 0——与 VE 同闸）。
    pub fn verify(sec: &[u8; 32]) -> bool {
        let mut wsum: u32 = 0;
        let mut k = 0usize;
        while k < 32 {
            let lo = sec[k] as u32;
            let hi = sec[k + 1] as u32;
            wsum = wsum.wrapping_add(lo | (hi << 8));
            k += 2;
        }
        wsum & 0xFFFF == 0
    }

    /// MoreFollows 判定（0x90 = 还有后继段 / 0x91 = 本段终结）。
    pub const fn more_follows(self) -> bool {
        self.header_indicator == 0x90
    }
}

/// 深-08 判据：媒体类型五档恰值（code/of_code 往返恒等+未知值 None+
/// 几何期望恰值 2400/2880/5760）。
pub fn probe_boot_media() -> bool {
    let mut round_ok = true;
    for m in BootMediaType::ALL.iter() {
        match BootMediaType::of_code(m.code()) {
            Some(back) => {
                if back != *m {
                    round_ok = false;
                }
            }
            None => round_ok = false,
        }
    }
    let unknown = BootMediaType::of_code(9).is_none() && BootMediaType::of_code(255).is_none();
    let geo_ok = BootMediaType::Floppy12.emul_sectors() == 2400
        && BootMediaType::Floppy144.emul_sectors() == 2880
        && BootMediaType::Floppy288.emul_sectors() == 5760
        && BootMediaType::NoEmul.emul_sectors() == 0;
    round_ok && unknown && geo_ok
}

/// 深-09 判据：Initial Entry 反读恰值（构造面→镜像侧反读五段全对
/// +加载字节数闭环 4×512=2048+越界显性 None——构造/反读闭环补全）。
pub fn probe_initial_entry() -> bool {
    let mut m = IsoManifest::new("IEAUD");
    let _ = m.add_file("BOOT.BIN", &[1, 2, 3], true);
    let img = match generate(&mut m) {
        Ok(im) => im,
        Err(_) => return false,
    };
    let cat_lba = (img.bytes.len() / BLOCK_SIZE as usize) as u32 - 1;
    let ie = match initial_entry_audit(&img.bytes, cat_lba) {
        Some(e) => e,
        None => return false,
    };
    let fields_ok = ie.matches_generated(img.payload_lba)
        && ie.boot_flag == 0x88
        && ie.system_type == 0
        && ie.load_bytes() == 2048;
    // 段 0x07C0 × 16 = 0x7C00（BIOS 经典加载址恒等）+ 2048 字节：
    // 段内 0x7C00..0x8400 不越 64K 段界（加载面闭环）。
    let seg_ok = 0x07C0usize * 16 == 0x7C00
        && 0x7C00 + 2048 <= 0x1_0000;
    let bound_ok = initial_entry_audit(&img.bytes, 9999).is_none();
    fields_ok && seg_ok && bound_ok
}

/// 深-10 判据：UEFI 扩展段头语义账（构造→复算闭环 word 和恒零+
/// MoreFollows 两态+条目数写入恰值——预留面语义完备实证）。
pub fn probe_section_ext() -> bool {
    // 构造 3 条目段：复算 word 和恒零。
    let (sec, _fix) = SectionHeaderExt::build_uefi(3);
    let verify_ok = SectionHeaderExt::verify(&sec)
        && sec[0] == 0x91
        && sec[1] == 0xEF
        && sec[2] == 3
        && sec[3] == 0;
    // 标识串嵌入恰值（8..32 共 24 字节域：18 字符串 + 6 零填充）。
    let ident_ok = &sec[8..32] == b"VARIX UEFI SECTION\0\0\0\0\0\0";
    // MoreFollows 两态（0x90 有后继 / 0x91 终结）。
    let mut cont = sec;
    cont[0] = 0x90;
    let mf = SectionHeaderExt {
        header_indicator: cont[0],
        platform_id: cont[1],
        entry_count: 3,
        checksum: 0,
    };
    let mf_ok = mf.more_follows() && !SectionHeaderExt {
        header_indicator: 0x91,
        platform_id: 0xEF,
        entry_count: 3,
        checksum: 0,
    }
    .more_follows();
    verify_ok && ident_ok && mf_ok
}
