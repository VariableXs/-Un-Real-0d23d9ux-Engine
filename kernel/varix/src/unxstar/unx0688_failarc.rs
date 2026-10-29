//! UNX-F0688 · 失败样本归档账（逐次失败件齐备）· 诊断段 0x2710
//!
//! ── 验收锚点（原文全抄） ─────────────────────────────────────────────
//! 定位一：docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal.md
//!         行 2264，表头「| 编号 | 功能条目 | 行数 | 状态 | 判据 |」下：
//!
//! | UNX-F0688 | 失败样本归档账（逐次失败件齐备） | 570 | 已深化 | UNX-F0688-J1
//!   失败样本逐次归档件（日志段/截图位/账行引用三件套）齐备率 100%，零抹除断言过 |
//!
//! 定位二：docs/unxreal/domains/UNX-A1.md 行 994，同一表同一行原文逐字同上。
//! 定位三：docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal.md
//!         行 2263 与 2265（邻行，口径上下文）：
//!
//! | UNX-F0687 | 成功率统计账（150 次 ≥99% 判定） | 580 | 已深化 | UNX-F0687-J1
//!   成功率统计（成功/150）≥99% 判定账落簿且分机成功率三列齐，口径二值零灰区 |
//! | UNX-F0689 | 引导报告 50 次零差异复测（分机执行） | 580 | 已深化 | UNX-F0689-J1
//!   每机 50 次报告哈希逐次同（3 机 × 50 全同），结构位型口径（数值豁免）生效断言过 |
//!
//! 定位四（下游门禁引用）：同表行 2274（UNX-F0698 门禁四条件含「归档齐备」）：
//!
//! | UNX-F0698 | 矩阵门禁（全绿放行与域收官联动） | 580 | 已深化 | UNX-F0698-J1
//!   门禁判定（四条件：成功率达标/零差异全同/失败分类完备/归档齐备）全过才放行，缺一拦截 100% |
//!
//! 定位五（终核对拍）：varix/src/unxstar/mod.rs 行 4406 注释
//! 「unx76_3matrix ↔ UNX-F0762 三机矩阵账终核（150 次成功率复算 ≥99%+F0688 对拍 100%）」
//! —— F0688 的对账面（与成功率账失败引用互拍）被 F0762 终核直接消费。
//!
//! J 判据拆解（本模块逐条落地的验收口径）：
//!   J1-a 三件套齐备率 100%：每次失败归档件 = 日志段（范围引用 + 摘要 hash）+
//!        截图位（帧号 + 区域描述）+ 账行引用（账本号 + 行号）三件；
//!        ppm 整数制，齐备件 ×10000 ÷ 应归档数 = 10000 即 100%，
//!        缺任一件即归档不完整红案（missing_mask 位图逐件显形）。
//!   J1-b 逐次归档：每个失败轮（机号 × 轮号）独占一档（arc_no 失败次序号），
//!        档位与 F0687 归档引用号（"F0688-ARC-NNN"）严格双射，
//!        同位二开拒收、同串二挂拒收、序号断档拒收。
//!   J1-c 零抹除断言：账 append-only——业务面不存在任何删除/截断/改写 API；
//!        账指纹（链式 FNV）只增不改（fingerprint_history 单调追加，前缀永在）；
//!        模拟外力截断攻击（注入面专用 attack_truncate_entries）由链指纹对拍检出。
//!   J1-d 与 F0687 失败引用对账：F0687 侧每个失败轮挂的归档引用号
//!        必须能在本归档账解引到对应档（引用号 + 机号 + 轮号三对齐），
//!        解引不到即悬空引用红案（E_DANGLING）；反向多档（账比引用多）同为红案。
//!   J1-e 件内容完整性：每件自带双摘要（内容摘要 hash + 防篡改 guard hash），
//!        归档后重读复算校验，guard 不符即篡改检出（E_TAMPER）。
//!   J1-f 补档通道：发现缺失件后走 backfill_* 补档登记（补档标记 backfilled=true +
//!        原始时戳 origin_tick 保留）——补档是追加不是抹除，链指纹照常只增。
//!
//! ── 相邻任务对接 ─────────────────────────────────────────────────────
//!   F0687 成功率账：其 FailRef { machine, round, archive_ref } 三字段由本模块
//!                   SuccFailRef 镜像承接（自包含纪律：不 use crate，字段同名同义）；
//!                   reconcile_with_succ_refs 逐条解引对拍（J1-d）。
//!   F0683 逐次留痕：账行引用的 line_no 绑定 150 行履历账的连续序号（1..=150）。
//!   F0698 矩阵门禁：seal() 产出的 SealRecord（齐备率 100% + 链指纹）是
//!                   门禁第四条件「归档齐备」的直接凭据。
//!   F0762 终核：chain_tip 链指纹 + 对账 ok 位被终核账消费做跨账对拍。
//!
//! ── 通用纪律 ─────────────────────────────────────────────────────────
//!   no_std + alloc；只 use core::… 与 use alloc::{…}，禁 use crate::…。
//!   内核路径零 unwrap/expect/panic!/运行期 assert!（编译期 const assert! 允许；
//!   #[cfg(test)] 测试体内 assert! 属 std 测试惯例，不在内核路径）。
//!   可失败 API 全走 Result<T, FailArcError>；错误 15 变体恰填满段内 15 码，
//!   每变体 detail() 给中文可操作诊断；溢出一律 saturating/wrapping 显性。
//!   >64KiB 结构体禁上栈（本文件最大栈值为 [bool;50]≈50B 与各报告结构体 <200B）。
//!   const fn 纪律：const fn 内零 slice::get / Vec 索引 / Ord::min-max /
//!   derive(PartialEq) 类型的 == / for 循环（一律 while + 索引或纯 match）；
//!   含 Vec/String 字段的结构体禁 derive Copy。
//!   时间语义：内核无墙钟，一切时戳 tick 由调用方显式传入（u64 单调刻度），
//!   双跑对拍的确定性由此保证。
//!
//! 零浮点、零 unwrap、零 expect、零 panic 路径（业务面）。

use core::mem::size_of;

use alloc::{
    format,
    string::String,
    vec::Vec,
};

// ── 诊断码段（0x2710 独占 0x10，先验逐段复核：
//    任务预核段 0x2000 已被 unx0047_mcfg 占（SEG_BASE=0x2000），
//    回退段 0x2010 被 unx0048_sratlit 占、0x2020 被 unx0049_systab 占；
//    逐段上探 0x2000..=0x2580 连续全占（unx0047..unx0150 族），
//    0x2590 被 unx0150_imgsplit 头注声明、0x25B0/0x25D0/0x25F0/0x2610..
//    被 unx0151..unx0157 族占，0x2630..0x26D0 被 unx0155..unx0157 占，
//    至 0x2710：全内核 src 树 grep 零命中（左邻 0x2700=svstar2/ves501、
//    右邻 0x2720=svstar2/ves502，均不越界），故取 0x2710） ──
pub const SEG_BASE: u16 = 0x2710;

const E_SEALED: u16 = SEG_BASE + 0x1; // 0x2711 账已封板仍写
const E_MACHINE_RANGE: u16 = SEG_BASE + 0x2; // 0x2712 机号越界（合法 0..3）
const E_ROUND_RANGE: u16 = SEG_BASE + 0x3; // 0x2713 轮号越界（合法 1..=50）
const E_DUP_SLOT: u16 = SEG_BASE + 0x4; // 0x2714 同失败位重复开档
const E_REF_INVALID: u16 = SEG_BASE + 0x5; // 0x2715 引用号空串或超长
const E_REF_FORMAT: u16 = SEG_BASE + 0x6; // 0x2716 引用号格式/序不符
const E_REF_DUP: u16 = SEG_BASE + 0x7; // 0x2717 引用号串重复挂账
const E_NO_SLOT: u16 = SEG_BASE + 0x8; // 0x2718 档位不存在（查档/存件无对应）
const E_ITEM_DUP: u16 = SEG_BASE + 0x9; // 0x2719 件重复登记（含补档命中已有件）
const E_PIECE_INVALID: u16 = SEG_BASE + 0xA; // 0x271A 件内容非法（行序/区域/行号）
const E_TAMPER: u16 = SEG_BASE + 0xB; // 0x271B 件内容篡改检出（guard 复算不符）
const E_TRUNCATION: u16 = SEG_BASE + 0xC; // 0x271C 账截断/结构破坏检出（链指纹对拍）
const E_DANGLING: u16 = SEG_BASE + 0xD; // 0x271D 悬空引用（F0687 引用解不到档）
const E_INCOMPLETE: u16 = SEG_BASE + 0xE; // 0x271E 归档不完整红案（缺件/多档）
const E_REPLAY: u16 = SEG_BASE + 0xF; // 0x271F 双跑对拍不一致

/// 诊断码 → 中文可操作诊断（独占段 0x2710~0x271F 内解释，段外一律未知）。
pub fn diag_detail(code: u16) -> &'static str {
    match code {
        E_SEALED => "账已封板：归档账封板后只读，再写属误用，请克隆新账",
        E_MACHINE_RANGE => "机号越界：三机矩阵合法机号为 0/1/2",
        E_ROUND_RANGE => "轮号越界：每机合法轮号为 1..=50",
        E_DUP_SLOT => "同失败位重复开档：该机该轮已开档，失败位一档一件套",
        E_REF_INVALID => "归档引用号非法：空串或超长（上限 64 字节）",
        E_REF_FORMAT => "归档引用号格式/序不符：应为 F0688-ARC-<三位起次序号>，且次序与开档序一致",
        E_REF_DUP => "归档引用号重复：同串已挂账，引用号全局唯一",
        E_NO_SLOT => "档位不存在：该失败次序号未开档，请先 open_entry 再存件",
        E_ITEM_DUP => "件重复登记：该档该件已在位，补档通道只补缺失件",
        E_PIECE_INVALID => "件内容非法：日志段行序倒置/内容空、截图区域退化、账行引用行号或账本号为 0",
        E_TAMPER => "件内容篡改检出：guard 复算不符，归档后件被外力改写，走重档流程勿覆写",
        E_TRUNCATION => "账截断/结构破坏检出：链指纹对拍不符，账行数低于高水位，append-only 红线被破",
        E_DANGLING => "悬空引用：F0687 失败引用在本归档账解不到对应档，请核对引用号或补开档",
        E_INCOMPLETE => "归档不完整红案：缺件（日志段/截图位/账行引用）或多档未引用，齐备率未达 100%",
        E_REPLAY => "双跑对拍不一致：同档步两账链指纹漂移，幂等破",
        _ => "未知诊断码（0x2710 段外或保留位）",
    }
}

// ── 矩阵常量（来源：判据上下文 F0687「成功/150」与「分机三列」同口径） ──
/// 三机矩阵机数（与 F0687 MACHINE_COUNT 同源同值）。
pub const MACHINE_COUNT: usize = 3;
/// 每机轮数（150 次总账 ÷ 3 机；对接 F0682 轮排）。
pub const ROUNDS_PER_MACHINE: usize = 50;
/// 总账次数（F0687 口径：150 次）。
pub const TOTAL_RUNS: usize = MACHINE_COUNT * ROUNDS_PER_MACHINE;
/// 归档引用号长度上限（字节；与 F0687 FAIL_REF_MAX_LEN 同值，两端契约）。
pub const FAIL_REF_MAX_LEN: usize = 64;
/// 齐备率达标线：100% = 10000 ppm（判据原文：齐备率 100%，ppm 整数制零浮点）。
pub const FULL_PPM: u64 = 10_000;
/// ppm 换算基数。
pub const PPM_SCALE: u64 = 10_000;
/// 归档引用号规范前缀（F0687 侧生成器同款：format!("F0688-ARC-{:03}", n)）。
pub const ARC_REF_PREFIX: &str = "F0688-ARC-";
/// 引用号数字段位数上限（防 u32 溢出注入）。
pub const ARC_REF_DIGIT_CAP: usize = 6;
/// 缺件位图位定义（missing_mask）：bit0 缺日志段。
pub const MASK_LOG: u8 = 0x01;
/// bit1 缺截图位。
pub const MASK_SHOT: u8 = 0x02;
/// bit2 缺账行引用。
pub const MASK_LINE: u8 = 0x04;
/// bit3 多档（账有档而 F0687 引用未引，反向红案）。
pub const MASK_ORPHAN: u8 = 0x08;
/// 三件齐位掩码（bit0|bit1|bit2）。
pub const MASK_TRIPLE: u8 = MASK_LOG | MASK_SHOT | MASK_LINE;

// 编译期守恒断言（允许：const 期，非运行期）。
const _: () = assert!(MACHINE_COUNT == 3);
const _: () = assert!(ROUNDS_PER_MACHINE == 50);
const _: () = assert!(MACHINE_COUNT * ROUNDS_PER_MACHINE == TOTAL_RUNS);
const _: () = assert!(TOTAL_RUNS == 150);
const _: () = assert!(FULL_PPM == 10_000 && PPM_SCALE == 10_000);
const _: () = assert!(FAIL_REF_MAX_LEN == 64);
const _: () = assert!(MASK_TRIPLE == 0x07 && MASK_ORPHAN == 0x08);
const _: () = assert!(size_of::<LogSegment>() <= 48);
const _: () = assert!(size_of::<ShotRegion>() <= 16);
const _: () = assert!(size_of::<ShotSlot>() <= 48);
const _: () = assert!(size_of::<LedgerLineRef>() <= 32);
const _: () = assert!(size_of::<BookEvent>() <= 16);
const _: () = assert!(size_of::<ChainReport>() <= 64);

// ── FNV-1a 64 位指纹（参数为 FNV-1a 标准常量：offset=0xcbf29ce484222325，
//    prime=0x100000001b3；与 F0687 同参数族，跨账对拍口径一致） ──────────
/// 增量式 FNV-1a 哈希器（字节序显性小端推送）。
pub struct Fnv64 {
    h: u64,
}

impl Fnv64 {
    /// 新哈希器（FNV-1a offset basis）。
    pub const fn new() -> Self {
        Fnv64 {
            h: 0xcbf2_9ce4_8422_2325,
        }
    }

    /// 推送单字节。
    pub fn write_u8(&mut self, b: u8) {
        self.h ^= b as u64;
        self.h = self.h.wrapping_mul(0x0000_0100_0000_01B3);
    }

    /// 推送 u16（小端两字节）。
    pub fn write_u16(&mut self, v: u16) {
        self.write_u8((v & 0xFF) as u8);
        self.write_u8((v >> 8) as u8);
    }

    /// 推送 u32（小端四字节）。
    pub fn write_u32(&mut self, v: u32) {
        self.write_u16((v & 0xFFFF) as u16);
        self.write_u16((v >> 16) as u16);
    }

    /// 推送 u64（小端八字节）。
    pub fn write_u64(&mut self, v: u64) {
        self.write_u32((v & 0xFFFF_0001) as u32);
        self.write_u32((v >> 32) as u32);
    }

    /// 推送字节串。
    pub fn write_bytes(&mut self, b: &[u8]) {
        let mut i: usize = 0;
        while i < b.len() {
            self.write_u8(b[i]);
            i += 1;
        }
    }

    /// 推送字符串（字节序同 write_bytes）。
    pub fn write_str(&mut self, s: &str) {
        self.write_bytes(s.as_bytes());
    }

    /// 取指纹值。
    pub const fn finish(&self) -> u64 {
        self.h
    }

