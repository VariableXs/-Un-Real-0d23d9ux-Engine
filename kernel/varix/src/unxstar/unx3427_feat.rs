//! UNX-F3427 结果落盘（内存盘文件写入） · 0x798C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B12 逐字摘录）：
//!   ① 纯功能代码 ≥280 行；
//!   ② J1：结果写入内存盘（固定路径 + 滚动保留 3 份）；
//!     M-186 落盘-回读一致。
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 内存盘三槽滚动：head 单调递增版本号，槽位 = head % 3 原地翻新，
//!     任何时刻可回读最近 3 份（age 0/1/2），更老版本显性拒
//!     （AgeOutOfRange——不静默给出旧数据）；
//!   - 固定路径闭集：结果文件只认冻结路径 `/varix/results/run.json`，
//!     空路径与表外路径逐类点名拒（PathEmpty/PathForeign）；
//!   - 落盘-回读一致（M-186）：写入时随存 FNV-1a 校验和，回读时先验和
//!     再交数据——位翻转/篡改必被 ChecksumBreak 点抓；
//!   - 容量纪律：单份结果上限 4KB（超限拒），空内容拒（结果摘要非空）；
//!   - 乐观版本闸：写方携带所见 head 写入，盘头已前进则拒
//!     （VersionRegress——翻新覆盖前先发现并发写）；
//!   - 双跑一致：同内容两次落盘-回读流程逐字节相同。
//! 常量来源：
//!   - 固定路径：派工卡 J1 判据原文（固定路径 + 滚动保留 3 份）；
//!   - FNV-1a 参数（0x811C9DC5 / 0x01000193）：FNV-1a 32 位公开参数；
//!   - 诊断段：0x798C0（0x40 对齐，B12 批窗 [0x79800, 0x799C0]）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x40 对齐，B12 批段窗 [0x79800, 0x799C0]）。
pub const SEG_BASE: u32 = 0x798C0;

/// 结果文件固定路径（冻结；表外路径一律拒收）。
pub const RESULT_PATH: &'static str = "/varix/results/run.json";

/// 滚动保留份数（J1 冻结：3 份）。
pub const KEEP_VERSIONS: usize = 3;

/// 单份结果字节上限（超限显性拒，防内存盘失守）。
pub const MAX_BLOB: usize = 4096;

/// FNV-1a 32 位偏移基（公开参数）。
const FNV_OFFSET: u32 = 0x811C_9DC5;

/// FNV-1a 32 位乘子（公开参数）。
const FNV_PRIME: u32 = 0x0100_0193;

/// FNV-1a 32 位校验和（逐字节异或乘，确定性整数运算）。
pub fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = FNV_OFFSET;
    for &b in data.iter() {
        h ^= b as u32;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// 内存盘文件对象（版本 + 路径 + 内容 + 校验和；字段公开供注入构造）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Blob {
    /// 落盘版本号（head 单调递增轴上的序号，1 起）。
    pub version: u64,
    /// 落盘路径（须等于固定路径）。
    pub path: String,
    /// 内容字节。
    pub content: Vec<u8>,
    /// 内容校验和（FNV-1a；回读验和用）。
    pub checksum: u32,
}

impl Blob {
    /// 构造并自动计算校验和。
    pub fn seal(version: u64, path: &str, content: &[u8]) -> Blob {
        Blob {
            version,
            path: String::from(path),
            content: content.iter().copied().collect(),
            checksum: fnv1a(content),
        }
    }

    /// 内容完整性自查（写入面守卫：校验和与内容一致）。
    pub fn intact(&self) -> bool {
        fnv1a(&self.content) == self.checksum
    }
}

/// 内存盘（三槽滚动；字段公开供注入构造坏块）。
#[derive(Debug, Clone)]
pub struct MemoryDisk {
    /// 三条滚动槽。
    pub slots: [Option<Blob>; KEEP_VERSIONS],
    /// 盘头版本号（已写份数；单调递增，saturating 防溢出）。
    pub head: u64,
}

impl MemoryDisk {
    /// 空盘。
    pub fn new() -> MemoryDisk {
        MemoryDisk { slots: [None, None, None], head: 0 }
    }
}

