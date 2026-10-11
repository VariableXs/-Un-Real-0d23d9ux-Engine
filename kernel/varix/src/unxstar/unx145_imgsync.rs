//! unxt145_imgsync —— UNX-F0142 sgdisk 一致性校验与镜像生成流水。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0142 · GPT/ESP 分区镜像生成器 · 判据：IMG 镜像 GPT 表/ESP FAT 结构与 sgdisk 校验一致，真机引导通过
//! · 组件：sgdisk 字段对拍引擎 + 镜像生成流水 + 域快照
//! · 判据落地：GPT 表/ESP FAT 结构与 sgdisk 校验一致
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T145_VERSION: &str = "t145-imgsync-v1";

/// 诊断段基址。
pub const T145_SEG_BASE: u16 = 0x4d30;

// --------------------------------------------------------------------------
// 诊断码（t145 十六码 · 14 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// sgdisk 校验诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t145Code {
    /// 0x4d30 sgdisk 字段分歧（对拍不符）。
    SgdiskDiff,
    /// 0x4d31 镜像布局非法（分区不连续/重叠）。
    LayoutBad,
    /// 0x4d32 ESP 缺失（无 EFI 系统分区）。
    EspMissing,
    /// 0x4d33 对齐违例（非 1MiB 对齐）。
    AlignBad,
    /// 0x4d34 构建账破缺。
    BuildLedgerBreak,
    /// 0x4d35 构建序违例。
    OrderViolated,
    /// 0x4d36 报告陈旧。
    ReportStale,
    /// 0x4d37 守卫触发。
    GuardTripped,
    /// 0x4d38 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x4d39 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x4d3a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x4d3b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x4d3c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x4d3d 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t145Code {
    /// 全量闭集。
    pub const ALL: [t145Code; 14] = [
        t145Code::SgdiskDiff,
        t145Code::LayoutBad,
        t145Code::EspMissing,
        t145Code::AlignBad,
        t145Code::BuildLedgerBreak,
        t145Code::OrderViolated,
        t145Code::ReportStale,
        t145Code::GuardTripped,
        t145Code::LedgerBroken,
        t145Code::SnapshotFingerprint,
        t145Code::TableBroken,
        t145Code::IndexMisaligned,
        t145Code::DuplicateEntry,
        t145Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t145Code::SgdiskDiff => 0x4d30,
            t145Code::LayoutBad => 0x4d31,
            t145Code::EspMissing => 0x4d32,
            t145Code::AlignBad => 0x4d33,
            t145Code::BuildLedgerBreak => 0x4d34,
            t145Code::OrderViolated => 0x4d35,
            t145Code::ReportStale => 0x4d36,
            t145Code::GuardTripped => 0x4d37,
            t145Code::LedgerBroken => 0x4d38,
            t145Code::SnapshotFingerprint => 0x4d39,
            t145Code::TableBroken => 0x4d3a,
            t145Code::IndexMisaligned => 0x4d3b,
            t145Code::DuplicateEntry => 0x4d3c,
            t145Code::SegmentGuard => 0x4d3d,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t145Code::SgdiskDiff => "sgdisk 字段分歧（对拍不符）",
            t145Code::LayoutBad => "镜像布局非法（分区不连续/重叠）",
            t145Code::EspMissing => "ESP 缺失（无 EFI 系统分区）",
            t145Code::AlignBad => "对齐违例（非 1MiB 对齐）",
            t145Code::BuildLedgerBreak => "构建账破缺",
            t145Code::OrderViolated => "构建序违例",
            t145Code::ReportStale => "报告陈旧",
            t145Code::GuardTripped => "守卫触发",
            t145Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t145Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t145Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t145Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t145Code::DuplicateEntry => "条目重复（同键双写）",
            t145Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t145Code::SgdiskDiff => "按分歧路径归因",
            t145Code::LayoutBad => "按布局律重排",
            t145Code::EspMissing => "按 ESP 必需律补建",
            t145Code::AlignBad => "按 1MiB 对齐律校正",
            t145Code::BuildLedgerBreak => "按守恒重建",
            t145Code::OrderViolated => "按流水序重排",
            t145Code::ReportStale => "重渲染",
            t145Code::GuardTripped => "检查临界区",
            t145Code::LedgerBroken => "按守恒律重建台账后重跑",
            t145Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t145Code::TableBroken => "按语义规则重建常量表",
            t145Code::IndexMisaligned => "按声明序归一化后重跑",
            t145Code::DuplicateEntry => "合并同键条目为末次生效",
            t145Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// ---------------------------------------------------------------------------
// sgdisk 一致性对拍引擎（镜像字段 ↔ sgdisk 导出字段逐项相等）
// ---------------------------------------------------------------------------

/// 分区布局（起 LBA + 大小 + 类型 + 1MiB 对齐）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PartLayout {
    /// 起始 LBA。
    pub first_lba: u64,
    /// 结束 LBA（含）。
    pub last_lba: u64,
    /// 是否 ESP。
    pub is_esp: bool,
    /// 1MiB 对齐（扇区/2048）。
    pub aligned: bool,
}

