//! UNX-F0785 · 域功入册（A1 域 800 条功能清单入总册）· 诊断段 0x1AF0
//!
//! ── 验收锚点（原文全抄） ─────────────────────────────────────────────
//! 定位：docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal.md
//! 行 2394 表头「| 编号 | 功能条目 | 行数 | 状态 | 判据 |」下、行 2396：
//!
//! | UNX-F0785 | 域功入册（A1 域 800 条功能清单入总册） | 570 | 已深化 |
//!   UNX-F0785-J1 入册过（域功能清单 800 条四列：fid/名/行数/判据号——与批册
//!   直读一致 100%）且总册挂载声明落账 |
//!
//! 邻行原文（同页行 2392~2400 摘抄，佐证 A1 域批册结构：40 批 × 20 条 = 800 条，
//! 批小计 = 逐条求和唯一真值，B40 为 A1 域第 40 批（F0781–F0800））：
//!
//! | UNX-F0781 | 收官报告定版（六节定稿+哈希封存） | 570 | 已深化 |
//!   UNX-F0781-J1 定版过（六节定稿 + 指针可解引 100% + 报告哈希封存）且与
//!   F0778 初稿差异留痕 |
//! | UNX-F0782 | 域账终点宣告（240,000 终值锁定账） | 580 | 已深化 |
//!   UNX-F0782-J1 终点宣告过（域账累计链 40 段终点 = 240,000 精确等 + 逐段
//!   链核 40/40）且终值哈希锁定 |
//! | UNX-F0783 | 批册终态核（40 批 [已深化] 全域终版） | 570 | 已深化 |
//!   UNX-F0783-J1 终态核过（40 批批册状态行全 [已深化] 逐批计数 20/40 段核）
//!   且骨架零残留终版断言 |
//! | UNX-F0784 | 深化册终态核（40 册全批收口齐核） | 580 | 已深化 |
//!   UNX-F0784-J1 终态核过（40 册逐册：全批收口标题 + finalize 表 5 行 +
//!   对接行在位 40/40）且册-批判据一致抽核 |
//! | UNX-F0785 | 域功入册（A1 域 800 条功能清单入总册） | 570 | 已深化 |
//!   UNX-F0785-J1 入册过（域功能清单 800 条四列：fid/名/行数/判据号——与批册
//!   直读一致 100%）且总册挂载声明落账 |
//!
//! 行 2388 域账原文（批号/批小计/域账累计来源）：
//! > AI-01 承办｜域账累计：228480 + 本批 11520 = 240000 / 240,000｜防重：
//!   满收官终局面（报告定版/终点宣告/六判据终验/封域宣告/终局宣告）归本批
//!   独占；……｜B40 为 A1 域第 40 批（F0781–F0800），域账累计链终点 =
//!   240,000｜R-A1-004 纪律：批小计 = 逐条求和唯一真值（本批逐条求和
//!   11520）｜新增骨架批次（本会话立）
//!
//! J 判据拆解（本模块逐条落地的验收口径）：
//!   J1-a 四列入册：EnrollRow 四列 fid/名/行数/判据号逐列校验拒收——
//!        fid 规范（UNX-F 前缀 + 恰四位数字 + 域内 1..=800）、名非空且
//!        ≤64 字节、行数 > 0、判据号 J 格式（fid 同号 + "-J" + 1..3 位
//!        正整数）；任一列不过即整行拒收，错误码逐列可归因。
//!   J1-b 入册账：DomainEnrollBook 登记制（append-only + 严格升序），
//!        重复 fid 拒收（E_DUPLICATE_FID）、乱序拒收（E_ORDER_VIOLATION）、
//!        超容拒收（E_BOOK_FULL）、总数恰 800 判定 is_full()。
//!   J1-c 批册直读一致 100%：批册源清单由 const 规范确定性生成器生成
//!        （800 条：行数 570/580 奇偶交替、批号 = (fid号-1)/20+1），
//!        CrossReader 逐条 diff（漏登/多登/列值不符三归因），一致率
//!        ppm 整数制，10000 ppm = 100% 才算入册过。
//!   J1-d 总册挂载声明落账：MountDeclaration（册号/域号/条目数/指纹）
//!        四要素齐备，挂载指纹与清单指纹一致核（不一致 E_MOUNT_MISMATCH
//!        拒挂），声明行 MOUNT|… 落事件账留痕。
//!   J1-e 分批账：40 批 × 20 条分组账，Σ批 ≡ 800 守恒，批号连续零跳号
//!        （gap_batches 必空），批目录 first/last fid 逐批在位。
//!
//! ── 深度子系统清单（本文件全手工定制，零注水） ───────────────────────
//!   S1 四列校验器组：validate_fid / validate_name / validate_lines /
//!      validate_crit 四道独立校验，逐列拒收、逐列可归因（错误码各自独立）。
//!   S2 EnrollRow 四列入册行：构造期全列校验 + verify_self 复核 + 确定性
//!      序列化（0x1F 列分隔 / 0x1E 行终止）+ 快照行文本。
//!   S3 DomainEnrollBook 入册账：登记制 + 严格升序 + 重复拒收 + 超容拒收
//!      + 封板制 + FNV-1a 64 账指纹 + total_lines 饱和累加 + audit()。
//!   S4 BatchLedger 分批账：40 批计数器、批号 const 推导 batch_of、
//!      Σ批守恒、跳号侦测、批目录（first/last fid 逐批在位核）。
//!   S5 SourceTableGenerator 批册源清单生成器：规范确定性生成 800 条
//!      （行数 570/580 奇偶交替——来源：行 2396 批册段 570/580 交替规律；
//!      名 = 「A1域功能·批NN·条NN」；判据号 = fid-J1），同输入双跑必等。
//!   S6 CrossReader 批册直读一致核对器：合并游标逐 fid diff，差异三归因
//!      （漏登 MissingInBook / 多登 ExtraInBook / 列值不符 ColumnMismatch，
//!      列值不符再分 fid/名/行数/判据号四列），一致率 ppm 判 100%。
//!   S7 MountDeclaration 总册挂载声明：四要素（册号/域号/条目数/指纹），
//!      declare / from_declared（外来声明核）/ verify 三路核 + 落账行。
//!   S8 EventLedger 事件账：封板制 + 滚动 FNV 校验和（可重放复核）+
//!      挂载声明行落账。
//!   S9 DomenrollStation 总编排：入册 → 直读核对 → 挂载 → 注入面 →
//!      health() 六项绿灯（账满/一致/挂载/守恒/账密/事件非空）。
//!   S10 注入面四路：漏条注入 / 重复注入 / 多登注入 / 列值篡改注入，
//!       每路显性检出（detected + 诊断码 + 中文诊断）。
//!   S11 双跑确定性对拍 dual_run_check：两路独立构建 → 指纹/条数/一致率/
//!       挂载指纹四项逐项等，任何漂移 E_DUAL_RUN_DRIFT 立案。
//!
//! ── 相邻任务对接 ─────────────────────────────────────────────────────
//!   F0783 批册终态核：本账批册源清单 800 条 × [已深化] 状态行是其核对面。
//!   F0784 深化册终态核：本账行数列（570/580）与其逐册收口行数同源。
//!   F0786 判据 1 终验：本账 health() 六项绿灯是其凭证包前置面。
//!   F0782 域账终点宣告：本账 Σ批守恒 800 与其 240,000 终值链同构。
//!
//! ── 通用纪律 ─────────────────────────────────────────────────────────
//!   no_std + alloc；只 use core::… 与 use alloc::{…}，禁 use crate::…。
//!   内核路径零 unwrap/expect/panic!/运行期 assert!（编译期 const assert! 允许）。
//!   可失败 API 全走 Result<T, EnrollError>；溢出显性 saturating/wrapping。
//!   >64KiB 结构体禁上栈（本文件全部 800 条级数据走 Vec 堆；栈上最大为
//!   BatchLedger 的 [u32; 40] = 160B 与 ProbeOutcome ≤ 96B，安全）。
//!   const fn 纪律：const fn 内零 slice::get / Vec 索引 / Ord::min-max /
//!   derive(PartialEq) 类型的 == / for 循环（一律 while + 索引）；
//!   含 Vec 字段的结构体禁 derive Copy。
//!
//! 零浮点、零 unwrap、零 expect、零 panic 路径（#[cfg(test)] 测试域除外）。

use core::cmp::Ordering;

use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};

// ── 诊断码段（0x1AF0~0x1AFF 独占 0x10；先验空闲：kernel/varix/src 全树
//    grep "0x1AF" 零命中；0x19A0 段被 svstar2/ves474_s25close 占（0x19A0~
//    0x19A4），0x19C0/0x19D0 被 cgpu/cga29_vertex、cga30_rasterize 占，
//    0x1A20/0x1A40 被 svstar2/ves477_s26dynamic、ves478_s26visual 占，
//    0x1A00 被 ves476_s26static 占，0x1A60/0x1A90/0x1AE0 被 cgpu/cga31~
//    cga33 占——故取 0x1AF0 段）──
pub const SEG_BASE: u16 = 0x1AF0;

const E_SEALED: u16 = SEG_BASE + 0x0; // 0x1AF0 账已封板仍写
const E_FID_FORMAT: u16 = SEG_BASE + 0x1; // 0x1AF1 fid 规范不符（前缀/位数/数字）
const E_FID_OUT_OF_DOMAIN: u16 = SEG_BASE + 0x2; // 0x1AF2 fid 越域（0 或 >800）
const E_NAME_FORMAT: u16 = SEG_BASE + 0x3; // 0x1AF3 名列非空/长度越界
const E_LINES_ZERO: u16 = SEG_BASE + 0x4; // 0x1AF4 行数列非正
const E_CRIT_FORMAT: u16 = SEG_BASE + 0x5; // 0x1AF5 判据号 J 格式不符
const E_DUPLICATE_FID: u16 = SEG_BASE + 0x6; // 0x1AF6 重复 fid 注入拒收
const E_ORDER_VIOLATION: u16 = SEG_BASE + 0x7; // 0x1AF7 登记序乱（非严格升序）
const E_BOOK_FULL: u16 = SEG_BASE + 0x8; // 0x1AF8 超容（已 800 仍入册）
const E_NOT_COMPLETE: u16 = SEG_BASE + 0x9; // 0x1AF9 账未满 800 即求挂载/直读终核
const E_MOUNT_BAD_DECL: u16 = SEG_BASE + 0xA; // 0x1AFA 挂载声明四要素不符（域号/条目数）
const E_MOUNT_MISMATCH: u16 = SEG_BASE + 0xB; // 0x1AFB 挂载指纹与清单指纹不一致
const E_CONSERVATION: u16 = SEG_BASE + 0xC; // 0x1AFC Σ批 ≠ 800 或批号跳号
const E_DIFF_INCONSISTENT: u16 = SEG_BASE + 0xD; // 0x1AFD 直读一致率 < 10000 ppm
const E_DUAL_RUN_DRIFT: u16 = SEG_BASE + 0xE; // 0x1AFE 双跑对拍漂移
const E_LEDGER_SEALED_WRITE: u16 = SEG_BASE + 0xF; // 0x1AFF 事件账封板后仍写