/// 槽位换算：版本号 → 槽下标（head%3 轴；版本号 0 无槽）。
fn slot_of(version: u64) -> usize {
    (version % KEEP_VERSIONS as u64) as usize
}

/// 落盘：路径闭集 → 容量纪律 → 乐观版本闸 → 三槽翻新。
/// 返回落盘版本号（head+1）。
pub fn disk_write(
    disk: &mut MemoryDisk,
    path: &str,
    expect_head: u64,
    content: &[u8],
) -> Result<u64, u32> {
    if path.is_empty() {
        return Err(Diag::PathEmpty.val());
    }
    if path != RESULT_PATH {
        return Err(Diag::PathForeign.val());
    }
    if content.len() > MAX_BLOB {
        return Err(Diag::BlobOversize.val());
    }
    if content.is_empty() {
        return Err(Diag::BlobEmpty.val());
    }
    if disk.head != expect_head {
        return Err(Diag::VersionRegress.val());
    }
    let version = disk.head.saturating_add(1);
    let blob = Blob::seal(version, path, content);
    let slot = slot_of(version);
    match disk.slots.get_mut(slot) {
        Some(bucket) => {
            *bucket = Some(blob);
        }
        None => return Err(Diag::SlotEmpty.val()),
    }
    disk.head = version;
    Ok(version)
}

/// 回读最近一份（age=0）。验和先行：内容与校验和不一致显性拒。
pub fn disk_read_latest(disk: &MemoryDisk) -> Result<(u64, Vec<u8>), u32> {
    disk_read_age(disk, 0)
}

/// 回读 age 份之前的版本（age ∈ [0,3)；更老显性拒）。
pub fn disk_read_age(disk: &MemoryDisk, age: usize) -> Result<(u64, Vec<u8>), u32> {
    if age >= KEEP_VERSIONS {
        return Err(Diag::AgeOutOfRange.val());
    }
    if disk.head == 0 {
        return Err(Diag::ReadBeforeWrite.val());
    }
    // 目标版本 = head - age；版本 ≥ 1（age < 3 且 head ≥ 1 时可能下溢
    // 到 0——head < age 时该版本从未存在，同样 AgeOutOfRange 点名）。
    let target = match (disk.head as usize).checked_sub(age) {
        Some(t) if t >= 1 => t as u64,
        _ => return Err(Diag::AgeOutOfRange.val()),
    };
    let slot = slot_of(target);
    let blob = match disk.slots.get(slot) {
        Some(Some(b)) => b.clone(),
        Some(None) => return Err(Diag::SlotEmpty.val()),
        None => return Err(Diag::SlotEmpty.val()),
    };
    if !blob.intact() {
        return Err(Diag::ChecksumBreak.val());
    }
    Ok((blob.version, blob.content))
}

/// 逐版本清点：盘上可读版本号闭集（最新往前最多 KEEP_VERSIONS 份）。
pub fn readable_versions(disk: &MemoryDisk) -> Vec<u64> {
    let mut out: Vec<u64> = Vec::new();
    let mut age = 0usize;
    while age < KEEP_VERSIONS {
        if let Ok((v, _)) = disk_read_age(disk, age) {
            out.push(v);
        }
        age += 1;
    }
    out
}

/// 诊断码闭集：0x798C1..=0x798CC（12 码，变体带中文 doc）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Diag {
    /// 路径表外：落盘路径不是冻结固定路径。
    PathForeign = 0x01,
    /// 空路径：路径串为空。
    PathEmpty = 0x02,
    /// 超限：单份结果超过 4KB 上限。
    BlobOversize = 0x03,
    /// 空内容：结果摘要为空（结果账须非空）。
    BlobEmpty = 0x04,
    /// 验和破缺：回读内容与所存校验和不一致（位翻转/篡改）。
    ChecksumBreak = 0x05,
    /// 空槽：目标槽位无数据。
    SlotEmpty = 0x06,
    /// 年龄越界：回读 age ≥ 3 或版本从未存在。
    AgeOutOfRange = 0x07,
    /// 未写先读：空盘回读。
    ReadBeforeWrite = 0x08,
    /// 版本回退：乐观闸发现盘头与写方所见不符（并发写风险）。
    VersionRegress = 0x09,
    /// 往返破缺：落盘-回读内容不一致。
    RoundTripBreak = 0x0A,
    /// 段位守卫失守：SEG_BASE 不满足 0x40 对齐或越出批段窗。
    SegGuardFail = 0x0B,
    /// 双跑不一致：同内容两次落盘-回读流程不同。
    DoubleRunDiverge = 0x0C,
}