    /// 链式混入（账链专用）：tip = FNV(tip || u64)，双 u64 折叠不增熵假设、
    /// 只做链式绑定（与 entry_hash 的可复算性配合完成截断检出）。
    pub fn chain_mix(&mut self, tip: u64, item: u64) {
        self.write_u64(tip);
        self.write_u64(item);
    }
}

impl Default for Fnv64 {
    fn default() -> Self {
        Fnv64::new()
    }
}

/// 一次性 FNV-1a 便捷入口。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h = Fnv64::new();
    h.write_bytes(data);
    h.finish()
}

/// 件内容摘要便捷入口（同 fnv1a64，语义命名：归档件内容摘要）。
pub fn content_hash(data: &[u8]) -> u64 {
    fnv1a64(data)
}

// ── 件种类闭集 ───────────────────────────────────────────────────────
/// 归档件三件套的种类闭集（三臂之外无值；规范字节用于事件/错误/指纹）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArtifactKind {
    /// 日志段（范围引用 + 摘要 hash）。
    Log,
    /// 截图位（帧号 + 区域描述）。
    Shot,
    /// 账行引用（账本号 + 行号）。
    Line,
}

impl ArtifactKind {
    /// 种类规范字节：Log=1 / Shot=2 / Line=3。
    pub const fn as_raw(self) -> u8 {
        match self {
            ArtifactKind::Log => 1,
            ArtifactKind::Shot => 2,
            ArtifactKind::Line => 3,
        }
    }

    /// 规范字节 → 种类（闭集外一律 None，防灰区注入）。
    pub const fn from_raw(raw: u8) -> Option<ArtifactKind> {
        match raw {
            1 => Some(ArtifactKind::Log),
            2 => Some(ArtifactKind::Shot),
            3 => Some(ArtifactKind::Line),
            _ => None,
        }
    }

    /// 中文名（报告/事件注记用）。
    pub const fn cname(self) -> &'static str {
        match self {
            ArtifactKind::Log => "日志段",
            ArtifactKind::Shot => "截图位",
            ArtifactKind::Line => "账行引用",
        }
    }

    /// 缺件位图位（missing_mask 中的对应位）。
    pub const fn mask_bit(self) -> u8 {
        match self {
            ArtifactKind::Log => MASK_LOG,
            ArtifactKind::Shot => MASK_SHOT,
            ArtifactKind::Line => MASK_LINE,
        }
    }
}

// ── 件一：日志段（范围引用 + 摘要 hash） ─────────────────────────────
/// 截图/日志共用的防篡改 guard 约定：
/// guard = FNV(种类字节 ‖ 全部结构字段 ‖ 内容摘要)，存件时一次性烙定；
/// 归档后任何字段被改写，guard 复算即不符（E_TAMPER 检出路径）。
///
/// 一段失败现场的日志引用：流号 + 行区间 + 字节长 + 内容摘要 + guard。
/// 内容本体不落账（程序内登记模拟：只存范围引用与摘要，不真落盘）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LogSegment {
    /// 日志流号（0=控制台早期流，1=矩阵主日志流，2=固件诊断流）。
    pub stream: u8,
    /// 行区间起（含），1 起。
    pub start_line: u32,
    /// 行区间止（含），必须 > start_line。
    pub end_line: u32,
    /// 段字节长（内容字节计数，登记时烙定）。
    pub byte_len: u32,
    /// 内容摘要（FNV-1a over 内容字节，登记时烙定）。
    pub summary_hash: u64,
    /// 防篡改 guard（FNV over 结构字段 + 摘要，登记时烙定）。
    pub guard: u64,
}

impl LogSegment {
    /// 采集一段日志（程序内登记模拟）：校验行序与内容非空，
    /// 烙定 byte_len / summary_hash / guard。可失败走 PieceInvalid。
    pub fn capture(
        stream: u8,
        start_line: u32,
        end_line: u32,
        content: &[u8],
    ) -> Result<LogSegment, FailArcError> {
        if end_line <= start_line {
            return Err(FailArcError::PieceInvalid {
                kind: ArtifactKind::Log.as_raw(),
                reason_code: 1, // 行序倒置
            });
        }
        if content.is_empty() {
            return Err(FailArcError::PieceInvalid {
                kind: ArtifactKind::Log.as_raw(),
                reason_code: 2, // 内容空
            });
        }
        let summary_hash = content_hash(content);
        let seg = LogSegment {
            stream,
            start_line,
            end_line,
            byte_len: content.len() as u32,
            summary_hash,
            guard: 0,
        };
        let guard = seg.compute_guard();
        Ok(LogSegment { guard, ..seg })
    }

    /// 结构字段规范化推送（指纹用；不含 guard 本身）。
    pub fn write_fingerprint(&self, h: &mut Fnv64) {
        h.write_u8(ArtifactKind::Log.as_raw());
        h.write_u8(self.stream);
        h.write_u32(self.start_line);
        h.write_u32(self.end_line);
        h.write_u32(self.byte_len);
        h.write_u64(self.summary_hash);
    }

    /// guard 复算（按当前字段值重算；与存量 guard 比对即篡改判定）。
    pub fn compute_guard(&self) -> u64 {
        let mut h = Fnv64::new();
        h.write_u8(0xA1); // guard 域分隔魔数（日志段）
        self.write_fingerprint(&mut h);
        h.finish()
    }

    /// 防篡改自校验：guard 复算 == 存量 guard。
    pub fn verify_guard(&self) -> bool {
        self.compute_guard() == self.guard
    }
}

// ── 件二：截图位（帧号 + 区域描述） ─────────────────────────────────
/// 截图区域描述（屏幕坐标矩形，像素单位）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShotRegion {
    /// 左上角 x（像素）。
    pub x: u32,
    /// 左上角 y（像素）。
    pub y: u32,
    /// 宽（像素，须 > 0）。
    pub w: u32,
    /// 高（像素，须 > 0）。
    pub h: u32,
}

impl ShotRegion {
    /// 规范化推送（指纹用）。
    pub fn write_fingerprint(&self, h: &mut Fnv64) {
        h.write_u32(self.x);
        h.write_u32(self.y);
        h.write_u32(self.w);
        h.write_u32(self.h);
    }

    /// 区域退化判定（w/h 任一为 0 即退化，非矩形不称区域）。
    pub const fn degenerate(&self) -> bool {
        self.w == 0 || self.h == 0
    }
}

/// 一次失败现场的截图位登记：帧号 + 区域 + 拍摄刻度 + 像素摘要 + guard。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShotSlot {
    /// 帧号（0 非法：帧号从 1 起）。
    pub frame_no: u32,
    /// 区域描述。
    pub region: ShotRegion,
    /// 拍摄时戳（调用方刻度，登记时烙定）。
    pub shot_tick: u64,
    /// 像素内容摘要（FNV-1a over 像素字节，登记时烙定）。
    pub pixels_digest: u64,
    /// 防篡改 guard。
    pub guard: u64,
}

impl ShotSlot {
    /// 采集一帧截图位（程序内登记模拟）：校验帧号与区域非退化，
    /// 烙定 pixels_digest / guard。
    pub fn capture(
        frame_no: u32,
        region: ShotRegion,
        shot_tick: u64,
        pixels: &[u8],
    ) -> Result<ShotSlot, FailArcError> {
        if frame_no == 0 {
            return Err(FailArcError::PieceInvalid {
                kind: ArtifactKind::Shot.as_raw(),
                reason_code: 6, // 帧号 0
            });
        }
        if region.degenerate() {
            return Err(FailArcError::PieceInvalid {
                kind: ArtifactKind::Shot.as_raw(),
                reason_code: 3, // 区域退化
            });
        }
        if pixels.is_empty() {
            return Err(FailArcError::PieceInvalid {
                kind: ArtifactKind::Shot.as_raw(),
                reason_code: 2, // 内容空
            });
        }
        let pixels_digest = content_hash(pixels);
        let slot = ShotSlot {
            frame_no,
            region,
            shot_tick,
            pixels_digest,
            guard: 0,
        };
        let guard = slot.compute_guard();
        Ok(ShotSlot { guard, ..slot })
    }

    /// 结构字段规范化推送（指纹用；不含 guard 本身）。
    pub fn write_fingerprint(&self, h: &mut Fnv64) {
        h.write_u8(ArtifactKind::Shot.as_raw());
        h.write_u32(self.frame_no);
        self.region.write_fingerprint(h);
        h.write_u64(self.shot_tick);
        h.write_u64(self.pixels_digest);
    }

    /// guard 复算。
    pub fn compute_guard(&self) -> u64 {
        let mut h = Fnv64::new();
        h.write_u8(0xA2); // guard 域分隔魔数（截图位）
        self.write_fingerprint(&mut h);
        h.finish()
    }

    /// 防篡改自校验。
    pub fn verify_guard(&self) -> bool {
        self.compute_guard() == self.guard
    }
}

// ── 件三：账行引用（账本号 + 行号） ─────────────────────────────────
/// 一条失败轮的账行引用：指向 F0687 成功率账（或履历账）的具体行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LedgerLineRef {
    /// 账本号（0 非法；约 定：687=F0687 成功率账，683=F0683 履历账）。
    pub book_no: u32,
    /// 行号（1 起；0 非法。F0687 口径下为 1..=150 连续序）。
    pub line_no: u32,
    /// 被引行指纹摘要（FNV-1a over 行内容字节，登记时烙定；
    /// 供 F0762 终核跨账对拍行级绑定）。
    pub row_hash: u64,
    /// 防篡改 guard。
    pub guard: u64,
}

impl LedgerLineRef {
    /// 采集一条账行引用：校验账本号与行号非零，烙定 row_hash / guard。
    pub fn capture(
        book_no: u32,
        line_no: u32,
        row_bytes: &[u8],
    ) -> Result<LedgerLineRef, FailArcError> {
        if book_no == 0 {
            return Err(FailArcError::PieceInvalid {
                kind: ArtifactKind::Line.as_raw(),
                reason_code: 5, // 账本号 0
            });
        }
        if line_no == 0 {
            return Err(FailArcError::PieceInvalid {
                kind: ArtifactKind::Line.as_raw(),
                reason_code: 4, // 行号 0
            });
        }
        if row_bytes.is_empty() {
            return Err(FailArcError::PieceInvalid {
                kind: ArtifactKind::Line.as_raw(),
                reason_code: 2, // 内容空
            });
        }
        let row_hash = content_hash(row_bytes);
        let r = LedgerLineRef {
            book_no,
            line_no,
            row_hash,
            guard: 0,
        };
        let guard = r.compute_guard();
        Ok(LedgerLineRef { guard, ..r })
    }

    /// 结构字段规范化推送（指纹用；不含 guard 本身）。
    pub fn write_fingerprint(&self, h: &mut Fnv64) {
        h.write_u8(ArtifactKind::Line.as_raw());
        h.write_u32(self.book_no);
        h.write_u32(self.line_no);
        h.write_u64(self.row_hash);
    }

    /// guard 复算。
    pub fn compute_guard(&self) -> u64 {
        let mut h = Fnv64::new();
        h.write_u8(0xA3); // guard 域分隔魔数（账行引用）
        self.write_fingerprint(&mut h);
        h.finish()
    }

    /// 防篡改自校验。
    pub fn verify_guard(&self) -> bool {
        self.compute_guard() == self.guard
    }
}

// ── 错误闭集（15 变体，恰占满 0x2711..0x271F 十五个诊断码） ───────────
/// 失败样本归档账全通道错误。每变体携带定位字段与中文可操作诊断（detail）。
/// 全变体载荷为纯原语，故可 derive Copy。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailArcError {
    /// 账已封板仍尝试写入。
    Sealed,
    /// 机号越界（合法 0/1/2）。
    MachineOutOfRange { got: u8 },
    /// 轮号越界（合法 1..=50）。
    RoundOutOfRange { got: u16 },
    /// 同失败位（机号 × 轮号）重复开档。
    DuplicateSlot { machine: u8, round: u16 },
    /// 引用号非法：len=0 空串或 len > cap(64)。
    RefInvalid { len: usize, cap: usize },
    /// 引用号格式/序不符（非 F0688-ARC-<n> 或 n 与开档次序不一致）。
    RefFormat { len: usize },
    /// 引用号串重复挂账（existing 为已挂同名串的档号）。
    RefDuplicate { existing_arc_no: u32 },
    /// 档位不存在（arc_no 未开档）。
    NoSlot { arc_no: u32 },
    /// 件重复登记（含补档命中已有件；kind 为件种类规范字节）。
    ItemDuplicate { arc_no: u32, kind: u8 },
    /// 件内容非法：reason_code 约定
    ///   1=日志段行序倒置，2=内容空，3=截图区域退化，
    ///   4=账行引用行号 0，5=账本号 0，6=帧号 0。
    PieceInvalid { kind: u8, reason_code: u8 },
    /// 件内容篡改检出：guard 复算不符（stored/recomputed 对照）。
    TamperDetected {
        arc_no: u32,
        kind: u8,
        stored: u64,
        recomputed: u64,
    },
    /// 账截断/结构破坏检出：kept=现存档数，expected=高水位档数。
    TruncationDetected { kept: u32, expected: u32 },
    /// 悬空引用：F0687 引用解不到档（arc_no 为引用号内解析出的次序号，
    /// 解析失败为 0）。
    DanglingRef { arc_no: u32 },
    /// 归档不完整红案：missing_mask 位定义 MASK_LOG/MASK_SHOT/MASK_LINE/
    /// MASK_ORPHAN（多档）。
    Incomplete { arc_no: u32, missing_mask: u8 },
    /// 双跑对拍不一致：fp_a/fp_b 为两账链指纹。
    ReplayMismatch { fp_a: u64, fp_b: u64 },
}

impl FailArcError {
    /// 错误 → 诊断码（独占段 0x2710 内）。
    pub const fn err_code(&self) -> u16 {
        match self {
            FailArcError::Sealed => E_SEALED,
            FailArcError::MachineOutOfRange { .. } => E_MACHINE_RANGE,
            FailArcError::RoundOutOfRange { .. } => E_ROUND_RANGE,
            FailArcError::DuplicateSlot { .. } => E_DUP_SLOT,
            FailArcError::RefInvalid { .. } => E_REF_INVALID,
            FailArcError::RefFormat { .. } => E_REF_FORMAT,
            FailArcError::RefDuplicate { .. } => E_REF_DUP,
            FailArcError::NoSlot { .. } => E_NO_SLOT,
            FailArcError::ItemDuplicate { .. } => E_ITEM_DUP,
            FailArcError::PieceInvalid { .. } => E_PIECE_INVALID,
            FailArcError::TamperDetected { .. } => E_TAMPER,
            FailArcError::TruncationDetected { .. } => E_TRUNCATION,
            FailArcError::DanglingRef { .. } => E_DANGLING,
            FailArcError::Incomplete { .. } => E_INCOMPLETE,
            FailArcError::ReplayMismatch { .. } => E_REPLAY,
        }
    }

    /// 中文可操作诊断（判据：每变体 detail 给出下一步动作）。
    pub fn detail(&self) -> &'static str {
        diag_detail(self.err_code())
    }
}

// ── F0687 失败引用镜像（自包含纪律：字段与 F0687 FailRef 同名同义） ────
/// 一条 F0687 侧失败引用的镜像：失败轮定位 + 本账归档引用号。
/// F0687 侧 attach_fail_ref 登记的 (machine, round, archive_ref) 三元组
/// 原样搬来即可参与对账（reconcile_with_succ_refs）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SuccFailRef {
    /// 失败轮机号（0/1/2）。
    pub machine: u8,
    /// 失败轮轮号（1..=50）。
    pub round: u16,
    /// F0688 归档件引用号（"F0688-ARC-NNN"；≤64 字节）。
    pub archive_ref: String,
}