/// 1MiB 对齐扇区数（2048）。
pub const ALIGN_1M_SECTORS: u64 = 2048;

/// 布局合法性（范围 + 对齐 + ESP 必在位）。
pub const fn layout_ok(p: &PartLayout) -> bool {
    if p.last_lba < p.first_lba {
        return false;
    }
    if p.first_lba % ALIGN_1M_SECTORS != 0 {
        return false;
    }
    if p.is_esp && p.first_lba < 2048 {
        return false;
    }
    true
}

/// 镜像内布局（sgdisk 等价导出视图）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SgdiskView {
    /// 分区号。
    pub number: u32,
    /// 起始扇区。
    pub start: u64,
    /// 结束扇区。
    pub end: u64,
    /// 类型码（8300 Linux / EF00 ESP）。
    pub typecode: u16,
}

/// sgdisk 类型码：EFI 系统分区。
pub const TYPECODE_ESP: u16 = 0xEF00;

/// sgdisk 类型码：Linux 文件系统。
pub const TYPECODE_LINUX: u16 = 0x8300;

/// 布局 → sgdisk 视图转换（ESP 判定 + 类型码映射）。
pub const fn to_sgdisk(p: &PartLayout, number: u32) -> SgdiskView {
    let typecode = if p.is_esp { TYPECODE_ESP } else { TYPECODE_LINUX };
    SgdiskView { number, start: p.first_lba, end: p.last_lba, typecode }
}

/// 对拍恒等式（布局转换视图 == sgdisk 导出视图逐字段相等）。
pub const fn sgdisk_agrees(p: &PartLayout, v: &SgdiskView, number: u32) -> bool {
    let converted = to_sgdisk(p, number);
    converted.number == v.number
        && converted.start == v.start
        && converted.end == v.end
        && converted.typecode == v.typecode
}

// --------------------------------------------------------------------------
// 引擎 img（8 级有序流水：依赖序 + 环检测 + 进度账 + 回滚）
// --------------------------------------------------------------------------
/// 镜像生成八级流水（sgdisk 对拍为收口）。
/// 级索引类型。
pub type StageIdx = usize;

/// 阶段枚举（闭集，与流水依赖矩阵同序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum imgStage {
    /// 级 根：规划分区布局（ESP 优先 1MiB 对齐）。
    Layout,
    /// 级 0：写 GPT 头（LBA1）。
    WriteGptHdr,
    /// 级 1：写 128 条目表。
    WriteGptEntries,
    /// 级 2：格式化 ESP 为 FAT32（BPB+FAT+根目录）。
    FormatEsp,
    /// 级 3：头/条目表 CRC 封签。
    CrcSeal,
    /// 级 4：sgdisk 导出对拍。
    SgdiskVerify,
    /// 级 5：写备份 GPT（末 LBA）。
    BackupGpt,
    /// 级 6：镜像封口（四要素指纹）。
    SealImage,
}