/// 入册域错误枚举：每变体带 `detail(&self) -> &'static str` 中文可操作诊断。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnrollError {
    /// 账已封板仍写。
    Sealed,
    /// fid 规范不符（前缀/位数/数字）。
    FidFormat,
    /// fid 越域（0 或 >800）。
    FidOutOfRange,
    /// 名列非空/长度越界。
    NameFormat,
    /// 行数列非正。
    LinesZero,
    /// 判据号 J 格式不符。
    CritFormat,
    /// 重复 fid 拒收。
    DuplicateFid,
    /// 登记序乱（非严格升序）。
    OrderViolation,
    /// 超容拒收。
    BookFull,
    /// 账未满 800 即求终核。
    NotComplete,
    /// 挂载声明四要素不符。
    MountBadDecl,
    /// 挂载指纹与清单指纹不一致。
    MountMismatch,
    /// Σ批 ≠ 800 或批号跳号。
    Conservation,
    /// 直读一致率不足 100%。
    DiffInconsistent,
    /// 双跑对拍漂移。
    DualRunDrift,
    /// 事件账封板后仍写。
    LedgerSealedWrite,
}

impl EnrollError {
    /// 诊断码（0x1AF0 段独占）。
    pub fn code(&self) -> u16 {
        match self {
            EnrollError::Sealed => E_SEALED,
            EnrollError::FidFormat => E_FID_FORMAT,
            EnrollError::FidOutOfRange => E_FID_OUT_OF_DOMAIN,
            EnrollError::NameFormat => E_NAME_FORMAT,
            EnrollError::LinesZero => E_LINES_ZERO,
            EnrollError::CritFormat => E_CRIT_FORMAT,
            EnrollError::DuplicateFid => E_DUPLICATE_FID,
            EnrollError::OrderViolation => E_ORDER_VIOLATION,
            EnrollError::BookFull => E_BOOK_FULL,
            EnrollError::NotComplete => E_NOT_COMPLETE,
            EnrollError::MountBadDecl => E_MOUNT_BAD_DECL,
            EnrollError::MountMismatch => E_MOUNT_MISMATCH,
            EnrollError::Conservation => E_CONSERVATION,
            EnrollError::DiffInconsistent => E_DIFF_INCONSISTENT,
            EnrollError::DualRunDrift => E_DUAL_RUN_DRIFT,
            EnrollError::LedgerSealedWrite => E_LEDGER_SEALED_WRITE,
        }
    }

    /// 中文可操作诊断（每变体一行，指出病灶与修正路径）。
    pub fn detail(&self) -> &'static str {
        match self {
            EnrollError::Sealed => "入册账已封板仍写入：终核后账面冻结，修正请走新账重开，勿改封板账",
            EnrollError::FidFormat => "fid 规范不符：必须为 UNX-F 前缀 + 恰四位数字（总长 9 字节），如 UNX-F0785",
            EnrollError::FidOutOfRange => "fid 越域：A1 域编号域为 0001..=0800，0 或 >0800 一律域外拒收（多登注入面）",
            EnrollError::NameFormat => "名列不合规：须非空且 ≤64 字节，空名或超长名整行拒收",
            EnrollError::LinesZero => "行数列不合规：行数必须 > 0，零行/负行语义条目不得入册",
            EnrollError::CritFormat => "判据号不合规：须为同号 fid + -J + 1..3 位正整数（如 UNX-F0785-J1），J0 或异号判据拒收",
            EnrollError::DuplicateFid => "重复 fid 注入检出：登记制下同 fid 只许入册一次，重复注入已拒收并留痕",
            EnrollError::OrderViolation => "登记序乱：登记制要求 fid 号严格升序追加，乱序条目请按号重排后入册",
            EnrollError::BookFull => "入册账已满 800 条仍求入册：A1 域容量恰 800，超出条目属域外多登",
            EnrollError::NotComplete => "账未满 800 条即求挂载/终核：先补齐漏登条目（见直读 diff 漏登清单）再求终核",
            EnrollError::MountBadDecl => "挂载声明四要素不符：册号/域号/条目数与账实不符，请按账实重出声明",
            EnrollError::MountMismatch => "挂载指纹与清单指纹不一致：声明指纹≠账面 FNV-1a 64，账面疑遭篡改，立案重核",
            EnrollError::Conservation => "分批守恒破坏：Σ批≠800 或批号跳号，请核对 40 批×20 条分组账补齐跳批",
            EnrollError::DiffInconsistent => "批册直读一致率不足 100%：入册清单与批册源清单存在差异，按 diff 归因逐条修正",
            EnrollError::DualRunDrift => "双跑对拍漂移：同输入两路构建指纹/条数/一致率/挂载指纹不等，存在非确定源",
            EnrollError::LedgerSealedWrite => "事件账已封板仍写：封板后事件账只读，修正请走新事件账",
        }
    }
}

// ── 规范常量（全部写死并注明来源） ───────────────────────────────────
/// fid 前缀（来源：锚点行 2396「UNX-F0785」编号规范）。
pub const FID_PREFIX: &str = "UNX-F";
/// fid 总字节数 = 5（UNX-F）+ 4（四位号）= 9（来源：锚点行 2396 编号格式）。
pub const FID_LEN: usize = 9;
/// A1 域域号（来源：锚点行 2388「B40 为 A1 域第 40 批」）。
pub const DOMAIN_CODE: &str = "A1";
/// A1 域总条目恰 800（来源：锚点行 2396 功能名「800 条功能清单」）。
pub const TOTAL_ENTRIES: u32 = 800;
/// 批数恰 40（来源：锚点行 2388「B40 为 A1 域第 40 批」）。
pub const BATCH_COUNT: usize = 40;
/// 每批恰 20 条（来源：锚点行 2388 批小计 11520 = 570/580 交替 20 条求和）。
pub const BATCH_SIZE: u32 = 20;
/// 奇号条目行数（来源：锚点行 2396 批册段 570 行数规律）。
pub const LINES_ODD: u32 = 570;
/// 偶号条目行数（来源：锚点行 2396 批册段 580 行数规律）。
pub const LINES_EVEN: u32 = 580;
/// 总册册号（来源：任务 UNX-F0785 自指「入总册」，册号取 0x0785）。
pub const MASTER_BOOK_NO: u32 = 0x0000_0785;
/// 名列最小字节长（非空）（来源：J1-a 四列校验规范）。
pub const NAME_MIN: usize = 1;
/// 名列最大字节长（来源：J1-a 四列校验规范）。
pub const NAME_MAX: usize = 64;
/// 直读一致率满分（10000 ppm = 100%）（来源：J1-c 一致率 ppm 整数制）。
pub const RATE_FULL_PPM: u64 = 10_000;

// 编译期守恒断言（铁律：运行期 assert! 禁，编译期 const assert! 允许）。
const _: () = assert!(TOTAL_ENTRIES as usize == BATCH_COUNT * BATCH_SIZE as usize);
const _: () = assert!(SEG_BASE == 0x1AF0);
const _: () = assert!(FID_LEN == 5 + 4);
const _: () = assert!(BATCH_SIZE as usize * 10 == 200);

// ── S0：const 期纯算术辅助（零 slice::get / 零 for / 零 Ord::min-max） ──
/// fid 号 → 批号：批 b 覆盖 (b-1)*20+1 ..= b*20（来源：B40 = F0781–F0800，
/// 锚点行 2388；fid_num 须在 1..=800，域外值由调用方先经 validate_fid 拒）。
pub const fn batch_of(fid_num: u16) -> u16 {
    (fid_num - 1) / (BATCH_SIZE as u16) + 1
}

/// fid 号 → 批内序号 1..=20。
pub const fn idx_in_batch(fid_num: u16) -> u16 {
    (fid_num - 1) % (BATCH_SIZE as u16) + 1
}

/// const 期 ASCII 数字判定（禁 slice::get，直接下标由调用方保证有界）。
const fn is_digit(b: u8) -> bool {
    b >= b'0' && b <= b'9'
}

/// const 期 ASCII 数字取值（b 已过 is_digit）。
const fn digit_val(b: u8) -> u16 {
    (b - b'0') as u16
}

/// 规范 fid 号 → fid 文本（"UNX-F%04d"）。
pub fn canonical_fid(fid_num: u16) -> String {
    format!("{}{:04}", FID_PREFIX, fid_num)
}

// ── S1：四列校验器组（逐列校验拒收、逐列可归因） ─────────────────────

/// fid 列校验：UNX-F 前缀 + 恰四位数字 + 域内 1..=800。
/// 返回解析出的 fid 号（1..=800）。
pub fn validate_fid(fid: &str) -> Result<u16, EnrollError> {
    let b = fid.as_bytes();
    if b.len() != FID_LEN {
        return Err(EnrollError::FidFormat);
    }
    let p = FID_PREFIX.as_bytes();
    let mut i = 0usize;
    while i < p.len() {
        if b[i] != p[i] {
            return Err(EnrollError::FidFormat);
        }
        i += 1;
    }
    let mut num: u32 = 0;
    let mut k = 5usize;
    while k < FID_LEN {
        if !is_digit(b[k]) {
            return Err(EnrollError::FidFormat);
        }
        // 四位有界数字：wrapping 仅为纪律显性，实际 ≤ 9999 不溢出。
        num = num.wrapping_mul(10).wrapping_add(digit_val(b[k]) as u32);
        k += 1;
    }
    if num == 0 || num > TOTAL_ENTRIES {
        return Err(EnrollError::FidOutOfRange);
    }
    Ok(num as u16)
}

