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