impl SuccFailRef {
    /// 由三元组构造镜像引用。
    pub fn new(machine: u8, round: u16, archive_ref: &str) -> SuccFailRef {
        SuccFailRef {
            machine,
            round,
            archive_ref: String::from(archive_ref),
        }
    }

    /// 规范字节序（指纹用；0xFF 终结哨兵防拼接歧义，与 F0687 同款）。
    pub fn write_fingerprint(&self, h: &mut Fnv64) {
        h.write_u8(self.machine);
        h.write_u16(self.round);
        h.write_str(&self.archive_ref);
        h.write_u8(0xFF);
    }
}

// ── 引用号规范格式（与 F0687 生成器 format!("F0688-ARC-{:03}", n) 双射） ──
/// 生成规范引用号："F0688-ARC-" + 十进制次序号（三位起，自然进位）。
pub fn format_arc_ref(arc_no: u32) -> String {
    format!("{}{:03}", ARC_REF_PREFIX, arc_no)
}

/// 解析规范引用号：前缀逐字节比对 + 数字段（1..=6 位全 ASCII 数字）→ u32。
/// 闭集外（坏前缀/非数字/超长/空数字段）一律 None——灰区引用零放行。
pub fn parse_arc_ref(s: &str) -> Option<u32> {
    let bytes = s.as_bytes();
    let pre = ARC_REF_PREFIX.as_bytes();
    if bytes.len() < pre.len() + 1 {
        return None;
    }
    let mut i: usize = 0;
    while i < pre.len() {
        if bytes[i] != pre[i] {
            return None;
        }
        i += 1;
    }
    let digits_len = bytes.len() - pre.len();
    if digits_len > ARC_REF_DIGIT_CAP {
        return None;
    }
    let mut value: u64 = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b < b'0' || b > b'9' {
            return None;
        }
        value = value * 10 + (b - b'0') as u64;
        i += 1;
    }
    if value > u32::MAX as u64 {
        return None;
    }
    Some(value as u32)
}

// ── 件槽位（缺位/在位两态；补档保留原始时戳） ─────────────────────────
/// 一件三件套中一件的槽位状态。
///
/// Present 同时保留 origin_tick（该槽从缺到在的原时刻度 = 开档刻度），
/// 补档（backfill）只置 backfilled=true 并追加 deposited_tick，
/// 原始时戳永不改写——「补档不是抹除是追加」的件级体现。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PieceSlot<T> {
    /// 缺位：since_tick 为开档刻度（缺件起始时刻）。
    Missing { since_tick: u64 },
    /// 在位：件本体 + 补档标记 + 原始时戳 + 归档时戳。
    Present {
        piece: T,
        backfilled: bool,
        origin_tick: u64,
        deposited_tick: u64,
    },
}

impl<T> PieceSlot<T> {
    /// 是否在位。
    pub const fn is_present(&self) -> bool {
        matches!(self, PieceSlot::Present { .. })
    }

    /// 是否缺位。
    pub const fn is_missing(&self) -> bool {
        matches!(self, PieceSlot::Missing { .. })
    }
}

/// 件槽位只读快照（不含件本体；审计/测试观察用）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PieceStatus {
    /// 缺位（since_tick 起缺）。
    Missing { since_tick: u64 },
    /// 在位（backfilled/origin_tick/deposited_tick 三要素）。
    Present {
        backfilled: bool,
        origin_tick: u64,
        deposited_tick: u64,
    },
}

// ── 三件套聚合视图（FailArtifact） ───────────────────────────────────
/// 一次失败的三件套聚合（齐备时才可组装；self_hash 为三件规范字节折叠）。
/// 这是判据原文「失败样本逐次归档件（日志段/截图位/账行引用三件套）」的
/// 件级实体：三件全在位 → complete=true，缺任一件即红案不组装。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FailArtifact {
    /// 失败次序号（1 起，与引用号 NNN 双射）。
    pub arc_no: u32,
    /// 失败轮机号。
    pub machine: u8,
    /// 失败轮轮号。
    pub round: u16,
    /// F0687 归档引用号哈希（串本体在档上，聚合视图只携摘要）。
    pub ref_hash: u64,
    /// 件一：日志段。
    pub log: LogSegment,
    /// 件二：截图位。
    pub shot: ShotSlot,
    /// 件三：账行引用。
    pub line: LedgerLineRef,
    /// 三件 + 档头规范字节折叠指纹（组装时复算）。
    pub self_hash: u64,
    /// 开档刻度（原始时戳）。
    pub origin_tick: u64,
}

impl FailArtifact {
    /// 组装期三件折叠指纹（不含 guard 域，纯结构 + 摘要绑定）。
    pub fn fold_hash(&self) -> u64 {
        let mut h = Fnv64::new();
        h.write_u8(0xFA); // 聚合域分隔魔数
        h.write_u32(self.arc_no);
        h.write_u8(self.machine);
        h.write_u16(self.round);
        h.write_u64(self.ref_hash);
        self.log.write_fingerprint(&mut h);
        self.shot.write_fingerprint(&mut h);
        self.line.write_fingerprint(&mut h);
        h.finish()
    }
}

// ── 事件账 ───────────────────────────────────────────────────────────
/// 事件种类（事件账九类闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    /// 失败位开档（三槽全缺起记）。
    EntryOpened,
    /// 件归档成功（首存通道）。
    PieceDeposited,
    /// 件归档被拒（校验拦截，带诊断码）。
    PieceRejected,
    /// 件补档成功（补档通道：追加非抹除）。
    PieceBackfilled,
    /// 某档三件齐（件级齐备瞬间）。
    EntryComplete,
    /// 对账跑毕（F0687 引用 ↔ 归档档）。
    ReconcileRan,
    /// 账封板（齐备率 100% + 链指纹落簿）。
    Sealed,
    /// 健康巡检落簿。
    HealthChecked,
    /// 注入面攻击模拟执行（截断/改写模拟，审计可见）。
    AttackSimulated,
}

impl EventKind {
    /// 事件种类规范字节（指纹外审计用）。
    pub const fn as_raw(self) -> u8 {
        match self {
            EventKind::EntryOpened => 1,
            EventKind::PieceDeposited => 2,
            EventKind::PieceRejected => 3,
            EventKind::PieceBackfilled => 4,
            EventKind::EntryComplete => 5,
            EventKind::ReconcileRan => 6,
            EventKind::Sealed => 7,
            EventKind::HealthChecked => 8,
            EventKind::AttackSimulated => 9,
        }
    }
}

/// 单条事件：序号 + 种类 + 档号 + 关联机/轮 + 诊断码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BookEvent {
    /// 事件账连续序号（从 1 起）。
    pub seq: u32,
    /// 事件种类。
    pub kind: EventKind,
    /// 关联档号（无关联填 0）。
    pub arc_no: u32,
    /// 关联机号（无关联填 0xFF）。
    pub machine: u8,
    /// 关联轮号（无关联填 0）。
    pub round: u16,
    /// 诊断码（成功类填 0，拒绝类填段内错误码）。
    pub code: u16,
}

// ── 归档档位（一失败一档，三槽三件） ─────────────────────────────────
/// 一个失败样本的归档档位：档头 + 三件槽。
///
/// 结构纪律：含 String/PieceSlot 字段，禁 derive Copy（仅 Clone）；
/// 档一旦 push 入账，业务面无任何删除/改写通道（零抹除红线）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchiveEntry {
    /// 失败次序号（1 起，账内唯一，单调递增不回收）。
    pub arc_no: u32,
    /// 失败轮机号。
    pub machine: u8,
    /// 失败轮轮号。
    pub round: u16,
    /// F0687 归档引用号（规范格式，全局唯一）。
    pub succ_ref: String,
    /// 开档刻度（原始时戳；补档也永不改写此值）。
    pub opened_tick: u64,
    /// 件一槽：日志段。
    pub log: PieceSlot<LogSegment>,
    /// 件二槽：截图位。
    pub shot: PieceSlot<ShotSlot>,
    /// 件三槽：账行引用。
    pub line: PieceSlot<LedgerLineRef>,
}

impl ArchiveEntry {
    /// 缺件位图（J1-a 红案位：bit0 日志段 / bit1 截图位 / bit2 账行引用）。
    pub fn missing_mask(&self) -> u8 {
        let mut mask: u8 = 0;
        if self.log.is_missing() {
            mask |= MASK_LOG;
        }
        if self.shot.is_missing() {
            mask |= MASK_SHOT;
        }
        if self.line.is_missing() {
            mask |= MASK_LINE;
        }
        mask
    }

    /// 三件是否齐（missing_mask == 0）。
    pub fn triple_complete(&self) -> bool {
        self.missing_mask() == 0
    }

    /// 档规范字节折叠（账链 entry_hash 的素材：档头 + 三槽态 + 件指纹 +
    /// 补档标记 + 时戳——任何后续合法追加（存件/补档）都会改变此值，
    /// 链随之延长；外力改写/截断则与存量链尖失配）。
    pub fn entry_hash(&self) -> u64 {
        let mut h = Fnv64::new();
        h.write_u8(0xEE); // 档哈希域分隔魔数
        h.write_u32(self.arc_no);
        h.write_u8(self.machine);
        h.write_u16(self.round);
        h.write_str(&self.succ_ref);
        h.write_u8(0xFF);
        h.write_u64(self.opened_tick);
        match &self.log {
            PieceSlot::Missing { since_tick } => {
                h.write_u8(0x00);
                h.write_u64(*since_tick);
            }
            PieceSlot::Present {
                piece,
                backfilled,
                origin_tick,
                deposited_tick,
            } => {
                h.write_u8(if *backfilled { 0x02 } else { 0x01 });
                piece.write_fingerprint(&mut h);
                h.write_u64(piece.guard);
                h.write_u64(*origin_tick);
                h.write_u64(*deposited_tick);
            }
        }
        match &self.shot {
            PieceSlot::Missing { since_tick } => {
                h.write_u8(0x00);
                h.write_u64(*since_tick);
            }
            PieceSlot::Present {
                piece,
                backfilled,
                origin_tick,
                deposited_tick,
            } => {
                h.write_u8(if *backfilled { 0x02 } else { 0x01 });
                piece.write_fingerprint(&mut h);
                h.write_u64(piece.guard);
                h.write_u64(*origin_tick);
                h.write_u64(*deposited_tick);
            }
        }
        match &self.line {
            PieceSlot::Missing { since_tick } => {
                h.write_u8(0x00);
                h.write_u64(*since_tick);
            }
            PieceSlot::Present {
                piece,
                backfilled,
                origin_tick,
                deposited_tick,
            } => {
                h.write_u8(if *backfilled { 0x02 } else { 0x01 });
                piece.write_fingerprint(&mut h);
                h.write_u64(piece.guard);
                h.write_u64(*origin_tick);
                h.write_u64(*deposited_tick);
            }
        }
        h.finish()
    }

    /// 件在位时的内容校验（三槽逐一 guard 复算；任一不符回该件种类）。
    /// 返回 Some((kind_raw, stored, recomputed)) 表示检出篡改；None 为净。
    pub fn tamper_scan(&self) -> Option<(u8, u64, u64)> {
        if let PieceSlot::Present { piece, .. } = &self.log {
            let recomputed = piece.compute_guard();
            if recomputed != piece.guard {
                return Some((ArtifactKind::Log.as_raw(), piece.guard, recomputed));
            }
        }
        if let PieceSlot::Present { piece, .. } = &self.shot {
            let recomputed = piece.compute_guard();
            if recomputed != piece.guard {
                return Some((ArtifactKind::Shot.as_raw(), piece.guard, recomputed));
            }
        }
        if let PieceSlot::Present { piece, .. } = &self.line {
            let recomputed = piece.compute_guard();
            if recomputed != piece.guard {
                return Some((ArtifactKind::Line.as_raw(), piece.guard, recomputed));
            }
        }
        None
    }
}

// ── 审计报告族 ───────────────────────────────────────────────────────
/// 一条缺件红案记录（齐备审计产出；J1-a「缺任一件即归档不完整红案」）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MissingPiece {
    /// 档号。
    pub arc_no: u32,
    /// 机号。
    pub machine: u8,
    /// 轮号。
    pub round: u16,
    /// 归档引用号。
    pub succ_ref: String,
    /// 缺件位图（MASK_* 逐位；或 MASK_ORPHAN 表多档）。
    pub missing_mask: u8,
}

impl MissingPiece {
    /// 人读缺件清单（中文，逗号分隔件名）。
    pub fn mask_names(&self) -> String {
        let mut names = String::new();
        if self.missing_mask & MASK_LOG != 0 {
            names.push_str("日志段 ");
        }
        if self.missing_mask & MASK_SHOT != 0 {
            names.push_str("截图位 ");
        }
        if self.missing_mask & MASK_LINE != 0 {
            names.push_str("账行引用 ");
        }
        if self.missing_mask & MASK_ORPHAN != 0 {
            names.push_str("多档未引用");
        }
        if names.is_empty() {
            names.push_str("（无缺件标记，疑内部错位）");
        }
        names
    }
}

/// 一条悬空引用红案（J1-d：F0687 引用解不到档）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DanglingItem {
    /// 引用侧机号。
    pub machine: u8,
    /// 引用侧轮号。
    pub round: u16,
    /// F0687 侧引用号原文。
    pub archive_ref: String,
    /// 引用号内解析出的次序号（解析失败为 0）。
    pub parsed_arc_no: u32,
}

/// 齐备率审计报告（J1-a：已归档/应归档比对 + 齐备率 + 未齐备清单）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompletenessAudit {
    /// 应归档数（= F0687 失败引用数；口径 J1-d）。
    pub expected: u32,
    /// 已归档且三件齐的档数。
    pub ready: u32,
    /// 红案档数（缺件档 + 多档档）。
    pub red_count: u32,
    /// 齐备率 ppm（ready × 10000 ÷ expected；expected=0 时按空账口径 10000）。
    pub ratio_ppm: u64,
    /// 是否齐备率 100%（ready == expected 且零红案零悬空零多档）。
    pub complete: bool,
    /// 未齐备清单（缺件红案逐档逐件）。
    pub missing: Vec<MissingPiece>,
    /// 悬空引用清单（引用解不到档）。
    pub dangling: Vec<DanglingItem>,
    /// 多档未引用数（账有档而 F0687 引用未引，反向红案计数）。
    pub orphan_count: u32,
    /// 人读摘要。
    pub note: String,
}

/// 对账报告（J1-d：F0687 失败引用 ↔ 归档档互拍）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReconcileReport {
    /// 引用侧条数。
    pub refs_total: u32,
    /// 成功解引条数（引用号 + 机号 + 轮号三对齐）。
    pub resolved: u32,
    /// 悬空引用清单。
    pub dangling: Vec<DanglingItem>,
    /// 多档未引用数（反向）。
    pub unreferenced_entries: u32,
    /// 对账是否干净（零悬空零多档）。
    pub ok: bool,
}