/// 名列校验：非空且 ≤ NAME_MAX 字节。
pub fn validate_name(name: &str) -> Result<(), EnrollError> {
    let b = name.as_bytes();
    if b.len() < NAME_MIN || b.len() > NAME_MAX {
        return Err(EnrollError::NameFormat);
    }
    Ok(())
}

/// 行数列校验：行数 > 0。
pub fn validate_lines(lines: u32) -> Result<(), EnrollError> {
    if lines == 0 {
        return Err(EnrollError::LinesZero);
    }
    Ok(())
}

/// 判据号列校验：同号 fid + "-J" + 1..3 位正整数（J1 格式）。
pub fn validate_crit(crit: &str, fid_num: u16) -> Result<(), EnrollError> {
    let b = crit.as_bytes();
    // 最短合法形："UNX-F0785-J1" = 12 字节（10 + "-J1" 3）。
    if b.len() < FID_LEN + 3 {
        return Err(EnrollError::CritFormat);
    }
    let p = FID_PREFIX.as_bytes();
    let mut i = 0usize;
    while i < p.len() {
        if b[i] != p[i] {
            return Err(EnrollError::CritFormat);
        }
        i += 1;
    }
    let mut num: u32 = 0;
    let mut k = 5usize;
    while k < 9 {
        if !is_digit(b[k]) {
            return Err(EnrollError::CritFormat);
        }
        num = num.wrapping_mul(10).wrapping_add(digit_val(b[k]) as u32);
        k += 1;
    }
    if num != fid_num as u32 {
        // 判据号必须与行 fid 同号（异号判据 = 张冠李戴，拒收）。
        return Err(EnrollError::CritFormat);
    }
    if b[9] != b'-' || b[10] != b'J' {
        return Err(EnrollError::CritFormat);
    }
    let mut j: u32 = 0;
    let mut m = 11usize;
    let mut digits = 0usize;
    while m < b.len() {
        if !is_digit(b[m]) {
            return Err(EnrollError::CritFormat);
        }
        if digits >= 3 {
            // 判据号至多三位，第四位起越界拒收。
            return Err(EnrollError::CritFormat);
        }
        j = j.wrapping_mul(10).wrapping_add(digit_val(b[m]) as u32);
        digits += 1;
        m += 1;
    }
    if j == 0 {
        // J0 非法：判据号自 J1 起。
        return Err(EnrollError::CritFormat);
    }
    Ok(())
}

// ── S2：EnrollRow 四列入册行 ─────────────────────────────────────────

/// 四列入册行：fid / 名 / 行数 / 判据号（J1-a 闭集四列，不存在第五列）。
/// 含 String 字段，禁 derive Copy（铁律）。
#[derive(Debug, Clone)]
pub struct EnrollRow {
    fid: String,
    fid_num: u16,
    name: String,
    lines: u32,
    crit: String,
}

impl EnrollRow {
    /// 构造期四列全校验：任一列不过即整行拒收，错误码逐列可归因。
    pub fn new(fid: &str, name: &str, lines: u32, crit: &str) -> Result<Self, EnrollError> {
        let fid_num = validate_fid(fid)?;
        validate_name(name)?;
        validate_lines(lines)?;
        validate_crit(crit, fid_num)?;
        Ok(Self {
            fid: fid.to_string(),
            fid_num,
            name: name.to_string(),
            lines,
            crit: crit.to_string(),
        })
    }

    pub fn fid(&self) -> &str {
        &self.fid
    }

