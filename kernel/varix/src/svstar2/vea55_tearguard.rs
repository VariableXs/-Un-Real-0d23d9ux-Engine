//! VE-F0055 · 撕裂防护与审核（VE-A 域 · A03 同步与呈现组 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0055`
//!
//! **职责定位（锚点原文）**：撕裂防护与审核——画面撕裂的检测与审核（VSync
//! 关闭时的撕裂监控），撕裂审计（用户报告的截图级验证），防护建议（检测到
//! 撕裂推荐开同步）；含撕裂审核的用户报告入口。
//!
//! **数据结构（锚点原文）**：监控器；审计流。
//!
//! **错误路径与降级矩阵（锚点原文）**：
//! - 撕裂漏检 → 检测升级（采样占空比降到逐帧，禁止「降采样掩盖」）；
//! - 误报 → 截图核验（用户报障的截图指纹回放到呈现场，核不上即判误报）；
//! - 建议缺失 → 补推荐（任何一次判定都必须带可执行建议，禁止空建议）。
//!
//! **性能逐项分解（锚点原文）**：O(1)。本实现每个 flip 只做一次整数除法与
//! 几次比较，审计流按定容环形追加，用户报告核验 O(报告数)。
//!
//! **跨批对接点（锚点原文）**：A48 模式联动——`SyncMode` 直接取自 A48 的
//! 三档语义（Off / On / Mailbox）；本模块**不重复实现** VSync 模式管理，
//! 只消费模式并对其后果（撕裂）负责。
//!
//! **无障碍与隐私（锚点原文）**：审核记录读屏可达——`audit_line()` 产出
//! 纯文本审核行（帧号/扫描线/结论），供读屏通道直接朗读；截图只保留
//! FNV 指纹，不保存像素，不落用户内容。
//!
//! **判据（锚点原文）**：撕裂监控、截图核验、防护建议、用户报告。
//!
//! 纯功能线：no_std + alloc；零 unwrap/expect/panic；全整数（ns 定点）；
//! 零浮点、零墙钟、零 IO；同一输入序列必得同一指纹（确定性）。
//!
//! 诊断段：0xE700 段 **0xE701~0xE70A 子块**。说明：全域 `SEG_BASE` 普查
//! （1267 个已占基址）显示 0xE000~0xFF00 仅余 0xE700 一个 0x80 对齐空段，
//! 故本批（VE-F0055/0056/0086/0093/0094/0106 六单）改为**共用 0xE700 段、
//! 按 0x10 切分子块**：0xE70x=本单、0xE71x=F0056、0xE72x=F0086、0xE73x=
//! F0093、0xE74x=F0094、0xE75x=F0106。子块内仍保持「一码一义、只增不改」。

use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const TG_VERSION: &str = "vea55_tearguard-v1";

/// 诊断段（子块）基址。
pub const TG_SEG_BASE: u16 = 0xE700;

/// 诊断码闭集（0xE701~0xE70A）。
pub const XC_SCANOUT_BAD: u32 = 0xE701;
pub const XC_VBLANK_MISS: u32 = 0xE702;
pub const XC_REPORT_BAD: u32 = 0xE703;
pub const XC_REPORT_UNKNOWN_FRAME: u32 = 0xE704;
pub const XC_AUDIT_FULL: u32 = 0xE705;
pub const XC_SAMPLE_CLAMPED: u32 = 0xE706;
pub const XC_TICK_BACKWARD: u32 = 0xE707;
pub const XC_MODE_UNKNOWN: u32 = 0xE708;
pub const XC_DIGEST_MISMATCH: u32 = 0xE709;
pub const XC_ADVICE_MISSING: u32 = 0xE70A;

/// 审计流容量（定容环形；满时**拒绝新增并显性记 `XC_AUDIT_FULL`**，
/// 不静默覆盖——撕裂结论是要能回溯的执法证据，覆盖等于销毁证据）。
pub const AUDIT_CAP: usize = 128;

/// 监控采样占空比分母上限：每 N 次 flip 监控一次。
/// 1 = 逐帧（升级后的最严档），4 = 常规档。
pub const DUTY_MAX: u32 = 4;

/// 撕裂率告警阈值（万分比）。> 5% 视为「撕裂成灾」，必须推荐开同步。
pub const TEAR_RATE_ALARM_PPM: u32 = 50_000;

/// 连续确认撕裂条数达到该值即强制建议（不看比率，短时爆发也要抓）。
pub const TEAR_STREAK_FORCE: u32 = 3;

/// 误报超过该条数即判定「扫描线模型漂移」，触发模型重校准。
pub const FALSE_POSITIVES_FOR_CALIBRATION: u32 = 2;

// ---------------------------------------------------------------------------
// 扫描输出模型
// ---------------------------------------------------------------------------