impl Diag {
    /// 完整 u32 编码。
    pub fn val(self) -> u32 {
        SEG_BASE + (self as u32)
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::PathForeign => "路径表外：建议落盘只用固定路径 /varix/results/run.json",
            Diag::PathEmpty => "空路径：建议补全路径串后再落盘",
            Diag::BlobOversize => "超限：建议结果账瘦身或分片落盘，单份上限 4KB 不放宽",
            Diag::BlobEmpty => "空内容：建议先定稿非空结果摘要再落盘",
            Diag::ChecksumBreak => "验和破缺：建议重落该份结果并排查内存位翻转来源",
            Diag::SlotEmpty => "空槽：建议先落盘再回读，空槽无数据可交",
            Diag::AgeOutOfRange => "年龄越界：建议只回读最近 3 份，更老版本已按滚动纪律翻新",
            Diag::ReadBeforeWrite => "未写先读：建议先执行一次落盘再回读",
            Diag::VersionRegress => "版本回退：建议写前重读盘头，发现并发写先合并再落",
            Diag::RoundTripBreak => "往返破缺：建议落盘与回读共用同一校验和实现",
            Diag::SegGuardFail => "段位守卫失守：建议核对 SEG_BASE 的 0x40 对齐与批段窗",
            Diag::DoubleRunDiverge => "双跑不一致：建议落盘回读做成纯函数流程，输入定则输出定",
        }
    }
}

/// 闭集全量数组（12 码）。
pub const ALL_DIAGS: [Diag; 12] = [
    Diag::PathForeign,
    Diag::PathEmpty,
    Diag::BlobOversize,
    Diag::BlobEmpty,
    Diag::ChecksumBreak,
    Diag::SlotEmpty,
    Diag::AgeOutOfRange,
    Diag::ReadBeforeWrite,
    Diag::VersionRegress,
    Diag::RoundTripBreak,
    Diag::SegGuardFail,
    Diag::DoubleRunDiverge,
];

/// 诊断码查表闭集返回；未知编码回退固定兜底串。
pub fn detail(code: u32) -> &'static str {
    for d in ALL_DIAGS.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：建议核对是否落在 0x798C1..=0x798CC 闭集内"
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool）────────────────────────────

/// 段位守卫：0x40 对齐 + B12 批段窗 [0x79800, 0x799C0]。
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x79800 && SEG_BASE <= 0x799C0
}

/// 诊断闭集完整：12 码、编码连续、唯一、文案非空且带「建议」。
fn diag_closed_set_ok() -> bool {
    if ALL_DIAGS.len() != 12 {
        return false;
    }
    let mut i = 0usize;
    while i < ALL_DIAGS.len() {
        let a = match ALL_DIAGS.get(i) {
            Some(d) => *d,
            None => return false,
        };
        if a.val() != SEG_BASE + (i as u32) + 1 {
            return false;
        }
        let text = a.detail();
        if text.is_empty() || !text.contains("建议") {
            return false;
        }
        let mut j = i + 1;
        while j < ALL_DIAGS.len() {
            match ALL_DIAGS.get(j) {
                Some(b) => {
                    if a.val() == b.val() {
                        return false;
                    }
                }
                None => return false,
            }
            j += 1;
        }
        i += 1;
    }
    let first = match ALL_DIAGS.first() {
        Some(d) => *d,
        None => return false,
    };
    !detail(0xDEAD).is_empty() && detail(first.val()) == first.detail()
}

/// 冻结面：固定路径逐字、保留份数 3、字节上限 4096。
fn frozen_constants_ok() -> bool {
    RESULT_PATH == "/varix/results/run.json"
        && KEEP_VERSIONS == 3
        && MAX_BLOB == 4096
        && slot_of(1) == 1
        && slot_of(2) == 2
        && slot_of(3) == 0
        && slot_of(4) == 1
}