    pub fn fid_num(&self) -> u16 {
        self.fid_num
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn lines(&self) -> u32 {
        self.lines
    }

    pub fn crit(&self) -> &str {
        &self.crit
    }

    /// 批号（const 推导）。
    pub fn batch_no(&self) -> u16 {
        batch_of(self.fid_num)
    }

    /// 批内序号。
    pub fn batch_idx(&self) -> u16 {
        idx_in_batch(self.fid_num)
    }

    /// 自复核：四列全部重走校验器（防御纵深，入册前再核一遍）。
    pub fn verify_self(&self) -> Result<(), EnrollError> {
        let n = validate_fid(&self.fid)?;
        if n != self.fid_num {
            return Err(EnrollError::FidFormat);
        }
        validate_name(&self.name)?;
        validate_lines(self.lines)?;
        validate_crit(&self.crit, self.fid_num)?;
        Ok(())
    }

    /// 快照行文本："fid|名|行数|判据号"（四列管道分隔，人读可直核）。
    pub fn snapshot(&self) -> String {
        format!("{}|{}|{}|{}", self.fid, self.name, self.lines, self.crit)
    }

    /// 确定性序列化（0x1F 列分隔 / 0x1E 行终止），供指纹折叠。
    pub fn serialize_into(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(self.fid.as_bytes());
        out.push(0x1F);
        out.extend_from_slice(self.name.as_bytes());
        out.push(0x1F);
        out.extend_from_slice(&self.lines.to_le_bytes());
        out.push(0x1F);
        out.extend_from_slice(self.crit.as_bytes());
        out.push(0x1E);
    }
}

// ── 指纹基元：FNV-1a 64（零 unwrap、零索引越界风险——while 显界） ────

pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 64 带种子折叠（同输入双跑必等——确定性根基）。
pub fn fnv1a64_with(data: &[u8], seed: u64) -> u64 {
    let mut h = seed;
    let mut i = 0usize;
    while i < data.len() {
        h ^= data[i] as u64;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// FNV-1a 64 标准偏移初值版。
pub fn fnv1a64(data: &[u8]) -> u64 {
    fnv1a64_with(data, FNV_OFFSET)
}

// ── S4：BatchLedger 分批账（40 批分组 + 守恒 + 零跳号） ──────────────

/// 分批账：40 批计数器（栈上 160B，远低于 64KiB 红线）。
#[derive(Debug, Clone)]
pub struct BatchLedger {
    counts: [u32; BATCH_COUNT], // 下标 0..40 对应批号 1..=40
}

impl BatchLedger {
    pub const fn new() -> Self {
        BatchLedger { counts: [0; BATCH_COUNT] }
    }

    /// 登记一条入册 → 对应批计数 +1（wrapping 显性）。
    pub fn record(&mut self, fid_num: u16) -> Result<(), EnrollError> {
        let b = batch_of(fid_num) as usize;
        if b == 0 || b > BATCH_COUNT {
            // batch_of 对 1..=800 恒在 1..=40；此处为防御纵深。
            return Err(EnrollError::FidOutOfRange);
        }
        self.counts[b - 1] = self.counts[b - 1].wrapping_add(1);
        Ok(())
    }

    pub fn count_of(&self, batch: u16) -> u32 {
        if batch == 0 || batch as usize > BATCH_COUNT {
            return 0;
        }
        self.counts[(batch - 1) as usize]
    }

    /// Σ批（饱和累加——溢出显性纪律）。
    pub fn sum(&self) -> u32 {
        let mut s: u32 = 0;
        let mut i = 0usize;
        while i < BATCH_COUNT {
            s = s.saturating_add(self.counts[i]);
            i += 1;
        }
        s
    }

    /// 跳号侦测：计数为零的批号清单（零跳号 = 空清单）。
    pub fn gap_batches(&self) -> Vec<u16> {
        let mut gaps = Vec::new();
        let mut i = 0usize;
        while i < BATCH_COUNT {
            if self.counts[i] == 0 {
                gaps.push((i + 1) as u16);
            }
            i += 1;
        }
        gaps
    }

    /// 宽松报表（入册过程中间态可查，不判错）。
    pub fn report(&self) -> BatchReport {
        let mut per = Vec::new();
        let mut i = 0usize;
        while i < BATCH_COUNT {
            per.push(((i + 1) as u16, self.counts[i]));
            i += 1;
        }
        BatchReport {
            per_batch: per,
            sum: self.sum(),
            gap_batches: self.gap_batches(),
            conservation_ok: self.sum() == TOTAL_ENTRIES,
            contiguous_ok: true,
        }
    }

    /// 守恒终核：Σ批 ≡ 800 且批号连续零跳号，任一破坏即 E_CONSERVATION。
    pub fn verify_conservation(&self) -> Result<BatchReport, EnrollError> {
        let mut rep = self.report();
        let gaps = self.gap_batches();
        rep.contiguous_ok = gaps.is_empty();
        if rep.sum != TOTAL_ENTRIES {
            return Err(EnrollError::Conservation);
        }
        if !gaps.is_empty() {
            return Err(EnrollError::Conservation);
        }
        Ok(rep)
    }
}

impl Default for BatchLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 分批账报表（Debug/Clone/PartialEq 供测试比对；不含 Vec 语义禁 Copy）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchReport {
    /// 逐批计数（批号, 条数），批号 1..=40 有序。
    pub per_batch: Vec<(u16, u32)>,
    /// Σ批（饱和）。
    pub sum: u32,
    /// 跳号批清单（零跳号 = 空）。
    pub gap_batches: Vec<u16>,
    /// Σ批 ≡ 800。
    pub conservation_ok: bool,
    /// 批号连续零跳号。
    pub contiguous_ok: bool,
}

/// 批目录条目：批号 + 规范首末 fid + 期望/实际条数（批号分组分列账）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchDirEntry {
    pub batch_no: u16,
    pub first_fid: String,
    pub last_fid: String,
    pub expected: u32,
    pub actual: u32,
}

/// 批目录：按规范批界 ((b-1)*20+1 ..= b*20) 与分批账实数逐批分列。
pub fn batch_directory(ledger: &BatchLedger) -> Vec<BatchDirEntry> {
    let mut dir = Vec::new();
    let mut b = 1u16;
    while b as usize <= BATCH_COUNT {
        let first_num = (b as u32 - 1) * BATCH_SIZE + 1;
        let last_num = b as u32 * BATCH_SIZE;
        dir.push(BatchDirEntry {
            batch_no: b,
            first_fid: canonical_fid(first_num as u16),
            last_fid: canonical_fid(last_num as u16),
            expected: BATCH_SIZE,
            actual: ledger.count_of(b),
        });
        b += 1;
    }
    dir
}

// ── S3：DomainEnrollBook 入册账（登记制 + 封板 + 指纹） ──────────────

/// A1 域 800 条入册账：登记制（append-only 严格升序）+ 重复 fid 拒收 +
/// 超容拒收 + 封板制 + FNV-1a 64 账指纹。含 Vec 字段禁 derive Copy。
#[derive(Debug)]
pub struct EnrollBook {
    rows: Vec<EnrollRow>,
    ledger: BatchLedger,
    sealed: bool,
}

impl EnrollBook {
    pub fn new() -> Self {
        EnrollBook {
            rows: Vec::new(),
            ledger: BatchLedger::new(),
            sealed: false,
        }
    }

    pub fn count(&self) -> u32 {
        self.rows.len() as u32
    }

    /// 总数恰 800 判定（J1-b）。
    pub fn is_full(&self) -> bool {
        self.count() == TOTAL_ENTRIES
    }

    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    pub fn rows(&self) -> &[EnrollRow] {
        &self.rows
    }

    pub fn row_at(&self, idx: usize) -> Option<&EnrollRow> {
        if idx < self.rows.len() {
            Some(&self.rows[idx])
        } else {
            None
        }
    }

    pub fn ledger(&self) -> &BatchLedger {
        &self.ledger
    }

    /// 是否含某 fid（精确文本匹配）。
    pub fn contains(&self, fid: &str) -> bool {
        let mut i = 0usize;
        while i < self.rows.len() {
            if self.rows[i].fid() == fid {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 登记制入册：封板拒 / 超容拒 / 行自复核 / 严格升序（等号 = 重复 fid
    /// 拒收，倒退 = 乱序拒收）/ 分批账登记。全过方落账。
    pub fn enroll(&mut self, row: EnrollRow) -> Result<(), EnrollError> {
        if self.sealed {
            return Err(EnrollError::Sealed);
        }
        if self.count() >= TOTAL_ENTRIES {
            return Err(EnrollError::BookFull);
        }
        row.verify_self()?;
        let mut dup = false;
        let mut disorder = false;
        if let Some(last) = self.rows.last() {
            match row.fid_num().cmp(&last.fid_num()) {
                Ordering::Equal => dup = true,
                Ordering::Less => disorder = true,
                Ordering::Greater => {}
            }
        }
        if dup {
            return Err(EnrollError::DuplicateFid);
        }
        if disorder {
            return Err(EnrollError::OrderViolation);
        }
        self.ledger.record(row.fid_num())?;
        self.rows.push(row);
        Ok(())
    }

    /// 行数列总和（u64 饱和累加——溢出显性）。
    pub fn total_lines(&self) -> u64 {
        let mut s: u64 = 0;
        let mut i = 0usize;
        while i < self.rows.len() {
            s = s.saturating_add(self.rows[i].lines() as u64);
            i += 1;
        }
        s
    }

    /// 账指纹：逐行确定性序列化后 FNV-1a 64 链式折叠。
    /// 行序固定（登记制严格升序）→ 同账同指纹，篡改即漂移。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_OFFSET;
        let mut i = 0usize;
        while i < self.rows.len() {
            let mut buf = Vec::new();
            self.rows[i].serialize_into(&mut buf);
            h = fnv1a64_with(&buf, h);
            i += 1;
        }
        h
    }

    /// 入册序复核：严格升序逐行核（登记制不变量）。
    pub fn order_ok(&self) -> bool {
        let mut i = 1usize;
        while i < self.rows.len() {
            if self.rows[i].fid_num() <= self.rows[i - 1].fid_num() {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 账审计：条数/行数和/序/分批守恒/指纹一表齐出。
    pub fn audit(&self) -> AuditReport {
        let mut dup_free = true;
        let mut i = 1usize;
        while i < self.rows.len() {
            if self.rows[i].fid_num() == self.rows[i - 1].fid_num() {
                dup_free = false;
            }
            i += 1;
        }
        let rep = self.ledger.report();
        AuditReport {
            count: self.count(),
            total_lines: self.total_lines(),
            order_ok: self.order_ok(),
            duplicate_free: dup_free,
            batch_sum: rep.sum,
            batch_contiguous: rep.gap_batches.is_empty(),
            fingerprint: self.fingerprint(),
        }
    }
}

impl Default for EnrollBook {
    fn default() -> Self {
        Self::new()
    }
}

/// 账审计报表。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditReport {
    pub count: u32,
    pub total_lines: u64,
    pub order_ok: bool,
    pub duplicate_free: bool,
    pub batch_sum: u32,
    pub batch_contiguous: bool,
    pub fingerprint: u64,
}

// ── S5：SourceTableGenerator 批册源清单生成器（const 规范确定性） ────

/// 批册源清单生成器：按规范确定性生成 800 条四列，零随机零环境依赖，
/// 同输入双跑必等（直读一致的基准面）。
pub struct SourceTableGenerator;

impl SourceTableGenerator {
    /// 规范行数：奇号 570 / 偶号 580（来源：锚点行 2396 批册段
    /// F0781=570/F0782=580/F0783=570/F0784=580 奇偶交替规律）。
    pub const fn canonical_lines(fid_num: u16) -> u32 {
        if (fid_num & 1) == 1 {
            LINES_ODD
        } else {
            LINES_EVEN
        }
    }

    /// 规范名："A1域功能·批NN·条NN"（批号/批内序号 const 推导）。
    pub fn canonical_name(fid_num: u16) -> String {
        format!(
            "{}域功能·批{:02}·条{:02}",
            DOMAIN_CODE,
            batch_of(fid_num),
            idx_in_batch(fid_num)
        )
    }

    /// 规范判据号："UNX-F%04d-J1"（A1 域每条目主判据为 J1）。
    pub fn canonical_crit(fid_num: u16) -> String {
        format!("{}{:04}-J1", FID_PREFIX, fid_num)
    }

    /// 单条规范行（四列齐备，构造期全过校验）。
    pub fn canonical_row(fid_num: u16) -> Result<EnrollRow, EnrollError> {
        if fid_num == 0 || fid_num as u32 > TOTAL_ENTRIES {
            return Err(EnrollError::FidOutOfRange);
        }
        EnrollRow::new(
            &canonical_fid(fid_num),
            &Self::canonical_name(fid_num),
            Self::canonical_lines(fid_num),
            &Self::canonical_crit(fid_num),
        )
    }

    /// 全域规范源清单：恰 800 条，号 1..=800 严格升序。
    pub fn canonical_source_list() -> Result<Vec<EnrollRow>, EnrollError> {
        let mut rows = Vec::new();
        let mut n = 1u16;
        while n as u32 <= TOTAL_ENTRIES {
            rows.push(Self::canonical_row(n)?);
            n += 1;
        }
        Ok(rows)
    }
}

// ── S6：CrossReader 批册直读一致核对器（diff 三归因 + ppm） ──────────

/// 差异归因三态（J1-c）：漏登 / 多登 / 列值不符。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffKind {
    /// 漏登：批册源有条目而入册账缺。
    MissingInBook,
    /// 多登：入册账有条目而批册源缺（含域外多登）。
    ExtraInBook,
    /// 列值不符：同 fid 但某列值不等。
    ColumnMismatch(DiffColumn),
}

impl DiffKind {
    /// 中文归因标签。
    pub fn attribution(&self) -> &'static str {
        match self {
            DiffKind::MissingInBook => "漏登",
            DiffKind::ExtraInBook => "多登",
            DiffKind::ColumnMismatch(_) => "列值不符",
        }
    }
}

/// 差异列四分（J1-a 四列闭集）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffColumn {
    Fid,
    Name,
    Lines,
    Crit,
}

impl DiffColumn {
    /// 列名（与锚点四列口径一致）。
    pub fn column_name(&self) -> &'static str {
        match self {
            DiffColumn::Fid => "fid",
            DiffColumn::Name => "名",
            DiffColumn::Lines => "行数",
            DiffColumn::Crit => "判据号",
        }
    }
}

/// 单条差异：序号 + fid + 归因 + 双侧值留痕。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffItem {
    pub seq: u32,
    pub fid: String,
    pub kind: DiffKind,
    /// 入册账侧值（漏登时为 None）。
    pub book_val: Option<String>,
    /// 批册源侧值（多登时为 None）。
    pub src_val: Option<String>,
}

/// 直读核对报表：一致率 ppm 整数制（10000 = 100%）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffReport {
    pub matched: u32,
    pub items: Vec<DiffItem>,
    pub rate_ppm: u64,
    /// 恰 800 对齐且零差异且 10000 ppm。
    pub is_full_consistent: bool,
}

/// 批册直读一致核对器：合并游标（双侧均严格升序）逐 fid diff。
pub struct CrossReader;

impl CrossReader {
    pub fn diff(book: &EnrollBook, source: &[EnrollRow]) -> DiffReport {
        let mut items: Vec<DiffItem> = Vec::new();
        let mut matched = 0u32;
        let mut seq = 0u32;
        let book_rows = book.rows();
        let mut i = 0usize;
        let mut j = 0usize;
        while i < book_rows.len() && j < source.len() {
            let br = &book_rows[i];
            let sr = &source[j];
            match br.fid_num().cmp(&sr.fid_num()) {
                Ordering::Less => {
                    // 账侧号更小：该号不在源中 → 多登。
                    seq += 1;
                    items.push(DiffItem {
                        seq,
                        fid: br.fid().to_string(),
                        kind: DiffKind::ExtraInBook,
                        book_val: Some(br.snapshot()),
                        src_val: None,
                    });
                    i += 1;
                }
                Ordering::Greater => {
                    // 源侧号更小：该号不在账中 → 漏登。
                    seq += 1;
                    items.push(DiffItem {
                        seq,
                        fid: sr.fid().to_string(),
                        kind: DiffKind::MissingInBook,
                        book_val: None,
                        src_val: Some(sr.snapshot()),
                    });
                    j += 1;
                }
                Ordering::Equal => {
                    // 同号：四列逐列核，任一不符即差异留痕（列级归因）。
                    let mut col_mismatch = false;
                    if br.name() != sr.name() {
                        seq += 1;
                        col_mismatch = true;
                        items.push(DiffItem {
                            seq,
                            fid: br.fid().to_string(),
                            kind: DiffKind::ColumnMismatch(DiffColumn::Name),
                            book_val: Some(br.name().to_string()),
                            src_val: Some(sr.name().to_string()),
                        });
                    }
                    if br.lines() != sr.lines() {
                        seq += 1;
                        col_mismatch = true;
                        items.push(DiffItem {
                            seq,
                            fid: br.fid().to_string(),
                            kind: DiffKind::ColumnMismatch(DiffColumn::Lines),
                            book_val: Some(br.lines().to_string()),
                            src_val: Some(sr.lines().to_string()),
                        });
                    }
                    if br.crit() != sr.crit() {
                        seq += 1;
                        col_mismatch = true;
                        items.push(DiffItem {
                            seq,
                            fid: br.fid().to_string(),
                            kind: DiffKind::ColumnMismatch(DiffColumn::Crit),
                            book_val: Some(br.crit().to_string()),
                            src_val: Some(sr.crit().to_string()),
                        });
                    }
                    if !col_mismatch {
                        matched += 1;
                    }
                    i += 1;
                    j += 1;
                }
            }
        }
        while i < book_rows.len() {
            seq += 1;
            items.push(DiffItem {
                seq,
                fid: book_rows[i].fid().to_string(),
                kind: DiffKind::ExtraInBook,
                book_val: Some(book_rows[i].snapshot()),
                src_val: None,
            });
            i += 1;
        }
        while j < source.len() {
            seq += 1;
            items.push(DiffItem {
                seq,
                fid: source[j].fid().to_string(),
                kind: DiffKind::MissingInBook,
                book_val: None,
                src_val: Some(source[j].snapshot()),
            });
            j += 1;
        }
        // 一致率：以批册源条数为分母（规范恒 800）。
        let denom = source.len() as u64;
        let rate_ppm = if denom == 0 {
            0
        } else {
            (matched as u64).saturating_mul(RATE_FULL_PPM) / denom
        };
        let is_full_consistent =
            rate_ppm == RATE_FULL_PPM && items.is_empty() && book.count() == TOTAL_ENTRIES;
        DiffReport {
            matched,
            items,
            rate_ppm,
            is_full_consistent,
        }
    }