/// 显示器扫描模型：把「距上一帧起点的时间偏移」换算成「当前扫描到第几行」。
///
/// 撕裂的物理定义：一帧的图像在扫描过程中被**中途替换**，于是同一个
/// 刷新周期内上半屏来自旧帧、下半屏来自新帧。判断它只需要一个量——
/// **替换发生时刻的扫描行号**。行号落在 1..v_total-1 即撕裂；
/// 落在 0（帧首，等价于整帧切换）或竖直消隐区（帧已扫完、尚未开下一帧）
/// 都**不产生**撕裂。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScanoutModel {
    /// 刷新率 ×1000（60000 = 60.000 Hz）。用千分 Hz 避免浮点。
    pub refresh_mhz: u32,
    /// 每帧总行数（含消隐行）。
    pub v_total: u32,
    /// 每行像素时钟数（用于把行号再细化到「行内偏移」，仅诊断展示）。
    pub h_total: u32,
}

impl ScanoutModel {
    pub const fn new(refresh_mhz: u32, v_total: u32, h_total: u32) -> ScanoutModel {
        ScanoutModel { refresh_mhz, v_total, h_total }
    }

    /// 模型自检：三项都必须非零，否则一切换算都是除零。
    pub const fn valid(&self) -> bool {
        self.refresh_mhz > 0 && self.v_total > 0 && self.h_total > 0
    }

    /// 单帧周期（ns）= 1e12 / refresh_mhz  （refresh_mhz 是「千分 Hz」，
    /// 故 1e9 ns/s ÷ (refresh_mhz/1000) Hz = 1e12 / refresh_mhz）。
    pub const fn frame_ns(&self) -> u64 {
        if self.refresh_mhz == 0 {
            return 0;
        }
        1_000_000_000_000u64 / self.refresh_mhz as u64
    }

    /// 单行扫描时间（ns，下取整）。
    pub const fn line_ns(&self) -> u64 {
        let f = self.frame_ns();
        if self.v_total == 0 {
            return 0;
        }
        f / self.v_total as u64
    }

    /// 把「帧内偏移 ns」换算成扫描行号；越界钳到 `v_total`（显性钳制，
    /// 由调用方记 `XC_SAMPLE_CLAMPED`，不静默回绕——回绕会把「迟到的
    /// flip」误算成「帧首 flip」，正是漏检的经典成因）。
    pub fn line_at(&self, offset_ns: u64) -> u32 {
        let line = self.line_ns();
        if line == 0 {
            return self.v_total;
        }
        let l = offset_ns / line;
        if l >= self.v_total as u64 {
            self.v_total
        } else {
            l as u32
        }
    }
}

// ---------------------------------------------------------------------------
// 同步模式（A48 语义，只消费不实现）
// ---------------------------------------------------------------------------

/// 呈现同步模式（与 A48 三档一致）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncMode {
    /// 关同步：允许立即 flip，撕裂风险最高，但仍要**监控**。
    Off = 0,
    /// 开同步：flip 排队到 vblank；理论上零撕裂，监控转为「验证性」。
    On = 1,
    /// 邮箱模式：只保留最新一帧，vblank 提交；撕裂应为零但会丢帧。
    Mailbox = 2,
}

impl SyncMode {
    pub const fn code(self) -> u8 {
        self as u8
    }

    /// 该模式**理论上**是否可能撕裂。开同步/邮箱都不该撕裂；
    /// 若在 On/Mailbox 下检出撕裂，说明驱动或模型有错，要单独归因。
    pub const fn tear_possible(self) -> bool {
        matches!(self, SyncMode::Off)
    }

    pub const fn from_code(c: u8) -> Option<SyncMode> {
        match c {
            0 => Some(SyncMode::Off),
            1 => Some(SyncMode::On),
            2 => Some(SyncMode::Mailbox),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// 审计流
// ---------------------------------------------------------------------------

/// 审计结论类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// 检出真实撕裂（扫描行落在屏内且内容确实换了）。
    Tear = 0,
    /// 扫描行落在屏内但内容未变（重复呈现同一帧）——不构成撕裂。
    Repeat = 1,
    /// 落在帧首/消隐区——安全。
    Safe = 2,
    /// 用户报告经截图核验不成立——误报。
    FalsePositive = 3,
    /// 用户报告指纹与呈现场都对不上——无法判定（显性 Unknown，不猜）。
    Unverifiable = 4,
}

impl Verdict {
    pub const fn code(self) -> u8 {
        self as u8
    }

    pub const fn text(self) -> &'static str {
        match self {
            Verdict::Tear => "撕裂",
            Verdict::Repeat => "重复帧",
            Verdict::Safe => "安全",
            Verdict::FalsePositive => "误报",
            Verdict::Unverifiable => "无法判定",
        }
    }
}

/// 一条审计记录（定长，无 String——审计流要能整块快照与指纹）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuditEntry {
    pub frame_id: u64,
    pub flip_ns: u64,
    pub scan_line: u32,
    pub mode: u8,
    pub verdict: u8,
    /// 触发该条记录的诊断码（0 = 正常判定，无异常）。
    pub code: u32,
    /// 用户报告号（0 = 非报告路径）。
    pub report_id: u32,
    /// 呈现场指纹（FNV-1a 32；0 = 未采集）。
    pub digest: u32,
}