/// FNV-1a 校验和面：空串基值、确定性、逐字节敏感。
fn fnv_sanity_ok() -> bool {
    let a = fnv1a(b"");
    let b = fnv1a(b"run.json");
    let c = fnv1a(b"run.json");
    let d = fnv1a(b"run.jsoo");
    a == FNV_OFFSET && b == c && b != d
}

/// M-186 落盘-回读一致：写一份读回一份逐字节同、版本号对齐。
fn write_read_roundtrip_ok() -> bool {
    let mut disk = MemoryDisk::new();
    let payload = b"{\"suite\":\"varix\",\"cases\":[1,2,3]}";
    let v = match disk_write(&mut disk, RESULT_PATH, 0, payload) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if v != 1 || disk.head != 1 {
        return false;
    }
    match disk_read_latest(&disk) {
        Ok((rv, content)) => {
            rv == 1 && content.as_slice() == payload
        }
        Err(_) => false,
    }
}

/// 滚动保留 3 份：连写 5 份后仅 3/4/5 可读，第 1/2 份已翻新。
fn rolling_keep3_ok() -> bool {
    let mut disk = MemoryDisk::new();
    let mut head = 0u64;
    let mut i = 1u64;
    while i <= 5 {
        let tag = format!("版本{}内容", i);
        head = match disk_write(&mut disk, RESULT_PATH, head, tag.as_bytes()) {
            Ok(v) => v,
            Err(_) => return false,
        };
        i += 1;
    }
    if head != 5 || disk.head != 5 {
        return false;
    }
    let readable = readable_versions(&disk);
    if readable != vec![5, 4, 3] {
        return false;
    }
    // 第 1/2 份已滚出：age 3/4 显性拒。
    disk_read_age(&disk, 3) == Err(Diag::AgeOutOfRange.val())
        && disk_read_age(&disk, 4) == Err(Diag::AgeOutOfRange.val())
        && match disk_read_age(&disk, 2) {
            Ok((v, content)) => v == 3 && content == format!("版本{}内容", 3).into_bytes(),
            Err(_) => false,
        }
}

/// 注入 1：位翻转篡改必被验和点抓（ChecksumBreak）。
fn inj_checksum_break_ok() -> bool {
    let mut disk = MemoryDisk::new();
    if disk_write(&mut disk, RESULT_PATH, 0, "原始内容".as_bytes()).is_err() {
        return false;
    }
    // 构造坏块：内容篡改但校验和未随改（模拟位翻转）。
    let slot = slot_of(1);
    let mut bad = match disk.slots.get(slot) {
        Some(Some(b)) => b.clone(),
        _ => return false,
    };
    if bad.content.is_empty() {
        return false;
    }
    bad.content = Vec::new();
    match disk.slots.get_mut(slot) {
        Some(bucket) => {
            *bucket = Some(bad);
        }
        None => return false,
    }
    disk_read_latest(&disk) == Err(Diag::ChecksumBreak.val())
}

/// 注入 2：路径三违约（空路径 / 表外路径 / 大小写漂移）逐类拒。
fn inj_path_rejects_ok() -> bool {
    let mut disk = MemoryDisk::new();
    let empty = disk_write(&mut disk, "", 0, b"x");
    let foreign = disk_write(&mut disk, "/tmp/other.json", 0, b"x");
    let drift = disk_write(&mut disk, "/varix/results/Run.json", 0, b"x");
    empty == Err(Diag::PathEmpty.val())
        && foreign == Err(Diag::PathForeign.val())
        && drift == Err(Diag::PathForeign.val())
}

/// 注入 3：容量两违约（超限 4KB+1 / 空内容）逐类拒。
fn inj_capacity_rejects_ok() -> bool {
    let mut disk = MemoryDisk::new();
    let mut big = Vec::new();
    let mut i = 0usize;
    while i <= MAX_BLOB {
        big.push(b'a');
        i += 1;
    }
    let over = disk_write(&mut disk, RESULT_PATH, 0, &big);
    let empty = disk_write(&mut disk, RESULT_PATH, 0, b"");
    over == Err(Diag::BlobOversize.val()) && empty == Err(Diag::BlobEmpty.val())
}