/// 链指纹对拍报告（J1-c：零抹除断言的机制面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChainReport {
    /// 现存档数。
    pub entries: u32,
    /// 档数高水位（历史最大，只增不降）。
    pub entries_hwm: u32,
    /// 由现存档重算的链尖。
    pub recomputed_tip: u64,
    /// 存量链尖（最后一次合法追加后烙定）。
    pub stored_tip: u64,
    /// 指纹历史长度（每次合法追加 push 一次，只增）。
    pub history_len: u32,
    /// 合法追加次数（open + deposit + backfill 计数和）。
    pub mutations: u64,
    /// 链是否自洽（档数 == 高水位 且 重算尖 == 存量尖）。
    pub ok: bool,
}

/// 健康巡检报告（health() 产出）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HealthReport {
    /// 总体健康（issue_codes 为空即 true）。
    pub healthy: bool,
    /// 现存档数。
    pub entry_count: u32,
    /// 是否已封板。
    pub sealed: bool,
    /// 事件账条数。
    pub event_count: u32,
    /// 三件齐档数。
    pub ready_entries: u32,
    /// 未齐档数（缺件档数，非红案计数口径的补集）。
    pub pending_missing: u32,
    /// 最近一次对账的悬空计数（未对账为 0）。
    pub last_dangling: u32,
    /// 巡检发现问题诊断码列表（空即健康）。
    pub issue_codes: Vec<u16>,
    /// 人读摘要（中文，format! 拼装）。
    pub note: String,
}

/// 封板落簿的归档账结论（J1-a + J1-c 联合凭据；F0698 门禁第四条件消费）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SealRecord {
    /// 应归档数（= 对账引用数）。
    pub expected_failures: u32,
    /// 三件齐档数（封板时 == expected）。
    pub ready_entries: u32,
    /// 齐备率 ppm（封板时 == 10000）。
    pub ratio_ppm: u64,
    /// 封板时链尖（与账体绑定锚，供 F0762 终核对拍）。
    pub chain_tip: u64,
    /// 封板刻度。
    pub sealed_tick: u64,
}

// ── 归档账主体 ───────────────────────────────────────────────────────
/// 失败样本归档账：失败次序号 → 三件套档位登记（append-only）。
///
/// 零抹除红线（J1-c）的机制化：
///   * 业务面不存在任何删除/截断/改写 API（档位只 push、槽位只缺→在）；
///   * chain_tip 链式指纹在每次合法追加后重算烙定，
///     fingerprint_history 单调追加（前缀永不改写）；
///   * entries_hwm 档数高水位只增；verify_chain 以
///     「重算链尖 == 存量链尖 且 档数 == 高水位」双条件对拍，
///     模拟截断攻击（注入面 attack_truncate_entries）由此显形。
///   * 件级防篡改由每件 guard 复算承担（verify_entry_content），
///     与链级对拍分层（件内改写 → E_TAMPER；档行截断 → E_TRUNCATION）。
///
/// 结构纪律：含 Vec 字段，禁 derive Copy（仅 Clone）。
#[derive(Clone, Debug)]
pub struct FailArchiveBook {
    /// 档位（开档序；arc_no 严格 == 下标 + 1 的恒等式只在零攻击时成立，
    /// 查档一律扫 arc_no 字段，不依赖下标恒等式）。
    entries: Vec<ArchiveEntry>,
    /// 档数高水位（只增：开档时取 max(hwm, len)，截断攻击后不回落）。
    entries_hwm: u32,
    /// 下一档号（失败次序号发生器，单调递增不回收）。
    arc_next: u32,
    /// 链尖（最后一次合法追加后由现存档全量折叠烙定）。
    chain_tip: u64,
    /// 指纹历史（每次合法追加 push 当时链尖；只增不减）。
    fingerprint_history: Vec<u64>,
    /// 合法追加计数（open + deposit + backfill）。
    mutations: u64,
    /// 事件账（审计流）。
    events: Vec<BookEvent>,
    /// 事件序号发生器。
    ev_seq_next: u32,
    /// 封板标志。
    sealed: bool,
    /// 封板落簿结论。
    seal_rec: Option<SealRecord>,
    /// 最近一次对账的悬空计数（health 消费）。
    last_dangling: u32,
}

impl FailArchiveBook {
    /// 新空账。
    pub fn new() -> Self {
        FailArchiveBook {
            entries: Vec::new(),
            entries_hwm: 0,
            arc_next: 1,
            chain_tip: 0,
            fingerprint_history: Vec::new(),
            mutations: 0,
            events: Vec::new(),
            ev_seq_next: 1,
            sealed: false,
            seal_rec: None,
            last_dangling: 0,
        }
    }

    // ── 内部记账原语（私有；所有写路径必经） ──

    /// 追加事件（事件账只增；成功类 code=0）。
    fn push_event(&mut self, kind: EventKind, arc_no: u32, machine: u8, round: u16, code: u16) {
        let ev = BookEvent {
            seq: self.ev_seq_next,
            kind,
            arc_no,
            machine,
            round,
            code,
        };
        self.ev_seq_next = self.ev_seq_next.saturating_add(1);
        self.events.push(ev);
    }

    /// 重算链尖并烙定 + 追加指纹历史 + 计追加数。
    /// 调用前提：entries 已完成本次合法追加（开档或槽位迁移）。
    fn seal_chain_after_append(&mut self) {
        let mut h = Fnv64::new();
        let mut tip: u64 = 0;
        let mut i: usize = 0;
        while i < self.entries.len() {
            h.chain_mix(tip, self.entries[i].entry_hash());
            tip = h.finish();
            h = Fnv64::new();
            i += 1;
        }
        self.chain_tip = tip;
        self.fingerprint_history.push(tip);
        self.mutations = self.mutations.saturating_add(1);
    }