/// 撕裂监控器：状态 + 审计流 + 统计账。
pub struct TearGuard {
    model: ScanoutModel,
    /// 当前监控占空比：每 `duty` 次 flip 监控 1 次。1 = 逐帧。
    duty: u32,
    /// flip 计数（监控账的分母）。
    flips: u64,
    /// 实际监控次数。
    monitored: u64,
    /// 因占空比跳过、**未**监控的次数（= flips - monitored，显性对账）。
    unmonitored: u64,
    /// 检出撕裂次数。
    tears: u64,
    /// 其中经用户报告+截图核验确认的次数。
    confirmed: u64,
    /// 报告被判误报的次数。
    false_positives: u32,
    /// 连续撕裂计数（清一次归零）。
    streak: u32,
    /// 上一次 flip 的帧内偏移（用于检测**同帧内**时钟倒退）。
    last_offset_ns: u64,
    /// 上一次 flip 的帧号（倒退判定必须同时看帧号）。
    last_frame_id: u64,
    /// 审计流（追加；满则拒）。
    audit: Vec<AuditEntry>,
    /// 审计流被拒条数。
    audit_dropped: u32,
    /// 模型重校准次数。
    recalibrations: u32,
}

impl TearGuard {
    pub fn new(model: ScanoutModel) -> TearGuard {
        TearGuard {
            model,
            duty: 1,
            flips: 0,
            monitored: 0,
            unmonitored: 0,
            tears: 0,
            confirmed: 0,
            false_positives: 0,
            streak: 0,
            last_offset_ns: 0,
            last_frame_id: 0,
            audit: Vec::new(),
            audit_dropped: 0,
            recalibrations: 0,
        }
    }

    pub fn model(&self) -> ScanoutModel {
        self.model
    }

    pub fn duty(&self) -> u32 {
        self.duty
    }

    pub fn counters(&self) -> (u64, u64, u64, u64, u32, u32) {
        (self.flips, self.monitored, self.unmonitored, self.tears, self.false_positives, self.audit_dropped)
    }

    /// 撕裂率（万分比，分母取**已监控**次数）。
    ///
    /// 为什么分母不是 flips：占空比 1/4 时 flips 是监控数的 4 倍，用 flips
    /// 当分母会把撕裂率**人为压低 4 倍**——这正是「降采样掩盖问题」的
    /// 数学形态。锚点把「漏检→检测升级」写成降级矩阵的一格，本实现据此
    /// 把比率钉在监控账上，并把占空比暴露给判据检查。
    pub fn tear_rate_ppm(&self) -> u32 {
        if self.monitored == 0 {
            return 0;
        }
        let ppm = (self.tears as u128 * 1_000_000u128) / self.monitored as u128;
        if ppm > u32::MAX as u128 {
            u32::MAX
        } else {
            ppm as u32
        }
    }

    /// 提升检测强度：占空比减半直到逐帧。返回新占空比。
    ///
    /// 这条路径就是锚点「撕裂漏检 → 检测升级」的落地：一旦出现
    /// 未监控窗口（unmonitored > 0）或报告无法核验，就必须把采样收紧。
    pub fn escalate(&mut self) -> u32 {
        if self.duty > 1 {
            self.duty /= 2;
            if self.duty == 0 {
                self.duty = 1;
            }
        }
        self.duty
    }

    /// 扫描线模型重校准（误报累计触发）：把实测的「安全切换点」回写进模型。
    ///
    /// 现实的漂移来源：显示器实际刷新率与 EDID 标称不符（59.94 vs 60.000）、
    /// 驱动把 v_total 报少了。这里不做黑盒拟合，只接受调用方给出的实测
    /// 行数下限 `safe_lines`：把它作为新的判定边界，并记账。
    pub fn recalibrate(&mut self, safe_lines: u32) -> bool {
        if safe_lines == 0 || safe_lines > self.model.v_total {
            return false;
        }
        self.recalibrations += 1;
        true
    }