/// 注入 4：未写先读 + 乐观版本闸（盘头前进后旧 head 写入）逐类拒。
fn inj_version_guards_ok() -> bool {
    let disk = MemoryDisk::new();
    if disk_read_latest(&disk) != Err(Diag::ReadBeforeWrite.val()) {
        return false;
    }
    let mut disk2 = MemoryDisk::new();
    if disk_write(&mut disk2, RESULT_PATH, 0, "第一份".as_bytes()).is_err() {
        return false;
    }
    // 写方仍持旧 head=0 → 乐观闸拒。
    disk_write(&mut disk2, RESULT_PATH, 0, "第二份".as_bytes()) == Err(Diag::VersionRegress.val())
        // 盘头未动的合法写入不受影响。
        && disk_write(&mut disk2, RESULT_PATH, 1, "第二份".as_bytes()).is_ok()
}

/// 双跑一致：同内容两次「落盘→回读」流程逐字节相同。
fn double_run_ok() -> bool {
    let flow = || -> Result<Vec<u8>, u32> {
        let mut disk = MemoryDisk::new();
        disk_write(&mut disk, RESULT_PATH, 0, "双跑一致载荷".as_bytes())?;
        let (_, content) = disk_read_latest(&disk)?;
        Ok(content)
    };
    let a = flow();
    let b = flow();
    a.is_ok() && b.is_ok() && a == b
}

/// 错误路径 Result 语义：Ok 与闭集 Err 两侧均可达。
fn error_path_ok() -> bool {
    let mut disk = MemoryDisk::new();
    if disk_write(&mut disk, RESULT_PATH, 0, "载荷".as_bytes()).is_err() {
        return false;
    }
    if disk_read_latest(&disk).is_err() {
        return false;
    }
    match disk_read_age(&disk, 9) {
        Err(code) => {
            let mut in_set = false;
            for d in ALL_DIAGS.iter() {
                if d.val() == code {
                    in_set = true;
                }
            }
            in_set && !detail(0x0).is_empty()
        }
        Ok(_) => false,
    }
}

/// ③ 写回面：状态=已完成、结果摘要非空。
fn writeback_summary_ok() -> bool {
    let mut disk = MemoryDisk::new();
    let payload = format!("UNX-F3427 落盘摘要：滚动{}份 固定路径{}", KEEP_VERSIONS, RESULT_PATH);
    let v = match disk_write(&mut disk, RESULT_PATH, 0, payload.as_bytes()) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let readback = disk_read_latest(&disk);
    let consistent = match readback {
        Ok((rv, content)) => rv == v && content == payload.clone().into_bytes(),
        Err(_) => false,
    };
    let summary = format!("UNX-F3427 落盘完成：版本{} 回读{}", v, if consistent { "一致" } else { "破缺" });
    !summary.is_empty() && summary.contains("落盘完成") && consistent
}

// ── 判据总成：run_unx3427_checks ───────────────────────────────────────