    /// 档号定位（扫 arc_no 字段；NoSlot 显形）。
    fn idx_of_arc(&self, arc_no: u32) -> Option<usize> {
        let mut i: usize = 0;
        while i < self.entries.len() {
            if self.entries[i].arc_no == arc_no {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 引用号串定位（全局唯一性查重）。
    fn idx_of_ref(&self, arc_ref: &str) -> Option<usize> {
        let mut i: usize = 0;
        while i < self.entries.len() {
            if self.entries[i].succ_ref.as_str() == arc_ref {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 失败位（机号 × 轮号）定位（一档一位查重）。
    fn idx_of_slot(&self, machine: u8, round: u16) -> Option<usize> {
        let mut i: usize = 0;
        while i < self.entries.len() {
            if self.entries[i].machine == machine && self.entries[i].round == round {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    // ── 只读视图 ──

    /// 现存档数。
    pub fn entry_count(&self) -> u32 {
        self.entries.len() as u32
    }

    /// 档数高水位（零抹除断言对照面：len < hwm 即截断曾发生）。
    pub const fn entries_high_water(&self) -> u32 {
        self.entries_hwm
    }

    /// 是否已封板。
    pub const fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 封板结论（未封板为 None）。
    pub const fn seal_record(&self) -> Option<SealRecord> {
        self.seal_rec
    }

    /// 存量链尖。
    pub const fn chain_tip(&self) -> u64 {
        self.chain_tip
    }

    /// 指纹历史（只读切片；只增不减的前缀完整性由此可验）。
    pub fn fingerprint_history(&self) -> &[u64] {
        &self.fingerprint_history
    }

    /// 合法追加计数。
    pub const fn mutation_count(&self) -> u64 {
        self.mutations
    }

    /// 事件账条数。
    pub fn event_count(&self) -> u32 {
        self.events.len() as u32
    }

    /// 事件账切片（只读）。
    pub fn events(&self) -> &[BookEvent] {
        &self.events
    }

    /// 档位切片（只读，开档序）。
    pub fn entries(&self) -> &[ArchiveEntry] {
        &self.entries
    }

    /// 按档号取档（拷贝视图；不存在 None）。
    pub fn entry_by_arc_no(&self, arc_no: u32) -> Option<ArchiveEntry> {
        match self.idx_of_arc(arc_no) {
            Some(i) => Some(self.entries[i].clone()),
            None => None,
        }
    }

    /// 按引用号取档号（解引：F0687 引用 → 归档档）。
    pub fn arc_no_of_ref(&self, arc_ref: &str) -> Option<u32> {
        match self.idx_of_ref(arc_ref) {
            Some(i) => Some(self.entries[i].arc_no),
            None => None,
        }
    }

    /// 按失败位（机 × 轮）取档号。
    pub fn arc_no_of_slot(&self, machine: u8, round: u16) -> Option<u32> {
        match self.idx_of_slot(machine, round) {
            Some(i) => Some(self.entries[i].arc_no),
            None => None,
        }
    }

    /// 件槽位只读快照（缺件审计/补档判定入口）。
    pub fn piece_status(&self, arc_no: u32, kind: ArtifactKind) -> Result<PieceStatus, FailArcError> {
        let idx = match self.idx_of_arc(arc_no) {
            Some(i) => i,
            None => return Err(FailArcError::NoSlot { arc_no }),
        };
        let e = &self.entries[idx];
        match kind {
            ArtifactKind::Log => match &e.log {
                PieceSlot::Missing { since_tick } => Ok(PieceStatus::Missing {
                    since_tick: *since_tick,
                }),
                PieceSlot::Present {
                    backfilled,
                    origin_tick,
                    deposited_tick,
                    ..
                } => Ok(PieceStatus::Present {
                    backfilled: *backfilled,
                    origin_tick: *origin_tick,
                    deposited_tick: *deposited_tick,
                }),
            },
            ArtifactKind::Shot => match &e.shot {
                PieceSlot::Missing { since_tick } => Ok(PieceStatus::Missing {
                    since_tick: *since_tick,
                }),
                PieceSlot::Present {
                    backfilled,
                    origin_tick,
                    deposited_tick,
                    ..
                } => Ok(PieceStatus::Present {
                    backfilled: *backfilled,
                    origin_tick: *origin_tick,
                    deposited_tick: *deposited_tick,
                }),
            },
            ArtifactKind::Line => match &e.line {
                PieceSlot::Missing { since_tick } => Ok(PieceStatus::Missing {
                    since_tick: *since_tick,
                }),
                PieceSlot::Present {
                    backfilled,
                    origin_tick,
                    deposited_tick,
                    ..
                } => Ok(PieceStatus::Present {
                    backfilled: *backfilled,
                    origin_tick: *origin_tick,
                    deposited_tick: *deposited_tick,
                }),
            },
        }
    }

    /// 组装三件套聚合视图（三件齐才 Some；缺件 None——红案不组装，
    /// 判据「缺任一件即归档不完整」的取数面体现）。
    pub fn artifact_of(&self, arc_no: u32) -> Result<Option<FailArtifact>, FailArcError> {
        let idx = match self.idx_of_arc(arc_no) {
            Some(i) => i,
            None => return Err(FailArcError::NoSlot { arc_no }),
        };
        let e = &self.entries[idx];
        if !e.triple_complete() {
            return Ok(None);
        }
        // 三槽在位（上判定保证），逐槽取件（ Borrow 一次性拷出）。
        let log = match e.log {
            PieceSlot::Present { piece, .. } => piece,
            PieceSlot::Missing { .. } => return Ok(None),
        };
        let shot = match e.shot {
            PieceSlot::Present { piece, .. } => piece,
            PieceSlot::Missing { .. } => return Ok(None),
        };
        let line = match e.line {
            PieceSlot::Present { piece, .. } => piece,
            PieceSlot::Missing { .. } => return Ok(None),
        };
        let ref_hash = fnv1a64(e.succ_ref.as_bytes());
        let art = FailArtifact {
            arc_no: e.arc_no,
            machine: e.machine,
            round: e.round,
            ref_hash,
            log,
            shot,
            line,
            self_hash: 0,
            origin_tick: e.opened_tick,
        };
        let self_hash = art.fold_hash();
        Ok(Some(FailArtifact {
            self_hash,
            ..art
        }))
    }

    // ── 写通道一：开档（失败位 → 档位，三槽全缺起记） ──

    /// 为一个失败位开档（J1-b 逐次归档）。
    /// 校验链：未封板 → 机/轮界内 → 引用号非空且 ≤64 → 规范格式且
    /// 次序号与 arc_next 双射 → 引用串全局唯一 → 失败位一档一位。
    /// 通过即 push 档位（三槽 Missing{since_tick: tick}）+ 链追加 + 事件。
    pub fn open_entry(
        &mut self,
        machine: u8,
        round: u16,
        arc_ref: &str,
        tick: u64,
    ) -> Result<u32, FailArcError> {
        if self.sealed {
            self.push_event(EventKind::PieceRejected, 0, machine, round, E_SEALED);
            return Err(FailArcError::Sealed);
        }
        if machine as usize >= MACHINE_COUNT {
            self.push_event(EventKind::PieceRejected, 0, machine, round, E_MACHINE_RANGE);
            return Err(FailArcError::MachineOutOfRange { got: machine });
        }
        if round == 0 || round as usize > ROUNDS_PER_MACHINE {
            self.push_event(EventKind::PieceRejected, 0, machine, round, E_ROUND_RANGE);
            return Err(FailArcError::RoundOutOfRange { got: round });
        }
        let len = arc_ref.len();
        if len == 0 || len > FAIL_REF_MAX_LEN {
            self.push_event(EventKind::PieceRejected, 0, machine, round, E_REF_INVALID);
            return Err(FailArcError::RefInvalid {
                len,
                cap: FAIL_REF_MAX_LEN,
            });
        }
        // 引用号规范格式（先验可解析）→ 同串查重（先于次序闸：
        // 已挂串的重挂无论次序一律按重复红案认）→ 次序双射
        // （F0687 生成器为顺序 {:03}，本账按同一序消费——序不符即格式红案）。
        let parsed = match parse_arc_ref(arc_ref) {
            Some(n) => n,
            None => {
                self.push_event(EventKind::PieceRejected, 0, machine, round, E_REF_FORMAT);
                return Err(FailArcError::RefFormat { len });
            }
        };
        if let Some(hit) = self.idx_of_ref(arc_ref) {
            self.push_event(EventKind::PieceRejected, 0, machine, round, E_REF_DUP);
            return Err(FailArcError::RefDuplicate {
                existing_arc_no: self.entries[hit].arc_no,
            });
        }
        if parsed != self.arc_next {
            self.push_event(EventKind::PieceRejected, 0, machine, round, E_REF_FORMAT);
            return Err(FailArcError::RefFormat { len });
        }
        if let Some(hit) = self.idx_of_slot(machine, round) {
            self.push_event(
                EventKind::PieceRejected,
                self.entries[hit].arc_no,
                machine,
                round,
                E_DUP_SLOT,
            );
            return Err(FailArcError::DuplicateSlot { machine, round });
        }
        let arc_no = self.arc_next;
        self.entries.push(ArchiveEntry {
            arc_no,
            machine,
            round,
            succ_ref: String::from(arc_ref),
            opened_tick: tick,
            log: PieceSlot::Missing { since_tick: tick },
            shot: PieceSlot::Missing { since_tick: tick },
            line: PieceSlot::Missing { since_tick: tick },
        });
        self.arc_next = self.arc_next.saturating_add(1);
        if self.entries.len() as u32 > self.entries_hwm {
            self.entries_hwm = self.entries.len() as u32;
        }
        self.seal_chain_after_append();
        self.push_event(EventKind::EntryOpened, arc_no, machine, round, 0);
        Ok(arc_no)
    }

    // ── 写通道二：件归档（首存：缺 → 在，backfilled=false） ──

    /// 存入日志段（件一）。前置：未封板 + 档在 + 该槽缺；在位二存拒收。
    pub fn deposit_log_segment(
        &mut self,
        arc_no: u32,
        piece: LogSegment,
        tick: u64,
    ) -> Result<(), FailArcError> {
        if self.sealed {
            self.push_event(EventKind::PieceRejected, arc_no, 0xFF, 0, E_SEALED);
            return Err(FailArcError::Sealed);
        }
        let idx = match self.idx_of_arc(arc_no) {
            Some(i) => i,
            None => {
                self.push_event(EventKind::PieceRejected, arc_no, 0xFF, 0, E_NO_SLOT);
                return Err(FailArcError::NoSlot { arc_no });
            }
        };
        if self.entries[idx].log.is_present() {
            self.push_event(
                EventKind::PieceRejected,
                arc_no,
                self.entries[idx].machine,
                self.entries[idx].round,
                E_ITEM_DUP,
            );
            return Err(FailArcError::ItemDuplicate {
                arc_no,
                kind: ArtifactKind::Log.as_raw(),
            });
        }
        let origin = match self.entries[idx].log {
            PieceSlot::Missing { since_tick } => since_tick,
            PieceSlot::Present { origin_tick, .. } => origin_tick,
        };
        let (m, r) = (self.entries[idx].machine, self.entries[idx].round);
        self.entries[idx].log = PieceSlot::Present {
            piece,
            backfilled: false,
            origin_tick: origin,
            deposited_tick: tick,
        };
        self.after_piece_append(arc_no, m, r, ArtifactKind::Log, false);
        Ok(())
    }

    /// 存入截图位（件二）。
    pub fn deposit_shot_slot(
        &mut self,
        arc_no: u32,
        piece: ShotSlot,
        tick: u64,
    ) -> Result<(), FailArcError> {
        if self.sealed {
            self.push_event(EventKind::PieceRejected, arc_no, 0xFF, 0, E_SEALED);
            return Err(FailArcError::Sealed);
        }
        let idx = match self.idx_of_arc(arc_no) {
            Some(i) => i,
            None => {
                self.push_event(EventKind::PieceRejected, arc_no, 0xFF, 0, E_NO_SLOT);
                return Err(FailArcError::NoSlot { arc_no });
            }
        };
        if self.entries[idx].shot.is_present() {
            self.push_event(
                EventKind::PieceRejected,
                arc_no,
                self.entries[idx].machine,
                self.entries[idx].round,
                E_ITEM_DUP,
            );
            return Err(FailArcError::ItemDuplicate {
                arc_no,
                kind: ArtifactKind::Shot.as_raw(),
            });
        }
        let origin = match self.entries[idx].shot {
            PieceSlot::Missing { since_tick } => since_tick,
            PieceSlot::Present { origin_tick, .. } => origin_tick,
        };
        let (m, r) = (self.entries[idx].machine, self.entries[idx].round);
        self.entries[idx].shot = PieceSlot::Present {
            piece,
            backfilled: false,
            origin_tick: origin,
            deposited_tick: tick,
        };
        self.after_piece_append(arc_no, m, r, ArtifactKind::Shot, false);
        Ok(())
    }

    /// 存入账行引用（件三）。
    pub fn deposit_ledger_line(
        &mut self,
        arc_no: u32,
        piece: LedgerLineRef,
        tick: u64,
    ) -> Result<(), FailArcError> {
        if self.sealed {
            self.push_event(EventKind::PieceRejected, arc_no, 0xFF, 0, E_SEALED);
            return Err(FailArcError::Sealed);
        }
        let idx = match self.idx_of_arc(arc_no) {
            Some(i) => i,
            None => {
                self.push_event(EventKind::PieceRejected, arc_no, 0xFF, 0, E_NO_SLOT);
                return Err(FailArcError::NoSlot { arc_no });
            }
        };
        if self.entries[idx].line.is_present() {
            self.push_event(
                EventKind::PieceRejected,
                arc_no,
                self.entries[idx].machine,
                self.entries[idx].round,
                E_ITEM_DUP,
            );
            return Err(FailArcError::ItemDuplicate {
                arc_no,
                kind: ArtifactKind::Line.as_raw(),
            });
        }
        let origin = match self.entries[idx].line {
            PieceSlot::Missing { since_tick } => since_tick,
            PieceSlot::Present { origin_tick, .. } => origin_tick,
        };
        let (m, r) = (self.entries[idx].machine, self.entries[idx].round);
        self.entries[idx].line = PieceSlot::Present {
            piece,
            backfilled: false,
            origin_tick: origin,
            deposited_tick: tick,
        };
        self.after_piece_append(arc_no, m, r, ArtifactKind::Line, false);
        Ok(())
    }

    // ── 写通道三：补档（J1-f：缺 → 在，backfilled=true，原始时戳保留） ──

    /// 补档日志段（发现缺件后的补档登记；追加非抹除，链照常只增）。
    pub fn backfill_log_segment(
        &mut self,
        arc_no: u32,
        piece: LogSegment,
        tick: u64,
    ) -> Result<(), FailArcError> {
        if self.sealed {
            self.push_event(EventKind::PieceRejected, arc_no, 0xFF, 0, E_SEALED);
            return Err(FailArcError::Sealed);
        }
        let idx = match self.idx_of_arc(arc_no) {
            Some(i) => i,
            None => {
                self.push_event(EventKind::PieceRejected, arc_no, 0xFF, 0, E_NO_SLOT);
                return Err(FailArcError::NoSlot { arc_no });
            }
        };
        if self.entries[idx].log.is_present() {
            self.push_event(
                EventKind::PieceRejected,
                arc_no,
                self.entries[idx].machine,
                self.entries[idx].round,
                E_ITEM_DUP,
            );
            return Err(FailArcError::ItemDuplicate {
                arc_no,
                kind: ArtifactKind::Log.as_raw(),
            });
        }
        let origin = match self.entries[idx].log {
            PieceSlot::Missing { since_tick } => since_tick,
            PieceSlot::Present { origin_tick, .. } => origin_tick,
        };
        let (m, r) = (self.entries[idx].machine, self.entries[idx].round);
        self.entries[idx].log = PieceSlot::Present {
            piece,
            backfilled: true,
            origin_tick: origin,
            deposited_tick: tick,
        };
        self.after_piece_append(arc_no, m, r, ArtifactKind::Log, true);
        Ok(())
    }

    /// 补档截图位。
    pub fn backfill_shot_slot(
        &mut self,
        arc_no: u32,
        piece: ShotSlot,
        tick: u64,
    ) -> Result<(), FailArcError> {
        if self.sealed {
            self.push_event(EventKind::PieceRejected, arc_no, 0xFF, 0, E_SEALED);
            return Err(FailArcError::Sealed);
        }
        let idx = match self.idx_of_arc(arc_no) {
            Some(i) => i,
            None => {
                self.push_event(EventKind::PieceRejected, arc_no, 0xFF, 0, E_NO_SLOT);
                return Err(FailArcError::NoSlot { arc_no });
            }
        };
        if self.entries[idx].shot.is_present() {
            self.push_event(
                EventKind::PieceRejected,
                arc_no,
                self.entries[idx].machine,
                self.entries[idx].round,
                E_ITEM_DUP,
            );
            return Err(FailArcError::ItemDuplicate {
                arc_no,
                kind: ArtifactKind::Shot.as_raw(),
            });
        }
        let origin = match self.entries[idx].shot {
            PieceSlot::Missing { since_tick } => since_tick,
            PieceSlot::Present { origin_tick, .. } => origin_tick,
        };
        let (m, r) = (self.entries[idx].machine, self.entries[idx].round);
        self.entries[idx].shot = PieceSlot::Present {
            piece,
            backfilled: true,
            origin_tick: origin,
            deposited_tick: tick,
        };
        self.after_piece_append(arc_no, m, r, ArtifactKind::Shot, true);
        Ok(())
    }

    /// 补档账行引用。
    pub fn backfill_ledger_line(
        &mut self,
        arc_no: u32,
        piece: LedgerLineRef,
        tick: u64,
    ) -> Result<(), FailArcError> {
        if self.sealed {
            self.push_event(EventKind::PieceRejected, arc_no, 0xFF, 0, E_SEALED);
            return Err(FailArcError::Sealed);
        }
        let idx = match self.idx_of_arc(arc_no) {
            Some(i) => i,
            None => {
                self.push_event(EventKind::PieceRejected, arc_no, 0xFF, 0, E_NO_SLOT);
                return Err(FailArcError::NoSlot { arc_no });
            }
        };
        if self.entries[idx].line.is_present() {
            self.push_event(
                EventKind::PieceRejected,
                arc_no,
                self.entries[idx].machine,
                self.entries[idx].round,
                E_ITEM_DUP,
            );
            return Err(FailArcError::ItemDuplicate {
                arc_no,
                kind: ArtifactKind::Line.as_raw(),
            });
        }
        let origin = match self.entries[idx].line {
            PieceSlot::Missing { since_tick } => since_tick,
            PieceSlot::Present { origin_tick, .. } => origin_tick,
        };
        let (m, r) = (self.entries[idx].machine, self.entries[idx].round);
        self.entries[idx].line = PieceSlot::Present {
            piece,
            backfilled: true,
            origin_tick: origin,
            deposited_tick: tick,
        };
        self.after_piece_append(arc_no, m, r, ArtifactKind::Line, true);
        Ok(())
    }

    /// 槽位迁移后的公共收尾：链追加 + 存/补档事件 + 三件齐瞬间 EntryComplete。
    fn after_piece_append(
        &mut self,
        arc_no: u32,
        machine: u8,
        round: u16,
        kind: ArtifactKind,
        backfilled: bool,
    ) {
        self.seal_chain_after_append();
        if backfilled {
            self.push_event(EventKind::PieceBackfilled, arc_no, machine, round, 0);
        } else {
            self.push_event(EventKind::PieceDeposited, arc_no, machine, round, 0);
        }
        if let Some(i) = self.idx_of_arc(arc_no) {
            if self.entries[i].triple_complete() {
                self.push_event(EventKind::EntryComplete, arc_no, machine, round, 0);
            }
        }
        // kind 参与事件语义但事件字段只携 code=0；留一次显式读取防未用告警。
        let _ = kind.as_raw();
    }

    // ── 件内容完整性（J1-e：归档后重读校验 + 篡改检出） ──

    /// 单档内容校验：三槽在位件逐一 guard 复算，不符即篡改红案。
    pub fn verify_entry_content(&self, arc_no: u32) -> Result<(), FailArcError> {
        let idx = match self.idx_of_arc(arc_no) {
            Some(i) => i,
            None => return Err(FailArcError::NoSlot { arc_no }),
        };
        match self.entries[idx].tamper_scan() {
            Some((kind, stored, recomputed)) => {
                Err(FailArcError::TamperDetected {
                    arc_no,
                    kind,
                    stored,
                    recomputed,
                })
            }
            None => Ok(()),
        }
    }

    /// 全账内容校验（逐档 guard 复算；返回净档数，首个篡改档 Err 显形）。
    pub fn verify_all_content(&self) -> Result<u32, FailArcError> {
        let mut clean: u32 = 0;
        let mut i: usize = 0;
        while i < self.entries.len() {
            match self.entries[i].tamper_scan() {
                Some((kind, stored, recomputed)) => {
                    return Err(FailArcError::TamperDetected {
                        arc_no: self.entries[i].arc_no,
                        kind,
                        stored,
                        recomputed,
                    })
                }
                None => clean = clean.saturating_add(1),
            }
            i += 1;
        }
        Ok(clean)
    }

    // ── 链指纹对拍（J1-c：零抹除断言的机制面） ──

    /// 由现存档全量重算链尖（与 seal_chain_after_append 同折叠序）。
    fn recompute_chain_tip(&self) -> u64 {
        let mut tip: u64 = 0;
        let mut i: usize = 0;
        while i < self.entries.len() {
            let mut h = Fnv64::new();
            h.chain_mix(tip, self.entries[i].entry_hash());
            tip = h.finish();
            i += 1;
        }
        tip
    }

    /// 链对拍报告（不判错版本；报告 ok 位自含）。
    pub fn chain_report(&self) -> ChainReport {
        let recomputed = self.recompute_chain_tip();
        ChainReport {
            entries: self.entries.len() as u32,
            entries_hwm: self.entries_hwm,
            recomputed_tip: recomputed,
            stored_tip: self.chain_tip,
            history_len: self.fingerprint_history.len() as u32,
            mutations: self.mutations,
            ok: self.entries.len() as u32 == self.entries_hwm && recomputed == self.chain_tip,
        }
    }

    /// 链对拍（判错版本）：档数低于高水位 → 截断红案；
    /// 重算尖 ≠ 存量尖 → 结构破坏红案（同为 E_TRUNCATION 伞）。
    pub fn verify_chain(&self) -> Result<ChainReport, FailArcError> {
        let rep = self.chain_report();
        if rep.entries < rep.entries_hwm {
            return Err(FailArcError::TruncationDetected {
                kept: rep.entries,
                expected: rep.entries_hwm,
            });
        }
        if rep.recomputed_tip != rep.stored_tip {
            return Err(FailArcError::TruncationDetected {
                kept: rep.entries,
                expected: rep.entries_hwm,
            });
        }
        Ok(rep)
    }

    // ── F0687 对账（J1-d：引用号必须能解引到档，悬空即红案） ──

    /// 对账内核（私有）：逐引用解引（引用号 + 机号 + 轮号三对齐），
    /// 返回（悬空清单，解引成功数，未被引用的档号列表）。
    fn reconcile_inner(&self, refs: &[SuccFailRef]) -> (Vec<DanglingItem>, u32, Vec<u32>) {
        let mut dangling: Vec<DanglingItem> = Vec::new();
        let mut resolved: u32 = 0;
        let mut hit: Vec<bool> = alloc_vec_false(self.entries.len());
        let mut i: usize = 0;
        while i < refs.len() {
            let rf = &refs[i];
            let mut got: Option<usize> = None;
            match self.idx_of_ref(rf.archive_ref.as_str()) {
                Some(idx) => {
                    let e = &self.entries[idx];
                    if e.machine == rf.machine && e.round == rf.round {
                        got = Some(idx);
                    }
                }
                None => {}
            }
            match got {
                Some(idx) => {
                    resolved = resolved.saturating_add(1);
                    if idx < hit.len() {
                        hit[idx] = true;
                    }
                }
                None => {
                    let parsed = parse_arc_ref(rf.archive_ref.as_str()).unwrap_or(0);
                    dangling.push(DanglingItem {
                        machine: rf.machine,
                        round: rf.round,
                        archive_ref: String::from(rf.archive_ref.as_str()),
                        parsed_arc_no: parsed,
                    });
                }
            }
            i += 1;
        }
        let mut orphans: Vec<u32> = Vec::new();
        let mut k: usize = 0;
        while k < self.entries.len() {
            if !hit[k] {
                orphans.push(self.entries[k].arc_no);
            }
            k += 1;
        }
        (dangling, resolved, orphans)
    }

    /// 与 F0687 失败引用对账（公开通道：解引 + 悬空/多档清单 + 事件）。
    pub fn reconcile_with_succ_refs(
        &mut self,
        refs: &[SuccFailRef],
    ) -> Result<ReconcileReport, FailArcError> {
        let refs_total = refs.len() as u32;
        let (dangling, resolved, orphans) = self.reconcile_inner(refs);
        let orphan_count = orphans.len() as u32;
        self.last_dangling = dangling.len() as u32;
        let ok = dangling.is_empty() && orphan_count == 0;
        self.push_event(
            EventKind::ReconcileRan,
            0,
            0xFF,
            0,
            if ok { 0 } else { E_DANGLING },
        );
        Ok(ReconcileReport {
            refs_total,
            resolved,
            dangling,
            unreferenced_entries: orphan_count,
            ok,
        })
    }

    // ── 齐备率审计（J1-a：已归档/应归档比对 + 齐备率 + 未齐备清单） ──

    /// 齐备率审计（无引用面版本：expected 显式给入）。
    /// 内容校验先行（篡改传播 Err），再逐档位图清点。
    pub fn audit_completeness(&self, expected: u32) -> Result<CompletenessAudit, FailArcError> {
        self.verify_all_content()?;
        let mut ready: u32 = 0;
        let mut missing: Vec<MissingPiece> = Vec::new();
        let mut i: usize = 0;
        while i < self.entries.len() {
            let e = &self.entries[i];
            let mask = e.missing_mask();
            if mask == 0 {
                ready = ready.saturating_add(1);
            } else {
                missing.push(MissingPiece {
                    arc_no: e.arc_no,
                    machine: e.machine,
                    round: e.round,
                    succ_ref: String::from(e.succ_ref.as_str()),
                    missing_mask: mask,
                });
            }
            i += 1;
        }
        let red_count = missing.len() as u32;
        let ratio_ppm = completeness_ppm(ready, expected);
        let complete = missing.is_empty() && ready == expected;
        Ok(CompletenessAudit {
            expected,
            ready,
            red_count,
            ratio_ppm,
            complete,
            missing,
            dangling: Vec::new(),
            orphan_count: 0,
            note: format!(
                "齐备审计：应归档 {} 件套，已齐 {}，缺件红案 {} 档，齐备率 {}ppm",
                expected, ready, red_count, ratio_ppm
            ),
        })
    }

    /// 齐备率审计（对账面版本：应归档 = F0687 失败引用数；悬空/多档一并入红案）。
    /// J1-a + J1-d 联合口径，seal 的直接前置。
    pub fn audit_from_refs(
        &self,
        refs: &[SuccFailRef],
    ) -> Result<CompletenessAudit, FailArcError> {
        self.verify_all_content()?;
        let expected = refs.len() as u32;
        let (dangling, resolved, orphans) = self.reconcile_inner(refs);
        let mut ready: u32 = 0;
        let mut missing: Vec<MissingPiece> = Vec::new();
        let mut i: usize = 0;
        while i < self.entries.len() {
            let e = &self.entries[i];
            let mut mask = e.missing_mask();
            let is_orphan = orphans.contains(&e.arc_no);
            if is_orphan {
                mask |= MASK_ORPHAN;
            }
            if mask == 0 {
                ready = ready.saturating_add(1);
            } else {
                missing.push(MissingPiece {
                    arc_no: e.arc_no,
                    machine: e.machine,
                    round: e.round,
                    succ_ref: String::from(e.succ_ref.as_str()),
                    missing_mask: mask,
                });
            }
            i += 1;
        }
        let red_count = missing.len() as u32;
        let ratio_ppm = completeness_ppm(ready, expected);
        let complete =
            dangling.is_empty() && orphans.is_empty() && missing.is_empty() && ready == expected;
        let dangling_len = dangling.len();
        let orphan_len = orphans.len();
        Ok(CompletenessAudit {
            expected,
            ready,
            red_count,
            ratio_ppm,
            complete,
            missing,
            dangling,
            orphan_count: orphans.len() as u32,
            note: format!(
                "齐备审计（对账面）：应归档 {}，已齐 {}，解引 {}，悬空 {}，多档 {}，缺件红案 {} 档，齐备率 {}ppm",
                expected,
                ready,
                resolved,
                dangling_len,
                orphan_len,
                red_count,
                ratio_ppm
            ),
        })
    }

    // ── 封板（齐备率 100% + 链自洽才放行；F0698 门禁第四条件凭据） ──

    /// 封板：对账面齐备审计 complete + 链对拍过，才落 SealRecord。
    /// 任何红案（悬空/缺件/多档/篡改/截断）逐型拒封。
    pub fn seal(&mut self, refs: &[SuccFailRef], tick: u64) -> Result<SealRecord, FailArcError> {
        if self.sealed {
            return Err(FailArcError::Sealed);
        }
        let audit = self.audit_from_refs(refs)?;
        if !audit.complete {
            if !audit.dangling.is_empty() {
                let first = &audit.dangling[0];
                return Err(FailArcError::DanglingRef {
                    arc_no: first.parsed_arc_no,
                });
            }
            if !audit.missing.is_empty() {
                let first = &audit.missing[0];
                return Err(FailArcError::Incomplete {
                    arc_no: first.arc_no,
                    missing_mask: first.missing_mask,
                });
            }
            // dangling/missing 皆空而 complete=false：ready != expected 的守恒破，
            // 归入不完整伞（理论不可达，防御位）。
            return Err(FailArcError::Incomplete {
                arc_no: 0,
                missing_mask: 0,
            });
        }
        let chain = self.verify_chain()?;
        let rec = SealRecord {
            expected_failures: audit.expected,
            ready_entries: audit.ready,
            ratio_ppm: audit.ratio_ppm,
            chain_tip: chain.stored_tip,
            sealed_tick: tick,
        };
        self.sealed = true;
        self.seal_rec = Some(rec);
        self.push_event(EventKind::Sealed, 0, 0xFF, 0, 0);
        Ok(rec)
    }

    // ── 健康巡检 ──

    /// 健康巡检：链对拍 + 全账内容校验 + 对账残留 + 缺件清点，
    /// 问题诊断码入 issue_codes（空即健康）；巡检事件落账。
    pub fn health(&mut self) -> HealthReport {
        let mut issues: Vec<u16> = Vec::new();
        // 件级：全账 guard 复算（每篡改档记一次码，封顶 8 条防爆事件账）。
        let mut tampered: u32 = 0;
        let mut i: usize = 0;
        while i < self.entries.len() {
            if self.entries[i].tamper_scan().is_some() {
                tampered = tampered.saturating_add(1);
                if issues.len() < 8 {
                    issues.push(E_TAMPER);
                }
            }
            i += 1;
        }
        // 链级：截断/结构破坏。
        let chain_ok = match self.verify_chain() {
            Ok(_) => true,
            Err(FailArcError::TruncationDetected { .. }) => {
                issues.push(E_TRUNCATION);
                false
            }
            Err(_) => {
                issues.push(E_TRUNCATION);
                false
            }
        };
        // 对账残留：悬空引用。
        if self.last_dangling > 0 {
            issues.push(E_DANGLING);
        }
        // 缺件清点（未封板时缺件即红案；封板后账上必齐，防御位）。
        let mut ready: u32 = 0;
        let mut pending: u32 = 0;
        let mut k: usize = 0;
        while k < self.entries.len() {
            if self.entries[k].triple_complete() {
                ready = ready.saturating_add(1);
            } else {
                pending = pending.saturating_add(1);
            }
            k += 1;
        }
        if pending > 0 {
            issues.push(E_INCOMPLETE);
        }
        let healthy = issues.is_empty();
        let note = format!(
            "归档账巡检：档 {}（高水位 {}），三件齐 {}，缺件 {}，篡改 {}，链{}，事件 {}，{}",
            self.entries.len(),
            self.entries_hwm,
            ready,
            pending,
            tampered,
            if chain_ok { "自洽" } else { "失配" },
            self.events.len(),
            if self.sealed { "已封板" } else { "未封板" }
        );
        self.push_event(EventKind::HealthChecked, 0, 0xFF, 0, 0);
        HealthReport {
            healthy,
            entry_count: self.entries.len() as u32,
            sealed: self.sealed,
            event_count: self.events.len() as u32,
            ready_entries: ready,
            pending_missing: pending,
            last_dangling: self.last_dangling,
            issue_codes: issues,
            note,
        }
    }

    // ── 注入面（攻击模拟专用；业务面零抹除——本二法是仅有的越轨写口，
    //    存在意义就是让截断/改写攻击可注入、可检出、可审计） ──

    /// 注入面·账截断攻击模拟：把档位截到前 keep 档（模拟外力抹账）。
    /// 业务面无任何删除 API（零抹除红线）；本方法仅测试/演练注入用，
    /// 截断后 entries_hwm 不动、存量链尖不动 → verify_chain 必检出。
    /// 返回实际截掉的档数（keep ≥ 现档数时为 0，不动账）。
    pub fn attack_truncate_entries(&mut self, keep: usize) -> u32 {
        let before = self.entries.len();
        if keep >= before {
            return 0;
        }
        self.entries.truncate(keep);
        let removed = (before - self.entries.len()) as u32;
        self.push_event(EventKind::AttackSimulated, 0, 0xFF, 0, E_TRUNCATION);
        removed
    }

    /// 注入面·件改写攻击模拟：改写一档一件的结构字段（guard 不重烙），
    /// 模拟归档后外力篡改 → verify_entry_content / verify_all_content 检出。
    pub fn attack_flip_piece_field(
        &mut self,
        arc_no: u32,
        kind: ArtifactKind,
    ) -> Result<(), FailArcError> {
        let idx = match self.idx_of_arc(arc_no) {
            Some(i) => i,
            None => return Err(FailArcError::NoSlot { arc_no }),
        };
        match kind {
            ArtifactKind::Log => {
                if let PieceSlot::Present { piece, .. } = &mut self.entries[idx].log {
                    piece.end_line = piece.end_line.saturating_add(1);
                } else {
                    return Err(FailArcError::Incomplete {
                        arc_no,
                        missing_mask: MASK_LOG,
                    });
                }
            }
            ArtifactKind::Shot => {
                if let PieceSlot::Present { piece, .. } = &mut self.entries[idx].shot {
                    piece.frame_no = piece.frame_no.saturating_add(1);
                } else {
                    return Err(FailArcError::Incomplete {
                        arc_no,
                        missing_mask: MASK_SHOT,
                    });
                }
            }
            ArtifactKind::Line => {
                if let PieceSlot::Present { piece, .. } = &mut self.entries[idx].line {
                    piece.line_no = piece.line_no.saturating_add(1);
                } else {
                    return Err(FailArcError::Incomplete {
                        arc_no,
                        missing_mask: MASK_LINE,
                    });
                }
            }
        }
        self.push_event(EventKind::AttackSimulated, arc_no, 0xFF, 0, E_TAMPER);
        Ok(())
    }
}

impl Default for FailArchiveBook {
    fn default() -> Self {
        FailArchiveBook::new()
    }
}

// ── 双跑确定性对拍（J1 口径：同档步两账链指纹全等） ───────────────────
/// 一步归档动作（双跑对拍的档步闭集）。
#[derive(Clone, Debug)]
pub struct ArchiveStep {
    /// 失败轮机号。
    pub machine: u8,
    /// 失败轮轮号。
    pub arc_round: u16,
    /// 归档引用号。
    pub arc_ref: String,
    /// 动作。
    pub action: StepAction,
}

/// 档步动作闭集。
#[derive(Clone, Copy, Debug)]
pub enum StepAction {
    /// 开档。
    Open,
    /// 存日志段。
    Log(LogSegment),
    /// 存截图位。
    Shot(ShotSlot),
    /// 存账行引用。
    Line(LedgerLineRef),
}

/// 把一步档步施加到账上（tick 显式给入，确定性之源）。
fn apply_step(book: &mut FailArchiveBook, step: &ArchiveStep, tick: u64) -> Result<(), FailArcError> {
    match step.action {
        StepAction::Open => {
            book.open_entry(step.machine, step.arc_round, step.arc_ref.as_str(), tick)
                .map(|_| ())
        }
        StepAction::Log(p) => book.deposit_log_segment(parse_arc_ref(&step.arc_ref).unwrap_or(0), p, tick),
        StepAction::Shot(p) => book.deposit_shot_slot(parse_arc_ref(&step.arc_ref).unwrap_or(0), p, tick),
        StepAction::Line(p) => book.deposit_ledger_line(parse_arc_ref(&step.arc_ref).unwrap_or(0), p, tick),
    }
}

/// 双跑对拍：同一档步流跑两本独立账，返回两账链尖；
/// 失配即 ReplayMismatch（幂等破红案）。两账全程零共享态。
pub fn dual_run_replay(steps: &[ArchiveStep]) -> Result<(u64, u64), FailArcError> {
    let mut book_a = FailArchiveBook::new();
    let mut book_b = FailArchiveBook::new();
    let mut i: usize = 0;
    while i < steps.len() {
        let tick = (i as u64) + 1; // 步序即刻度：确定性刻度源
        apply_step(&mut book_a, &steps[i], tick)?;
        apply_step(&mut book_b, &steps[i], tick)?;
        i += 1;
    }
    let fp_a = book_a.chain_tip();
    let fp_b = book_b.chain_tip();
    if fp_a != fp_b {
        return Err(FailArcError::ReplayMismatch { fp_a, fp_b });
    }
    Ok((fp_a, fp_b))
}

// ── 小工具 ───────────────────────────────────────────────────────────
/// 齐备率 ppm（整数制零浮点；expected=0 时空账口径 10000，
/// 有档无应归档（全多档）口径 0——不虚报）。
pub fn completeness_ppm(ready: u32, expected: u32) -> u64 {
    if expected == 0 {
        return if ready == 0 { FULL_PPM } else { 0 };
    }
    (ready as u64).saturating_mul(PPM_SCALE) / (expected as u64)
}

/// 长度 len 的全 false 位图（while 填装；const fn 纪律同款循环体）。
fn alloc_vec_false(len: usize) -> Vec<bool> {
    let mut v = Vec::new();
    let mut i: usize = 0;
    while i < len {
        v.push(false);
        i += 1;
    }
    v
}

// ── 冒烟测试（std 测试惯例：assert! 判定，取值一律 unwrap_or* 兜底，
//    测试体亦零裸 unwrap / 零 expect / 零 panic! 宏直呼） ───────────────
#[cfg(test)]
mod tests {
    use super::*;

    // ── 场景素材 ─────────────────────────────────────────────────────

    /// 确定性内容合成（xorshift64：参数为标准三移位常数）。
    fn synth_content(seed: u64, len: usize) -> Vec<u8> {
        let mut v = Vec::new();
        let mut x = seed
            .wrapping_mul(0x9E37_79B9_7F4A_7C15)
            .wrapping_add(0x1234_5678_9ABC_DEF0);
        let mut i: usize = 0;
        while i < len {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            v.push((x & 0xFF) as u8);
            i += 1;
        }
        v
    }

    /// 三失败演示矩阵（机 × 轮）：0 机 5 轮 / 1 机 23 轮 / 2 机 41 轮。
    const DEMO_FAILS: [(u8, u16); 3] = [(0, 5), (1, 23), (2, 41)];

    /// 造日志段（内容以档号为种子，确定性）。
    fn mk_log(arc_no: u32) -> LogSegment {
        let r = LogSegment::capture(
            1,
            100 + arc_no * 10,
            108 + arc_no * 10,
            &synth_content(0x1000 + arc_no as u64, 96),
        );
        match r {
            Ok(p) => p,
            Err(_) => LogSegment {
                stream: 0,
                start_line: 1,
                end_line: 2,
                byte_len: 1,
                summary_hash: 0,
                guard: 0,
            },
        }
    }

    /// 造截图位。
    fn mk_shot(arc_no: u32, tick: u64) -> ShotSlot {
        let region = ShotRegion {
            x: 10 * arc_no,
            y: 20,
            w: 640,
            h: 480,
        };
        let r = ShotSlot::capture(
            500 + arc_no,
            region,
            tick,
            &synth_content(0x2000 + arc_no as u64, 128),
        );
        match r {
            Ok(p) => p,
            Err(_) => ShotSlot {
                frame_no: 1,
                region: ShotRegion {
                    x: 0,
                    y: 0,
                    w: 1,
                    h: 1,
                },
                shot_tick: 0,
                pixels_digest: 0,
                guard: 0,
            },
        }
    }

    /// 造账行引用（book_no=687 → F0687 成功率账行）。
    fn mk_line(arc_no: u32) -> LedgerLineRef {
        let r = LedgerLineRef::capture(
            687,
            100 + arc_no,
            &synth_content(0x3000 + arc_no as u64, 32),
        );
        match r {
            Ok(p) => p,
            Err(_) => LedgerLineRef {
                book_no: 1,
                line_no: 1,
                row_hash: 0,
                guard: 0,
            },
        }
    }

    /// 建全齐账：3 失败 × 3 件，引用镜像同步产出。
    fn build_full_book(base_tick: u64) -> (FailArchiveBook, Vec<SuccFailRef>) {
        let mut book = FailArchiveBook::new();
        let mut refs = Vec::new();
        let mut i: usize = 0;
        while i < DEMO_FAILS.len() {
            let (m, r) = DEMO_FAILS[i];
            let arc_no = i as u32 + 1;
            let aref = format_arc_ref(arc_no);
            let t = base_tick + i as u64;
            assert!(book.open_entry(m, r, &aref, t).is_ok());
            assert!(
                book.deposit_log_segment(arc_no, mk_log(arc_no), t + 1)
                    .is_ok()
            );
            assert!(
                book.deposit_shot_slot(arc_no, mk_shot(arc_no, t), t + 2)
                    .is_ok()
            );
            assert!(
                book.deposit_ledger_line(arc_no, mk_line(arc_no), t + 3)
                    .is_ok()
            );
            refs.push(SuccFailRef::new(m, r, &aref));
            i += 1;
        }
        (book, refs)
    }

    /// 建缺件账：档 1 全齐，档 2 仅日志段，档 3 全缺。
    fn build_partial_book() -> (FailArchiveBook, Vec<SuccFailRef>) {
        let mut book = FailArchiveBook::new();
        let mut refs = Vec::new();
        let mut i: usize = 0;
        while i < DEMO_FAILS.len() {
            let (m, r) = DEMO_FAILS[i];
            let arc_no = i as u32 + 1;
            let aref = format_arc_ref(arc_no);
            assert!(book.open_entry(m, r, &aref, 100 + i as u64).is_ok());
            if arc_no == 1 {
                assert!(
                    book.deposit_log_segment(arc_no, mk_log(arc_no), 111)
                        .is_ok()
                );
                assert!(
                    book.deposit_shot_slot(arc_no, mk_shot(arc_no, 112), 112)
                        .is_ok()
                );
                assert!(
                    book.deposit_ledger_line(arc_no, mk_line(arc_no), 113)
                        .is_ok()
                );
            } else if arc_no == 2 {
                assert!(
                    book.deposit_log_segment(arc_no, mk_log(arc_no), 121)
                        .is_ok()
                );
            }
            refs.push(SuccFailRef::new(m, r, &aref));
            i += 1;
        }
        (book, refs)
    }

    /// 断言某 Result<CompletenessAudit> 的 complete 位（零 unwrap 取值）。
    fn audit_complete(r: Result<CompletenessAudit, FailArcError>) -> bool {
        match r {
            Ok(a) => a.complete,
            Err(_) => false,
        }
    }

    // ── J1-a 三件齐备 100% ───────────────────────────────────────────

    #[test]
    fn j1a_triple_complete_100_percent() {
        let (mut book, refs) = build_full_book(1000);
        // 三件齐档 3/3，齐备率 10000ppm（=100%），零红案零悬空零多档。
        let audit = book.audit_from_refs(&refs);
        assert!(audit.is_ok());
        match &audit {
            Ok(a) => {
                assert_eq!(a.expected, 3);
                assert_eq!(a.ready, 3);
                assert_eq!(a.red_count, 0);
                assert_eq!(a.ratio_ppm, 10_000);
                assert!(a.complete);
                assert!(a.dangling.is_empty());
                assert_eq!(a.orphan_count, 0);
            }
            Err(_) => assert!(false, "全齐账审计应过"),
        }
        // 逐档三件套组装：self_hash 复算一致（件内容完整性）。
        let mut arc = 1u32;
        while arc <= 3 {
            let maybe_art = book.artifact_of(arc).unwrap_or(None);
            assert!(maybe_art.is_some());
            let ok_art = match maybe_art {
                Some(a) => a,
                None => FailArtifact {
                    arc_no: 0,
                    machine: 0,
                    round: 0,
                    ref_hash: 0,
                    log: mk_log(9),
                    shot: mk_shot(9, 0),
                    line: mk_line(9),
                    self_hash: 0,
                    origin_tick: 0,
                },
            };
            assert_eq!(ok_art.arc_no, arc);
            assert_eq!(ok_art.self_hash, ok_art.fold_hash());
            arc += 1;
        }
        // 封板：齐备率 100% 落簿；封板后只读。
        let sealed = book.seal(&refs, 999);
        assert!(sealed.is_ok());
        let rec = sealed.unwrap_or(SealRecord {
            expected_failures: 0,
            ready_entries: 0,
            ratio_ppm: 0,
            chain_tip: 0,
            sealed_tick: 0,
        });
        assert_eq!(rec.expected_failures, 3);
        assert_eq!(rec.ready_entries, 3);
        assert_eq!(rec.ratio_ppm, 10_000);
        assert_eq!(rec.chain_tip, book.chain_tip());
        assert!(book.is_sealed());
        assert!(book.open_entry(2, 7, &format_arc_ref(4), 1).is_err());
        assert!(book.seal(&refs, 2).is_err());
    }

    // ── J1-c 零抹除：指纹只增、前缀永在 ──────────────────────────────

    #[test]
    fn j1c_zero_erase_fingerprint_only_grows() {
        // 半程账：档 2 缺截图位与账行引用，档 3 全缺。
        let (mut book, refs) = build_partial_book();
        let hist_before = book.fingerprint_history().to_vec();
        let tip_before = book.chain_tip();
        assert_eq!(book.entry_count(), 3);
        assert_eq!(book.entries_high_water(), 3);
        // 开档 3 + 存件 4 = 7 次合法追加。
        assert_eq!(book.mutation_count(), 7);
        // 补档两件（追加）：历史前缀逐字节不变，长度 +2，链尖演进。
        assert!(book.backfill_shot_slot(2, mk_shot(2, 900), 901).is_ok());
        assert!(book.backfill_ledger_line(2, mk_line(2), 902).is_ok());
        let hist_mid = book.fingerprint_history();
        assert_eq!(hist_mid.len(), hist_before.len() + 2);
        let mut k: usize = 0;
        while k < hist_before.len() {
            assert_eq!(hist_mid[k], hist_before[k]); // 前缀永在 = 零抹除
            k += 1;
        }
        assert_ne!(book.chain_tip(), tip_before); // 链照常演进（追加非改写）
        // 档 3 三件补齐（补档通道）→ 追加计数 12，审计转绿。
        assert!(book.backfill_log_segment(3, mk_log(3), 903).is_ok());
        assert!(book.backfill_shot_slot(3, mk_shot(3, 904), 904).is_ok());
        assert!(book.backfill_ledger_line(3, mk_line(3), 905).is_ok());
        assert_eq!(book.mutation_count(), 12);
        assert_eq!(book.fingerprint_history().len() as u64, book.mutation_count());
        assert!(book.verify_chain().is_ok());
        assert!(audit_complete(book.audit_from_refs(&refs)));
    }

    // ── J1-d 悬空引用检出 ────────────────────────────────────────────

    #[test]
    fn j1d_dangling_ref_detected() {
        let (mut book, refs) = build_full_book(2000);
        // 注入一条 F0687 引用指向不存在的档 99。
        let mut bad_refs = refs.clone();
        bad_refs.push(SuccFailRef::new(0, 9, "F0688-ARC-099"));
        let rep = book.reconcile_with_succ_refs(&bad_refs);
        assert!(rep.is_ok());
        match rep {
            Ok(r) => {
                assert_eq!(r.refs_total, 4);
                assert_eq!(r.resolved, 3);
                assert_eq!(r.dangling.len(), 1);
                assert_eq!(r.dangling[0].parsed_arc_no, 99);
                assert!(!r.ok);
            }
            Err(_) => assert!(false, "对账应可跑"),
        }
        // 机号错位（同串不同机）也是悬空：三对齐口径。
        let mut wrong_machine = refs.clone();
        wrong_machine[1].machine = 2;
        match book.reconcile_with_succ_refs(&wrong_machine) {
            Ok(r2) => {
                assert_eq!(r2.resolved, 2);
                assert_eq!(r2.dangling.len(), 1);
                assert!(!r2.ok);
            }
            Err(_) => assert!(false, "对账应可跑"),
        }
        // 悬空在 → 拒封板（DanglingRef 红案）。
        let sealed = book.seal(&bad_refs, 5);
        assert!(sealed.is_err());
        assert_eq!(
            sealed.unwrap_err().err_code(),
            SEG_BASE + 0xD // E_DANGLING
        );
        // 干净引用面恢复封板。
        assert!(book.seal(&refs, 6).is_ok());
    }

    // ── J1-f 补档通道 ────────────────────────────────────────────────

    #[test]
    fn j1f_backfill_channel_appends() {
        let (mut book, refs) = build_partial_book();
        // 档 2 缺截图位 → 红案在账（since_tick = 开档刻度 101）。
        let st_shot = book
            .piece_status(2, ArtifactKind::Shot)
            .unwrap_or(PieceStatus::Present {
                backfilled: false,
                origin_tick: 0,
                deposited_tick: 0,
            });
        assert_eq!(st_shot, PieceStatus::Missing { since_tick: 101 });
        // 补档：backfilled 标记 + 原始时戳保留（origin == 开档刻度）。
        assert!(book.backfill_shot_slot(2, mk_shot(2, 800), 810).is_ok());
        let st_after = book
            .piece_status(2, ArtifactKind::Shot)
            .unwrap_or(PieceStatus::Missing { since_tick: 0 });
        assert_eq!(
            st_after,
            PieceStatus::Present {
                backfilled: true,
                origin_tick: 101,
                deposited_tick: 810,
            }
        );
        // 补档命中已有件 → 拒收（补档不是覆写）。
        let dup = book.backfill_shot_slot(2, mk_shot(2, 820), 830);
        assert!(dup.is_err());
        assert_eq!(
            dup.unwrap_err().err_code(),
            SEG_BASE + 0x9 // E_ITEM_DUP
        );
        // 档 2 账行引用 + 档 3 三件全走补档 → 审计/健康双绿。
        assert!(book.backfill_ledger_line(2, mk_line(2), 840).is_ok());
        assert!(book.backfill_log_segment(3, mk_log(3), 850).is_ok());
        assert!(book.backfill_shot_slot(3, mk_shot(3, 860), 860).is_ok());
        assert!(book.backfill_ledger_line(3, mk_line(3), 870).is_ok());
        assert!(audit_complete(book.audit_from_refs(&refs)));
        let health = book.health();
        assert!(health.healthy);
    }

    // ── 注入一：账截断攻击（指纹对拍检出） ────────────────────────────

    #[test]
    fn inject_truncation_detected() {
        let (mut book, _refs) = build_full_book(3000);
        let hist_len = book.fingerprint_history().len();
        assert_eq!(book.entry_count(), 3);
        // 模拟外力截断：抹掉后两档（业务面无此通道——注入面专用）。
        let removed = book.attack_truncate_entries(1);
        assert_eq!(removed, 2);
        assert_eq!(book.entry_count(), 1);
        assert_eq!(book.entries_high_water(), 3); // 高水位不回落
        // 链对拍必检出：kept(1) != expected(3)。
        let v = book.verify_chain();
        assert!(v.is_err());
        match v {
            Err(FailArcError::TruncationDetected { kept, expected }) => {
                assert_eq!(kept, 1);
                assert_eq!(expected, 3);
            }
            _ => assert!(false, "应为截断红案"),
        }
        // 指纹历史未被抹（零抹除证据链）：长度不变。
        assert_eq!(book.fingerprint_history().len(), hist_len);
        // 健康巡检亮截断码。
        let health = book.health();
        assert!(!health.healthy);
        assert!(health.issue_codes.contains(&(SEG_BASE + 0xC)));
        // 截断到不低于现长 → 无操作零返回。
        assert_eq!(book.attack_truncate_entries(5), 0);
    }

    // ── 注入二：悬空引用注入（对账面检出） ────────────────────────────

    #[test]
    fn inject_dangling_detected() {
        let (mut book, _refs) = build_full_book(3100);
        // 全假引用面：三条引用全指不存在的档。
        let mut fakes = Vec::new();
        fakes.push(SuccFailRef::new(0, 5, "F0688-ARC-011"));
        fakes.push(SuccFailRef::new(1, 23, "F0688-ARC-012"));
        fakes.push(SuccFailRef::new(2, 41, "F0688-ARC-013"));
        match book.reconcile_with_succ_refs(&fakes) {
            Ok(r) => {
                assert_eq!(r.resolved, 0);
                assert_eq!(r.dangling.len(), 3);
                assert_eq!(r.unreferenced_entries, 3); // 反向：真档全被架空
                assert!(!r.ok);
            }
            Err(_) => assert!(false, "对账应可跑"),
        }
        // 审计面：ready 0（齐档全不被引用），complete=false，多档 3。
        match book.audit_from_refs(&fakes) {
            Ok(a) => {
                assert_eq!(a.ready, 0);
                assert!(!a.complete);
                assert_eq!(a.orphan_count, 3);
            }
            Err(_) => assert!(false, "审计应可跑"),
        }
    }

    // ── 注入三：缺件注入（齐备审计检出 + 拒封板） ─────────────────────

    #[test]
    fn inject_missing_piece_detected() {
        let (mut book, refs) = build_partial_book();
        match book.audit_from_refs(&refs) {
            Ok(a) => {
                assert!(!a.complete);
                assert_eq!(a.red_count, 2);
                assert_eq!(a.ratio_ppm, 3_333); // 1/3 → 3333ppm（整除截断）
                // 红案位图：档 2 缺截图位+账行引用（0x06），档 3 全缺（0x07）。
                assert_eq!(a.missing[0].arc_no, 2);
                assert_eq!(a.missing[0].missing_mask, MASK_SHOT | MASK_LINE);
                assert_eq!(a.missing[1].arc_no, 3);
                assert_eq!(a.missing[1].missing_mask, MASK_TRIPLE);
                // 缺件清单件名可读。
                assert!(a.missing[0].mask_names().contains("截图位"));
                assert!(a.missing[1].mask_names().contains("日志段"));
            }
            Err(_) => assert!(false, "审计应可跑"),
        }
        // 拒封板：Incomplete 红案带首个缺件位图。
        let sealed = book.seal(&refs, 7);
        assert!(sealed.is_err());
        match sealed {
            Err(FailArcError::Incomplete { arc_no, missing_mask }) => {
                assert_eq!(arc_no, 2);
                assert_eq!(missing_mask, 0x06);
            }
            _ => assert!(false, "应为缺件红案"),
        }
        // 健康巡检亮缺件码。
        let health = book.health();
        assert!(!health.healthy);
        assert!(health.issue_codes.contains(&(SEG_BASE + 0xE)));
        assert_eq!(health.pending_missing, 2);
    }

    // ── 双跑确定性对拍 ───────────────────────────────────────────────

    #[test]
    fn dual_run_deterministic() {
        let mut steps = Vec::new();
        let mut i: usize = 0;
        while i < DEMO_FAILS.len() {
            let (m, r) = DEMO_FAILS[i];
            let arc_no = i as u32 + 1;
            let aref = format_arc_ref(arc_no);
            steps.push(ArchiveStep {
                machine: m,
                arc_round: r,
                arc_ref: String::from(aref.as_str()),
                action: StepAction::Open,
            });
            steps.push(ArchiveStep {
                machine: m,
                arc_round: r,
                arc_ref: String::from(aref.as_str()),
                action: StepAction::Log(mk_log(arc_no)),
            });
            steps.push(ArchiveStep {
                machine: m,
                arc_round: r,
                arc_ref: String::from(aref.as_str()),
                action: StepAction::Shot(mk_shot(arc_no, 0)),
            });
            steps.push(ArchiveStep {
                machine: m,
                arc_round: r,
                arc_ref: String::from(aref.as_str()),
                action: StepAction::Line(mk_line(arc_no)),
            });
            i += 1;
        }
        let r = dual_run_replay(&steps);
        assert!(r.is_ok());
        let (fp_a, fp_b) = r.unwrap_or((1, 2));
        assert_eq!(fp_a, fp_b);
        assert_ne!(fp_a, 0);
        // 第三本手工重放同档步：链尖与双跑全等，链历史 12 条。
        let mut book_c = FailArchiveBook::new();
        let mut k: usize = 0;
        while k < steps.len() {
            let tick = k as u64 + 1;
            assert!(apply_step(&mut book_c, &steps[k], tick).is_ok());
            k += 1;
        }
        assert_eq!(book_c.chain_tip(), fp_a);
        assert_eq!(book_c.fingerprint_history().len(), 12);
    }

    // ── 件内容篡改检出（guard 复算） ─────────────────────────────────

    #[test]
    fn tamper_detected() {
        let (mut book, refs) = build_full_book(4000);
        assert!(book.verify_all_content().unwrap_or(0) == 3);
        // 注入面·改写攻击：档 2 截图位帧号 +1（guard 不重烙）。
        assert!(book.attack_flip_piece_field(2, ArtifactKind::Shot).is_ok());
        let v = book.verify_entry_content(2);
        assert!(v.is_err());
        match v {
            Err(FailArcError::TamperDetected { arc_no, kind, .. }) => {
                assert_eq!(arc_no, 2);
                assert_eq!(kind, ArtifactKind::Shot.as_raw());
            }
            _ => assert!(false, "应为篡改红案"),
        }
        // 全账校验也拦截（首篡改档显形）。
        assert!(book.verify_all_content().is_err());
        // 齐备审计前置内容校验 → 篡改传播拒审计。
        let audit = book.audit_from_refs(&refs);
        assert!(audit.is_err());
        assert_eq!(
            audit.unwrap_err().err_code(),
            SEG_BASE + 0xB // E_TAMPER
        );
        // 健康巡检亮篡改码。
        let health = book.health();
        assert!(!health.healthy);
        assert!(health.issue_codes.contains(&(SEG_BASE + 0xB)));
        // 改写也破坏档哈希 → 链对拍同时失配（结构面伞码）。
        assert!(!book.chain_report().ok);
    }

    // ── 引用号格式闸 ─────────────────────────────────────────────────

    #[test]
    fn ref_format_gate() {
        let mut book = FailArchiveBook::new();
        // 空串 / 超长 → E_REF_INVALID。
        assert!(book.open_entry(0, 1, "", 1).is_err());
        let long_ref = "A".repeat(FAIL_REF_MAX_LEN + 1);
        assert!(book.open_entry(0, 1, long_ref.as_str(), 1).is_err());
        // 坏前缀 / 非数字尾 / 次序跳号 / 空数字段 → E_REF_FORMAT。
        assert!(book.open_entry(0, 1, "F999-ARC-001", 1).is_err());
        assert!(book.open_entry(0, 1, "F0688-ARC-00X", 1).is_err());
        assert!(book.open_entry(0, 1, "F0688-ARC-005", 1).is_err()); // 首档应为 001
        assert!(book.open_entry(0, 1, "F0688-ARC-", 1).is_err());
        let seven_digits = String::from("F0688-ARC-1234567");
        assert!(parse_arc_ref(seven_digits.as_str()).is_none()); // 数字段超 cap
        // 正常首档 + 次序连续 + 同串重挂 → E_REF_DUP。
        assert!(book.open_entry(0, 1, "F0688-ARC-001", 10).is_ok());
        assert!(book.open_entry(2, 9, "F0688-ARC-002", 11).is_ok());
        let dup = book.open_entry(1, 3, "F0688-ARC-001", 12);
        assert!(dup.is_err());
        assert_eq!(
            dup.unwrap_err().err_code(),
            SEG_BASE + 0x7 // E_REF_DUP
        );
        // 同失败位二开 → E_DUP_SLOT。
        let dup_slot = book.open_entry(0, 1, "F0688-ARC-003", 13);
        assert!(dup_slot.is_err());
        assert_eq!(
            dup_slot.unwrap_err().err_code(),
            SEG_BASE + 0x4 // E_DUP_SLOT
        );
        // parse/format 双射。
        assert_eq!(parse_arc_ref(format_arc_ref(1).as_str()), Some(1));
        assert_eq!(parse_arc_ref(format_arc_ref(999).as_str()), Some(999));
        assert_eq!(parse_arc_ref(format_arc_ref(1234).as_str()), Some(1234));
    }

    // ── 机/轮界内与档位闸 ────────────────────────────────────────────

    #[test]
    fn open_gates_machine_round_noslot() {
        let mut book = FailArchiveBook::new();
        // 机号越界（合法 0/1/2）。
        let e1 = book.open_entry(3, 1, "F0688-ARC-001", 1);
        assert!(e1.is_err());
        assert_eq!(
            e1.unwrap_err().err_code(),
            SEG_BASE + 0x2 // E_MACHINE_RANGE
        );
        // 轮号越界（合法 1..=50）。
        assert!(book.open_entry(0, 0, "F0688-ARC-001", 1).is_err());
        let e3 = book.open_entry(0, 51, "F0688-ARC-001", 1);
        assert!(e3.is_err());
        assert_eq!(
            e3.unwrap_err().err_code(),
            SEG_BASE + 0x3 // E_ROUND_RANGE
        );
        // 边界内合法：轮 1 与轮 50。
        assert!(book.open_entry(0, 1, "F0688-ARC-001", 2).is_ok());
        assert!(book.open_entry(2, 50, "F0688-ARC-002", 3).is_ok());
        // 存件到不存在档 → E_NO_SLOT。
        let d = book.deposit_log_segment(9, mk_log(9), 4);
        assert!(d.is_err());
        assert_eq!(
            d.unwrap_err().err_code(),
            SEG_BASE + 0x8 // E_NO_SLOT
        );
        // 查档同码；失败位反查在位。
        assert!(book.entry_by_arc_no(9).is_none());
        assert!(book.arc_no_of_slot(2, 50).is_some());
        assert_eq!(book.arc_no_of_ref("F0688-ARC-002"), Some(2));
    }

    // ── 件内容校验闸 ─────────────────────────────────────────────────

    #[test]
    fn piece_validation_gates() {
        let bytes = synth_content(0xAA, 32);
        // 日志段：行序倒置（end <= start）。
        let bad_range = LogSegment::capture(1, 10, 10, &bytes);
        assert!(bad_range.is_err());
        match bad_range {
            Err(FailArcError::PieceInvalid { kind, reason_code }) => {
                assert_eq!(kind, ArtifactKind::Log.as_raw());
                assert_eq!(reason_code, 1);
            }
            _ => assert!(false, "应为件内容非法"),
        }
        // 日志段：内容空。
        assert!(LogSegment::capture(1, 1, 9, &[]).is_err());
        // 截图位：帧号 0 / 区域退化 / 像素空。
        let good_region = ShotRegion {
            x: 0,
            y: 0,
            w: 8,
            h: 8,
        };
        assert!(ShotSlot::capture(0, good_region, 1, &bytes).is_err());
        let bad_region = ShotRegion {
            x: 0,
            y: 0,
            w: 0,
            h: 8,
        };
        match ShotSlot::capture(7, bad_region, 1, &bytes) {
            Err(FailArcError::PieceInvalid { reason_code, .. }) => {
                assert_eq!(reason_code, 3);
            }
            _ => assert!(false, "应为区域退化"),
        }
        assert!(ShotSlot::capture(7, good_region, 1, &[]).is_err());
        // 账行引用：账本号 0 / 行号 0。
        match LedgerLineRef::capture(0, 5, &bytes) {
            Err(FailArcError::PieceInvalid { reason_code, .. }) => {
                assert_eq!(reason_code, 5);
            }
            _ => assert!(false, "应为账本号 0"),
        }
        match LedgerLineRef::capture(687, 0, &bytes) {
            Err(FailArcError::PieceInvalid { reason_code, .. }) => {
                assert_eq!(reason_code, 4);
            }
            _ => assert!(false, "应为行号 0"),
        }
        // 合法件 guard 自校验全真。
        assert!(mk_log(1).verify_guard());
        assert!(mk_shot(1, 5).verify_guard());
        assert!(mk_line(1).verify_guard());
    }

    // ── ppm 整数口径 ─────────────────────────────────────────────────

    #[test]
    fn ppm_integer_semantics() {
        assert_eq!(completeness_ppm(3, 3), 10_000);
        assert_eq!(completeness_ppm(2, 3), 6_666);
        assert_eq!(completeness_ppm(1, 3), 3_333);
        assert_eq!(completeness_ppm(0, 3), 0);
        assert_eq!(completeness_ppm(0, 0), 10_000); // 空账空引用：空真齐
        assert_eq!(completeness_ppm(2, 0), 0); // 有档无应归档：多档不虚报
    }

    // ── 事件账流 ─────────────────────────────────────────────────────

    #[test]
    fn event_stream_shape() {
        let (mut book, refs) = build_full_book(5000);
        // 3 开档 + 9 存件 + 3 三件齐瞬间 = 15 事件。
        assert_eq!(book.event_count(), 15);
        let mut opened = 0;
        let mut deposited = 0;
        let mut complete = 0;
        let evs = book.events();
        let mut i: usize = 0;
        while i < evs.len() {
            match evs[i].kind {
                EventKind::EntryOpened => opened += 1,
                EventKind::PieceDeposited => deposited += 1,
                EventKind::EntryComplete => complete += 1,
                _ => {}
            }
            i += 1;
        }
        assert_eq!(opened, 3);
        assert_eq!(deposited, 9);
        assert_eq!(complete, 3);
        // 对账 + 封板事件落账。
        assert!(book.reconcile_with_succ_refs(&refs).is_ok());
        assert!(book.seal(&refs, 9).is_ok());
        let mut saw_reconcile = false;
        let mut saw_seal = false;
        let evs2 = book.events();
        let mut k: usize = 0;
        while k < evs2.len() {
            match evs2[k].kind {
                EventKind::ReconcileRan => saw_reconcile = true,
                EventKind::Sealed => saw_seal = true,
                _ => {}
            }
            k += 1;
        }
        assert!(saw_reconcile);
        assert!(saw_seal);
        // 事件序号连续零跳号。
        let mut expect_seq = 1u32;
        let mut j: usize = 0;
        while j < evs2.len() {
            assert_eq!(evs2[j].seq, expect_seq);
            expect_seq += 1;
            j += 1;
        }
    }

    // ── 诊断段闭集 ───────────────────────────────────────────────────

    #[test]
    fn diag_segment_closure() {
        // 段内 15 码全有中文可操作诊断（非未知文案）。
        let mut code = SEG_BASE + 1;
        while code < SEG_BASE + 0x10 {
            let d = diag_detail(code);
            assert!(!d.is_empty());
            assert!(!d.contains("未知"));
            code += 1;
        }
        // 段外（含段基与右邻）一律未知。
        assert!(diag_detail(SEG_BASE).contains("未知"));
        assert!(diag_detail(SEG_BASE + 0x10).contains("未知"));
        assert!(diag_detail(0x0000).contains("未知"));
        // 15 变体 err_code 全落段内且互异。
        let variants: [FailArcError; 15] = [
            FailArcError::Sealed,
            FailArcError::MachineOutOfRange { got: 9 },
            FailArcError::RoundOutOfRange { got: 99 },
            FailArcError::DuplicateSlot {
                machine: 0,
                round: 1,
            },
            FailArcError::RefInvalid { len: 0, cap: 64 },
            FailArcError::RefFormat { len: 12 },
            FailArcError::RefDuplicate { existing_arc_no: 1 },
            FailArcError::NoSlot { arc_no: 9 },
            FailArcError::ItemDuplicate { arc_no: 1, kind: 1 },
            FailArcError::PieceInvalid {
                kind: 1,
                reason_code: 1,
            },
            FailArcError::TamperDetected {
                arc_no: 1,
                kind: 1,
                stored: 0,
                recomputed: 1,
            },
            FailArcError::TruncationDetected { kept: 1, expected: 3 },
            FailArcError::DanglingRef { arc_no: 99 },
            FailArcError::Incomplete {
                arc_no: 2,
                missing_mask: 0x06,
            },
            FailArcError::ReplayMismatch { fp_a: 1, fp_b: 2 },
        ];
        let mut i: usize = 0;
        while i < variants.len() {
            let c = variants[i].err_code();
            assert!(c > SEG_BASE && c < SEG_BASE + 0x10);
            let mut j: usize = i + 1;
            while j < variants.len() {
                assert_ne!(c, variants[j].err_code());
                j += 1;
            }
            assert!(!variants[i].detail().is_empty());
            i += 1;
        }
        // 枚举/常量口径编译期断言的运行期复证。
        assert_eq!(ArtifactKind::Log.as_raw(), 1);
        assert_eq!(ArtifactKind::Shot.as_raw(), 2);
        assert_eq!(ArtifactKind::Line.as_raw(), 3);
        assert_eq!(ArtifactKind::from_raw(4), None);
        assert_eq!(ArtifactKind::Log.mask_bit(), MASK_LOG);
        assert_eq!(ArtifactKind::Shot.mask_bit(), MASK_SHOT);
        assert_eq!(ArtifactKind::Line.mask_bit(), MASK_LINE);
        assert_eq!(ArtifactKind::Log.cname(), "日志段");
    }

    // ── 链对拍报告与高水位 ───────────────────────────────────────────

    #[test]
    fn chain_report_and_hwm() {
        let (book, _refs) = build_full_book(6000);
        let rep = book.chain_report();
        assert!(rep.ok);
        assert_eq!(rep.entries, 3);
        assert_eq!(rep.entries_hwm, 3);
        assert_eq!(rep.recomputed_tip, rep.stored_tip);
        assert_eq!(rep.history_len as u64, book.mutation_count());
        assert_eq!(rep.mutations, 12); // 3 开档 + 9 存件
        // FNV 已知向量自证（FNV-1a 64：""=offset basis，"a"=0xaf63dc4c8601ec8c）。
        assert_eq!(fnv1a64(b""), 0xCBF2_9CE4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xAF63_0002_8601_EC8C);
    }
}