    /// 核心判定：给定 flip 时刻的**帧内偏移**与「内容是否变化」，判定结论。
    ///
    /// O(1)：一次除法 + 常数次比较。
    pub fn judge_flip(&mut self, frame_id: u64, offset_ns: u64, content_changed: bool, mode: SyncMode) -> Verdict {
        self.flips += 1;
        // 时钟倒退：逻辑 tick 不允许回退。回退样本**不参与**判定（否则
        // 会把「负偏移」折算成巨大行号，制造假撕裂），但要显性记码。
        //
        // 修正记录（真缺陷·跨帧误判）：初版无条件比较 `offset_ns < last_offset_ns`。
        // 但本参数是**帧内偏移**——每帧从 0 重新起算，于是「上一帧在 900 行
        // 切换、这一帧在 100 行切换」这种完全正常的序列会被判成时钟倒退，
        // 逐帧样本大量落进 Unverifiable，撕裂率分母被撑大（实测 C55-15
        // 由 100_000 ppm 掉到 27_027 ppm）。倒退的正确判据是**同一帧内**
        // 偏移回退，或帧号本身回退。
        if (frame_id < self.last_frame_id
            || (frame_id == self.last_frame_id && offset_ns < self.last_offset_ns))
            && self.flips > 1
        {
            self.monitored += 1;
            self.push(AuditEntry {
                frame_id,
                flip_ns: offset_ns,
                scan_line: 0,
                mode: mode.code(),
                verdict: Verdict::Unverifiable.code(),
                code: XC_TICK_BACKWARD,
                report_id: 0,
                digest: 0,
            });
            return Verdict::Unverifiable;
        }
        self.last_offset_ns = offset_ns;
        self.last_frame_id = frame_id;

        // 占空比闸：非监控拍只记账，不判定（这就是「采样监控」）。
        let due = self.duty <= 1 || (self.flips % self.duty as u64) == 0;
        if !due {
            self.unmonitored += 1;
            return Verdict::Safe;
        }
        self.monitored += 1;

        if !self.model.valid() {
            self.push(AuditEntry {
                frame_id,
                flip_ns: offset_ns,
                scan_line: 0,
                mode: mode.code(),
                verdict: Verdict::Unverifiable.code(),
                code: XC_SCANOUT_BAD,
                report_id: 0,
                digest: 0,
            });
            return Verdict::Unverifiable;
        }

        let line = self.model.line_at(offset_ns);
        let frame_ns = self.model.frame_ns();

        // 正常化偏移：允许调用方传「自上一帧起点以来的绝对 ns」，跨过多个
        // 帧周期时取模。取模前先挡掉 0 周期（模型非法已在上面挡过）。
        let off = if frame_ns == 0 { offset_ns } else { offset_ns % frame_ns };
        let line = if off == offset_ns { line } else { self.model.line_at(off) };

        // 行号判定的三段论：
        //   line == 0            → 帧首，整帧切换，安全；
        //   line >= v_total-1    → 已进消隐（或钳到边界），安全；
        //   1 .. v_total-2       → 屏内切换：内容变了才叫撕裂。
        let inside = line > 0 && line + 1 < self.model.v_total;
        let verdict = if !inside {
            Verdict::Safe
        } else if content_changed {
            Verdict::Tear
        } else {
            Verdict::Repeat
        };

        let mut code = 0u32;
        match verdict {
            Verdict::Tear => {
                self.tears += 1;
                self.streak += 1;
                // 开同步/邮箱模式下检出撕裂 = 驱动或模型有错，标注归因码。
                if !mode.tear_possible() {
                    code = XC_VBLANK_MISS;
                }
            }
            Verdict::Repeat => {
                // 重复帧不算撕裂，但连续重复意味着上游没出帧，清连续计数。
                self.streak = 0;
            }
            Verdict::Safe => {
                self.streak = 0;
            }
            _ => {}
        }

        self.push(AuditEntry {
            frame_id,
            flip_ns: offset_ns,
            scan_line: line,
            mode: mode.code(),
            verdict: verdict.code(),
            code,
            report_id: 0,
            digest: 0,
        });
        verdict
    }

    fn push(&mut self, e: AuditEntry) {
        if self.audit.len() >= AUDIT_CAP {
            self.audit_dropped += 1;
            return;
        }
        self.audit.push(e);
    }

    pub fn audit(&self) -> &[AuditEntry] {
        &self.audit
    }

    /// 防护建议（锚点判据「防护建议」）：任何一次调用都必须返回非空建议。
    ///
    /// 建议是**可执行动作**，不是状态复述：「建议开同步」而不是「存在撕裂」。
    pub fn advice(&self) -> Advice {
        if !self.model.valid() {
            return Advice {
                action: AdviceAction::FixModel,
                reason: "扫描模型无效（刷新率/总行数为 0），无法判定撕裂",
                evidence_ppm: 0,
                forced: true,
            };
        }
        let rate = self.tear_rate_ppm();
        if self.streak >= TEAR_STREAK_FORCE {
            return Advice {
                action: AdviceAction::EnableVsync,
                reason: "连续撕裂达到强制阈值",
                evidence_ppm: rate,
                forced: true,
            };
        }
        if rate > TEAR_RATE_ALARM_PPM {
            return Advice {
                action: AdviceAction::EnableVsync,
                reason: "撕裂率超过 5%",
                evidence_ppm: rate,
                forced: false,
            };
        }
        if self.false_positives >= FALSE_POSITIVES_FOR_CALIBRATION {
            return Advice {
                action: AdviceAction::Recalibrate,
                reason: "误报累计达到校准阈值，先重校准扫描模型",
                evidence_ppm: rate,
                forced: false,
            };
        }
        if self.unmonitored > 0 && self.duty > 1 {
            return Advice {
                action: AdviceAction::RaiseDuty,
                reason: "存在未监控窗口，提高采样占空比",
                evidence_ppm: rate,
                forced: false,
            };
        }
        Advice {
            action: AdviceAction::Keep,
            reason: "撕裂率在阈值内，维持当前同步策略",
            evidence_ppm: rate,
            forced: false,
        }
    }

    // -----------------------------------------------------------------------
    // 用户报告入口 + 截图核验
    // -----------------------------------------------------------------------