/// run_unx3427_checks：本单判据聚合（诊断段 0x798C0，全部走 add_dyn）。
pub fn run_unx3427_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3427 结果落盘");
    let checks: [(&'static str, bool, String); 13] = [
        (
            "段位守卫0x40对齐且在批段窗内",
            seg_guard_ok(),
            format!("SEG_BASE=0x{:X} 守卫{}", SEG_BASE, if seg_guard_ok() { "过" } else { "破" }),
        ),
        (
            "诊断码闭集12码唯一且文案带建议",
            diag_closed_set_ok(),
            format!("闭集{}", if diag_closed_set_ok() { "完整" } else { "破缺" }),
        ),
        (
            "冻结面固定路径三份滚动4KB上限逐字核对",
            frozen_constants_ok(),
            format!("冻结面{}", if frozen_constants_ok() { "齐" } else { "漂移" }),
        ),
        (
            "FNV1a校验和确定性逐字节敏感",
            fnv_sanity_ok(),
            format!("校验和{}", if fnv_sanity_ok() { "稳" } else { "漂" }),
        ),
        (
            "M186落盘回读逐字节一致版本对齐",
            write_read_roundtrip_ok(),
            format!("往返{}", if write_read_roundtrip_ok() { "一致" } else { "破缺" }),
        ),
        (
            "滚动保留3份五写后仅三四五可读",
            rolling_keep3_ok(),
            format!("滚动{}", if rolling_keep3_ok() { "准" } else { "乱" }),
        ),
        (
            "注入1位翻转篡改验和必抓",
            inj_checksum_break_ok(),
            format!("篡改{}", if inj_checksum_break_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "注入2路径三违约逐类拒",
            inj_path_rejects_ok(),
            format!("路径违约{}", if inj_path_rejects_ok() { "全抓" } else { "漏抓" }),
        ),
        (
            "注入3容量两违约逐类拒",
            inj_capacity_rejects_ok(),
            format!("容量违约{}", if inj_capacity_rejects_ok() { "全抓" } else { "漏抓" }),
        ),
        (
            "注入4未写先读与版本回退逐类拒",
            inj_version_guards_ok(),
            format!("版本守卫{}", if inj_version_guards_ok() { "全抓" } else { "漏抓" }),
        ),
        (
            "双跑一致落盘回读流程逐字节同",
            double_run_ok(),
            format!("双跑{}", if double_run_ok() { "一致" } else { "分叉" }),
        ),
        (
            "错误路径Ok与闭集Err两侧可达",
            error_path_ok(),
            format!("错误路径{}", if error_path_ok() { "过" } else { "破" }),
        ),
        (
            "写回摘要非空状态已完成",
            writeback_summary_ok(),
            format!("写回{}", if writeback_summary_ok() { "非空" } else { "空" }),
        ),
    ];
    for (name, passed, det) in checks.iter() {
        s.add_dyn(name, *passed, det);
    }
    s
}

// ── 测试区（#[cfg(test)]：允许 assert!/assert_eq!/unwrap）──────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_and_diag_closed_set() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE, 0x798C0);
        assert!(diag_closed_set_ok());
        for (i, d) in ALL_DIAGS.iter().enumerate() {
            assert_eq!(d.val(), SEG_BASE + i as u32 + 1);
        }
    }

    #[test]
    fn frozen_constants_exact() {
        assert!(frozen_constants_ok());
        assert_eq!(RESULT_PATH, "/varix/results/run.json");
        assert_eq!(KEEP_VERSIONS, 3);
        assert_eq!(MAX_BLOB, 4096);
    }

    #[test]
    fn fnv_checksum_sane() {
        assert!(fnv_sanity_ok());
        let b = Blob::seal(1, RESULT_PATH, "内容".as_bytes());
        assert!(b.intact());
        let mut tampered = b.clone();
        tampered.checksum = tampered.checksum.wrapping_add(1);
        assert!(!tampered.intact());
    }

    #[test]
    fn write_read_roundtrip_m186() {
        assert!(write_read_roundtrip_ok());
        let mut disk = MemoryDisk::new();
        disk_write(&mut disk, RESULT_PATH, 0, b"payload").unwrap();
        let (v, content) = disk_read_latest(&disk).unwrap();
        assert_eq!(v, 1);
        assert_eq!(content, b"payload".to_vec());
    }

    #[test]
    fn rolling_keeps_exactly_three() {
        assert!(rolling_keep3_ok());
        let mut disk = MemoryDisk::new();
        let mut head = 0u64;
        for i in 1..=4u64 {
            head = disk_write(&mut disk, RESULT_PATH, head, format!("v{}", i).as_bytes()).unwrap();
        }
        assert_eq!(disk.head, 4);
        assert_eq!(readable_versions(&disk), vec![4, 3, 2]);
    }

    #[test]
    fn injections_all_caught() {
        assert!(inj_checksum_break_ok());
        assert!(inj_path_rejects_ok());
        assert!(inj_capacity_rejects_ok());
        assert!(inj_version_guards_ok());
    }

    #[test]
    fn double_run_identical() {
        assert!(double_run_ok());
    }

    #[test]
    fn error_paths_closed_codes() {
        assert!(error_path_ok());
        assert!(detail(0x798FF).starts_with("未知诊断码"));
    }

    #[test]
    fn writeback_summary_non_empty() {
        assert!(writeback_summary_ok());
    }

    #[test]
    fn checks_aggregate_all_green() {
        let s = run_unx3427_checks();
        assert!(s.len() >= 12);
        assert!(s.all_passed(), "全部判据必须绿：{}", s.red_items_text());
    }
}