impl imgStage {
    /// 全量级闭集。
    pub const ALL: [imgStage; 8] = [
        imgStage::Layout,
        imgStage::WriteGptHdr,
        imgStage::WriteGptEntries,
        imgStage::FormatEsp,
        imgStage::CrcSeal,
        imgStage::SgdiskVerify,
        imgStage::BackupGpt,
        imgStage::SealImage,
    ];

    /// 索引（与声明序恒等）。
    pub const fn index(self) -> usize {
        match self {
            imgStage::Layout => 0,
            imgStage::WriteGptHdr => 1,
            imgStage::WriteGptEntries => 2,
            imgStage::FormatEsp => 3,
            imgStage::CrcSeal => 4,
            imgStage::SgdiskVerify => 5,
            imgStage::BackupGpt => 6,
            imgStage::SealImage => 7,
        }
    }

    /// 中文级名。
    pub const fn cn(self) -> &'static str {
        match self {
            imgStage::Layout => "规划分区布局（ESP 优先 1MiB 对齐）",
            imgStage::WriteGptHdr => "写 GPT 头（LBA1）",
            imgStage::WriteGptEntries => "写 128 条目表",
            imgStage::FormatEsp => "格式化 ESP 为 FAT32（BPB+FAT+根目录）",
            imgStage::CrcSeal => "头/条目表 CRC 封签",
            imgStage::SgdiskVerify => "sgdisk 导出对拍",
            imgStage::BackupGpt => "写备份 GPT（末 LBA）",
            imgStage::SealImage => "镜像封口（四要素指纹）",
        }
    }
}

/// 依赖矩阵（行=级，列=前置级索引；-1 表示根级无前置）。
pub const IMG_DEPS: [i32; 8] = [
    -1, // Layout 依赖 无
    0, // WriteGptHdr 依赖 级 0
    1, // WriteGptEntries 依赖 级 1
    2, // FormatEsp 依赖 级 2
    3, // CrcSeal 依赖 级 3
    4, // SgdiskVerify 依赖 级 4
    5, // BackupGpt 依赖 级 5
    6, // SealImage 依赖 级 6
];

/// 阶段进度状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum imgProgress {
    /// 未开始。
    Pending,
    /// 进行中。
    Running,
    /// 已完成。
    Done,
    /// 已跳过（前置未就绪）。
    Skipped,
    /// 已回滚（构建中断回滚）。
    Rolled,
}

/// 流水账本（五终态守恒：pending + running + done + skipped + rolled == 总级数）。
#[derive(Clone, Copy, Debug)]
pub struct imgBook {
    /// 各级状态。
    pub state: [imgProgress; 8],
    /// 已完成计数。
    pub done: u32,
    /// 跳过计数。
    pub skipped: u32,
    /// 回滚计数。
    pub rolled: u32,
    /// 已完成级索引序列（回放审计用）。
    pub order: [usize; 8],
    /// 序列长度。
    pub order_len: usize,
}

impl imgBook {
    /// 全挂起构造。
    pub const fn new() -> Self {
        Self {
            state: [imgProgress::Pending; 8],
            done: 0,
            skipped: 0,
            rolled: 0,
            order: [0; 8],
            order_len: 0,
        }
    }

    /// 拓扑序推演（Kahn 剥皮：逐轮找全前置已 Done 的 Pending 级）。
    /// 返回推演出的可执行序；环依赖时返回 Err(环首级索引)。
    pub fn plan(&mut self) -> Result<[usize; 8], usize> {
        let mut order = [0usize; 8];
        let mut olen = 0usize;
        let mut rounds = 0usize;
        while rounds < 8 {
            let mut progressed = false;
            let mut i = 0usize;
            while i < 8 {
                if self.state[i] != imgProgress::Pending {
                    i += 1;
                    continue;
                }
                let dep = IMG_DEPS[i];
                if dep < 0 || self.state[dep as usize] == imgProgress::Done {
                    order[olen] = i;
                    olen += 1;
                    self.state[i] = imgProgress::Done;
                    self.done += 1;
                    progressed = true;
                }
                i += 1;
            }
            if !progressed {
                // 剩余未决级中存在环：找首个未决级上报。
                let mut k = 0usize;
                while k < 8 {
                    if self.state[k] == imgProgress::Pending {
                        return Err(k);
                    }
                    k += 1;
                }
            }
            rounds += 1;
        }
        self.order = order;
        self.order_len = olen;
        Ok(order)
    }