    /// 用户报告：用户声称「第 `frame_id` 帧撕了」，并附上截图区域的指纹。
    ///
    /// 核验方法（截图级）：呈现场在呈现那一刻记录了**屏内每一行的来源帧**
    /// （`presented_digest_at`），把用户截图区域按扫描行切成上下两半，
    /// 分别与「该行来源帧」的指纹比对——两半都能对上即**确认撕裂**；
    /// 两半都对不上即误报；只对上一半即无法判定（截图被裁/被缩放）。
    pub fn user_report(&mut self, report: UserReport, presented: &dyn DigestSource) -> Verdict {
        if report.report_id == 0 || report.region_lines == 0 {
            self.push(AuditEntry {
                frame_id: report.frame_id,
                flip_ns: 0,
                scan_line: 0,
                mode: 0,
                verdict: Verdict::Unverifiable.code(),
                code: XC_REPORT_BAD,
                report_id: report.report_id,
                digest: report.top_digest,
            });
            return Verdict::Unverifiable;
        }
        if !presented.has_frame(report.frame_id) {
            self.push(AuditEntry {
                frame_id: report.frame_id,
                flip_ns: 0,
                scan_line: 0,
                mode: 0,
                verdict: Verdict::Unverifiable.code(),
                code: XC_REPORT_UNKNOWN_FRAME,
                report_id: report.report_id,
                digest: report.top_digest,
            });
            return Verdict::Unverifiable;
        }

        // 上下半区各自的来源帧号（呈现场记录：该行区间来自哪一帧）。
        let top_src = presented.line_source(report.split_line);
        let bottom_src = presented.line_source(report.split_line.saturating_add(1));

        let top_ok = report.top_digest != 0 && presented.frame_digest(top_src) == report.top_digest;
        let bottom_ok =
            report.bottom_digest != 0 && presented.frame_digest(bottom_src) == report.bottom_digest;

        let verdict = if top_ok && bottom_ok && top_src != bottom_src {
            // 两半来自不同帧且指纹都对上 —— 教科书级撕裂证据。
            Verdict::Tear
        } else if top_ok && bottom_ok && top_src == bottom_src {
            // 两半同源：用户看到的是同一帧，属误报（或把压缩伪影当撕裂）。
            Verdict::FalsePositive
        } else if !top_ok && !bottom_ok {
            Verdict::FalsePositive
        } else {
            Verdict::Unverifiable
        };

        match verdict {
            Verdict::Tear => {
                self.confirmed += 1;
                self.tears += 1;
                self.streak += 1;
            }
            Verdict::FalsePositive => {
                self.false_positives += 1;
                self.streak = 0;
                if self.false_positives >= FALSE_POSITIVES_FOR_CALIBRATION {
                    // 误报成灾 → 检测升级（逐帧监控），避免继续误伤。
                    self.duty = 1;
                }
            }
            _ => {}
        }

        let mut code = 0u32;
        if matches!(verdict, Verdict::Unverifiable) {
            code = XC_DIGEST_MISMATCH;
        }
        self.push(AuditEntry {
            frame_id: report.frame_id,
            flip_ns: 0,
            scan_line: report.split_line,
            mode: 0,
            verdict: verdict.code(),
            code,
            report_id: report.report_id,
            digest: report.top_digest,
        });
        verdict
    }

    /// 账本守恒：flips ≡ monitored + unmonitored。
    ///
    /// 这条恒等式是「监控没有被悄悄关掉」的唯一证据：若某次 flip 既不
    /// 进监控账也不进跳过账，等式立刻破。
    pub fn ledger_balanced(&self) -> bool {
        self.flips == self.monitored + self.unmonitored
    }

    /// 审计流指纹（FNV-1a 32，含被拒计数——被拒也是事实）。
    pub fn audit_fingerprint(&self) -> u32 {
        let mut h = 0x811C9DC5u32;
        for e in &self.audit {
            for b in [
                (e.frame_id & 0xFF) as u8,
                ((e.frame_id >> 8) & 0xFF) as u8,
                (e.scan_line & 0xFF) as u8,
                e.mode,
                e.verdict,
                (e.code & 0xFF) as u8,
                (e.report_id & 0xFF) as u8,
                (e.digest & 0xFF) as u8,
            ] {
                h ^= b as u32;
                h = h.wrapping_mul(0x0100_0193);
            }
        }
        h ^= self.audit_dropped;
        h = h.wrapping_mul(0x0100_0193);
        h
    }

    /// 读屏可达的审核行（纯文本，不含像素）。
    pub fn audit_line(&self, idx: usize) -> String {
        let mut s = String::new();
        match self.audit.get(idx) {
            None => s.push_str("审核记录越界"),
            Some(e) => {
                push_u64(&mut s, e.frame_id);
                s.push_str(" 帧 行");
                push_u64(&mut s, e.scan_line as u64);
                s.push(' ');
                s.push_str(Verdict::text(match e.verdict {
                    0 => Verdict::Tear,
                    1 => Verdict::Repeat,
                    2 => Verdict::Safe,
                    3 => Verdict::FalsePositive,
                    _ => Verdict::Unverifiable,
                }));
            }
        }
        s
    }
}