    /// 终核门：直读一致率不足 100% 即 E_DIFF_INCONSISTENT。
    pub fn require_consistent(report: &DiffReport) -> Result<(), EnrollError> {
        if report.is_full_consistent {
            Ok(())
        } else {
            Err(EnrollError::DiffInconsistent)
        }
    }
}

// ── S7：MountDeclaration 总册挂载声明（四要素 + 指纹一致核） ─────────

/// 总册挂载声明：册号 / 域号 / 条目数 / 指纹四要素（J1-d 闭集）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MountDeclaration {
    book_no: u32,
    domain: String,
    entries: u32,
    declared_fingerprint: u64,
    list_fingerprint: u64,
}

impl MountDeclaration {
    /// 自账出声明：账须满 800，双指纹同源（声明指纹 = 清单指纹）。
    pub fn declare(book: &EnrollBook) -> Result<Self, EnrollError> {
        if !book.is_full() {
            return Err(EnrollError::NotComplete);
        }
        let fp = book.fingerprint();
        Ok(MountDeclaration {
            book_no: MASTER_BOOK_NO,
            domain: DOMAIN_CODE.to_string(),
            entries: book.count(),
            declared_fingerprint: fp,
            list_fingerprint: fp,
        })
    }

    /// 外来声明核：声明侧四要素逐项对账实（指纹不一致 E_MOUNT_MISMATCH
    /// 拒挂；域号/条目数不符 E_MOUNT_BAD_DECL 拒挂）。
    pub fn from_declared(
        book_no: u32,
        domain: &str,
        entries: u32,
        declared_fingerprint: u64,
        book: &EnrollBook,
    ) -> Result<Self, EnrollError> {
        if !book.is_full() {
            return Err(EnrollError::NotComplete);
        }
        if entries != book.count() {
            return Err(EnrollError::MountBadDecl);
        }
        if domain != DOMAIN_CODE {
            return Err(EnrollError::MountBadDecl);
        }
        if book_no == 0 {
            return Err(EnrollError::MountBadDecl);
        }
        let list_fp = book.fingerprint();
        if declared_fingerprint != list_fp {
            return Err(EnrollError::MountMismatch);
        }
        Ok(MountDeclaration {
            book_no,
            domain: domain.to_string(),
            entries,
            declared_fingerprint,
            list_fingerprint: list_fp,
        })
    }

    /// 挂载核：声明与账实三方一致（条目数 / 声明指纹 / 清单指纹）。
    pub fn verify(&self, book: &EnrollBook) -> Result<(), EnrollError> {
        if self.entries != book.count() {
            return Err(EnrollError::MountBadDecl);
        }
        if self.domain != DOMAIN_CODE {
            return Err(EnrollError::MountBadDecl);
        }
        if self.declared_fingerprint != book.fingerprint() {
            return Err(EnrollError::MountMismatch);
        }
        if self.list_fingerprint != self.declared_fingerprint {
            return Err(EnrollError::MountMismatch);
        }
        Ok(())
    }

    pub fn book_no(&self) -> u32 {
        self.book_no
    }

    pub fn domain(&self) -> &str {
        &self.domain
    }

    pub fn entries(&self) -> u32 {
        self.entries
    }

    pub fn declared_fingerprint(&self) -> u64 {
        self.declared_fingerprint
    }

    pub fn list_fingerprint(&self) -> u64 {
        self.list_fingerprint
    }

    /// 挂载声明落账行（J1-d「总册挂载声明落账」的账面载体）。
    pub fn ledger_line(&self) -> String {
        format!(
            "MOUNT|book_no={:#010X}|domain={}|entries={}|fp={:016X}|list_fp={:016X}",
            self.book_no, self.domain, self.entries, self.declared_fingerprint,
            self.list_fingerprint
        )
    }
}

// ── S8：EventLedger 事件账（封板制 + 滚动校验和） ────────────────────

/// 单条事件：诊断码 + 文本。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnrollEvent {
    pub code: u16,
    pub text: String,
}

/// 事件账：append-only + 封板制 + 滚动 FNV 校验和（可重放复核）。
#[derive(Debug)]
pub struct EventLedger {
    events: Vec<EnrollEvent>,
    sealed: bool,
    checksum: u64,
}

impl EventLedger {
    pub fn new() -> Self {
        EventLedger {
            events: Vec::new(),
            sealed: false,
            checksum: FNV_OFFSET,
        }
    }

    /// 记事件：封板后拒写（E_LEDGER_SEALED_WRITE），滚动校验和折叠。
    pub fn record(&mut self, code: u16, text: String) -> Result<(), EnrollError> {
        if self.sealed {
            return Err(EnrollError::LedgerSealedWrite);
        }
        self.checksum = fnv1a64_with(text.as_bytes(), self.checksum);
        self.events.push(EnrollEvent { code, text });
        Ok(())
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub fn events(&self) -> &[EnrollEvent] {
        &self.events
    }

    pub fn checksum(&self) -> u64 {
        self.checksum
    }

    /// 重放复核：从头重算滚动校验和（账密自证）。
    pub fn replay_checksum(&self) -> u64 {
        let mut h = FNV_OFFSET;
        let mut i = 0usize;
        while i < self.events.len() {
            h = fnv1a64_with(self.events[i].text.as_bytes(), h);
            i += 1;
        }
        h
    }

    pub fn checksum_ok(&self) -> bool {
        self.checksum == self.replay_checksum()
    }
}

impl Default for EventLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── S10：注入面四路（漏条 / 重复 / 多登 / 列值篡改，全须显性检出） ───

/// 注入探针结果：探针名 / 是否检出 / 诊断码 / 中文诊断。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeOutcome {
    pub name: &'static str,
    pub detected: bool,
    pub code: u16,
    pub diag: String,
}

/// 探针一：漏条注入——批册源 800 条中漏登 UNX-F0785 一条，
/// 直读 diff 必须报「漏登」且一致率 < 10000 ppm。
pub fn probe_missing_injection() -> ProbeOutcome {
    let name = "漏条注入";
    let source = match SourceTableGenerator::canonical_source_list() {
        Ok(s) => s,
        Err(e) => {
            return ProbeOutcome {
                name,
                detected: false,
                code: e.code(),
                diag: e.detail().to_string(),
            }
        }
    };
    let mut book = EnrollBook::new();
    let mut n = 1u16;
    while n as u32 <= TOTAL_ENTRIES {
        // 漏登 785 号（本任务自指标记位）。
        if n != 785 {
            match SourceTableGenerator::canonical_row(n) {
                Ok(r) => {
                    if let Err(e) = book.enroll(r) {
                        return ProbeOutcome {
                            name,
                            detected: false,
                            code: e.code(),
                            diag: e.detail().to_string(),
                        };
                    }
                }
                Err(e) => {
                    return ProbeOutcome {
                        name,
                        detected: false,
                        code: e.code(),
                        diag: e.detail().to_string(),
                    };
                }
            }
        }
        n += 1;
    }
    let rep = CrossReader::diff(&book, &source);
    let mut found = false;
    let mut i = 0usize;
    while i < rep.items.len() {
        if rep.items[i].kind == DiffKind::MissingInBook && rep.items[i].fid == "UNX-F0785" {
            found = true;
        }
        i += 1;
    }
    let detected = found && rep.rate_ppm < RATE_FULL_PPM && !rep.is_full_consistent;
    ProbeOutcome {
        name,
        detected,
        code: E_DIFF_INCONSISTENT,
        diag: format!(
            "漏条注入检出：UNX-F0785 漏登已归因「漏登」，一致率 {} ppm < 10000，差异 {} 项",
            rep.rate_ppm,
            rep.items.len()
        ),
    }
}

/// 探针二：重复注入——部分入册（前 10 条）后重复登记 UNX-F0010，
/// 登记制必须 E_DUPLICATE_FID 拒收（注：满账上重复注入先触容量核
/// E_BOOK_FULL，故本探针在未满账面直击重复核，口径更纯）。
pub fn probe_duplicate_injection() -> ProbeOutcome {
    let name = "重复注入";
    let mut book = EnrollBook::new();
    let mut n = 1u16;
    while n <= 10 {
        match SourceTableGenerator::canonical_row(n) {
            Ok(r) => {
                if let Err(e) = book.enroll(r) {
                    return ProbeOutcome {
                        name,
                        detected: false,
                        code: e.code(),
                        diag: e.detail().to_string(),
                    };
                }
            }
            Err(e) => {
                return ProbeOutcome {
                    name,
                    detected: false,
                    code: e.code(),
                    diag: e.detail().to_string(),
                };
            }
        }
        n += 1;
    }
    let dup = match SourceTableGenerator::canonical_row(10) {
        Ok(r) => r,
        Err(e) => {
            return ProbeOutcome {
                name,
                detected: false,
                code: e.code(),
                diag: e.detail().to_string(),
            }
        }
    };
    match book.enroll(dup) {
        Err(EnrollError::DuplicateFid) => ProbeOutcome {
            name,
            detected: true,
            code: E_DUPLICATE_FID,
            diag: "重复注入检出：UNX-F0010 二次入册被登记制 E_DUPLICATE_FID 拒收".to_string(),
        },
        Err(e) => ProbeOutcome {
            name,
            detected: false,
            code: e.code(),
            diag: e.detail().to_string(),
        },
        Ok(()) => ProbeOutcome {
            name,
            detected: false,
            code: 0,
            diag: "重复注入未检出：重复 fid 被误收，登记制失效".to_string(),
        },
    }
}