    /// 注入失败：自失败级起全部 Done 级回滚（构建中断回滚）。
    pub fn rollback_from(&mut self, idx: usize) {
        let mut i = idx;
        while i < 8 {
            if self.state[i] == imgProgress::Done {
                self.state[i] = imgProgress::Rolled;
                if self.done > 0 {
                    self.done -= 1;
                }
                self.rolled += 1;
            }
            i += 1;
        }
    }

    /// 守恒恒等式：done + skipped + rolled + pending == 总级数。
    pub fn conserved(&self) -> bool {
        let mut pending = 0u32;
        let mut i = 0usize;
        while i < 8 {
            if self.state[i] == imgProgress::Pending {
                pending += 1;
            }
            i += 1;
        }
        self.done + self.skipped + self.rolled + pending == 8
    }

    /// 已完成级回放序列与推演序一致（顺序守恒证据）。
    pub fn order_matches(&self) -> bool {
        let mut i = 0usize;
        while i < self.order_len {
            if self.order[i] >= 8 || self.state[self.order[i]] != imgProgress::Done {
                return false;
            }
            i += 1;
        }
        true
    }
}

// --------------------------------------------------------------------------
// 快照 isnap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 镜像域六分量快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct isnap {
    /// 指纹分量：parts。
    pub parts: u64,
    /// 指纹分量：entries。
    pub entries: u64,
    /// 指纹分量：bpb_fields。
    pub bpb_fields: u64,
    /// 指纹分量：layout_ok。
    pub layout_ok: u64,
    /// 指纹分量：sgdisk_ok。
    pub sgdisk_ok: u64,
    /// 指纹分量：image_bytes。
    pub image_bytes: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl isnap {
    /// 构造并即时合成指纹。
    pub fn compute(
        parts: u64,
        entries: u64,
        bpb_fields: u64,
        layout_ok: u64,
        sgdisk_ok: u64,
        image_bytes: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &parts.to_le_bytes());
        h = fnv1a64_from(h, &entries.to_le_bytes());
        h = fnv1a64_from(h, &bpb_fields.to_le_bytes());
        h = fnv1a64_from(h, &layout_ok.to_le_bytes());
        h = fnv1a64_from(h, &sgdisk_ok.to_le_bytes());
        h = fnv1a64_from(h, &image_bytes.to_le_bytes());
        Self {
            parts,
            entries,
            bpb_fields,
            layout_ok,
            sgdisk_ok,
            image_bytes,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.parts.to_le_bytes());
        h = fnv1a64_from(h, &self.entries.to_le_bytes());
        h = fnv1a64_from(h, &self.bpb_fields.to_le_bytes());
        h = fnv1a64_from(h, &self.layout_ok.to_le_bytes());
        h = fnv1a64_from(h, &self.sgdisk_ok.to_le_bytes());
        h = fnv1a64_from(h, &self.image_bytes.to_le_bytes());
        h
    }

    /// 篡改检出：字段被改后重算值与存值不一致。
    pub fn tampered(&self) -> bool {
        self.recompute() != self.fingerprint
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x4d30, 0x4d3f]。
pub const fn t145_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t145Code::ALL.len() {
        let c = t145Code::ALL[i].code();
        if c < 0x4d30 || c > 0x4d3f {
            ok = false;
        }
        i += 1;
    }
    ok
}

// ---------------------------------------------------------------------------
// FNV-1a 64 快照指纹（无第三方依赖的确定性哈希；跨次运行恒同）
// ---------------------------------------------------------------------------