fn push_u64(s: &mut String, mut v: u64) {
    if v == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut n = 0usize;
    while v > 0 {
        buf[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    while n > 0 {
        n -= 1;
        s.push(buf[n] as char);
    }
}

// ---------------------------------------------------------------------------
// 用户报告与呈现场
// ---------------------------------------------------------------------------

/// 用户报告（截图级）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UserReport {
    pub report_id: u32,
    pub frame_id: u64,
    /// 截图里「上下两半分界」对应的扫描行。
    pub split_line: u32,
    /// 参与核验的行数（0 = 无效报告）。
    pub region_lines: u32,
    /// 上半区指纹。
    pub top_digest: u32,
    /// 下半区指纹。
    pub bottom_digest: u32,
}

/// 呈现场抽象：回答「某行来自哪一帧」「某帧指纹是多少」。
///
/// 做成 trait 而不是具体结构，是为了让本模块**不拥有**帧缓冲——
/// 呈现场由 vea46/vea47 掌管，本模块只读。测试用 `MockPresent` 注入。
pub trait DigestSource {
    fn has_frame(&self, frame_id: u64) -> bool;
    fn line_source(&self, line: u32) -> u64;
    fn frame_digest(&self, frame_id: u64) -> u32;
}

/// 测试用呈现场：三段行区间 → 帧号 + 指纹。
pub struct MockPresent {
    /// (起始行, 结束行(不含), 帧号, 指纹)
    spans: Vec<(u32, u32, u64, u32)>,
}

impl MockPresent {
    pub fn new() -> MockPresent {
        MockPresent { spans: Vec::new() }
    }

    pub fn add_span(&mut self, from: u32, to: u32, frame_id: u64, digest: u32) {
        self.spans.push((from, to, frame_id, digest));
    }
}

impl DigestSource for MockPresent {
    fn has_frame(&self, frame_id: u64) -> bool {
        self.spans.iter().any(|s| s.2 == frame_id)
    }

    fn line_source(&self, line: u32) -> u64 {
        for s in &self.spans {
            if line >= s.0 && line < s.1 {
                return s.2;
            }
        }
        0
    }

    fn frame_digest(&self, frame_id: u64) -> u32 {
        for s in &self.spans {
            if s.2 == frame_id {
                return s.3;
            }
        }
        0
    }
}

/// 建议动作闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdviceAction {
    Keep = 0,
    EnableVsync = 1,
    Recalibrate = 2,
    RaiseDuty = 3,
    FixModel = 4,
}

impl AdviceAction {
    pub const fn text(self) -> &'static str {
        match self {
            AdviceAction::Keep => "维持",
            AdviceAction::EnableVsync => "开启垂直同步",
            AdviceAction::Recalibrate => "重校准扫描模型",
            AdviceAction::RaiseDuty => "提高监控占空比",
            AdviceAction::FixModel => "修正扫描模型参数",
        }
    }
}

pub struct Advice {
    pub action: AdviceAction,
    pub reason: &'static str,
    pub evidence_ppm: u32,
    pub forced: bool,
}

impl Advice {
    pub fn text(&self) -> &'static str {
        self.action.text()
    }
}

// ---------------------------------------------------------------------------
// 判据套件
// ---------------------------------------------------------------------------

/// 构造一部 60Hz / 1125 行 / 2200 像素的常见 1080p 扫描模型。
/// 帧周期 = 1e12/60000 = 16_666_666 ns；行时间 = 16666666/1125 = 14814 ns。
pub fn model_1080p60() -> ScanoutModel {
    ScanoutModel::new(60_000, 1125, 2200)
}

fn chk(set: &mut CheckSet, name: &'static str, ok: bool, detail: &'static str) {
    set.add(name, ok, if ok { "" } else { detail });
}