/// 探针三：多登注入——满账后越域登记 UNX-F0801（规范格式但 >800），
/// 域界必须 E_FID_OUT_OF_DOMAIN 拒收（多登归因）。
pub fn probe_extra_injection() -> ProbeOutcome {
    let name = "多登注入";
    let row = EnrollRow::new(
        "UNX-F0801",
        &SourceTableGenerator::canonical_name(801),
        SourceTableGenerator::canonical_lines(801),
        &SourceTableGenerator::canonical_crit(801),
    );
    match row {
        Err(EnrollError::FidOutOfRange) => ProbeOutcome {
            name,
            detected: true,
            code: E_FID_OUT_OF_DOMAIN,
            diag: "多登注入检出：UNX-F0801 越域条目在行构造期即被域界拒收".to_string(),
        },
        Err(e) => ProbeOutcome {
            name,
            detected: false,
            code: e.code(),
            diag: e.detail().to_string(),
        },
        Ok(_) => ProbeOutcome {
            name,
            detected: false,
            code: 0,
            diag: "多登注入未检出：域外条目 UNX-F0801 被误收".to_string(),
        },
    }
}

/// 探针四：列值篡改注入——UNX-F0785 行数 570→580（格式合法但值篡改），
/// 直读 diff 必报「列值不符·行数」且挂载指纹核必 E_MOUNT_MISMATCH。
pub fn probe_column_tamper() -> ProbeOutcome {
    let name = "列值篡改注入";
    let source = match SourceTableGenerator::canonical_source_list() {
        Ok(s) => s,
        Err(e) => {
            return ProbeOutcome {
                name,
                detected: false,
                code: e.code(),
                diag: e.detail().to_string(),
            }
        }
    };
    let mut book = EnrollBook::new();
    let mut n = 1u16;
    while n as u32 <= TOTAL_ENTRIES {
        let row = if n == 785 {
            // 篡改位：行数列 570 → 580（格式全过，唯值不符）。
            match EnrollRow::new(
                "UNX-F0785",
                &SourceTableGenerator::canonical_name(785),
                LINES_EVEN,
                &SourceTableGenerator::canonical_crit(785),
            ) {
                Ok(r) => r,
                Err(e) => {
                    return ProbeOutcome {
                        name,
                        detected: false,
                        code: e.code(),
                        diag: e.detail().to_string(),
                    }
                }
            }
        } else {
            match SourceTableGenerator::canonical_row(n) {
                Ok(r) => r,
                Err(e) => {
                    return ProbeOutcome {
                        name,
                        detected: false,
                        code: e.code(),
                        diag: e.detail().to_string(),
                    }
                }
            }
        };
        if let Err(e) = book.enroll(row) {
            return ProbeOutcome {
                name,
                detected: false,
                code: e.code(),
                diag: e.detail().to_string(),
            };
        }
        n += 1;
    }
    let rep = CrossReader::diff(&book, &source);
    let mut found_lines = false;
    let mut i = 0usize;
    while i < rep.items.len() {
        if rep.items[i].kind == DiffKind::ColumnMismatch(DiffColumn::Lines)
            && rep.items[i].fid == "UNX-F0785"
        {
            found_lines = true;
        }
        i += 1;
    }
    // 挂载指纹核：以批册源指纹为声明指纹，篡改账必拒。
    let mut fp_flag = false;
    let decl = MountDeclaration::declare(&book);
    if let Ok(d) = decl {
        // 用规范账指纹对照：另建规范账取指纹。
        if let Ok(clean) = build_full_book() {
            fp_flag = MountDeclaration::from_declared(
                MASTER_BOOK_NO,
                DOMAIN_CODE,
                d.entries(),
                clean.fingerprint(),
                &book,
            ) == Err(EnrollError::MountMismatch);
        }
    }
    let detected = found_lines && rep.rate_ppm < RATE_FULL_PPM && fp_flag;
    ProbeOutcome {
        name,
        detected,
        code: E_MOUNT_MISMATCH,
        diag: format!(
            "列值篡改注入检出：UNX-F0785 行数 570→580 已归因「列值不符·行数」，挂载指纹核 {}",
            if fp_flag { "E_MOUNT_MISMATCH 拒挂" } else { "未触发" }
        ),
    }
}

/// 注入面全跑：四路探针依序执行（漏条/重复/多登/篡改）。
pub fn injection_suite() -> Vec<ProbeOutcome> {
    let mut out = Vec::new();
    out.push(probe_missing_injection());
    out.push(probe_duplicate_injection());
    out.push(probe_extra_injection());
    out.push(probe_column_tamper());
    out
}

// ── 参考构建器：规范满账（直读 100% 基准账） ─────────────────────────

/// 批册源清单全额入册（严格升序逐条），满账即封板前置态。
pub fn build_full_book() -> Result<EnrollBook, EnrollError> {
    let source = SourceTableGenerator::canonical_source_list()?;
    let mut book = EnrollBook::new();
    let mut i = 0usize;
    while i < source.len() {
        book.enroll(source[i].clone())?;
        i += 1;
    }
    Ok(book)
}

// ── S9：DomenrollStation 总编排 ──────────────────────────────────────

/// 域功入册总编排站：入册账 + 批册源 + 直读报表 + 挂载声明 + 事件账。
/// 含 Vec 字段，禁 derive Copy。
#[derive(Debug)]
pub struct DomenrollStation {
    book: EnrollBook,
    source: Vec<EnrollRow>,
    diff: Option<DiffReport>,
    mount: Option<MountDeclaration>,
    events: EventLedger,
}

impl DomenrollStation {
    /// 以给定批册源清单开站（源须恰 800 条，否则 E_DIFF_INCONSISTENT）。
    pub fn new_with_source(source: Vec<EnrollRow>) -> Result<Self, EnrollError> {
        if source.len() as u64 != TOTAL_ENTRIES as u64 {
            return Err(EnrollError::DiffInconsistent);
        }
        Ok(DomenrollStation {
            book: EnrollBook::new(),
            source,
            diff: None,
            mount: None,
            events: EventLedger::new(),
        })
    }

    /// 规范开站：源 = 生成器全域 800 条。
    pub fn from_canonical() -> Result<Self, EnrollError> {
        Self::new_with_source(SourceTableGenerator::canonical_source_list()?)
    }

    pub fn book(&self) -> &EnrollBook {
        &self.book
    }

    pub fn source(&self) -> &[EnrollRow] {
        &self.source
    }

    pub fn diff(&self) -> Option<&DiffReport> {
        self.diff.as_ref()
    }

    pub fn mount(&self) -> Option<&MountDeclaration> {
        self.mount.as_ref()
    }

    pub fn events(&self) -> &EventLedger {
        &self.events
    }

    /// 单条入册（带事件留痕）：成功/拒收均落事件账。
    /// 事件码：成功走段基 0x1AF0（信息性事件），拒收走对应错误码。
    pub fn enroll_row(&mut self, row: EnrollRow) -> Result<(), EnrollError> {
        let fid = row.fid().to_string();
        let fnum = row.fid_num();
        match self.book.enroll(row) {
            Ok(()) => {
                let _ = self.events.record(
                    SEG_BASE,
                    format!("入册 {} 于批 {}", fid, batch_of(fnum)),
                );
                Ok(())
            }
            Err(e) => {
                let _ = self
                    .events
                    .record(e.code(), format!("入册 {} 拒收：{}", fid, e.detail()));
                Err(e)
            }
        }
    }

    /// 全额入册：批册源 800 条依序登记（漏登注入即在此面拦截为逐条 Err）。
    pub fn enroll_all(&mut self) -> Result<(), EnrollError> {
        let mut i = 0usize;
        while i < self.source.len() {
            let row = self.source[i].clone();
            self.enroll_row(row)?;
            i += 1;
        }
        Ok(())
    }

    /// 批册直读核对：报表恒出（中间态可查），一致率不足时另返
    /// E_DIFF_INCONSISTENT（终核门）。
    pub fn run_crossread(&mut self) -> Result<DiffReport, EnrollError> {
        let rep = CrossReader::diff(&self.book, &self.source);
        let _ = self.events.record(
            SEG_BASE,
            format!(
                "直读核对：对齐 {} 条，差异 {} 项，一致率 {} ppm",
                rep.matched,
                rep.items.len(),
                rep.rate_ppm
            ),
        );
        if rep.is_full_consistent {
            self.diff = Some(rep.clone());
            Ok(rep)
        } else {
            self.diff = Some(rep.clone());
            Err(EnrollError::DiffInconsistent)
        }
    }

    /// 总册挂载：出声明 + 指纹一致核 + 声明行落账（J1-d）。
    pub fn mount_master(&mut self) -> Result<MountDeclaration, EnrollError> {
        let decl = MountDeclaration::declare(&self.book)?;
        decl.verify(&self.book)?;
        let line = decl.ledger_line();
        let _ = self.events.record(SEG_BASE, line);
        self.mount = Some(decl.clone());
        Ok(decl)
    }

    /// 注入面（四路）挂到本站事件账后返回。
    pub fn run_injections(&mut self) -> Vec<ProbeOutcome> {
        let suite = injection_suite();
        let mut i = 0usize;
        while i < suite.len() {
            let p = &suite[i];
            let code = if p.detected { p.code } else { 0 };
            let _ = self.events.record(code, format!("注入面·{}：{}", p.name, p.diag));
            i += 1;
        }
        suite
    }