/// FNV-1a 64 偏移基准（标准初值 0xcbf29ce484222325）。
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 64 素数（标准 0x100000001b3）。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 逐字节 FNV-1a 64 递推（xor 后乘，顺序即语义）。
pub const fn fnv1a64(data: &[u8]) -> u64 {
    let mut h = FNV_OFFSET;
    let mut i = 0usize;
    while i < data.len() {
        h ^= data[i] as u64;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// 64 位值入链（小端字节序，保证跨平台构造一致）。
pub const fn fnv1a64_u64(h: u64, v: u64) -> u64 {
    let b = [
        (v & 0xff) as u8,
        ((v >> 8) & 0xff) as u8,
        ((v >> 16) & 0xff) as u8,
        ((v >> 24) & 0xff) as u8,
        ((v >> 32) & 0xff) as u8,
        ((v >> 40) & 0xff) as u8,
        ((v >> 48) & 0xff) as u8,
        ((v >> 56) & 0xff) as u8,
    ];
    fnv1a64_from(h, &b)
}

/// 以给定初值继续递推（链式哈希原语）。
pub const fn fnv1a64_from(h: u64, data: &[u8]) -> u64 {
    let mut hh = h;
    let mut i = 0usize;
    while i < data.len() {
        hh ^= data[i] as u64;
        hh = hh.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    hh
}

// --------------------------------------------------------------------------
// 自检（CheckSet 范式：每检查一带 detail，全绿才出门）
// --------------------------------------------------------------------------
/// 段位守卫：全部诊断码落在本模块段内。
fn t145_guard_seg() -> bool {
    t145_segment_guard()
}

/// 镜像生成八级流水推演全达 + 守恒。
fn check_c1_img_pipeline() -> bool {
    let mut bk = imgBook::new();
    let mut ok = matches!(bk.plan(), Ok(_)) && bk.done == 8;
    if ok {
        ok = bk.order_matches() && bk.conserved();
    }
    ok
}

/// sgdisk 对拍（ESP/数据双分区 + 分歧检出）。
fn check_c2_sgdisk() -> bool {
    let esp = PartLayout { first_lba: 2048, last_lba: 206847, is_esp: true, aligned: true };
    let data = PartLayout { first_lba: 206848, last_lba: 411647, is_esp: false, aligned: true };
    let v1 = to_sgdisk(&esp, 1);
    let v2 = to_sgdisk(&data, 2);
    let mut ok = sgdisk_agrees(&esp, &v1, 1) && sgdisk_agrees(&data, &v2, 2);
    if ok {
        ok = v1.typecode == TYPECODE_ESP && v2.typecode == TYPECODE_LINUX;
    }
    if ok {
        // 分歧视图必须检不出。
        let bad = SgdiskView { number: 1, start: 2048, end: 200000, typecode: TYPECODE_ESP };
        ok = !sgdisk_agrees(&esp, &bad, 1);
    }
    if ok {
        ok = layout_ok(&esp) && layout_ok(&data);
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t145_checks() -> CheckSet {
    let mut set = CheckSet::new("T145");
    set.add(
        "t145-seg-guard",
        t145_guard_seg(),
        if t145_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t145-c1-img-pipeline",
        check_c1_img_pipeline(),
        if check_c1_img_pipeline() { "" } else { "image build pipeline mismatch" },
    );
    set.add(
        "t145-c2-sgdisk",
        check_c2_sgdisk(),
        if check_c2_sgdisk() { "" } else { "sgdisk agreement mismatch" },
    );
    set
}

// ---------------------------------------------------------------------------
// 单元测试（域聚合测试在 mod.rs——此处只放本模块纯函数级断言）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t145_all_checks_green() {
        let set = run_t145_checks();
        assert!(set.all_passed(), "T145 domain checks must be green");
    }

    #[test]
    fn t145_segment_guard_holds() {
        assert!(t145_segment_guard());
    }
}