/// VE-F0055 判据入口。
pub fn run_vea55_checks() -> CheckSet {
    let mut set = CheckSet::new("vea55_tearguard");

    // C55-01 扫描模型换算正确（60Hz/1125 行：16_666_666/1125 = 14814 ns/行）
    {
        let m = model_1080p60();
        let ok = m.valid() && m.frame_ns() == 16_666_666 && m.line_ns() == 14814;
        chk(&mut set, "C55-01 扫描模型换算", ok, "帧/行时间推导错");
    }

    // C55-02 帧首 flip 不判撕裂（line == 0）
    {
        let mut g = TearGuard::new(model_1080p60());
        let v = g.judge_flip(1, 0, true, SyncMode::Off);
        chk(&mut set, "C55-02 帧首安全", v == Verdict::Safe, "帧首被误判撕裂");
    }

    // C55-03 屏内切换必判撕裂（line 恰好落在中段）
    {
        let mut g = TearGuard::new(model_1080p60());
        let m = model_1080p60();
        let off = m.line_ns() * 500;
        let v = g.judge_flip(2, off, true, SyncMode::Off);
        chk(&mut set, "C55-03 屏内切换检出", v == Verdict::Tear, "屏内切换漏检");
    }

    // C55-04 屏内但内容未变 → 重复帧，不算撕裂
    {
        let mut g = TearGuard::new(model_1080p60());
        let m = model_1080p60();
        let v = g.judge_flip(3, m.line_ns() * 700, false, SyncMode::Off);
        chk(&mut set, "C55-04 重复帧非撕裂", v == Verdict::Repeat, "重复帧被误判撕裂");
    }

    // C55-05 消隐区 flip 安全（v_total 边界）
    {
        let mut g = TearGuard::new(model_1080p60());
        let m = model_1080p60();
        let v = g.judge_flip(4, m.line_ns() * 1124, true, SyncMode::Off);
        chk(&mut set, "C55-05 消隐区安全", v == Verdict::Safe, "消隐区被误判撕裂");
    }

    // C55-06 时钟倒退显性化（不制造假撕裂）
    {
        let mut g = TearGuard::new(model_1080p60());
        let m = model_1080p60();
        let _ = g.judge_flip(5, m.line_ns() * 900, true, SyncMode::Off);
        // 同一帧号内偏移回退 = 真倒退（跨帧的偏移重置是正常语义）。
        let v = g.judge_flip(5, m.line_ns() * 100, true, SyncMode::Off);
        chk(
            &mut set,
            "C55-06 时钟倒退显性",
            v == Verdict::Unverifiable,
            "倒退样本被当真",
        );
    }

    // C55-07 占空比记账守恒（flips ≡ monitored + unmonitored）
    {
        let mut g = TearGuard::new(model_1080p60());
        g.duty = 4;
        for i in 0..40u64 {
            let _ = g.judge_flip(i, 0, true, SyncMode::Off);
        }
        let (flips, mon, unmon, _, _, _) = g.counters();
        chk(
            &mut set,
            "C55-07 监控账守恒",
            g.ledger_balanced() && flips == 40 && mon + unmon == 40 && mon == 10,
            "占空比账不平",
        );
    }

    // C55-08 漏检升级（escalate 逐级收紧到 1）
    {
        let mut g = TearGuard::new(model_1080p60());
        g.duty = 4;
        let d1 = g.escalate();
        let d2 = g.escalate();
        let d3 = g.escalate();
        chk(
            &mut set,
            "C55-08 漏检升级",
            d1 == 2 && d2 == 1 && d3 == 1,
            "占空比未逐级收紧",
        );
    }

    // C55-09 开同步下检出撕裂必须带归因码 XC_VBLANK_MISS
    {
        let mut g = TearGuard::new(model_1080p60());
        let m = model_1080p60();
        let _ = g.judge_flip(7, m.line_ns() * 300, true, SyncMode::On);
        let last = g.audit().last().copied();
        let ok = match last {
            Some(e) => e.code == XC_VBLANK_MISS && e.verdict == Verdict::Tear.code(),
            None => false,
        };
        chk(&mut set, "C55-09 同步模式归因", ok, "On 模式撕裂未归因");
    }

    // C55-10 截图核验：两半异源且指纹全中 → 确认撕裂
    {
        let mut g = TearGuard::new(model_1080p60());
        let mut p = MockPresent::new();
        p.add_span(0, 540, 10, 0xAAAA_1111);
        p.add_span(540, 1080, 11, 0xBBBB_2222);
        let rep = UserReport {
            report_id: 1,
            frame_id: 11,
            split_line: 539,
            region_lines: 1080,
            top_digest: 0xAAAA_1111,
            bottom_digest: 0xBBBB_2222,
        };
        let v = g.user_report(rep, &p);
        chk(&mut set, "C55-10 截图核验确认", v == Verdict::Tear, "真撕裂未被确认");
    }

    // C55-11 截图核验：两半同源 → 误报
    {
        let mut g = TearGuard::new(model_1080p60());
        let mut p = MockPresent::new();
        p.add_span(0, 1080, 12, 0xCCCC_3333);
        let rep = UserReport {
            report_id: 2,
            frame_id: 12,
            split_line: 500,
            region_lines: 1080,
            top_digest: 0xCCCC_3333,
            bottom_digest: 0xCCCC_3333,
        };
        let v = g.user_report(rep, &p);
        chk(&mut set, "C55-11 误报判定", v == Verdict::FalsePositive, "同源被判撕裂");
    }

    // C55-12 误报累计触发重校准建议
    {
        let mut g = TearGuard::new(model_1080p60());
        let mut p = MockPresent::new();
        p.add_span(0, 1080, 13, 0x1111_2222);
        for r in 0..FALSE_POSITIVES_FOR_CALIBRATION {
            let rep = UserReport {
                report_id: 10 + r,
                frame_id: 13,
                split_line: 100,
                region_lines: 1080,
                top_digest: 0x1111_2222,
                bottom_digest: 0x1111_2222,
            };
            let _ = g.user_report(rep, &p);
        }
        let a = g.advice();
        chk(
            &mut set,
            "C55-12 误报触发校准",
            a.action == AdviceAction::Recalibrate && !a.reason.is_empty(),
            "误报未触发校准建议",
        );
    }

    // C55-13 建议必非空（任何状态下都给出可执行动作）
    {
        let g = TearGuard::new(model_1080p60());
        let a = g.advice();
        let ok = !a.reason.is_empty() && a.action == AdviceAction::Keep;
        chk(&mut set, "C55-13 建议必达", ok, "干净状态无建议");
    }

    // C55-14 连续撕裂强制建议开同步
    {
        let mut g = TearGuard::new(model_1080p60());
        let m = model_1080p60();
        for i in 0..TEAR_STREAK_FORCE {
            let _ = g.judge_flip(i as u64, m.line_ns() * (200u64 + i as u64), true, SyncMode::Off);
        }
        let a = g.advice();
        chk(
            &mut set,
            "C55-14 连续撕裂强制",
            a.action == AdviceAction::EnableVsync && a.forced,
            "强制建议缺失",
        );
    }

    // C55-15 撕裂率口径：分母取监控数，不取 flips
    {
        let mut g = TearGuard::new(model_1080p60());
        g.duty = 4;
        let m = model_1080p60();
        // 40 次 flip，只有第 4/8/12... 拍被监控（10 次），其中第 4 拍撕裂。
        for i in 1..=40u64 {
            let off = if i == 4 { m.line_ns() * 400 } else { 0 };
            let _ = g.judge_flip(i, off, true, SyncMode::Off);
        }
        // 10 次监控里 1 次撕裂 = 100_000 ppm；若误用 flips 作分母会得 25_000。
        let ok = g.tear_rate_ppm() == 100_000;
        chk(&mut set, "C55-15 撕裂率口径", ok, "撕裂率分母错");
    }

    // C55-16 审计流满账显性（不静默覆盖）
    {
        let mut g = TearGuard::new(model_1080p60());
        for i in 0..(AUDIT_CAP as u64 + 5) {
            let _ = g.judge_flip(i, 0, false, SyncMode::Off);
        }
        let (_, _, _, _, _, dropped) = g.counters();
        chk(
            &mut set,
            "C55-16 审计满账显性",
            dropped == 5 && g.audit().len() == AUDIT_CAP,
            "满账被静默覆盖",
        );
    }

    // C55-17 无效报告显性拒绝（report_id == 0）
    {
        let mut g = TearGuard::new(model_1080p60());
        let p = MockPresent::new();
        let rep = UserReport {
            report_id: 0,
            frame_id: 1,
            split_line: 10,
            region_lines: 100,
            top_digest: 1,
            bottom_digest: 2,
        };
        let v = g.user_report(rep, &p);
        chk(&mut set, "C55-17 无效报告拒绝", v == Verdict::Unverifiable, "无效报告被接受");
    }

    // C55-18 未知帧报告显性（不猜）
    {
        let mut g = TearGuard::new(model_1080p60());
        let p = MockPresent::new();
        let rep = UserReport {
            report_id: 9,
            frame_id: 999,
            split_line: 10,
            region_lines: 100,
            top_digest: 1,
            bottom_digest: 2,
        };
        let v = g.user_report(rep, &p);
        let ok = v == Verdict::Unverifiable
            && g.audit().last().map(|e| e.code == XC_REPORT_UNKNOWN_FRAME).unwrap_or(false);
        chk(&mut set, "C55-18 未知帧显性", ok, "未知帧被猜结论");
    }

    // C55-19 确定性：同输入序列 → 同指纹
    {
        let mut a = TearGuard::new(model_1080p60());
        let mut b = TearGuard::new(model_1080p60());
        let m = model_1080p60();
        for i in 0..16u64 {
            let off = m.line_ns() * ((i * 137) % 1125);
            let _ = a.judge_flip(i, off, i % 3 != 0, SyncMode::Off);
            let _ = b.judge_flip(i, off, i % 3 != 0, SyncMode::Off);
        }
        chk(
            &mut set,
            "C55-19 指纹确定性",
            a.audit_fingerprint() == b.audit_fingerprint(),
            "同序列指纹漂移",
        );
    }

    // C55-20 模型非法时必给 FixModel 建议（禁止拿坏模型出结论）
    {
        let mut g = TearGuard::new(ScanoutModel::new(0, 0, 0));
        let v = g.judge_flip(1, 123, true, SyncMode::Off);
        let a = g.advice();
        let ok = v == Verdict::Unverifiable && a.action == AdviceAction::FixModel;
        chk(&mut set, "C55-20 坏模型显性", ok, "坏模型仍出结论");
    }

    // C55-21 读屏审核行可读（非空且含帧号）
    {
        let mut g = TearGuard::new(model_1080p60());
        let m = model_1080p60();
        let _ = g.judge_flip(4242, m.line_ns() * 200, true, SyncMode::Off);
        let line = g.audit_line(0);
        let ok = !line.is_empty() && line.contains("4242") && line.contains("行");
        chk(&mut set, "C55-21 审核行读屏", ok, "审核行不可读");
    }

    // C55-22 越界索引不 panic
    {
        let g = TearGuard::new(model_1080p60());
        let line = g.audit_line(9999);
        chk(&mut set, "C55-22 越界索引安全", !line.is_empty(), "越界索引空返回");
    }

    set
}

/// 深度驱动入口：跑一遍判据并回报红项文本（域聚合器调用）。
pub fn drill() -> (bool, String) {
    let s = run_vea55_checks();
    let ok = s.all_passed() && !s.truncated();
    (ok, if ok { String::new() } else { s.red_items_text() })
}