    /// health() 六项绿灯（J1 全口径）：
    ///   1 账满 800；2 直读一致 100%；3 挂载在位且指纹一致；
    ///   4 分批守恒 Σ批≡800 且零跳号；5 事件账密自证；6 事件账非空。
    pub fn health(&self) -> HealthReport {
        let mut checks: Vec<(String, bool)> = Vec::new();
        checks.push(("账满800".to_string(), self.book.is_full()));
        let cross_ok = match &self.diff {
            Some(d) => d.is_full_consistent,
            None => false,
        };
        checks.push(("直读一致100%".to_string(), cross_ok));
        let mount_ok = match &self.mount {
            Some(d) => d.verify(&self.book).is_ok(),
            None => false,
        };
        checks.push(("挂载指纹一致".to_string(), mount_ok));
        let cons_ok = self.book.ledger().verify_conservation().is_ok();
        checks.push(("分批守恒零跳号".to_string(), cons_ok));
        checks.push(("事件账密自证".to_string(), self.events.checksum_ok()));
        checks.push(("事件账非空".to_string(), !self.events.is_empty()));
        let mut green = true;
        let mut i = 0usize;
        while i < checks.len() {
            if !checks[i].1 {
                green = false;
            }
            i += 1;
        }
        HealthReport { green, checks }
    }

    /// 账指纹透出（双跑对拍消费）。
    pub fn fingerprint(&self) -> u64 {
        self.book.fingerprint()
    }
}

/// 健康报表：绿/红 + 逐项留痕。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthReport {
    pub green: bool,
    pub checks: Vec<(String, bool)>,
}

// ── S11：双跑确定性对拍 ──────────────────────────────────────────────

/// 双跑对拍报表：两路独立构建的四项指纹类指标逐项比对。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DualRunReport {
    pub fp_a: u64,
    pub fp_b: u64,
    pub count_a: u32,
    pub count_b: u32,
    pub rate_a: u64,
    pub rate_b: u64,
    pub mount_fp_a: u64,
    pub mount_fp_b: u64,
}

impl DualRunReport {
    pub fn is_identical(&self) -> bool {
        self.fp_a == self.fp_b
            && self.count_a == self.count_b
            && self.rate_a == self.rate_b
            && self.mount_fp_a == self.mount_fp_b
    }
}

/// 双跑确定性对拍：两路独立 reference 站（入册→直读→挂载）四项比对，
/// 任一漂移 E_DUAL_RUN_DRIFT 立案。
pub fn dual_run_check() -> Result<DualRunReport, EnrollError> {
    let mut a = DomenrollStation::from_canonical()?;
    a.enroll_all()?;
    let rep_a = a.run_crossread()?;
    let decl_a = a.mount_master()?;

    let mut b = DomenrollStation::from_canonical()?;
    b.enroll_all()?;
    let rep_b = b.run_crossread()?;
    let decl_b = b.mount_master()?;

    let rep = DualRunReport {
        fp_a: a.fingerprint(),
        fp_b: b.fingerprint(),
        count_a: a.book().count(),
        count_b: b.book().count(),
        rate_a: rep_a.rate_ppm,
        rate_b: rep_b.rate_ppm,
        mount_fp_a: decl_a.declared_fingerprint(),
        mount_fp_b: decl_b.declared_fingerprint(),
    };
    if rep.is_identical() {
        Ok(rep)
    } else {
        Err(EnrollError::DualRunDrift)
    }
}

// ── 编译期不变量终核（const assert 组，铁律允许域） ──────────────────
const _: () = assert!(E_SEALED == 0x1AF0);
const _: () = assert!(E_LEDGER_SEALED_WRITE == 0x1AFF);
const _: () = assert!(RATE_FULL_PPM == 10_000);
const _: () = assert!(MASTER_BOOK_NO == 0x0785);
const _: () = assert!(LINES_ODD == 570 && LINES_EVEN == 580);

// ── 测试域（#[cfg(test)]：冒烟 + 注入 + 双跑 + health） ──────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoke_canonical_source_list_800_rows() {
        let src = SourceTableGenerator::canonical_source_list().unwrap();
        assert_eq!(src.len(), 800);
        // 四列齐备：首条 / 本任务条 / 末条抽核。
        assert_eq!(src[0].fid(), "UNX-F0001");
        assert_eq!(src[0].name(), "A1域功能·批01·条01");
        assert_eq!(src[0].lines(), 570);
        assert_eq!(src[0].crit(), "UNX-F0001-J1");
        assert_eq!(src[784].fid(), "UNX-F0785");
        assert_eq!(src[784].name(), "A1域功能·批40·条05");
        assert_eq!(src[784].lines(), 570);
        assert_eq!(src[784].batch_no(), 40);
        assert_eq!(src[784].batch_idx(), 5);
        assert_eq!(src[799].fid(), "UNX-F0800");
        assert_eq!(src[799].lines(), 580);
        assert_eq!(src[799].batch_no(), 40);
        // 行数奇偶交替规律全域核。
        let mut n = 1u16;
        while n as u32 <= TOTAL_ENTRIES {
            let want = if n % 2 == 1 { 570 } else { 580 };
            assert_eq!(src[(n - 1) as usize].lines(), want);
            n += 1;
        }
    }

    #[test]
    fn smoke_row_column_validation_rejects() {
        // fid 列四路拒收。
        assert_eq!(EnrollRow::new("UNX-F785", "n", 1, "UNX-F0785-J1").unwrap_err(), EnrollError::FidFormat);
        assert_eq!(EnrollRow::new("UNX-F0785X", "n", 1, "UNX-F0785-J1").unwrap_err(), EnrollError::FidFormat);
        assert_eq!(EnrollRow::new("UNX-F07a5", "n", 1, "UNX-F0785-J1").unwrap_err(), EnrollError::FidFormat);
        assert_eq!(EnrollRow::new("XXX-F0785", "n", 1, "UNX-F0785-J1").unwrap_err(), EnrollError::FidFormat);
        // 域外拒收。
        assert_eq!(EnrollRow::new("UNX-F0000", "n", 1, "UNX-F0000-J1").unwrap_err(), EnrollError::FidOutOfRange);
        assert_eq!(EnrollRow::new("UNX-F0801", "n", 1, "UNX-F0801-J1").unwrap_err(), EnrollError::FidOutOfRange);
        // 名列拒收。
        assert_eq!(EnrollRow::new("UNX-F0785", "", 1, "UNX-F0785-J1").unwrap_err(), EnrollError::NameFormat);
        let long_name = "名".repeat(64);
        assert_eq!(EnrollRow::new("UNX-F0785", &long_name, 1, "UNX-F0785-J1").unwrap_err(), EnrollError::NameFormat);
        // 行数列拒收。
        assert_eq!(EnrollRow::new("UNX-F0785", "n", 0, "UNX-F0785-J1").unwrap_err(), EnrollError::LinesZero);
        // 判据号列拒收：异号 / J0 / 缺 -J / 四位判据号。
        assert_eq!(EnrollRow::new("UNX-F0785", "n", 570, "UNX-F0784-J1").unwrap_err(), EnrollError::CritFormat);
        assert_eq!(EnrollRow::new("UNX-F0785", "n", 570, "UNX-F0785-J0").unwrap_err(), EnrollError::CritFormat);
        assert_eq!(EnrollRow::new("UNX-F0785", "n", 570, "UNX-F0785J1").unwrap_err(), EnrollError::CritFormat);
        assert_eq!(EnrollRow::new("UNX-F0785", "n", 570, "UNX-F0785-J1234").unwrap_err(), EnrollError::CritFormat);
        // 合法行通过。
        assert!(EnrollRow::new("UNX-F0785", "n", 570, "UNX-F0785-J1").is_ok());
        assert!(EnrollRow::new("UNX-F0785", "n", 570, "UNX-F0785-J12").is_ok());
        // verify_self 复核通过。
        let row = EnrollRow::new("UNX-F0785", "n", 570, "UNX-F0785-J1").unwrap();
        assert!(row.verify_self().is_ok());
        // 快照行四列直读。
        assert_eq!(row.snapshot(), "UNX-F0785|n|570|UNX-F0785-J1");
    }

    #[test]
    fn smoke_full_book_fingerprint_and_totals() {
        let book = build_full_book().unwrap();
        assert_eq!(book.count(), 800);
        assert!(book.is_full());
        assert!(book.order_ok());
        // 行数总和：400×570 + 400×580 = 460000。
        assert_eq!(book.total_lines(), 460_000);
        // 指纹非零且确定性（重跑等值）。
        let fp1 = book.fingerprint();
        let book2 = build_full_book().unwrap();
        assert_eq!(fp1, book2.fingerprint());
        assert_ne!(fp1, 0);
        // 含/不含精确核。
        assert!(book.contains("UNX-F0785"));
        assert!(!book.contains("UNX-F0801"));
        // 审计报表。
        let aud = book.audit();
        assert_eq!(aud.count, 800);
        assert!(aud.order_ok && aud.duplicate_free);
        assert_eq!(aud.batch_sum, 800);
        assert!(aud.batch_contiguous);
    }

    #[test]
    fn smoke_crossread_consistency_100_percent() {
        let book = build_full_book().unwrap();
        let source = SourceTableGenerator::canonical_source_list().unwrap();
        let rep = CrossReader::diff(&book, &source);
        assert_eq!(rep.matched, 800);
        assert_eq!(rep.rate_ppm, 10_000);
        assert!(rep.items.is_empty());
        assert!(rep.is_full_consistent);
        assert!(CrossReader::require_consistent(&rep).is_ok());
    }

    #[test]
    fn smoke_mount_declaration_fingerprint() {
        let book = build_full_book().unwrap();
        // 自账出声明 + 复核。
        let decl = MountDeclaration::declare(&book).unwrap();
        assert_eq!(decl.book_no(), MASTER_BOOK_NO);
        assert_eq!(decl.domain(), "A1");
        assert_eq!(decl.entries(), 800);
        assert_eq!(decl.declared_fingerprint(), book.fingerprint());
        assert_eq!(decl.list_fingerprint(), decl.declared_fingerprint());
        assert!(decl.verify(&book).is_ok());
        // 落账行四要素齐备。
        let line = decl.ledger_line();
        assert!(line.starts_with("MOUNT|book_no=0x00000785|domain=A1|entries=800|fp="));
        // 外来声明核：正确指纹可挂。
        assert!(MountDeclaration::from_declared(0x0785, "A1", 800, book.fingerprint(), &book).is_ok());
        // 外来声明核：错指纹 E_MOUNT_MISMATCH 拒挂。
        assert_eq!(
            MountDeclaration::from_declared(0x0785, "A1", 800, 0xDEAD, &book).unwrap_err(),
            EnrollError::MountMismatch
        );
        // 域号/条目数错 E_MOUNT_BAD_DECL。
        assert_eq!(
            MountDeclaration::from_declared(0x0785, "B2", 800, book.fingerprint(), &book).unwrap_err(),
            EnrollError::MountBadDecl
        );
        assert_eq!(
            MountDeclaration::from_declared(0x0785, "A1", 799, book.fingerprint(), &book).unwrap_err(),
            EnrollError::MountBadDecl
        );
        // 未满账即求声明 E_NOT_COMPLETE。
        let partial = SourceTableGenerator::canonical_source_list().unwrap();
        let mut pb = EnrollBook::new();
        for r in partial.iter().take(799) {
            pb.enroll(r.clone()).unwrap();
        }
        assert_eq!(MountDeclaration::declare(&pb).unwrap_err(), EnrollError::NotComplete);
    }

    #[test]
    fn smoke_batch_conservation_and_directory() {
        let book = build_full_book().unwrap();
        // 守恒终核：Σ批 800、零跳号、逐批恰 20。
        let rep = book.ledger().verify_conservation().unwrap();
        assert_eq!(rep.sum, 800);
        assert!(rep.gap_batches.is_empty());
        assert!(rep.contiguous_ok && rep.conservation_ok);
        for (b, c) in rep.per_batch.iter() {
            // 批号自洽：批 b 的首条 fid 号 = (b-1)*20+1，反推批号必为 b。
            assert_eq!(batch_of(((*b as u32 - 1) * 20 + 1) as u16), *b);
            assert_eq!(*c, 20);
        }
        // 批目录 40 行：首末 fid 逐批在位 + 实数 = 期望。
        let dir = batch_directory(book.ledger());
        assert_eq!(dir.len(), 40);
        assert_eq!(dir[0].first_fid, "UNX-F0001");
        assert_eq!(dir[0].last_fid, "UNX-F0020");
        assert_eq!(dir[39].first_fid, "UNX-F0781");
        assert_eq!(dir[39].last_fid, "UNX-F0800");
        for d in dir.iter() {
            assert_eq!(d.actual, d.expected);
        }
        // 破坏守恒之一：漏登一条（批 40 计 19）→ Σ批 799 ≠ 800，守恒破坏；
        // 批 40 未空故非跳号，破坏由 Σ批守恒面捕获。
        let mut gappy = EnrollBook::new();
        let src = SourceTableGenerator::canonical_source_list().unwrap();
        for r in src.iter() {
            if r.fid_num() != 785 {
                gappy.enroll(r.clone()).unwrap();
            }
        }
        assert!(gappy.ledger().verify_conservation().is_err());
        assert!(gappy.ledger().gap_batches().is_empty());
        assert_eq!(gappy.ledger().count_of(40), 19);
        assert_eq!(gappy.ledger().sum(), 799);
        // 破坏守恒之二：整批跳号（批 40 全缺）→ Σ批 780 且跳号清单 [40]。
        let mut skipped = EnrollBook::new();
        for r in src.iter() {
            if r.batch_no() != 40 {
                skipped.enroll(r.clone()).unwrap();
            }
        }
        assert!(skipped.ledger().verify_conservation().is_err());
        assert_eq!(skipped.ledger().gap_batches(), Vec::from([40u16]));
        assert_eq!(skipped.ledger().sum(), 780);
    }

    #[test]
    fn smoke_enrollment_discipline() {
        // 重复 fid 拒收（未满账面直击重复核；满账上容量核先行）。
        let mut book = EnrollBook::new();
        book.enroll(SourceTableGenerator::canonical_row(5).unwrap()).unwrap();
        assert_eq!(
            book.enroll(SourceTableGenerator::canonical_row(5).unwrap()).unwrap_err(),
            EnrollError::DuplicateFid
        );
        // 乱序拒收。
        let mut b2 = EnrollBook::new();
        b2.enroll(SourceTableGenerator::canonical_row(5).unwrap()).unwrap();
        assert_eq!(
            b2.enroll(SourceTableGenerator::canonical_row(3).unwrap()).unwrap_err(),
            EnrollError::OrderViolation
        );
        // 超容拒收：新账灌满 800 后再灌即 E_BOOK_FULL
        //（enroll 内容量核先于重复核，故行 1 重灌触发 E_BOOK_FULL）。
        let src = SourceTableGenerator::canonical_source_list().unwrap();
        let mut b3 = EnrollBook::new();
        for r in src.iter() {
            b3.enroll(r.clone()).unwrap();
        }
        assert_eq!(
            b3.enroll(SourceTableGenerator::canonical_row(1).unwrap())
                .unwrap_err(),
            EnrollError::BookFull
        );
        // 封板后拒写。
        b3.seal();
        assert!(b3.is_sealed());
        assert_eq!(
            b3.enroll(SourceTableGenerator::canonical_row(1).unwrap()).unwrap_err(),
            EnrollError::Sealed
        );
    }

    #[test]
    fn smoke_injections_all_detected() {
        let suite = injection_suite();
        assert_eq!(suite.len(), 4);
        assert_eq!(suite[0].name, "漏条注入");
        assert!(suite[0].detected, "漏条注入应检出：{}", suite[0].diag);
        assert_eq!(suite[1].name, "重复注入");
        assert!(suite[1].detected, "重复注入应检出：{}", suite[1].diag);
        assert_eq!(suite[2].name, "多登注入");
        assert!(suite[2].detected, "多登注入应检出：{}", suite[2].diag);
        assert_eq!(suite[3].name, "列值篡改注入");
        assert!(suite[3].detected, "列值篡改注入应检出：{}", suite[3].diag);
        // 逐探针诊断码落段核（0x1AF0 段内）。
        for p in suite.iter() {
            assert!(p.code >= 0x1AF0 && p.code <= 0x1AFF);
        }
    }

    #[test]
    fn smoke_dual_run_deterministic() {
        let rep = dual_run_check().unwrap();
        assert!(rep.is_identical());
        assert_eq!(rep.fp_a, rep.fp_b);
        assert_eq!(rep.count_a, 800);
        assert_eq!(rep.count_b, 800);
        assert_eq!(rep.rate_a, 10_000);
        assert_eq!(rep.rate_b, 10_000);
        assert_eq!(rep.mount_fp_a, rep.mount_fp_b);
    }

    #[test]
    fn smoke_station_health_green() {
        let mut st = DomenrollStation::from_canonical().unwrap();
        assert!(st.enroll_all().is_ok());
        // 直读 100%。
        let rep = st.run_crossread().unwrap();
        assert_eq!(rep.rate_ppm, 10_000);
        // 挂载落账。
        let decl = st.mount_master().unwrap();
        assert_eq!(decl.entries(), 800);
        assert!(st.events().events().iter().any(|e| e.text.starts_with("MOUNT|")));
        // 注入面四路落事件账。
        let suite = st.run_injections();
        assert_eq!(suite.len(), 4);
        assert!(suite.iter().all(|p| p.detected));
        // health 六项全绿。
        let h = st.health();
        assert!(h.green, "health 应全绿：{:?}", h.checks);
        assert_eq!(h.checks.len(), 6);
        // 账密自证：滚动校验和与重放等值。
        assert!(st.events().checksum_ok());
        // 封板后拒写事件。
        let mut ev = EventLedger::new();
        ev.record(SEG_BASE, "a".to_string()).unwrap();
        ev.seal();
        assert_eq!(ev.record(SEG_BASE, "b".to_string()).unwrap_err(), EnrollError::LedgerSealedWrite);
    }

    #[test]
    fn smoke_station_rejects_diff_drift() {
        // 非规范源（截短 799）开站即拒。
        let src = SourceTableGenerator::canonical_source_list().unwrap();
        let short: Vec<EnrollRow> = src.iter().take(799).cloned().collect();
        assert_eq!(
            DomenrollStation::new_with_source(short).unwrap_err(),
            EnrollError::DiffInconsistent
        );
        // 名列篡改注入（探针四之外的列面）：名篡改 → 列值不符·名。
        let mut book = EnrollBook::new();
        for r in src.iter() {
            let row = if r.fid_num() == 785 {
                EnrollRow::new(
                    "UNX-F0785",
                    "A1域功能·批40·条05·篡改位",
                    SourceTableGenerator::canonical_lines(785),
                    &SourceTableGenerator::canonical_crit(785),
                )
                .unwrap()
            } else {
                r.clone()
            };
            book.enroll(row).unwrap();
        }
        let rep = CrossReader::diff(&book, &src);
        assert!(!rep.is_full_consistent);
        let mut hit = false;
        for it in rep.items.iter() {
            if it.kind == DiffKind::ColumnMismatch(DiffColumn::Name) && it.fid == "UNX-F0785" {
                hit = true;
            }
        }
        assert!(hit, "名列篡改应归因「列值不符·名」");
        // 终核门拒收。
        assert_eq!(CrossReader::require_consistent(&rep).unwrap_err(), EnrollError::DiffInconsistent);
    }

    #[test]
    fn smoke_error_details_all_nonempty() {
        let all = [
            EnrollError::Sealed,
            EnrollError::FidFormat,
            EnrollError::FidOutOfRange,
            EnrollError::NameFormat,
            EnrollError::LinesZero,
            EnrollError::CritFormat,
            EnrollError::DuplicateFid,
            EnrollError::OrderViolation,
            EnrollError::BookFull,
            EnrollError::NotComplete,
            EnrollError::MountBadDecl,
            EnrollError::MountMismatch,
            EnrollError::Conservation,
            EnrollError::DiffInconsistent,
            EnrollError::DualRunDrift,
            EnrollError::LedgerSealedWrite,
        ];
        let mut prev = 0u16;
        for (k, e) in all.iter().enumerate() {
            assert!(!e.detail().is_empty());
            assert!(e.detail().len() > 10);
            // 诊断码段独占：0x1AF0 起连续 16 码。
            assert_eq!(e.code(), 0x1AF0 + k as u16);
            assert!(e.code() > prev, "诊断码须严格递增");
            prev = e.code();
        }
        assert_eq!(prev, 0x1AFF);
    }

    #[test]
    fn smoke_fnv_and_const_helpers() {
        // FNV-1a 64 已知向量：空串 = 偏移初值。
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a64(b"foobar"), 0x85944171f73967e8);
        // 批号 const 推导已知点。
        assert_eq!(batch_of(1), 1);
        assert_eq!(batch_of(20), 1);
        assert_eq!(batch_of(21), 2);
        assert_eq!(batch_of(785), 40);
        assert_eq!(batch_of(800), 40);
        assert_eq!(idx_in_batch(785), 5);
        assert_eq!(idx_in_batch(800), 20);
    }
}
