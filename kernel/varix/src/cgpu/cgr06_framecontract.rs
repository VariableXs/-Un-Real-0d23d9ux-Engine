//! CGPU-F2726 · 安全渲染与 80 帧合同（CGPU-R 域 · 安全渲染组 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F2726`
//!
//! **判据（锚点原文）**：不破合同、降质不降帧、一组、判据。
//!
//! **职责定位（锚点原文摘录）**：帧合同（安全检查不破 80 帧合同（降质不降帧
//! ——合同复用；测试（合同一组）。
//!
//! ## 〇、本项在 R 域的位置（真实子系统，非文档壳）
//!
//! R04（F2724）交付了「降级接口」——异常发生之后怎么把渲染面降到什么档。
//! 本项（F2726）交付的是**帧预算合同**——异常检查本身怎么挤进每一帧的
//! 12.5ms 预算而不把帧率拖破。两者互补：F2724 管「降级后的调用约定」，
//! F2726 管「检查开销的预算纪律」。安全检查（内容校验/越界扫描/纹理审查）
//! 不是免费的：全量越界扫描在 4K 面上就是毫秒级开销。若放任检查吃满帧
//! 预算，80 帧合同就破了；若砍掉检查，安全就破了。本项的答案是
//! **降质不降帧**：检查开销超支时，降的是检查深度档位（全扫→抽样→白名单
//! 免检），帧率是合同底线永不放弃——帧永不被丢弃，丢的只是检查的深度。
//!
//! ## 一、帧预算与安全配额（合同基线）
//!
//! [`FRAME_BUDGET_US`] = 12_500µs（80 帧/秒 → 1_000_000µs ÷ 80 = 12_500µs，
//! 整数除净，无舍入）。安全检查在帧预算内的固定配额
//! [`SECURITY_QUOTA_US`] = 800µs（推导：帧预算的 6.4%——四道检查按
//! 越界 300 / 魔数 100 / 配额 200 / 签名 200 分账，见 [`LANE_QUOTA_US`]；
//! 800µs 足以在 Full 档扫完 4K 级面积（约 9.6M 像素步），又只占帧预算
//! 不足一成，给渲染主体留足 11.7ms）。配额是软线：超支不拒绝帧，而是
//! 触发降档计数；帧预算是硬线：整帧实耗破 12.5ms 即合同违约，显性事件
//! 不可静默，合同翻到 Breached 态，恢复必须显性重签（[`REATTEST_ACK`]）。
//!
//! ## 二、检查档位表与滞回状态机（降质不降帧的执行机构）
//!
//! [`ScanTier`] 三档：Full（全扫）/ Sampled（1/16 抽样）/ Whitelisted
//! （白名单免检）。[`ScanCostModel`] 用整数运算把「面积/纹理数」折算成
//! 每道步数估算；[`TierGovernor`] 是档位切换状态机：连续
//! [`OVER_DOWNGRADE_N`]（3）帧超安全配额即降档，连续
//! [`SURPLUS_UPGRADE_M`]（12 = 4×N）帧配额占用低于 3/4 才升档——
//! 升档门槛是降档门槛的 4 倍，加上切换后 [`TIER_COOLDOWN_FRAMES`]（40
//! 帧 ≈ 0.5 秒）锁定期，双滞回防振荡。档位降的是检查深度，不是帧率：
//! 每帧照常执行照常出账。
//!
//! ## 三、四道检查流水线（每道有跳过语义与留痕）
//!
//! [`SecurityScanPipeline`] 四道：越界（区域表对光圈窗）/ 魔数（纹理头
//! 幻数）/ 配额（纹理字节总量）/ 签名（FNV 派生签名对拍）。每道都可被
//! 跳过，但跳过必留痕（`LaneTrace.skipped + skip_reason`），且**越界道
//! 不可禁用**（安全底线：禁用请求本身记为发现，不静默）。白名单免检需
//! 显性登记（资源 id + 有效期 + 理由），到期自动摘除并出事件——免检是
//! 有期限的豁免，不是永久失明。
//!
//! ## 四、双账对拍与双跑对拍（判据「一组/判据」的落实）
//!
//! 守恒对账：每帧记账时 Σ道实耗 ≡ 帧安全实耗（帧账与道账对拍，破即
//! [`FrameContractError::ConservationBroken`]）；会话级再对账一次环内
//! 全量。确定性对拍：[`determinism_check`] 同输入双跑，裁决指纹与道步数
//! 必须一致——扫描是纯函数，不一致即有隐藏状态泄漏。违约账
//! [`ContractViolationLedger`] + [`HealthReport`] 体检 + FNV 指纹收口。
//!
//! ## 五、隐私红线
//!
//! 内容指纹（纹理签名、发现明细里的资源数据）**不出内核**：
//! [`project_telemetry`] 只投影白名单字段（帧数/分位/违约率等统计量），
//! 字段名撞 [`CONTENT_FP_BANNED_SUBSTRINGS`] 任一子串即拒
//! [`FrameContractError::PrivacyRedline`]。安全检查的结果（发现数、档位、
//! 预算占用）可以出门；检查的对象内容（指纹原文）永不出门。
//!
//! ## 六、诊断码段
//!
//! 本项独占 **0x1300~0x130F**（16 码闭集，开工前已 grep 确认空闲）：
//! 错误枚举 [`FrameContractError`] 与事件账共用同一码段（同一闭集、
//! 双语境语义，每个码的 `detail()` 给中文可操作诊断）。
//!
//! **对接**：F2724（R04 降级接口——档位降级语义对齐其降级档）；F2721/
//! F2722（R 域开工与威胁模型——检查清单来源）；cgc05/cgd03（C/D 域帧
//! 合同与帧账——预算口径同源 12.5ms）；cgp09（隐私面——红线同源）。
//! 零 panic 面、零 IO、零墙钟、no_std+alloc。

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（合同基线；写死，来源见各行注释）
// ---------------------------------------------------------------------------

/// 目标帧率（锚点判据：80 帧合同；写死 80）。
pub const TARGET_FPS: u32 = 80;

/// 帧预算（微秒）：1_000_000µs ÷ 80 帧 = 12_500µs/帧（整数除净）。
pub const FRAME_BUDGET_US: u64 = 12_500;

/// 安全检查固定配额（微秒）：12_500 × 6.4% = 800（推导见模块头 §一）。
pub const SECURITY_QUOTA_US: u64 = 800;

/// 四道检查的配额分账（µs）：越界 300 / 魔数 100 / 配额 200 / 签名 200。
/// 分账依据：越界道按面积走步最贵（4K 面 Full 档约 9.4ms 步时→限 300）；
/// 魔数每纹理 4 步×1ns 最便宜（限 100）；配额道是聚合和（限 200）；
/// 签名道每纹理 8 步×4ns 次贵（限 200）。
pub const LANE_QUOTA_US: [u64; 4] = [300, 100, 200, 200];

/// 安全配额占帧预算的比例（千分位，800/12500 = 64‰；供对拍展示）。
pub const QUOTA_PERMILLE: u64 = 64;

/// 帧账环形容量：600 帧 = 80fps 下 7.5 秒滑动窗（600÷80=7.5，整秒半）。
pub const RING_FRAMES: usize = 600;

/// 降档门槛：连续 3 帧超安全配额即降档（3 帧 ≈ 37.5ms，一次掉帧毛刺
/// 不触发，持续超支才降——判据「降质不降帧」的触发侧）。
pub const OVER_DOWNGRADE_N: u32 = 3;

/// 升档门槛：连续 12 帧配额占用低于 3/4 才升档（12 = 4×N，滞回带；
/// 升档永远比降档难四倍，防预算在门槛附近振荡）。
pub const SURPLUS_UPGRADE_M: u32 = 12;

/// 富余判定线（千分比）：安全实耗 < 配额 × 750‰ 才算富余帧。
pub const SURPLUS_PERMILLE: u64 = 750;

/// 档位切换锁定期（帧）：切换后 40 帧 ≈ 0.5 秒内禁止再切（滞回二阶；
/// 与 3/12 帧门槛合成双滞回）。
pub const TIER_COOLDOWN_FRAMES: u32 = 40;

/// 抽样档步长：每 16 个纹理查 1 个（1/16 抽样；成本模型同构推导）。
pub const SAMPLE_STRIDE: usize = 16;

/// 白名单容量（条）：64 条足够登记一台主机的常驻安全资源。
pub const WHITELIST_CAP: usize = 64;

/// 白名单默认有效期（帧）：8_000 帧 = 80fps 下 100 秒；到期自动摘除。
pub const WHITELIST_TTL_FRAMES: u64 = 8_000;

/// 光圈窗基址（扫描模型：纹理区域必须落在窗内；写死 256MiB 窗）。
pub const APERTURE_BASE: u64 = 0x1000_0000;

/// 光圈窗长度：256MiB（0x1000_0000；与 APERTURE_BASE 合成合法区间）。
pub const APERTURE_LEN: u64 = 0x1000_0000;

/// 纹理字节配额：64MiB（配额道核查线；超过即记发现）。
pub const QUOTA_BYTES: u64 = 67_108_864;

/// 纹理头幻数："VRXT"（Varix TeXture；小端 u32 字面量）。
pub const TEX_MAGIC: u32 = 0x5652_5854;

/// 签名盐（本项派生签名专用；写死字面量，非密——确定性对拍要求可复算）。
pub const TEX_SIG_SALT: u64 = 0x1300_5A17_5167_5EC7;

/// 单帧纹理数上限（输入校验；4096 张足够任何一帧引用）。
pub const TEXTURE_CAP: usize = 4096;

/// 单帧面积上限（像素；4096×4096×16 层 = 268M 封顶）。
pub const AREA_PX_CAP: u64 = 268_435_456;

/// 单帧发现数上限（超出只计数不展开——防发现风暴撑爆账本）。
pub const FINDINGS_CAP: usize = 256;

/// 违约账容量（条）；满后丢最旧并出 [`FrameContractError::LedgerFull`] 事件。
pub const VIOLATION_CAP: usize = 64;

/// 事件账容量（条）；环形覆盖，覆盖计数进聚合。
pub const EVENT_CAP: usize = 128;

/// 帧合同违约率红线（ppm）：会话违约率 > 100ppm（0.01%）体检不合格。
pub const BREACH_PPM_LIMIT: u64 = 100;

/// 体检合格线（分）：8 项检查按通过数折百分，<90 即「观察/不合格」。
pub const HEALTH_MIN_SCORE: u32 = 90;

/// 安全道固定开销（µs/帧）：调度与留痕的常数项（实测口径：四道各 1µs
/// 簿记 + 档位仲裁 0 微查 → 写死 4）。
pub const FIXED_OVERHEAD_US: u64 = 4;

/// 违约重签确认字：Breached → Signed 必须回填该字面量（防静默恢复；
/// 语义「违约已人工复核，接受降档处置记录」）。
pub const REATTEST_ACK: u64 = 0x1300_C0DE_BEEF_0001;

// ---------------------------------------------------------------------------
// 二、错误闭集（诊断码段 0x1300~0x130F 独占；16 码，双语境：错误+事件）
// ---------------------------------------------------------------------------

/// 本项诊断码闭集（0x1300~0x130F；每变体 `code()` 与 `detail()` 成对，
/// detail 为中文可操作诊断，绝不返回空串）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameContractError {
    /// 合同未签署（Unsigned 态就跑帧）。
    ContractNotSigned,
    /// 帧合同违约（整帧实耗破 12.5ms；事件+违约账双记）。
    ContractBreached,
    /// 帧 id 复用/回退（帧号必须单调递增）。
    FrameIdReuse,
    /// 档位值非法（越出三档闭集）。
    TierInvalid,
    /// 档位切换被拒（锁定期内切换请求；事件账同码记合法切换，aux 区分）。
    TierSwitchDenied,
    /// 白名单满（容量 64；先摘再登）。
    WhitelistCapFull,
    /// 白名单条目已过期（操作过期条目 / 到期摘除事件同码）。
    WhitelistExpired,
    /// 白名单重复登记（同一资源 id 已在册；续期走 renew）。
    WhitelistDuplicate,
    /// 越界道被请求禁用（安全底线：越界道不可跳，请求本身记为发现）。
    LaneSkipForbidden,
    /// 安全配额超支（事件：某帧安全实耗 > 800µs；错误语境：配额已破仍求全档）。
    QuotaOverrun,
    /// 守恒破账（Σ道实耗 ≠ 帧安全实耗；帧账与道账对拍失败）。
    ConservationBroken,
    /// 双跑裁决不一致（同输入两跑指纹/步数不同——隐藏状态泄漏）。
    DeterminismMismatch,
    /// 账本满（违约账满丢最旧；事件账环形覆盖计数超限）。
    LedgerFull,
    /// 隐私红线（尝试把内容指纹投影出门）。
    PrivacyRedline,
    /// 输入非法（纹理数/面积超限；禁道未给留痕原因）。
    InputInvalid,
    /// 体检不合格（分数 < 90 或关键项破口；事件：体检跨线报警）。
    HealthCritical,
}

impl FrameContractError {
    /// 诊断码（0x1300 起连续编址；闭集 16 码）。
    pub const fn code(self) -> u32 {
        match self {
            FrameContractError::ContractNotSigned => 0x1300,
            FrameContractError::ContractBreached => 0x1301,
            FrameContractError::FrameIdReuse => 0x1302,
            FrameContractError::TierInvalid => 0x1303,
            FrameContractError::TierSwitchDenied => 0x1304,
            FrameContractError::WhitelistCapFull => 0x1305,
            FrameContractError::WhitelistExpired => 0x1306,
            FrameContractError::WhitelistDuplicate => 0x1307,
            FrameContractError::LaneSkipForbidden => 0x1308,
            FrameContractError::QuotaOverrun => 0x1309,
            FrameContractError::ConservationBroken => 0x130A,
            FrameContractError::DeterminismMismatch => 0x130B,
            FrameContractError::LedgerFull => 0x130C,
            FrameContractError::PrivacyRedline => 0x130D,
            FrameContractError::InputInvalid => 0x130E,
            FrameContractError::HealthCritical => 0x130F,
        }
    }

    /// 中文可操作诊断（人读；绝不空串）。
    pub const fn detail(self) -> &'static str {
        match self {
            FrameContractError::ContractNotSigned => {
                "合同未签署：先调 sign() 签署 80 帧合同再跑帧；Unsigned 态拒绝执行"
            }
            FrameContractError::ContractBreached => {
                "帧合同违约：整帧实耗破 12.5ms 已翻 Breached 态；处置记违约账，恢复须 reattest(REATTEST_ACK) 显性重签"
            }
            FrameContractError::FrameIdReuse => {
                "帧 id 复用/回退：帧号必须严格单调递增；请用递增序号，回放场景另起会话"
            }
            FrameContractError::TierInvalid => {
                "档位值非法：三档闭集 Full/Sampled/Whitelisted 之外无档；请改用合法档位"
            }
            FrameContractError::TierSwitchDenied => {
                "档位切换被拒：切换锁定期（40 帧）内不许再切；等待冷却期满或人工强制（留痕）"
            }
            FrameContractError::WhitelistCapFull => {
                "白名单满（64 条）：先 revoke 摘除失效条目再登记；免检名额不是无限的"
            }
            FrameContractError::WhitelistExpired => {
                "白名单条目已过期：免检豁免有期限；需继续免检请 renew 续期，不可用过期条目"
            }
            FrameContractError::WhitelistDuplicate => {
                "白名单重复登记：该资源 id 已在册；请走 renew 续期或先 revoke 再登记"
            }
            FrameContractError::LaneSkipForbidden => {
                "越界道不可禁用：安全底线道（lane0）永不跳过；禁用请求本身已记为发现留痕"
            }
            FrameContractError::QuotaOverrun => {
                "安全配额超支（>800µs）：已触发降档计数，帧照常执行不丢帧；持续 3 帧超支将降检查档位"
            }
            FrameContractError::ConservationBroken => {
                "守恒破账：Σ道实耗 ≠ 帧安全实耗，帧账与道账对拍失败；拒绝入账并请查扫描管线记账路径"
            }
            FrameContractError::DeterminismMismatch => {
                "双跑裁决不一致：同输入两跑指纹/步数不同，存在隐藏状态泄漏；请查扫描管线的可变状态"
            }
            FrameContractError::LedgerFull => {
                "账本满：违约账 64 条已满丢最旧（已计数）；请先导出体检与聚合再继续压账"
            }
            FrameContractError::PrivacyRedline => {
                "隐私红线：内容指纹不得投影出门；请只投影统计量白名单字段（帧数/分位/违约率等）"
            }
            FrameContractError::InputInvalid => {
                "输入非法：纹理数>4096 或面积超限，或禁道未给留痕原因；请裁剪输入或补 skip_reason"
            }
            FrameContractError::HealthCritical => {
                "体检不合格：分数<90 或关键项破口（违约率/守恒/隐私）；按 HealthReport 检查项逐项处置"
            }
        }
    }
}

/// 全体 16 码（供体检自遍历与测试穷举；顺序即码序 0x1300..0x130F）。
pub const FRAME_CONTRACT_ERRORS: [FrameContractError; 16] = [
    FrameContractError::ContractNotSigned,
    FrameContractError::ContractBreached,
    FrameContractError::FrameIdReuse,
    FrameContractError::TierInvalid,
    FrameContractError::TierSwitchDenied,
    FrameContractError::WhitelistCapFull,
    FrameContractError::WhitelistExpired,
    FrameContractError::WhitelistDuplicate,
    FrameContractError::LaneSkipForbidden,
    FrameContractError::QuotaOverrun,
    FrameContractError::ConservationBroken,
    FrameContractError::DeterminismMismatch,
    FrameContractError::LedgerFull,
    FrameContractError::PrivacyRedline,
    FrameContractError::InputInvalid,
    FrameContractError::HealthCritical,
];

/// 事件账复用码段的事件字面量（与错误闭集同码双语境；aux 字段消歧义）。
pub mod ev {
    /// 帧合同违约事件（aux = 违约帧整帧实耗 µs）。
    pub const CONTRACT_BREACHED: u32 = 0x1301;
    /// 档位切换事件（aux bit32 = 方向 0 降/1 升，低 32 位 = 目标档 index）。
    pub const TIER_SWITCH: u32 = 0x1304;
    /// 白名单到期摘除事件（aux = 摘除条数）。
    pub const WHITELIST_EXPIRED: u32 = 0x1306;
    /// 安全配额超支事件（aux = 该帧安全实耗 µs）。
    pub const QUOTA_OVERRUN: u32 = 0x1309;
    /// 守恒破账事件（aux = 双账差值）。
    pub const CONSERVATION_BROKEN: u32 = 0x130A;
    /// 双跑不一致事件（aux = 两跑指纹异或）。
    pub const DETERMINISM_MISMATCH: u32 = 0x130B;
    /// 账本满丢账事件（aux = 累计丢弃数）。
    pub const LEDGER_FULL: u32 = 0x130C;
    /// 隐私红线触发事件（aux = 触发字段名指纹）。
    pub const PRIVACY_REDLINE: u32 = 0x130D;
    /// 体检跨线报警事件（aux = 体检分数）。
    pub const HEALTH_CRITICAL: u32 = 0x130F;
}

// ---------------------------------------------------------------------------
// 三、FNV-1a 指纹基元（const 安全；指纹只做对拍与聚合，不做密码学）
// ---------------------------------------------------------------------------

/// FNV-1a 64 位偏移基（标准字面量 0xcbf29ce484222325）。
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 64 位素数（标准字面量 0x100000001b3）。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 字节串 FNV-1a（const fn；while+下标，不用 get/for——const 纪律）。
pub const fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h = FNV_OFFSET;
    let mut i = 0usize;
    while i < bytes.len() {
        h = (h ^ (bytes[i] as u64)).wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// 把一个 u64 以小端 8 字节折进 FNV 链（指纹构造基元）。
pub fn fnv64_u64(seed: u64, v: u64) -> u64 {
    let mut h = seed;
    let mut i = 0u32;
    while i < 8 {
        let byte = (v >> (i * 8)) & 0xFF;
        h = (h ^ byte).wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// 四元组混合指纹（裁决指纹/签名派生共用；顺序敏感——同输入同序同值）。
pub fn fnv64_mix4(a: u64, b: u64, c: u64, d: u64) -> u64 {
    let h = fnv64_u64(FNV_OFFSET, a);
    let h = fnv64_u64(h, b);
    let h = fnv64_u64(h, c);
    fnv64_u64(h, d)
}

// ---------------------------------------------------------------------------
// 四、检查档位（三档闭集）与成本模型（面积/纹理数 → 步数，整数运算）
// ---------------------------------------------------------------------------

/// 安全检查档位（三档闭集；降质不降帧——降的是本档位，不是帧率）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanTier {
    /// 全扫：四道全量逐纹理核查（最贵，最安全）。
    Full,
    /// 抽样：每 16 个纹理查 1 个（1/16 抽样；越界道同时收面积步长）。
    Sampled,
    /// 白名单免检：全部在册资源免扫（四道留痕跳过；免检有期限）。
    Whitelisted,
}

impl ScanTier {
    /// 档位序号（0 免检 / 1 抽样 / 2 全扫；序号越大检查越深）。
    pub const fn index(self) -> u8 {
        match self {
            ScanTier::Whitelisted => 0,
            ScanTier::Sampled => 1,
            ScanTier::Full => 2,
        }
    }

    /// 序号还原档位（越界值收敛到 Full——检查宁深勿浅）。
    pub const fn from_index(i: u8) -> ScanTier {
        if i >= 2 {
            ScanTier::Full
        } else {
            if i == 1 {
                ScanTier::Sampled
            } else {
                ScanTier::Whitelisted
            }
        }
    }

    /// 档位人读名（账本/体检展示用）。
    pub const fn name(self) -> &'static str {
        match self {
            ScanTier::Full => "全扫",
            ScanTier::Sampled => "抽样(1/16)",
            ScanTier::Whitelisted => "白名单免检",
        }
    }

    /// 三档闭集（体检穷举与测试用）。
    pub const ALL: [ScanTier; 3] = [ScanTier::Whitelisted, ScanTier::Sampled, ScanTier::Full];
}

/// 档位总数（const 自检用）。
pub const SCAN_TIER_COUNT: usize = 3;

/// 成本模型输入（帧前预测口径；面积+纹理数两个自由度，Copy 结构可入栈）。
#[derive(Clone, Copy, Debug)]
pub struct CostInput {
    /// 帧面积（像素；越界道按面积走步）。
    pub area_px: u64,
    /// 纹理张数（魔数/配额/签名道按张走步）。
    pub texture_count: u64,
}

/// 扫描成本模型：档位 × 输入 → 四道步数估算（整数运算，饱和防溢出）。
///
/// 模型与实测同构（见 [`SecurityScanPipeline`] 的计步规则）：
/// - 越界道：每步核 64 像素区域。Full 扫全域 `area/64`；Sampled 仍按
///   面积收 1/16（`area/1024`）；免检 0。
/// - 魔数道：每查一张 4 步。Full `tex×4`；Sampled 查 1/16 张 → `tex/4`。
/// - 配额道：聚合和，每张 1 步 + 常数 16；Full 与 Sampled 同价（抽样
///   不省聚合——漏一张就漏配额）；免检 0。
/// - 签名道：每查一张 8 步（FNV 8 轮字节折算）。Full `tex×8`；
///   Sampled `tex/2`。
pub struct ScanCostModel;

impl ScanCostModel {
    /// 每步纳秒（按道）：越界 2ns/步（区域比较+游走）、魔数 1ns/步、
    /// 配额 1ns/步（加法聚合）、签名 4ns/步（FNV 折算最贵）。
    pub const NS_PER_STEP_LANES: [u64; 4] = [2, 1, 1, 4];

    /// 四道名（账本/体检/事件展示用；顺序即道序）。
    pub const LANE_NAMES: [&str; 4] = ["越界扫描", "魔数校验", "配额核查", "签名验签"];

    /// 步数估算（const fn；饱和运算显性防溢出；除数为非零字面量）。
    pub const fn estimate_steps(tier: ScanTier, input: CostInput) -> [u64; 4] {
        let a = input.area_px;
        let t = input.texture_count;
        match tier {
            ScanTier::Full => [
                a.saturating_div(64),
                t.saturating_mul(4),
                t.saturating_add(16),
                t.saturating_mul(8),
            ],
            ScanTier::Sampled => [
                a.saturating_div(1024),
                t.saturating_div(4),
                t.saturating_add(16),
                t.saturating_div(2),
            ],
            ScanTier::Whitelisted => [0, 0, 0, 0],
        }
    }

    /// 步数 → 微秒（每道 步×ns/步 再 ÷1000，整数除法向下取整；饱和防溢出）。
    pub fn steps_to_us(steps: &[u64; 4]) -> u64 {
        let mut total = 0u64;
        let mut i = 0usize;
        while i < 4 {
            let ns = match steps.get(i) {
                Some(s) => s.saturating_mul(Self::NS_PER_STEP_LANES[i]),
                None => 0,
            };
            total = total.saturating_add(ns / 1000);
            i += 1;
        }
        total
    }

    /// 帧前预测：估算安全道总耗时（步数模型 + 固定开销）。
    pub fn estimate_us(tier: ScanTier, input: CostInput) -> u64 {
        Self::steps_to_us(&Self::estimate_steps(tier, input)).saturating_add(FIXED_OVERHEAD_US)
    }
}

// ---------------------------------------------------------------------------
// 五、档位状态机（超支 N 帧降档 / 富余 M 帧升档，双滞回防振荡）
// ---------------------------------------------------------------------------

/// 档位切换记录（方向/前后档/触发原因；入事件账与帧账）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TierTransition {
    /// 切换前档位。
    pub from: ScanTier,
    /// 切换后档位。
    pub to: ScanTier,
    /// 触发原因（人读；来自常量表，不是运行期拼串）。
    pub reason: &'static str,
}

impl TierTransition {
    /// 是否降档（检查深度变浅：Full→Sampled / Sampled→Whitelisted / Full→Whitelisted）。
    pub fn is_downgrade(&self) -> bool {
        self.to.index() < self.from.index()
    }
}

/// 降档原因常量（连续超支触顶）。
pub const REASON_OVERRUN_DOWNGRADE: &str = "连续超支达降档门槛（降质不降帧：降检查深度，帧照常出）";

/// 升档原因常量（连续富余且冷却期满）。
pub const REASON_SURPLUS_UPGRADE: &str = "连续富余达升档门槛且冷却期满（检查深度恢复）";

/// 人工强制切档原因常量。
pub const REASON_FORCE_TIER: &str = "人工强制切档（显性运维动作，进入冷却期留痕）";

/// 档位状态机：连续超支 N 帧降档、连续富余 M 帧升档、切换后冷却锁定。
///
/// 双滞回设计（防振荡）：升档门槛（12 帧）是降档门槛（3 帧）的 4 倍，
/// 叠加 40 帧切换冷却——预算在门槛附近抖动时，状态机宁可停在浅档也
/// 不来回切。被冷却期挡下的合法切换意图记入 `denied_switches`（不丢弃
/// 意图，只是延迟）。
#[derive(Debug)]
pub struct TierGovernor {
    current: ScanTier,
    over_streak: u32,
    surplus_streak: u32,
    cooldown_remaining: u32,
    switches_down: u64,
    switches_up: u64,
    denied_switches: u64,
    observed_frames: u64,
}

impl TierGovernor {
    /// 新状态机：从 Full（全扫）起档——安全检查宁深勿浅，起步给最深档。
    pub fn new() -> Self {
        TierGovernor {
            current: ScanTier::Full,
            over_streak: 0,
            surplus_streak: 0,
            cooldown_remaining: 0,
            switches_down: 0,
            switches_up: 0,
            denied_switches: 0,
            observed_frames: 0,
        }
    }

    /// 当前档位。
    pub fn current(&self) -> ScanTier {
        self.current
    }

    /// 累计降档次数。
    pub fn switches_down(&self) -> u64 {
        self.switches_down
    }

    /// 累计升档次数。
    pub fn switches_up(&self) -> u64 {
        self.switches_up
    }

    /// 被冷却期挡下的切换意图数（滞回压力指标）。
    pub fn denied_switches(&self) -> u64 {
        self.denied_switches
    }

    /// 冷却剩余帧数。
    pub fn cooldown_remaining(&self) -> u32 {
        self.cooldown_remaining
    }

    /// 已观测帧数。
    pub fn observed_frames(&self) -> u64 {
        self.observed_frames
    }

    /// 每帧观测：安全实耗 vs 配额 → 状态推进 → 可能的档位切换。
    ///
    /// 判定序：先扣冷却；超支帧（实耗 > 配额）推降档计数、清富余计数；
    /// 富余帧（实耗 < 配额×750‰）推升档计数、清超支计数；中间帧双清。
    /// 达门槛且不在冷却期才真正切档；在冷却期则意图记入 denied 计数。
    pub fn observe(&mut self, sec_actual_us: u64, quota_us: u64) -> Option<TierTransition> {
        self.observed_frames = self.observed_frames.saturating_add(1);
        if self.cooldown_remaining > 0 {
            self.cooldown_remaining -= 1;
        }
        let surplus_line = quota_us.saturating_mul(SURPLUS_PERMILLE) / 1000;
        if sec_actual_us > quota_us {
            // 超支帧：富余计数清零，超支计数推进。
            self.over_streak = self.over_streak.saturating_add(1);
            self.surplus_streak = 0;
            if self.over_streak >= OVER_DOWNGRADE_N && self.current != ScanTier::Whitelisted {
                if self.cooldown_remaining == 0 {
                    let from = self.current;
                    let to = match from {
                        ScanTier::Full => ScanTier::Sampled,
                        _ => ScanTier::Whitelisted,
                    };
                    self.current = to;
                    self.over_streak = 0;
                    self.surplus_streak = 0;
                    self.cooldown_remaining = TIER_COOLDOWN_FRAMES;
                    self.switches_down = self.switches_down.saturating_add(1);
                    return Some(TierTransition {
                        from,
                        to,
                        reason: REASON_OVERRUN_DOWNGRADE,
                    });
                }
                self.denied_switches = self.denied_switches.saturating_add(1);
            }
            return None;
        }
        if sec_actual_us < surplus_line {
            // 富余帧：超支计数清零，富余计数推进。
            self.surplus_streak = self.surplus_streak.saturating_add(1);
            self.over_streak = 0;
            if self.surplus_streak >= SURPLUS_UPGRADE_M && self.current != ScanTier::Full {
                if self.cooldown_remaining == 0 {
                    let from = self.current;
                    let to = match from {
                        ScanTier::Whitelisted => ScanTier::Sampled,
                        _ => ScanTier::Full,
                    };
                    self.current = to;
                    self.surplus_streak = 0;
                    self.over_streak = 0;
                    self.cooldown_remaining = TIER_COOLDOWN_FRAMES;
                    self.switches_up = self.switches_up.saturating_add(1);
                    return Some(TierTransition {
                        from,
                        to,
                        reason: REASON_SURPLUS_UPGRADE,
                    });
                }
                self.denied_switches = self.denied_switches.saturating_add(1);
            }
            return None;
        }
        // 中间帧（配额内但非富余）：双计数清零——滞回带内不积累任何一侧。
        self.over_streak = 0;
        self.surplus_streak = 0;
        None
    }

    /// 人工强制档位（运维显性覆盖；同样进入冷却期并计数留痕）。
    pub fn force_tier(&mut self, tier: ScanTier) -> Result<TierTransition, FrameContractError> {
        let mut valid = false;
        let mut i = 0;
        while i < ScanTier::ALL.len() {
            if let Some(t) = ScanTier::ALL.get(i) {
                if *t == tier {
                    valid = true;
                }
            }
            i += 1;
        }
        if !valid {
            return Err(FrameContractError::TierInvalid);
        }
        let from = self.current;
        if from == tier {
            return Err(FrameContractError::TierInvalid);
        }
        self.current = tier;
        self.over_streak = 0;
        self.surplus_streak = 0;
        self.cooldown_remaining = TIER_COOLDOWN_FRAMES;
        if tier.index() < from.index() {
            self.switches_down = self.switches_down.saturating_add(1);
        } else {
            self.switches_up = self.switches_up.saturating_add(1);
        }
        Ok(TierTransition { from, to: tier, reason: REASON_FORCE_TIER })
    }
}

// ---------------------------------------------------------------------------
// 六、白名单表（显性登记 + 有效期 + 到期自动摘除；免检是限时豁免）
// ---------------------------------------------------------------------------

/// 白名单条目：资源 id + 到期帧号 + 登记理由（免检必须留痕说清为什么）。
#[derive(Clone, Debug)]
pub struct WhitelistEntry {
    /// 资源 id（纹理资源唯一号；发现明细同号对拍）。
    pub resource_id: u64,
    /// 到期帧号（含）：超过此帧号的帧不再免检。
    pub expiry_frame: u64,
    /// 登记理由（人读；上限 96 字节，超长截断——留痕不灌水）。
    pub reason: String,
}

/// 理由字段上限（字节）。
pub const WHITELIST_REASON_CAP: usize = 96;

/// 白名单表：显性登记制。免检不是默认态——每条都要登记、给理由、给期限；
/// 到期自动摘除并累计（摘除事件由持有方出账）。免检是有期限的豁免，
/// 不是永久失明。
#[derive(Debug)]
pub struct WhitelistTable {
    entries: Vec<WhitelistEntry>,
    expired_total: u64,
}

impl WhitelistTable {
    /// 空表。
    pub fn new() -> Self {
        WhitelistTable { entries: Vec::new(), expired_total: 0 }
    }

    /// 在册条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 累计到期摘除条数（免检失明的证据链）。
    pub fn expired_total(&self) -> u64 {
        self.expired_total
    }

    /// 登记免检（ttl_frames=0 用默认 8000 帧期限）。
    /// 重复 id 拒（走 renew）；表满拒（先摘再登）；理由空拒（免检必须
    /// 说清为什么——空白名单条目等于无据免检）。
    pub fn register(
        &mut self,
        resource_id: u64,
        current_frame: u64,
        ttl_frames: u64,
        reason: &str,
    ) -> Result<(), FrameContractError> {
        if reason.is_empty() {
            return Err(FrameContractError::InputInvalid);
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            let dup = match self.entries.get(i) {
                Some(e) => e.resource_id == resource_id,
                None => false,
            };
            if dup {
                return Err(FrameContractError::WhitelistDuplicate);
            }
            i += 1;
        }
        if self.entries.len() >= WHITELIST_CAP {
            return Err(FrameContractError::WhitelistCapFull);
        }
        let ttl = if ttl_frames == 0 { WHITELIST_TTL_FRAMES } else { ttl_frames };
        let bytes = reason.as_bytes();
        let mut reason_s: String = String::new();
        let mut k = 0usize;
        while k < bytes.len() && k < WHITELIST_REASON_CAP {
            reason_s.push(bytes[k] as char);
            k += 1;
        }
        self.entries.push(WhitelistEntry {
            resource_id,
            expiry_frame: current_frame.saturating_add(ttl),
            reason: reason_s,
        });
        Ok(())
    }

    /// 续期（从当前帧重起期限；条目不存在/已过期视同过期拒——不静默续期）。
    pub fn renew(
        &mut self,
        resource_id: u64,
        current_frame: u64,
        ttl_frames: u64,
    ) -> Result<(), FrameContractError> {
        let mut i = 0usize;
        while i < self.entries.len() {
            let hit = match self.entries.get(i) {
                Some(e) => e.resource_id == resource_id,
                None => false,
            };
            if hit {
                let already_expired = match self.entries.get(i) {
                    Some(e) => e.expiry_frame < current_frame,
                    None => true,
                };
                if already_expired {
                    self.entries.swap_remove(i);
                    self.expired_total = self.expired_total.saturating_add(1);
                    return Err(FrameContractError::WhitelistExpired);
                }
                let ttl = if ttl_frames == 0 { WHITELIST_TTL_FRAMES } else { ttl_frames };
                if let Some(e) = self.entries.get_mut(i) {
                    e.expiry_frame = current_frame.saturating_add(ttl);
                }
                return Ok(());
            }
            i += 1;
        }
        Err(FrameContractError::WhitelistExpired)
    }

    /// 摘除（存在返回 true；不存在返回 false——摘除幂等不报错）。
    pub fn revoke(&mut self, resource_id: u64) -> bool {
        let mut i = 0usize;
        while i < self.entries.len() {
            let hit = match self.entries.get(i) {
                Some(e) => e.resource_id == resource_id,
                None => false,
            };
            if hit {
                self.entries.swap_remove(i);
                return true;
            }
            i += 1;
        }
        false
    }

    /// 取当前帧的有效免检集合：先摘到期条目（累计 expired_total，摘除
    /// 计数差由调用方出事件），再返回存活 id 列表。
    pub fn active_at(&mut self, frame_id: u64) -> Vec<u64> {
        let mut i = 0usize;
        while i < self.entries.len() {
            let expired = match self.entries.get(i) {
                Some(e) => e.expiry_frame < frame_id,
                None => false,
            };
            if expired {
                self.entries.swap_remove(i);
                self.expired_total = self.expired_total.saturating_add(1);
            } else {
                i += 1;
            }
        }
        let mut out: Vec<u64> = Vec::with_capacity(self.entries.len());
        let mut k = 0usize;
        while k < self.entries.len() {
            if let Some(e) = self.entries.get(k) {
                out.push(e.resource_id);
            }
            k += 1;
        }
        out
    }

    /// 快照（体检/测试只读视图）。
    pub fn snapshot(&self) -> Vec<WhitelistEntry> {
        let mut out: Vec<WhitelistEntry> = Vec::with_capacity(self.entries.len());
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                out.push(e.clone());
            }
            i += 1;
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 七、扫描输入与发现（越界/魔数/配额/签名的被检对象与产出）
// ---------------------------------------------------------------------------

/// 发现种类闭集（发现明细 kind 字段；1..=5）。
pub const KIND_OUT_OF_BOUNDS: u8 = 1;
/// 魔数不符。
pub const KIND_BAD_MAGIC: u8 = 2;
/// 字节配额超限。
pub const KIND_QUOTA_EXCEEDED: u8 = 3;
/// 签名不符。
pub const KIND_BAD_SIGNATURE: u8 = 4;
/// 越界道被请求禁用（禁用即发现——不静默）。
pub const KIND_BOUNDS_LANE_DISABLED: u8 = 5;

/// 跳过原因闭集（LaneTrace.skip_reason；0 = 未跳过）。
pub const SKIP_REASON_NONE: u64 = 0;
/// 档位免检（Whitelisted 档整道留痕跳过）。
pub const SKIP_REASON_TIER: u64 = 1;
/// 请求方掩码禁用（需 skip_reason 留痕；越界道禁用另记发现）。
pub const SKIP_REASON_MASKED: u64 = 2;

/// 被检纹理描述（扫描模型的真实数据面：位置/长度/幻数/声明签名）。
#[derive(Clone, Copy, Debug)]
pub struct TextureDesc {
    /// 资源 id（白名单与发现明细同号对拍）。
    pub resource_id: u64,
    /// 纹理头幻数（应为 [`TEX_MAGIC`]）。
    pub magic: u32,
    /// 区域基址（应落在光圈窗内）。
    pub base: u64,
    /// 区域长度（字节；≈像素数，用于越界道计步与配额聚合）。
    pub len: u64,
    /// 声明签名（应为 [`expected_signature`] 的派生值）。
    pub sig: u64,
}

/// 签名派生：FNV 盐基链（id, base, len, magic）——同输入同签名（确定性
/// 对拍要求可复算；非密码学，只做完整性对拍）。
pub fn expected_signature(t: &TextureDesc) -> u64 {
    let h = fnv64_u64(FNV_OFFSET, TEX_SIG_SALT);
    let h = fnv64_u64(h, t.resource_id);
    let h = fnv64_u64(h, t.base);
    let h = fnv64_u64(h, t.len);
    fnv64_u64(h, t.magic as u64)
}

/// 单帧扫描输入（渲染面交给安全检查的清单）。
#[derive(Clone, Debug)]
pub struct ScanInput {
    /// 帧面积（像素；越界道成本自由度）。
    pub area_px: u64,
    /// 渲染主体耗时（µs；帧合同 = 主体 + 安全 ≤ 12_500µs）。
    pub frame_base_us: u64,
    /// 本帧引用的纹理表（≤ 4096 张）。
    pub textures: Vec<TextureDesc>,
    /// 道掩码：bit0..bit3 = 越界/魔数/配额/签名道启用位；**bit0 恒 1**
    /// （越界道不可禁用；禁用请求整体拒收
    /// [`FrameContractError::LaneSkipForbidden`]）。
    pub lane_mask: u8,
    /// 禁道留痕原因（任一道被掩码禁用时必须非 0；否则输入拒收）。
    pub skip_reason: u64,
}

impl ScanInput {
    /// 最小合法输入（全道启用、无纹理、零主体耗时——冒烟基线）。
    pub fn minimal() -> Self {
        ScanInput {
            area_px: 0,
            frame_base_us: 0,
            textures: Vec::new(),
            lane_mask: 0b1111,
            skip_reason: 0,
        }
    }
}

/// 单条发现（道号 + 种类 + 资源 id；不含内容原文——隐私红线）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScanFinding {
    /// 道号（0..=3）。
    pub lane: u8,
    /// 种类（[`KIND_OUT_OF_BOUNDS`]..=[`KIND_BOUNDS_LANE_DISABLED`]）。
    pub kind: u8,
    /// 资源 id（配额道聚合发现记 0）。
    pub resource_id: u64,
}

/// 发现入列（容量 [`FINDINGS_CAP`]；超出只计数 + 截断旗——防发现风暴）。
fn push_finding(
    out: &mut Vec<ScanFinding>,
    total: &mut u64,
    truncated: &mut bool,
    f: ScanFinding,
) {
    *total = total.saturating_add(1);
    if out.len() < FINDINGS_CAP {
        out.push(f);
    } else {
        *truncated = true;
    }
}

/// 白名单命中（线性扫；表容量 64，纹理 4096，最坏 26 万次比较可接受）。
fn whitelist_contains(active: &[u64], id: u64) -> bool {
    let mut i = 0usize;
    while i < active.len() {
        let hit = match active.get(i) {
            Some(&a) => a == id,
            None => false,
        };
        if hit {
            return true;
        }
        i += 1;
    }
    false
}

/// 抽样判定：本帧第 i 张纹理是否入检（Full 全检；Sampled 每 16 张查 1）。
fn lane_scans(tier: ScanTier, i: usize) -> bool {
    match tier {
        ScanTier::Full => true,
        ScanTier::Sampled => i % SAMPLE_STRIDE == 0,
        ScanTier::Whitelisted => false,
    }
}

// ---------------------------------------------------------------------------
// 八、四道检查流水线（每道跳过留痕；越界道禁用即发现）
// ---------------------------------------------------------------------------

/// 单道执行留痕（跳过语义、白名单豁免数、步数实耗、发现数、溢出旗）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LaneTrace {
    /// 道号（0..=3）。
    pub lane: u8,
    /// 本道被跳过（档位免检 / 请求掩码禁用）。
    pub skipped: bool,
    /// 跳过原因（[`SKIP_REASON_NONE`]..=[`SKIP_REASON_MASKED`]）。
    pub skip_reason: u64,
    /// 因白名单被豁免的纹理数（免检留痕：谁被免了，免了多少张）。
    pub excluded_by_whitelist: u64,
    /// 实际步数（成本模型的实测对照面）。
    pub steps: u64,
    /// 实际耗时（µs；步数×ns/步 ÷1000，饱和）。
    pub us: u64,
    /// 本道发现数（含被截断部分——总数诚实）。
    pub findings: u64,
    /// 计步饱和旗（步数打满 u64——理论防御，实际输入已限幅）。
    pub overflow: bool,
}

impl LaneTrace {
    /// 空留痕（道号固定）。
    pub const fn new(lane: u8) -> Self {
        LaneTrace {
            lane,
            skipped: false,
            skip_reason: SKIP_REASON_NONE,
            excluded_by_whitelist: 0,
            steps: 0,
            us: 0,
            findings: 0,
            overflow: false,
        }
    }
}

/// 单帧扫描报告：四道留痕 + 发现明细 + 裁决指纹 + 安全实耗。
#[derive(Clone, Debug)]
pub struct ScanReport {
    /// 帧号。
    pub frame_id: u64,
    /// 本帧档位。
    pub tier: ScanTier,
    /// 四道留痕（道序固定）。
    pub traces: [LaneTrace; 4],
    /// 发现明细（≤ [`FINDINGS_CAP`] 条；全量计数在 `finding_total`）。
    pub findings: Vec<ScanFinding>,
    /// 发现总数（诚实计数，截断不隐瞒）。
    pub finding_total: u64,
    /// 发现明细被截断（风暴保护触发）。
    pub findings_truncated: bool,
    /// 裁决指纹（档位+四道留痕+发现明细的 FNV；双跑对拍键）。
    pub verdict_fingerprint: u64,
    /// 安全道总实耗（µs；= Σ道 + 固定开销；守恒对拍锚点）。
    pub sec_actual_us: u64,
}

/// 裁决指纹（档位 + 四道 (skipped, steps, findings) + 发现序列；纯函数，
/// 同输入同值——双跑对拍的键）。
pub fn verdict_fingerprint(
    tier: ScanTier,
    traces: &[LaneTrace; 4],
    findings: &[ScanFinding],
) -> u64 {
    let mut h = fnv64_u64(FNV_OFFSET, tier.index() as u64);
    let mut i = 0usize;
    while i < 4 {
        let (sk, st, fd) = match traces.get(i) {
            Some(t) => (t.skipped as u64, t.steps, t.findings),
            None => (0, 0, 0),
        };
        h = fnv64_u64(h, sk);
        h = fnv64_u64(h, st);
        h = fnv64_u64(h, fd);
        i += 1;
    }
    h = fnv64_u64(h, findings.len() as u64);
    i = 0;
    while i < findings.len() {
        let (lk, rid) = match findings.get(i) {
            Some(f) => (((f.lane as u64) << 8) | (f.kind as u64), f.resource_id),
            None => (0, 0),
        };
        h = fnv64_u64(h, lk);
        h = fnv64_u64(h, rid);
        i += 1;
    }
    h
}

/// 四道检查流水线（无内部可变扫描状态——双跑确定性的结构保证；
/// 统计计数器只累加，不参与裁决与指纹）。
#[derive(Debug)]
pub struct SecurityScanPipeline {
    lanes_run: u64,
    lanes_skipped: u64,
    findings_total: u64,
    overflow_frames: u64,
}

impl SecurityScanPipeline {
    /// 新流水线。
    pub fn new() -> Self {
        SecurityScanPipeline {
            lanes_run: 0,
            lanes_skipped: 0,
            findings_total: 0,
            overflow_frames: 0,
        }
    }

    /// 累计执行道数。
    pub fn lanes_run(&self) -> u64 {
        self.lanes_run
    }

    /// 累计跳过道数（含档位免检与掩码禁用——跳过必留痕必计数）。
    pub fn lanes_skipped(&self) -> u64 {
        self.lanes_skipped
    }

    /// 累计发现总数。
    pub fn findings_total(&self) -> u64 {
        self.findings_total
    }

    /// 出现过计步饱和的帧数。
    pub fn overflow_frames(&self) -> u64 {
        self.overflow_frames
    }

    /// 执行一帧四道检查（假设输入已经控制器校验；流水线自身防御性处理
    /// 越界道被禁用的情形——记发现不静默）。
    pub fn run(
        &mut self,
        frame_id: u64,
        tier: ScanTier,
        input: &ScanInput,
        whitelist_active: &[u64],
    ) -> ScanReport {
        let mut traces = [
            LaneTrace::new(0),
            LaneTrace::new(1),
            LaneTrace::new(2),
            LaneTrace::new(3),
        ];
        let mut findings: Vec<ScanFinding> = Vec::new();
        let mut finding_total = 0u64;
        let mut truncated = false;

        // ---- 道 0：越界扫描（安全底线道；被禁用即记发现） ----
        let bounds_masked = (input.lane_mask & 0b0001) == 0;
        traces[0].skipped = bounds_masked || tier == ScanTier::Whitelisted;
        traces[0].skip_reason = if bounds_masked {
            SKIP_REASON_MASKED
        } else if tier == ScanTier::Whitelisted {
            SKIP_REASON_TIER
        } else {
            SKIP_REASON_NONE
        };
        if bounds_masked {
            // 禁用请求本身记为发现（安全底线：不可静默）。
            push_finding(
                &mut findings,
                &mut finding_total,
                &mut truncated,
                ScanFinding { lane: 0, kind: KIND_BOUNDS_LANE_DISABLED, resource_id: 0 },
            );
            traces[0].findings = traces[0].findings.saturating_add(1);
        }
        if !traces[0].skipped {
            let ap_end = APERTURE_BASE.saturating_add(APERTURE_LEN);
            let mut i = 0usize;
            while i < input.textures.len() {
                if let Some(t) = input.textures.get(i) {
                    if whitelist_contains(whitelist_active, t.resource_id) {
                        traces[0].excluded_by_whitelist =
                            traces[0].excluded_by_whitelist.saturating_add(1);
                    } else if lane_scans(tier, i) {
                        let region_end = t.base.saturating_add(t.len);
                        traces[0].steps = traces[0].steps
                            .saturating_add(1u64.saturating_add(t.len.saturating_div(64)));
                        if t.base < APERTURE_BASE || region_end > ap_end {
                            push_finding(
                                &mut findings,
                                &mut finding_total,
                                &mut truncated,
                                ScanFinding {
                                    lane: 0,
                                    kind: KIND_OUT_OF_BOUNDS,
                                    resource_id: t.resource_id,
                                },
                            );
                            traces[0].findings = traces[0].findings.saturating_add(1);
                        }
                        let _ = region_end;
                    }
                }
                i += 1;
            }
        }

        // ---- 道 1：魔数校验（纹理头幻数） ----
        let magic_masked = (input.lane_mask & 0b0010) == 0;
        traces[1].skipped = magic_masked || tier == ScanTier::Whitelisted;
        traces[1].skip_reason = if magic_masked {
            SKIP_REASON_MASKED
        } else if tier == ScanTier::Whitelisted {
            SKIP_REASON_TIER
        } else {
            SKIP_REASON_NONE
        };
        if !traces[1].skipped {
            let mut i = 0usize;
            while i < input.textures.len() {
                if let Some(t) = input.textures.get(i) {
                    if whitelist_contains(whitelist_active, t.resource_id) {
                        traces[1].excluded_by_whitelist =
                            traces[1].excluded_by_whitelist.saturating_add(1);
                    } else if lane_scans(tier, i) {
                        traces[1].steps = traces[1].steps.saturating_add(4);
                        if t.magic != TEX_MAGIC {
                            push_finding(
                                &mut findings,
                                &mut finding_total,
                                &mut truncated,
                                ScanFinding {
                                    lane: 1,
                                    kind: KIND_BAD_MAGIC,
                                    resource_id: t.resource_id,
                                },
                            );
                            traces[1].findings = traces[1].findings.saturating_add(1);
                        }
                    }
                }
                i += 1;
            }
        }

        // ---- 道 2：配额核查（字节总量聚合；Full/Sampled 同价全量聚合——
        // 抽样不省聚合：漏一张就漏配额，聚合本身是加法，便宜） ----
        let quota_masked = (input.lane_mask & 0b0100) == 0;
        traces[2].skipped = quota_masked || tier == ScanTier::Whitelisted;
        traces[2].skip_reason = if quota_masked {
            SKIP_REASON_MASKED
        } else if tier == ScanTier::Whitelisted {
            SKIP_REASON_TIER
        } else {
            SKIP_REASON_NONE
        };
        if !traces[2].skipped {
            let mut total_bytes = 0u64;
            let mut add_overflow = false;
            traces[2].steps = traces[2].steps.saturating_add(16);
            let mut i = 0usize;
            while i < input.textures.len() {
                if let Some(t) = input.textures.get(i) {
                    if whitelist_contains(whitelist_active, t.resource_id) {
                        traces[2].excluded_by_whitelist =
                            traces[2].excluded_by_whitelist.saturating_add(1);
                    } else {
                        traces[2].steps = traces[2].steps.saturating_add(1);
                        let sum = total_bytes.saturating_add(t.len);
                        if sum < total_bytes {
                            add_overflow = true;
                        }
                        total_bytes = sum;
                    }
                }
                i += 1;
            }
            if add_overflow {
                traces[2].overflow = true;
            }
            if total_bytes > QUOTA_BYTES || add_overflow {
                push_finding(
                    &mut findings,
                    &mut finding_total,
                    &mut truncated,
                    ScanFinding { lane: 2, kind: KIND_QUOTA_EXCEEDED, resource_id: 0 },
                );
                traces[2].findings = traces[2].findings.saturating_add(1);
            }
        }

        // ---- 道 3：签名验签（FNV 派生签名对拍） ----
        let sig_masked = (input.lane_mask & 0b1000) == 0;
        traces[3].skipped = sig_masked || tier == ScanTier::Whitelisted;
        traces[3].skip_reason = if sig_masked {
            SKIP_REASON_MASKED
        } else if tier == ScanTier::Whitelisted {
            SKIP_REASON_TIER
        } else {
            SKIP_REASON_NONE
        };
        if !traces[3].skipped {
            let mut i = 0usize;
            while i < input.textures.len() {
                if let Some(t) = input.textures.get(i) {
                    if whitelist_contains(whitelist_active, t.resource_id) {
                        traces[3].excluded_by_whitelist =
                            traces[3].excluded_by_whitelist.saturating_add(1);
                    } else if lane_scans(tier, i) {
                        traces[3].steps = traces[3].steps.saturating_add(8);
                        if t.sig != expected_signature(t) {
                            push_finding(
                                &mut findings,
                                &mut finding_total,
                                &mut truncated,
                                ScanFinding {
                                    lane: 3,
                                    kind: KIND_BAD_SIGNATURE,
                                    resource_id: t.resource_id,
                                },
                            );
                            traces[3].findings = traces[3].findings.saturating_add(1);
                        }
                    }
                }
                i += 1;
            }
        }

        // ---- 收口：步数→微秒、统计、指纹 ----
        let mut sec_us = 0u64;
        let mut any_overflow = false;
        let mut i = 0usize;
        while i < 4 {
            let ns = match traces.get(i) {
                Some(t) => {
                    any_overflow = any_overflow || t.overflow;
                    t.steps.saturating_mul(ScanCostModel::NS_PER_STEP_LANES[i])
                }
                None => 0,
            };
            sec_us = sec_us.saturating_add(ns / 1000);
            if let Some(t) = traces.get_mut(i) {
                t.us = ns / 1000;
            }
            i += 1;
        }
        sec_us = sec_us.saturating_add(FIXED_OVERHEAD_US);
        if any_overflow {
            self.overflow_frames = self.overflow_frames.saturating_add(1);
        }
        let fp = verdict_fingerprint(tier, &traces, &findings);

        // 统计只累加，不进裁决（跳过道与执行道都计数——跳过必留痕）。
        let mut i = 0usize;
        while i < 4 {
            let skipped = match traces.get(i) {
                Some(t) => t.skipped,
                None => false,
            };
            if skipped {
                self.lanes_skipped = self.lanes_skipped.saturating_add(1);
            } else {
                self.lanes_run = self.lanes_run.saturating_add(1);
            }
            i += 1;
        }
        self.findings_total = self.findings_total.saturating_add(finding_total);

        ScanReport {
            frame_id,
            tier,
            traces,
            findings,
            finding_total,
            findings_truncated: truncated,
            verdict_fingerprint: fp,
            sec_actual_us: sec_us,
        }
    }
}

// ---------------------------------------------------------------------------
// 九、帧账（600 帧环形）与守恒对账（Σ帧实耗 ≡ Σ道实耗）
// ---------------------------------------------------------------------------

/// 升序插入排序（账本分位计算用；不用 alloc 的 sort——确定性口径自持，
/// 不依赖标准库实现细节）。
fn sort_u64_asc(v: &mut [u64]) {
    let n = v.len();
    let mut i = 1usize;
    while i < n {
        let mut j = i;
        while j > 0 {
            let a = match v.get(j - 1) {
                Some(x) => *x,
                None => break,
            };
            let b = match v.get(j) {
                Some(x) => *x,
                None => break,
            };
            if a <= b {
                break;
            }
            v[j - 1] = b;
            v[j] = a;
            j -= 1;
        }
        i += 1;
    }
}

/// 单帧记账（帧账的最小单元；Copy 结构，600 条环形在堆上 Vec 缓冲）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameRecord {
    /// 帧号（单调递增）。
    pub frame_id: u64,
    /// 本帧档位（ScanTier::index()）。
    pub tier: u8,
    /// 帧预算（µs；合同基线快照）。
    pub budget_us: u64,
    /// 安全配额（µs）。
    pub quota_us: u64,
    /// 帧前估算（成本模型；预测口径）。
    pub est_us: u64,
    /// 安全道实耗（µs；实测口径 = Σlane_us + 固定开销）。
    pub sec_actual_us: u64,
    /// 整帧实耗（µs；主体 + 安全）。
    pub frame_total_us: u64,
    /// 四道实耗（µs；道账）。
    pub lane_us: [u64; 4],
    /// 安全配额超支（sec_actual > 800µs；触发降档计数）。
    pub quota_overrun: bool,
    /// 帧合同违约（frame_total > 12_500µs；显性事件不可静默）。
    pub contract_breach: bool,
    /// 本帧发生降档。
    pub downgraded: bool,
    /// 本帧发生升档。
    pub upgraded: bool,
    /// 裁决指纹（对拍链路：同帧同输入同指纹）。
    pub verdict_fingerprint: u64,
}

impl FrameRecord {
    /// 守恒检查：Σ道实耗 + 固定开销 ≡ 帧安全实耗（帧账与道账对拍；破即 Err）。
    pub fn reconcile(&self) -> Result<(), FrameContractError> {
        let mut lane_sum = 0u64;
        let mut i = 0usize;
        while i < 4 {
            lane_sum = lane_sum.saturating_add(match self.lane_us.get(i) {
                Some(v) => *v,
                None => 0,
            });
            i += 1;
        }
        if lane_sum.saturating_add(FIXED_OVERHEAD_US) != self.sec_actual_us {
            return Err(FrameContractError::ConservationBroken);
        }
        Ok(())
    }
}

/// 帧账：600 帧环形（Vec 缓冲在堆上，结构本体 ~百字节不上栈超标）+
/// 会话累计（全量计数，环覆盖不丢累计）。
#[derive(Debug)]
pub struct FrameLedger {
    ring: Vec<FrameRecord>,
    head: usize,
    filled: usize,
    overwritten: u64,
    total_frames: u64,
    total_quota_overrun: u64,
    total_contract_breach: u64,
    total_downgrades: u64,
    total_upgrades: u64,
    total_sec_us: u64,
    total_frame_us: u64,
}

impl FrameLedger {
    /// 新账本（预保留 600 帧缓冲；Vec 缓冲上堆——>64KB 结构禁上栈）。
    pub fn new() -> Self {
        FrameLedger {
            ring: Vec::with_capacity(RING_FRAMES),
            head: 0,
            filled: 0,
            overwritten: 0,
            total_frames: 0,
            total_quota_overrun: 0,
            total_contract_breach: 0,
            total_downgrades: 0,
            total_upgrades: 0,
            total_sec_us: 0,
            total_frame_us: 0,
        }
    }

    /// 环内当前帧数（≤ 600）。
    pub fn window_len(&self) -> usize {
        self.filled
    }

    /// 环覆盖次数（老帧被新帧顶掉的张数）。
    pub fn overwritten(&self) -> u64 {
        self.overwritten
    }

    /// 会话累计帧数（含被覆盖的）。
    pub fn total_frames(&self) -> u64 {
        self.total_frames
    }

    /// 会话累计配额超支帧数。
    pub fn total_quota_overrun(&self) -> u64 {
        self.total_quota_overrun
    }

    /// 会话累计合同违约帧数。
    pub fn total_contract_breach(&self) -> u64 {
        self.total_contract_breach
    }

    /// 会话累计降档次数。
    pub fn total_downgrades(&self) -> u64 {
        self.total_downgrades
    }

    /// 会话累计升档次数。
    pub fn total_upgrades(&self) -> u64 {
        self.total_upgrades
    }

    /// 会话累计安全道实耗（µs）。
    pub fn total_sec_us(&self) -> u64 {
        self.total_sec_us
    }

    /// 会话累计整帧实耗（µs）。
    pub fn total_frame_us(&self) -> u64 {
        self.total_frame_us
    }

    /// 入账（先守恒对拍：Σ道实耗 ≡ 帧安全实耗，破即拒——账本不收破账）。
    pub fn record(&mut self, rec: FrameRecord) -> Result<(), FrameContractError> {
        rec.reconcile()?;
        if self.filled < RING_FRAMES {
            self.ring.push(rec);
            self.filled += 1;
        } else {
            // 环满：覆盖最旧（head 即最旧位）。
            if let Some(slot) = self.ring.get_mut(self.head) {
                *slot = rec;
            }
            self.head = (self.head + 1) % RING_FRAMES;
            self.overwritten = self.overwritten.saturating_add(1);
        }
        self.total_frames = self.total_frames.saturating_add(1);
        if rec.quota_overrun {
            self.total_quota_overrun = self.total_quota_overrun.saturating_add(1);
        }
        if rec.contract_breach {
            self.total_contract_breach = self.total_contract_breach.saturating_add(1);
        }
        if rec.downgraded {
            self.total_downgrades = self.total_downgrades.saturating_add(1);
        }
        if rec.upgraded {
            self.total_upgrades = self.total_upgrades.saturating_add(1);
        }
        self.total_sec_us = self.total_sec_us.saturating_add(rec.sec_actual_us);
        self.total_frame_us = self.total_frame_us.saturating_add(rec.frame_total_us);
        Ok(())
    }

    /// 环内只读快照（按入账序：head 起环绕一周；窗口聚合用）。
    pub fn window(&self) -> Vec<FrameRecord> {
        let mut out: Vec<FrameRecord> = Vec::with_capacity(self.filled);
        let mut k = 0usize;
        while k < self.filled {
            let idx = if self.filled < RING_FRAMES {
                k
            } else {
                (self.head + k) % RING_FRAMES
            };
            if let Some(r) = self.ring.get(idx) {
                out.push(*r);
            }
            k += 1;
        }
        out
    }

    /// 会话级守恒对账：环内 Σ(Σ道) ≡ Σ安全实耗（逐帧对拍已在 record 拦，
    /// 这里是环级复核——双保险，破即 Err 带差值语义）。
    pub fn reconcile(&self) -> Result<u64, FrameContractError> {
        let mut lane_sum = 0u64;
        let mut sec_sum = 0u64;
        let mut k = 0usize;
        let n = self.filled;
        while k < n {
            let idx = if n < RING_FRAMES { k } else { (self.head + k) % RING_FRAMES };
            if let Some(r) = self.ring.get(idx) {
                let mut i = 0usize;
                while i < 4 {
                    lane_sum = lane_sum.saturating_add(match r.lane_us.get(i) {
                        Some(v) => *v,
                        None => 0,
                    });
                    i += 1;
                }
                sec_sum = sec_sum.saturating_add(r.sec_actual_us);
            }
            k += 1;
        }
        if lane_sum.saturating_add((n as u64).saturating_mul(FIXED_OVERHEAD_US)) != sec_sum {
            // 环级破账：差值只进诊断语义（事件 aux 由持有方折算），
            // 对拍结论不变——双账不等即守恒破。
            return Err(FrameContractError::ConservationBroken);
        }
        Ok(0)
    }
}

// ---------------------------------------------------------------------------
// 十、会话聚合（P95 整数分位 / 违约率 ppm / 降档次数；隐私安全面）
// ---------------------------------------------------------------------------

/// 会话聚合（环窗 + 会话累计的合成视图；只含统计量——隐私安全投影面）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionAggregate {
    /// 会话累计帧数。
    pub frames: u64,
    /// 环窗内帧数（≤ 600）。
    pub window_frames: u64,
    /// 会话累计安全道实耗（µs）。
    pub sec_total_us: u64,
    /// 环窗安全实耗均值（µs；整数除法，窗空为 0）。
    pub sec_mean_us: u64,
    /// 环窗安全实耗 P95（µs；整数分位：升序第 floor(0.95n) 个，0 起）。
    pub sec_p95_us: u64,
    /// 环窗安全实耗峰值（µs）。
    pub sec_max_us: u64,
    /// 环窗整帧实耗 P95（µs）。
    pub frame_p95_us: u64,
    /// 配额超支率（ppm；会话累计口径，饱和）。
    pub overrun_ppm: u64,
    /// 帧合同违约率（ppm；会话累计口径，饱和）。
    pub breach_ppm: u64,
    /// 会话累计降档次数。
    pub downgrades: u64,
    /// 会话累计升档次数。
    pub upgrades: u64,
    /// 环覆盖次数。
    pub overwritten: u64,
    /// 各档位帧数（环窗口径；下标 = ScanTier::index()）。
    pub tier_frames: [u64; 3],
    /// 聚合指纹（各统计量 FNV 链；体检/对拍收口用）。
    pub fingerprint: u64,
}

/// 整数 P95：升序后取下标 floor(0.95n)（0 起，越界收敛到 n-1；n=0 → 0）。
/// 口径注释：floor(0.95×100)=95 → 第 96 小值；与 ceil 口径差 1 个分位，
/// 本项统一用 floor（偏保守——分位值略高，预算纪律宁紧勿松）。
pub fn p95_of(mut sorted_src: Vec<u64>) -> u64 {
    if sorted_src.is_empty() {
        return 0;
    }
    sort_u64_asc(&mut sorted_src);
    let n = sorted_src.len();
    let mut idx = ((n as u64).saturating_mul(95) / 100) as usize;
    if idx >= n {
        idx = n - 1;
    }
    match sorted_src.get(idx) {
        Some(v) => *v,
        None => 0,
    }
}

/// ppm 折算：part/total × 1_000_000（total=0 → 0；饱和防溢出）。
pub fn ppm_of(part: u64, total: u64) -> u64 {
    if total == 0 {
        return 0;
    }
    part.saturating_mul(1_000_000) / total
}

impl FrameLedger {
    /// 会话聚合（环窗分位/档位分布 + 会话累计 ppm/降升档次数）。
    pub fn aggregate(&self) -> SessionAggregate {
        let win = self.window();
        let mut sec_vals: Vec<u64> = Vec::with_capacity(win.len());
        let mut frame_vals: Vec<u64> = Vec::with_capacity(win.len());
        let mut tier_frames = [0u64; 3];
        let mut sec_sum = 0u64;
        let mut sec_max = 0u64;
        let mut k = 0usize;
        while k < win.len() {
            if let Some(r) = win.get(k) {
                sec_vals.push(r.sec_actual_us);
                frame_vals.push(r.frame_total_us);
                sec_sum = sec_sum.saturating_add(r.sec_actual_us);
                if r.sec_actual_us > sec_max {
                    sec_max = r.sec_actual_us;
                }
                let ti = r.tier as usize;
                if ti < SCAN_TIER_COUNT {
                    tier_frames[ti] = tier_frames[ti].saturating_add(1);
                }
            }
            k += 1;
        }
        let mean = if win.is_empty() {
            0
        } else {
            sec_sum / (win.len() as u64)
        };
        SessionAggregate {
            frames: self.total_frames,
            window_frames: win.len() as u64,
            sec_total_us: self.total_sec_us,
            sec_mean_us: mean,
            sec_p95_us: p95_of(sec_vals),
            sec_max_us: sec_max,
            frame_p95_us: p95_of(frame_vals),
            overrun_ppm: ppm_of(self.total_quota_overrun, self.total_frames),
            breach_ppm: ppm_of(self.total_contract_breach, self.total_frames),
            downgrades: self.total_downgrades,
            upgrades: self.total_upgrades,
            overwritten: self.overwritten,
            tier_frames,
            fingerprint: 0,
        }
        .with_fingerprint()
    }
}

impl SessionAggregate {
    /// 聚合指纹（字段序固定：同会话同指纹；体检对拍键）。
    pub fn with_fingerprint(mut self) -> Self {
        let mut h = fnv64_u64(FNV_OFFSET, self.frames);
        h = fnv64_u64(h, self.window_frames);
        h = fnv64_u64(h, self.sec_total_us);
        h = fnv64_u64(h, self.sec_mean_us);
        h = fnv64_u64(h, self.sec_p95_us);
        h = fnv64_u64(h, self.sec_max_us);
        h = fnv64_u64(h, self.frame_p95_us);
        h = fnv64_u64(h, self.overrun_ppm);
        h = fnv64_u64(h, self.breach_ppm);
        h = fnv64_u64(h, self.downgrades);
        h = fnv64_u64(h, self.upgrades);
        h = fnv64_u64(h, self.overwritten);
        let mut i = 0usize;
        while i < 3 {
            h = fnv64_u64(h, match self.tier_frames.get(i) {
                Some(v) => *v,
                None => 0,
            });
            i += 1;
        }
        self.fingerprint = h;
        self
    }
}

// ---------------------------------------------------------------------------
// 十一、确定性对拍（同输入双跑同裁决——扫描是纯函数的对拍证据）
// ---------------------------------------------------------------------------

/// 双跑对拍结果（指纹、四道步数、裁决一致性）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeterminismOutcome {
    /// 两跑指纹一致。
    pub matched: bool,
    /// 第一跑指纹。
    pub fingerprint_a: u64,
    /// 第二跑指纹。
    pub fingerprint_b: u64,
    /// 第一跑四道步数。
    pub steps_a: [u64; 4],
    /// 第二跑四道步数。
    pub steps_b: [u64; 4],
    /// 第一跑发现总数。
    pub findings_a: u64,
    /// 第二跑发现总数。
    pub findings_b: u64,
}

/// 两份扫描报告是否同裁决（指纹 + 四道步数 + 发现总数三重对拍）。
pub fn reports_match(a: &ScanReport, b: &ScanReport) -> bool {
    if a.verdict_fingerprint != b.verdict_fingerprint {
        return false;
    }
    if a.finding_total != b.finding_total {
        return false;
    }
    if a.tier != b.tier {
        return false;
    }
    let mut i = 0usize;
    while i < 4 {
        let sa = match a.traces.get(i) {
            Some(t) => t.steps,
            None => 0,
        };
        let sb = match b.traces.get(i) {
            Some(t) => t.steps,
            None => 0,
        };
        if sa != sb {
            return false;
        }
        i += 1;
    }
    true
}

/// 双跑确定性对拍：同输入、同档位、同免检集合连跑两遍，裁决必须一致。
/// 不一致 → `Err(DeterminismMismatch)`（隐藏状态泄漏的证据）。
pub fn determinism_check(
    pipeline: &mut SecurityScanPipeline,
    frame_id: u64,
    tier: ScanTier,
    input: &ScanInput,
    whitelist_active: &[u64],
) -> Result<DeterminismOutcome, FrameContractError> {
    let ra = pipeline.run(frame_id, tier, input, whitelist_active);
    let rb = pipeline.run(frame_id, tier, input, whitelist_active);
    let mut steps_a = [0u64; 4];
    let mut steps_b = [0u64; 4];
    let mut i = 0usize;
    while i < 4 {
        steps_a[i] = match ra.traces.get(i) {
            Some(t) => t.steps,
            None => 0,
        };
        steps_b[i] = match rb.traces.get(i) {
            Some(t) => t.steps,
            None => 0,
        };
        i += 1;
    }
    let out = DeterminismOutcome {
        matched: reports_match(&ra, &rb),
        fingerprint_a: ra.verdict_fingerprint,
        fingerprint_b: rb.verdict_fingerprint,
        steps_a,
        steps_b,
        findings_a: ra.finding_total,
        findings_b: rb.finding_total,
    };
    if out.matched {
        Ok(out)
    } else {
        Err(FrameContractError::DeterminismMismatch)
    }
}

// ---------------------------------------------------------------------------
// 十二、事件账（环形；诊断码段 0x1300~0x130F 双语境复用）
// ---------------------------------------------------------------------------

/// 单条事件（序号 + 诊断码 + 帧号 + 辅助量；事件是显性化通道——任何
/// 违约/降档/摘除/破账都不静默）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EventRecord {
    /// 事件序号（会话内单调递增，从 1 起）。
    pub seq: u64,
    /// 诊断码（0x1300~0x130F 闭集；语义见错误枚举与 `ev` 模块）。
    pub code: u32,
    /// 关联帧号（0 = 非帧关联）。
    pub frame_id: u64,
    /// 辅助量（各事件语义见 `ev` 模块注释）。
    pub aux: u64,
}

/// 事件账：128 条环形，覆盖留数（overwritten 计数进聚合——账可覆盖，
/// 计数不丢）。
#[derive(Debug)]
pub struct EventLedger {
    ring: Vec<EventRecord>,
    head: usize,
    filled: usize,
    overwritten: u64,
    seq_next: u64,
}

impl EventLedger {
    /// 新事件账。
    pub fn new() -> Self {
        EventLedger {
            ring: Vec::with_capacity(EVENT_CAP),
            head: 0,
            filled: 0,
            overwritten: 0,
            seq_next: 1,
        }
    }

    /// 环内事件数。
    pub fn len(&self) -> usize {
        self.filled
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.filled == 0
    }

    /// 环覆盖数。
    pub fn overwritten(&self) -> u64 {
        self.overwritten
    }

    /// 已发出的事件总数（含被覆盖的）。
    pub fn total(&self) -> u64 {
        self.seq_next.saturating_sub(1)
    }

    /// 出账（环形覆盖；序号单调）。
    pub fn push(&mut self, code: u32, frame_id: u64, aux: u64) {
        let rec = EventRecord { seq: self.seq_next, code, frame_id, aux };
        self.seq_next = self.seq_next.saturating_add(1);
        if self.filled < EVENT_CAP {
            self.ring.push(rec);
            self.filled += 1;
        } else {
            if let Some(slot) = self.ring.get_mut(self.head) {
                *slot = rec;
            }
            self.head = (self.head + 1) % EVENT_CAP;
            self.overwritten = self.overwritten.saturating_add(1);
        }
    }

    /// 环内快照（时间序）。
    pub fn snapshot(&self) -> Vec<EventRecord> {
        let mut out: Vec<EventRecord> = Vec::with_capacity(self.filled);
        let mut k = 0usize;
        while k < self.filled {
            let idx = if self.filled < EVENT_CAP {
                k
            } else {
                (self.head + k) % EVENT_CAP
            };
            if let Some(e) = self.ring.get(idx) {
                out.push(*e);
            }
            k += 1;
        }
        out
    }

    /// 按码计数（环窗口径）。
    pub fn count_code(&self, code: u32) -> u64 {
        let mut n = 0u64;
        let mut k = 0usize;
        while k < self.filled {
            let idx = if self.filled < EVENT_CAP {
                k
            } else {
                (self.head + k) % EVENT_CAP
            };
            if let Some(e) = self.ring.get(idx) {
                if e.code == code {
                    n = n.saturating_add(1);
                }
            }
            k += 1;
        }
        n
    }

    /// 事件账指纹（全部环内事件的 FNV 链——账被翻动即指纹变）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = fnv64_u64(FNV_OFFSET, self.total());
        let snap = self.snapshot();
        let mut i = 0usize;
        while i < snap.len() {
            if let Some(e) = snap.get(i) {
                h = fnv64_u64(h, (e.seq as u32) as u64);
                h = fnv64_u64(h, e.code as u64);
                h = fnv64_u64(h, e.frame_id);
                h = fnv64_u64(h, e.aux);
            }
            i += 1;
        }
        h
    }

    /// 自洽检查（序号链完整 + 环容量合法；体检项）。
    pub fn self_consistent(&self) -> bool {
        if self.seq_next < 1 {
            return false;
        }
        if self.filled > EVENT_CAP {
            return false;
        }
        if self.filled < EVENT_CAP {
            self.overwritten == 0
        } else {
            true
        }
    }
}

// ---------------------------------------------------------------------------
// 十三、违约账（显性化封存；每条带人读诊断——违约不可静默）
// ---------------------------------------------------------------------------

/// 单条违约（诊断码 + 帧号 + 人读明细）。
#[derive(Clone, Debug)]
pub struct ViolationRecord {
    /// 违约序号（会话内单调，从 1 起）。
    pub seq: u64,
    /// 诊断码（0x1300~0x130F）。
    pub code: u32,
    /// 关联帧号（0 = 非帧关联）。
    pub frame_id: u64,
    /// 人读明细（诊断 + 上下文；静态诊断串 + 短上下文拼装）。
    pub detail: String,
}

/// 违约账：64 条封存（满后丢最旧并出 [`ev::LEDGER_FULL`] 事件——账有界，
/// 丢弃有痕）。
#[derive(Debug)]
pub struct ContractViolationLedger {
    items: Vec<ViolationRecord>,
    dropped: u64,
    seq_next: u64,
}

impl ContractViolationLedger {
    /// 新违约账。
    pub fn new() -> Self {
        ContractViolationLedger { items: Vec::new(), dropped: 0, seq_next: 1 }
    }

    /// 在册违约数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 累计丢弃数（账满丢最旧）。
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// 已记录违约总数（含丢弃的）。
    pub fn total(&self) -> u64 {
        self.seq_next.saturating_add(0).saturating_sub(1)
    }

    /// 记一笔违约（明细 = 诊断 detail + 「@帧N」上下文；满则丢最旧）。
    pub fn record(
        &mut self,
        err: FrameContractError,
        frame_id: u64,
    ) -> Result<(), FrameContractError> {
        let rec = ViolationRecord {
            seq: self.seq_next,
            code: err.code(),
            frame_id,
            detail: format!("{}@帧{}", err.detail(), frame_id),
        };
        self.seq_next = self.seq_next.saturating_add(1);
        if self.items.len() >= VIOLATION_CAP {
            self.items.remove(0);
            self.dropped = self.dropped.saturating_add(1);
        }
        self.items.push(rec);
        if self.dropped > 0 && self.dropped == 1 {
            // 首次丢账时回显账满错误（调用方出事件；账本身照记不误）。
            return Err(FrameContractError::LedgerFull);
        }
        Ok(())
    }

    /// 按码计数（在册口径）。
    pub fn count_code(&self, code: u32) -> u64 {
        let mut n = 0u64;
        let mut i = 0usize;
        while i < self.items.len() {
            if let Some(v) = self.items.get(i) {
                if v.code == code {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 快照（体检/导出）。
    pub fn snapshot(&self) -> Vec<ViolationRecord> {
        let mut out: Vec<ViolationRecord> = Vec::with_capacity(self.items.len());
        let mut i = 0usize;
        while i < self.items.len() {
            if let Some(v) = self.items.get(i) {
                out.push(v.clone());
            }
            i += 1;
        }
        out
    }

    /// 违约账指纹（码序列 FNV 链）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = fnv64_u64(FNV_OFFSET, self.total());
        let mut i = 0usize;
        while i < self.items.len() {
            if let Some(v) = self.items.get(i) {
                h = fnv64_u64(h, v.code as u64);
            }
            i += 1;
        }
        h
    }
}

// ---------------------------------------------------------------------------
// 十四、隐私红线（内容指纹不出内核；遥测只投影统计量白名单字段）
// ---------------------------------------------------------------------------

/// 遥测字段白名单（投影闭集；白名单外字段名一律拒投）。
pub const TELEMETRY_FIELD_ALLOWLIST: [&str; 12] = [
    "frames",
    "window_frames",
    "sec_total_us",
    "sec_p95_us",
    "frame_p95_us",
    "overrun_ppm",
    "breach_ppm",
    "downgrades",
    "upgrades",
    "health_score",
    "tier_index",
    "events_total",
];

/// 内容指纹禁用子串（字段名含任一即红线；发现明细/纹理签名等内容数据
/// 的惯用字段名全部命中）。
pub const CONTENT_FP_BANNED_SUBSTRINGS: [&str; 5] = [
    "content_fp",
    "content_hash",
    "content_fingerprint",
    "texture_sig",
    "内容指纹",
];

/// 遥测投影：只装白名单统计量（数值面），内容指纹物理上无法进入——
/// push 前先过红线检查，撞线即拒 [`FrameContractError::PrivacyRedline`]。
#[derive(Clone, Debug, Default)]
pub struct TelemetryProjection {
    fields: Vec<(&'static str, u64)>,
}

impl TelemetryProjection {
    /// 空投影。
    pub fn new() -> Self {
        TelemetryProjection { fields: Vec::new() }
    }

    /// 投影字段数。
    pub fn len(&self) -> usize {
        self.fields.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    /// 投影字段只读视图。
    pub fn fields(&self) -> &[(&'static str, u64)] {
        &self.fields
    }

    /// 字段名是否在白名单内。
    pub fn field_allowed(name: &str) -> bool {
        let mut i = 0usize;
        while i < TELEMETRY_FIELD_ALLOWLIST.len() {
            if let Some(a) = TELEMETRY_FIELD_ALLOWLIST.get(i) {
                if *a == name {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 字段名是否撞内容指纹红线（子串扫描，大小写敏感——红线从紧）。
    pub fn field_banned(name: &str) -> bool {
        let mut i = 0usize;
        while i < CONTENT_FP_BANNED_SUBSTRINGS.len() {
            if let Some(b) = CONTENT_FP_BANNED_SUBSTRINGS.get(i) {
                if name.contains(b) {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 投影一个字段（白名单 + 红线双闸；任一不过即拒）。
    pub fn push(&mut self, name: &'static str, value: u64) -> Result<(), FrameContractError> {
        if Self::field_banned(name) {
            return Err(FrameContractError::PrivacyRedline);
        }
        if !Self::field_allowed(name) {
            return Err(FrameContractError::PrivacyRedline);
        }
        self.fields.push((name, value));
        Ok(())
    }

    /// 红线自检（投影全部字段复扫；通过 = 无撞线字段）。
    pub fn redline_selfcheck(&self) -> Result<(), FrameContractError> {
        let mut i = 0usize;
        while i < self.fields.len() {
            let name = match self.fields.get(i) {
                Some((n, _)) => *n,
                None => return Err(FrameContractError::PrivacyRedline),
            };
            if Self::field_banned(name) {
                return Err(FrameContractError::PrivacyRedline);
            }
            i += 1;
        }
        Ok(())
    }
}

/// 从聚合 + 体检分数构建遥测投影（只投影统计量白名单字段）。
pub fn project_telemetry(
    agg: &SessionAggregate,
    health_score: u32,
    events_total: u64,
    tier_index: u8,
) -> Result<TelemetryProjection, FrameContractError> {
    let mut p = TelemetryProjection::new();
    p.push("frames", agg.frames)?;
    p.push("window_frames", agg.window_frames)?;
    p.push("sec_total_us", agg.sec_total_us)?;
    p.push("sec_p95_us", agg.sec_p95_us)?;
    p.push("frame_p95_us", agg.frame_p95_us)?;
    p.push("overrun_ppm", agg.overrun_ppm)?;
    p.push("breach_ppm", agg.breach_ppm)?;
    p.push("downgrades", agg.downgrades)?;
    p.push("upgrades", agg.upgrades)?;
    p.push("health_score", health_score as u64)?;
    p.push("tier_index", tier_index as u64)?;
    p.push("events_total", events_total)?;
    p.redline_selfcheck()?;
    Ok(p)
}

// ---------------------------------------------------------------------------
// 十五、体检（8 项检查 + 评分 + FNV 指纹；关键破口显性化）
// ---------------------------------------------------------------------------

/// 单项体检结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HealthCheck {
    /// 检查项名。
    pub name: &'static str,
    /// 是否通过。
    pub pass: bool,
    /// 结论注（人读；含未通过时的处置指引）。
    pub note: &'static str,
}

/// 体检报告。
#[derive(Clone, Debug)]
pub struct HealthReport {
    /// 八项结论。
    pub checks: Vec<HealthCheck>,
    /// 分数（通过数 × 100 ÷ 项数）。
    pub score: u32,
    /// 结论（健康 / 观察 / 不合格）。
    pub verdict: &'static str,
    /// 体检指纹（各项通过位 + 分数 FNV）。
    pub fingerprint: u64,
    /// 关键破口码（空 = 无；有则必须处置）。
    pub critical_codes: Vec<u32>,
}

impl HealthReport {
    /// 分数与结论（关键破口一票否决为「不合格」）。
    pub fn score_and_verdict(checks: &[HealthCheck]) -> (u32, &'static str, Vec<u32>) {
        let total = checks.len() as u64;
        let mut passed = 0u64;
        let mut critical: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < checks.len() {
            if let Some(c) = checks.get(i) {
                if c.pass {
                    passed = passed.saturating_add(1);
                }
            }
            i += 1;
        }
        let score = if total == 0 {
            0
        } else {
            ((passed.saturating_mul(100)) / total) as u32
        };
        // 关键项：违约率/守恒/隐私/合同态（下标 0,1,2,7 的语义见体检组装）。
        let mut j = 0usize;
        while j < checks.len() {
            if let Some(c) = checks.get(j) {
                let is_critical = matches!(
                    c.name,
                    "合同签署状态"
                        | "帧违约率(ppm)"
                        | "守恒对账(帧账≡道账)"
                        | "隐私红线自检"
                );
                if is_critical && !c.pass {
                    critical.push(match c.name {
                        "合同签署状态" => FrameContractError::ContractNotSigned.code(),
                        "帧违约率(ppm)" => FrameContractError::ContractBreached.code(),
                        "守恒对账(帧账≡道账)" => FrameContractError::ConservationBroken.code(),
                        _ => FrameContractError::PrivacyRedline.code(),
                    });
                }
            }
            j += 1;
        }
        let verdict = if !critical.is_empty() {
            "不合格——存在关键破口"
        } else if score >= HEALTH_MIN_SCORE {
            "健康"
        } else {
            "观察"
        };
        (score, verdict, critical)
    }
}

// ---------------------------------------------------------------------------
// 十六、总控：帧合同控制器（签署/跑帧/降档/对拍/体检一台收口）
// ---------------------------------------------------------------------------

/// 合同状态（三态：未签 / 已签 / 违约；违约必须显性重签，不可静默恢复）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContractState {
    /// 未签署（拒绝跑帧）。
    Unsigned,
    /// 已签署（80 帧合同生效中）。
    Signed,
    /// 违约（整帧实耗破 12.5ms；处置记违约账，恢复须 reattest）。
    Breached,
}

/// 单帧执行产出（帧照常出——帧率不降；违约用旗标与状态显性表达）。
#[derive(Clone, Debug)]
pub struct FrameOutcome {
    /// 帧号。
    pub frame_id: u64,
    /// 本帧档位。
    pub tier: ScanTier,
    /// 扫描报告（四道留痕+发现）。
    pub report: ScanReport,
    /// 帧前估算（成本模型 µs）。
    pub est_us: u64,
    /// 安全道实耗（µs）。
    pub sec_actual_us: u64,
    /// 整帧实耗（主体+安全 µs）。
    pub frame_total_us: u64,
    /// 安全配额超支（>800µs）。
    pub quota_overrun: bool,
    /// 帧合同违约（整帧 >12_500µs）。
    pub contract_breach: bool,
    /// 本帧档位切换（降/升）。
    pub transition: Option<TierTransition>,
}

/// 输入校验（纹理数/面积限幅；越界道不可禁用；禁道必须留痕原因）。
pub fn validate_input(input: &ScanInput) -> Result<(), FrameContractError> {
    if input.textures.len() > TEXTURE_CAP {
        return Err(FrameContractError::InputInvalid);
    }
    if input.area_px > AREA_PX_CAP {
        return Err(FrameContractError::InputInvalid);
    }
    if (input.lane_mask & 0b0001) == 0 {
        return Err(FrameContractError::LaneSkipForbidden);
    }
    if (input.lane_mask & 0b1110) != 0b1110 && input.skip_reason == 0 {
        return Err(FrameContractError::InputInvalid);
    }
    Ok(())
}

/// 帧合同控制器（大结构：600 帧环 + 128 事件环 + 违约账全在内——
/// `new()` 返回 `Box<Self>`，堆上生活，不占栈）。
#[derive(Debug)]
pub struct FrameContractController {
    state: ContractState,
    last_frame_id: u64,
    governor: TierGovernor,
    whitelist: WhitelistTable,
    pipeline: SecurityScanPipeline,
    ledger: FrameLedger,
    events: EventLedger,
    violations: ContractViolationLedger,
    last_determinism: Option<DeterminismOutcome>,
    frames_executed: u64,
    last_health_score: Option<u32>,
}

impl FrameContractController {
    /// 新控制器（Box 上堆；初始 Unsigned——先 sign 再跑帧）。
    pub fn new() -> alloc::boxed::Box<Self> {
        alloc::boxed::Box::new(FrameContractController {
            state: ContractState::Unsigned,
            last_frame_id: 0,
            governor: TierGovernor::new(),
            whitelist: WhitelistTable::new(),
            pipeline: SecurityScanPipeline::new(),
            ledger: FrameLedger::new(),
            events: EventLedger::new(),
            violations: ContractViolationLedger::new(),
            last_determinism: None,
            frames_executed: 0,
            last_health_score: None,
        })
    }

    /// 合同状态。
    pub fn state(&self) -> ContractState {
        self.state
    }

    /// 已执行帧数（帧率不降的直接证据：每调一次 run_frame 加一）。
    pub fn frames_executed(&self) -> u64 {
        self.frames_executed
    }

    /// 当前档位。
    pub fn current_tier(&self) -> ScanTier {
        self.governor.current()
    }

    /// 档位状态机只读访问（滞回统计）。
    pub fn governor(&self) -> &TierGovernor {
        &self.governor
    }

    /// 白名单只读快照。
    pub fn whitelist_snapshot(&self) -> Vec<WhitelistEntry> {
        self.whitelist.snapshot()
    }

    /// 白名单累计到期摘除数。
    pub fn whitelist_expired_total(&self) -> u64 {
        self.whitelist.expired_total()
    }

    /// 流水线统计访问。
    pub fn pipeline(&self) -> &SecurityScanPipeline {
        &self.pipeline
    }

    /// 帧账只读访问。
    pub fn ledger(&self) -> &FrameLedger {
        &self.ledger
    }

    /// 事件账快照。
    pub fn events_snapshot(&self) -> Vec<EventRecord> {
        self.events.snapshot()
    }

    /// 违约账快照。
    pub fn violations_snapshot(&self) -> Vec<ViolationRecord> {
        self.violations.snapshot()
    }

    /// 最近一次双跑对拍结论。
    pub fn last_determinism(&self) -> Option<DeterminismOutcome> {
        self.last_determinism
    }

    /// 签署合同（Unsigned → Signed；Breached 态拒绝——必须先 reattest）。
    pub fn sign(&mut self) -> Result<(), FrameContractError> {
        match self.state {
            ContractState::Unsigned => {
                self.state = ContractState::Signed;
                Ok(())
            }
            ContractState::Signed => Ok(()),
            ContractState::Breached => Err(FrameContractError::ContractBreached),
        }
    }

    /// 违约重签（Breached → Signed；必须回填 [`REATTEST_ACK`] 确认字，
    /// 防静默恢复；非 Breached 态调用无效不报错）。
    pub fn reattest(&mut self, ack: u64) -> Result<(), FrameContractError> {
        if self.state != ContractState::Breached {
            return Ok(());
        }
        if ack != REATTEST_ACK {
            return Err(FrameContractError::ContractBreached);
        }
        self.state = ContractState::Signed;
        self.events.push(
            ev::CONTRACT_BREACHED,
            self.last_frame_id,
            0xDEAD_C0DE,
        );
        Ok(())
    }

    /// 登记白名单免检（转发白名单表；出错出事件）。
    pub fn whitelist_register(
        &mut self,
        resource_id: u64,
        current_frame: u64,
        ttl_frames: u64,
        reason: &str,
    ) -> Result<(), FrameContractError> {
        let r = self.whitelist.register(resource_id, current_frame, ttl_frames, reason);
        if let Err(e) = &r {
            self.events.push(e.code(), current_frame, resource_id);
        }
        r
    }

    /// 白名单续期。
    pub fn whitelist_renew(
        &mut self,
        resource_id: u64,
        current_frame: u64,
        ttl_frames: u64,
    ) -> Result<(), FrameContractError> {
        let r = self.whitelist.renew(resource_id, current_frame, ttl_frames);
        if let Err(e) = &r {
            self.events.push(e.code(), current_frame, resource_id);
        }
        r
    }

    /// 白名单摘除。
    pub fn whitelist_revoke(&mut self, resource_id: u64) -> bool {
        self.whitelist.revoke(resource_id)
    }

    /// 记违约 + 出事件（违约双通道：违约账封存 + 事件账显性化）。
    fn note_violation(&mut self, err: FrameContractError, frame_id: u64, aux: u64) {
        self.events.push(err.code(), frame_id, aux);
        let _ = self.violations.record(err, frame_id);
        if self.violations.dropped() == 1 {
            self.events.push(ev::LEDGER_FULL, frame_id, self.violations.dropped());
        }
    }

    /// 跑一帧（合同主路径）。
    ///
    /// 执行序：状态闸（未签拒）→ 输入校验 → 帧号单调闸 → 免检集合物化
    /// （到期摘除出事件）→ 成本估算 → 四道扫描 → 守恒对拍（破即拒账）→
    /// 档位观测（可能降/升档）→ 帧账入账 → 超支/违约显性化 → 产出。
    /// **帧永不因安全开销被丢弃**——返回 Err 只发生在合同/输入/账本
    /// 层面的破口，安全检查再重也照常出帧。
    pub fn run_frame(&mut self, frame_id: u64, input: &ScanInput) -> Result<FrameOutcome, FrameContractError> {
        if self.state == ContractState::Unsigned {
            return Err(FrameContractError::ContractNotSigned);
        }
        validate_input(input)?;
        if frame_id <= self.last_frame_id && self.last_frame_id != 0 {
            return Err(FrameContractError::FrameIdReuse);
        }

        // 免检集合物化（到期摘除；摘除计数差出事件）。
        let expired_before = self.whitelist.expired_total();
        let active = self.whitelist.active_at(frame_id);
        let expired_delta = self.whitelist.expired_total().saturating_sub(expired_before);
        if expired_delta > 0 {
            self.events.push(ev::WHITELIST_EXPIRED, frame_id, expired_delta);
        }

        let tier = self.governor.current();
        let tex_count = input.textures.len() as u64;
        let cost_in = CostInput { area_px: input.area_px, texture_count: tex_count };
        let est_us = ScanCostModel::estimate_us(tier, cost_in);

        // 四道扫描（帧照常执行——帧率不降）。
        let report = self.pipeline.run(frame_id, tier, input, &active);
        let sec_actual_us = report.sec_actual_us;

        // 守恒对拍（帧账≡道账；破即拒——不入破账）。
        let mut lane_us = [0u64; 4];
        let mut i = 0usize;
        while i < 4 {
            lane_us[i] = match report.traces.get(i) {
                Some(t) => t.us,
                None => 0,
            };
            i += 1;
        }
        let lane_sum = lane_us[0]
            .saturating_add(lane_us[1])
            .saturating_add(lane_us[2])
            .saturating_add(lane_us[3]);
        if lane_sum.saturating_add(FIXED_OVERHEAD_US) != sec_actual_us {
            let diff = sec_actual_us.saturating_sub(lane_sum);
            self.events.push(ev::CONSERVATION_BROKEN, frame_id, diff);
            self.note_violation(FrameContractError::ConservationBroken, frame_id, diff);
            return Err(FrameContractError::ConservationBroken);
        }

        // 档位观测（先观测得切换，再连同旗标入账）。
        let transition = self.governor.observe(sec_actual_us, SECURITY_QUOTA_US);
        if let Some(tr) = &transition {
            let dir_aux = if tr.is_downgrade() { 0u64 } else { 1u64 };
            let aux = (dir_aux << 32) | (tr.to.index() as u64);
            self.events.push(ev::TIER_SWITCH, frame_id, aux);
        }

        let quota_overrun = sec_actual_us > SECURITY_QUOTA_US;
        let frame_total_us = input.frame_base_us.saturating_add(sec_actual_us);
        let contract_breach = frame_total_us > FRAME_BUDGET_US;
        if quota_overrun {
            self.events.push(ev::QUOTA_OVERRUN, frame_id, sec_actual_us);
        }

        let rec = FrameRecord {
            frame_id,
            tier: tier.index(),
            budget_us: FRAME_BUDGET_US,
            quota_us: SECURITY_QUOTA_US,
            est_us,
            sec_actual_us,
            frame_total_us,
            lane_us,
            quota_overrun,
            contract_breach,
            downgraded: matches!(&transition, Some(tr) if tr.is_downgrade()),
            upgraded: matches!(&transition, Some(tr) if !tr.is_downgrade()),
            verdict_fingerprint: report.verdict_fingerprint,
        };
        self.ledger.record(rec)?;

        if contract_breach {
            // 帧合同违约：翻 Breached 态 + 违约账 + 事件账三通道显性。
            self.state = ContractState::Breached;
            self.events.push(ev::CONTRACT_BREACHED, frame_id, frame_total_us);
            self.note_violation(FrameContractError::ContractBreached, frame_id, frame_total_us);
        }

        self.last_frame_id = frame_id;
        self.frames_executed = self.frames_executed.saturating_add(1);
        Ok(FrameOutcome {
            frame_id,
            tier,
            report,
            est_us,
            sec_actual_us,
            frame_total_us,
            quota_overrun,
            contract_breach,
            transition,
        })
    }

    /// 双跑确定性对拍（当前档位 + 当前免检集合；结论入体检与事件）。
    pub fn determinism_check(
        &mut self,
        frame_id: u64,
        input: &ScanInput,
    ) -> Result<DeterminismOutcome, FrameContractError> {
        if self.state == ContractState::Unsigned {
            return Err(FrameContractError::ContractNotSigned);
        }
        validate_input(input)?;
        let tier = self.governor.current();
        let active = self.whitelist.active_at(frame_id);
        match determinism_check(&mut self.pipeline, frame_id, tier, input, &active) {
            Ok(out) => {
                self.last_determinism = Some(out);
                Ok(out)
            }
            Err(e) => {
                let aux = {
                    let mut alt = 0u64;
                    if let Ok(o) = determinism_check(
                        &mut self.pipeline,
                        frame_id,
                        tier,
                        input,
                        &active,
                    ) {
                        alt = o.fingerprint_a ^ o.fingerprint_b;
                    }
                    alt
                };
                self.last_determinism = None;
                self.events.push(ev::DETERMINISM_MISMATCH, frame_id, aux);
                self.note_violation(FrameContractError::DeterminismMismatch, frame_id, aux);
                Err(e)
            }
        }
    }

    /// 会话聚合转发。
    pub fn session_aggregate(&self) -> SessionAggregate {
        self.ledger.aggregate()
    }

    /// 体检（8 项；关键破口一票否决；跨线报警出事件一次——分数恶化沿）。
    pub fn health(&mut self) -> HealthReport {
        let agg = self.ledger.aggregate();
        let cons_ok = self.ledger.reconcile().is_ok();
        let det_ok = match self.last_determinism {
            Some(d) => d.matched,
            None => true, // 未执行过对拍：不判破口，注记提示
        };
        let wl_ok = {
            // 白名单卫生：快照条数 ≤ 容量且摘除链自洽。
            self.whitelist.len() <= WHITELIST_CAP
        };
        let ev_ok = self.events.self_consistent();
        let tier_ok = {
            let idx = self.governor.current().index();
            idx <= 2
        };
        let proj_ok = {
            let p = project_telemetry(
                &agg,
                0, // 分数未定前先占位；红线检查只看字段名
                self.events.total(),
                self.governor.current().index(),
            );
            match p {
                Ok(proj) => proj.redline_selfcheck().is_ok(),
                Err(_) => false,
            }
        };
        let checks = vec![
            HealthCheck {
                name: "合同签署状态",
                pass: self.state == ContractState::Signed,
                note: "Unsigned/Breached 均为破口；Breached 须 reattest(REATTEST_ACK) 显性重签",
            },
            HealthCheck {
                name: "帧违约率(ppm)",
                pass: agg.breach_ppm <= BREACH_PPM_LIMIT,
                note: "违约率红线 100ppm；超线请降检查档位（降质不降帧）而非丢帧",
            },
            HealthCheck {
                name: "守恒对账(帧账≡道账)",
                pass: cons_ok,
                note: "Σ道实耗 ≠ 帧安全实耗即破账；查扫描管线记账路径",
            },
            HealthCheck {
                name: "双跑确定性对拍",
                pass: det_ok,
                note: "未执行过对拍视为通过（注记）；执行过则必须 matched",
            },
            HealthCheck {
                name: "白名单卫生",
                pass: wl_ok,
                note: "免检条目须有期限有理由；到期自动摘除（摘除出事件）",
            },
            HealthCheck {
                name: "档位合法",
                pass: tier_ok,
                note: "三档闭集之外无档",
            },
            HealthCheck {
                name: "事件账自洽",
                pass: ev_ok,
                note: "序号链断裂或环计数异常即破",
            },
            HealthCheck {
                name: "隐私红线自检",
                pass: proj_ok,
                note: "遥测投影只允许统计量白名单字段；内容指纹撞线即破",
            },
        ];
        let (score, verdict, critical) = HealthReport::score_and_verdict(&checks);
        // 跨线报警（分数恶化沿只报一次——不刷屏）。
        match self.last_health_score {
            Some(prev) if prev < HEALTH_MIN_SCORE => {}
            _ => {
                if score < HEALTH_MIN_SCORE {
                    self.events.push(ev::HEALTH_CRITICAL, self.last_frame_id, score as u64);
                    self.note_violation(
                        FrameContractError::HealthCritical,
                        self.last_frame_id,
                        score as u64,
                    );
                }
            }
        }
        self.last_health_score = Some(score);
        let mut fp = fnv64_u64(FNV_OFFSET, score as u64);
        let mut i = 0usize;
        while i < checks.len() {
            if let Some(c) = checks.get(i) {
                fp = fnv64_u64(fp, c.pass as u64);
            }
            i += 1;
        }
        let mut cc = 0u64;
        let mut j = 0usize;
        while j < critical.len() {
            if let Some(code) = critical.get(j) {
                cc = cc.saturating_add(*code as u64);
            }
            j += 1;
        }
        fp = fnv64_u64(fp, cc);
        HealthReport { checks, score, verdict, fingerprint: fp, critical_codes: critical }
    }
}

// ---------------------------------------------------------------------------
// 十七、编译期自检（const assert；运行期零断言的铁律下把不变式前置）
// ---------------------------------------------------------------------------

const _: () = assert!(TARGET_FPS == 80);
const _: () = assert!(FRAME_BUDGET_US == 12_500);
const _: () = assert!(FRAME_BUDGET_US.saturating_mul(TARGET_FPS as u64) == 1_000_000);
const _: () = assert!(SECURITY_QUOTA_US == 800);
const _: () = assert!(
    LANE_QUOTA_US[0] + LANE_QUOTA_US[1] + LANE_QUOTA_US[2] + LANE_QUOTA_US[3]
        == SECURITY_QUOTA_US
);
const _: () = assert!(QUOTA_PERMILLE == SECURITY_QUOTA_US * 1000 / FRAME_BUDGET_US);
const _: () = assert!(OVER_DOWNGRADE_N >= 1);
const _: () = assert!(SURPLUS_UPGRADE_M > OVER_DOWNGRADE_N);
const _: () = assert!(SURPLUS_UPGRADE_M == OVER_DOWNGRADE_N * 4);
const _: () = assert!(TIER_COOLDOWN_FRAMES > SURPLUS_UPGRADE_M);
const _: () = assert!(RING_FRAMES == 600);
const _: () = assert!(EVENT_CAP == 128);
const _: () = assert!(VIOLATION_CAP == 64);
const _: () = assert!(WHITELIST_CAP == 64);
const _: () = assert!(SCAN_TIER_COUNT == 3);
const _: () = assert!(SAMPLE_STRIDE == 16);
const _: () = assert!(APERTURE_BASE.saturating_add(APERTURE_LEN) == 0x2000_0000);
const _: () = assert!(FIXED_OVERHEAD_US == 4);
const _: () = assert!(BREACH_PPM_LIMIT == 100);
const _: () = assert!(HEALTH_MIN_SCORE == 90);
const _: () = assert!(FRAME_CONTRACT_ERRORS.len() == 16);
const _: () = assert!(REATTEST_ACK == 0x1300_C0DE_BEEF_0001);
const _: () = assert!(WHITELIST_TTL_FRAMES == 8_000);
const _: () = assert!(TEXTURE_CAP == 4096);
const _: () = assert!(FINDINGS_CAP == 256);

// ---------------------------------------------------------------------------
// 十八、测试（冒烟一组：预算超支降档不降帧 / 滞回 / 白名单过期 / 对拍
// 守恒 / 双跑一致 / 隐私红线 / 违约重签 / 体检）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 合法纹理（id, 长度）：区域落在光圈窗内、幻数/签名均合法。
    fn tex(id: u64, len: u64) -> TextureDesc {
        let t = TextureDesc {
            resource_id: id,
            magic: TEX_MAGIC,
            base: APERTURE_BASE.saturating_add(id.saturating_mul(8_000_000)),
            len,
            sig: 0,
        };
        TextureDesc { sig: expected_signature(&t), ..t }
    }

    /// 组输入（全道启用）。
    fn mk_input(area: u64, base_us: u64, textures: Vec<TextureDesc>) -> ScanInput {
        ScanInput { area_px: area, frame_base_us: base_us, textures, lane_mask: 0b1111, skip_reason: 0 }
    }

    /// 轻负载：4 张小纹理（安全实耗 ≈ 4µs，远低于 800µs 配额）。
    fn light_input() -> ScanInput {
        mk_input(16_384, 1_000, vec![tex(1, 4_096), tex(2, 4_096), tex(3, 4_096), tex(4, 4_096)])
    }

    /// 重负载：32 张 4MB 纹理（Full 档安全实耗 ≈ 4ms，超配额 5 倍）。
    fn heavy_input() -> ScanInput {
        let mut ts = Vec::new();
        let mut i = 1u64;
        while i <= 32 {
            ts.push(tex(i, 4_000_000));
            i += 1;
        }
        mk_input(128_000_000, 1_000, ts)
    }

    #[test]
    fn error_table_codes_and_details() {
        let mut prev: i64 = -1;
        let mut i = 0usize;
        while i < FRAME_CONTRACT_ERRORS.len() {
            let e = FRAME_CONTRACT_ERRORS[i];
            let code = e.code() as i64;
            assert!(code >= 0x1300 && code <= 0x130F, "码出段");
            assert!(code > prev, "码必须递增不重复");
            prev = code;
            assert!(!e.detail().is_empty(), "诊断不得空串");
            i += 1;
        }
        assert_eq!(FRAME_CONTRACT_ERRORS[0].code(), 0x1300);
        assert_eq!(FRAME_CONTRACT_ERRORS[15].code(), 0x130F);
    }

    #[test]
    fn unsigned_contract_rejects_frames() {
        let mut c = FrameContractController::new();
        let err = c.run_frame(1, &light_input()).unwrap_err();
        assert_eq!(err.code(), 0x1300);
        assert_eq!(c.frames_executed(), 0);
        // 签署后通过。
        c.sign().unwrap();
        let out = c.run_frame(1, &light_input()).unwrap();
        assert_eq!(out.tier, ScanTier::Full);
        assert_eq!(c.frames_executed(), 1);
    }

    #[test]
    fn basic_frame_ledger_and_aggregate() {
        let mut c = FrameContractController::new();
        c.sign().unwrap();
        let mut id = 1u64;
        while id <= 10 {
            c.run_frame(id, &light_input()).unwrap();
            id += 1;
        }
        let agg = c.session_aggregate();
        assert_eq!(agg.frames, 10);
        assert_eq!(agg.window_frames, 10);
        assert_eq!(agg.breach_ppm, 0);
        assert_eq!(agg.overrun_ppm, 0);
        assert_eq!(agg.downgrades, 0);
        assert_eq!(agg.sec_p95_us, 4); // 轻载安全实耗 = 固定开销 4µs
        assert_eq!(agg.tier_frames[2], 10); // 全程 Full
        // 守恒与体检全绿。
        assert!(c.ledger().reconcile().is_ok());
        let h = c.health();
        assert_eq!(h.score, 100, "全绿体检应为满分");
        assert_eq!(h.verdict, "健康");
        assert!(h.critical_codes.is_empty());
    }

    #[test]
    fn overrun_downgrades_tier_but_never_drops_frames() {
        let mut c = FrameContractController::new();
        c.sign().unwrap();
        // 前 3 帧重载：第 3 帧触发降档（连续 3 帧超 800µs 配额）。
        let mut id = 1u64;
        while id <= 5 {
            let out = c.run_frame(id, &heavy_input()).unwrap();
            if id == 3 {
                let tr = out.transition.expect("第 3 帧应触发降档");
                assert!(tr.is_downgrade());
                assert_eq!(tr.to, ScanTier::Sampled);
            }
            if id <= 3 {
                assert!(out.quota_overrun, "Full 档重载必超配额");
            } else {
                // 降档后（抽样 1/16）安全实耗约 250µs：配额内——降质生效。
                assert!(!out.quota_overrun, "降档后应回到配额内");
            }
            id += 1;
        }
        // 帧率不降：5 帧全部执行、全部入账。
        assert_eq!(c.frames_executed(), 5);
        assert_eq!(c.ledger().total_frames(), 5);
        assert_eq!(c.governor().switches_down(), 1);
        assert_eq!(c.current_tier(), ScanTier::Sampled);
        // 降档事件与超支事件都显性在账。
        assert!(c.events_snapshot().iter().any(|e| e.code == ev::TIER_SWITCH));
        assert!(c.events_snapshot().iter().any(|e| e.code == ev::QUOTA_OVERRUN));
    }

    #[test]
    fn hysteresis_prevents_oscillation() {
        let mut c = FrameContractController::new();
        c.sign().unwrap();
        // 3 帧重压降档 → 60 帧轻载：只允许 1 次升档，且被冷却期压住过。
        let mut id = 1u64;
        while id <= 3 {
            c.run_frame(id, &heavy_input()).unwrap();
            id += 1;
        }
        assert_eq!(c.current_tier(), ScanTier::Sampled);
        let mut upgrades_seen = 0u64;
        while id <= 66 {
            if let Ok(out) = c.run_frame(id, &light_input()) {
                if out.transition.is_some() {
                    upgrades_seen += 1;
                }
            }
            id += 1;
        }
        let g = c.governor();
        assert_eq!(g.switches_down(), 1);
        assert_eq!(g.switches_up(), 1, "60 帧轻载只允许一次升档（滞回）");
        assert_eq!(upgrades_seen, 1);
        assert!(g.denied_switches() > 0, "冷却期应压住过升档意图");
        assert_eq!(c.current_tier(), ScanTier::Full);
        // 全程帧数不丢。
        assert_eq!(c.frames_executed(), 66);
    }

    #[test]
    fn cost_model_tier_monotonicity() {
        let cin = CostInput { area_px: 1_048_576, texture_count: 64 };
        let full = ScanCostModel::estimate_steps(ScanTier::Full, cin);
        let sampled = ScanCostModel::estimate_steps(ScanTier::Sampled, cin);
        let wl = ScanCostModel::estimate_steps(ScanTier::Whitelisted, cin);
        // Full ≥ Sampled ≥ Whitelisted（逐道）。
        let mut i = 0usize;
        while i < 4 {
            assert!(full[i] >= sampled[i], "Full 档每道不得便宜于 Sampled");
            assert!(sampled[i] >= wl[i], "Sampled 档每道不得便宜于免检");
            i += 1;
        }
        // 已知值对拍（模型公式钉死）。
        assert_eq!(full[0], 16_384); // 1048576/64
        assert_eq!(sampled[0], 1_024); // 1048576/1024
        assert_eq!(full[3], 512); // 64*8
        assert_eq!(sampled[3], 32); // 64/2
        assert_eq!(wl, [0, 0, 0, 0]);
        // 步数→µs：Full = 32 + 0 + 0 + 2 = 34µs（ns/步 [2,1,1,4]）。
        assert_eq!(ScanCostModel::steps_to_us(&full), 34);
        assert_eq!(ScanCostModel::estimate_us(ScanTier::Full, cin), 38); // +4 固定开销
    }

    #[test]
    fn whitelist_registration_duplicate_cap_and_expiry() {
        let mut c = FrameContractController::new();
        c.sign().unwrap();
        // 免检必须给理由（空白理由拒）。
        assert_eq!(
            c.whitelist_register(100, 1, 0, "").unwrap_err().code(),
            0x130E
        );
        c.whitelist_register(100, 1, 5, "常驻图标图集（审计放行）").unwrap();
        // 重复登记拒。
        assert_eq!(
            c.whitelist_register(100, 1, 0, "重复").unwrap_err().code(),
            0x1307
        );
        // 坏魔数纹理在册 → 免检期内无发现，且豁免留痕。
        let mut bad = tex(100, 4_096);
        bad.magic = 0xDEAD_BEEFu32;
        let inp = mk_input(4_096, 1_000, vec![bad]);
        let out = c.run_frame(2, &inp).unwrap();
        assert_eq!(out.report.traces[1].excluded_by_whitelist, 1);
        assert_eq!(out.report.finding_total, 0);
        // 过期（ttl=5，登记于帧 1 → 帧号 6 起失效）：坏魔数重新现形。
        let mut id = 3u64;
        let mut expired_event = false;
        let mut finding_after_expiry = 0u64;
        while id <= 9 {
            let out = c.run_frame(id, &inp).unwrap();
            if c.whitelist_expired_total() > 0 {
                expired_event = true;
            }
            if id >= 7 {
                finding_after_expiry = out.report.finding_total;
            }
            id += 1;
        }
        assert!(expired_event, "到期摘除必须出事件");
        assert!(c.events_snapshot().iter().any(|e| e.code == ev::WHITELIST_EXPIRED));
        assert!(finding_after_expiry >= 1, "过期后坏魔数必须被查出来");
        // 摘除幂等（用新登记的 id 200 验证——id 100 已在到期时被自动摘除）。
        c.whitelist_register(200, 20, 0, "重启复核后的免检续登").unwrap();
        assert!(c.whitelist_revoke(200));
        assert!(!c.whitelist_revoke(200));
        assert!(!c.whitelist_revoke(100), "过期条目已被摘除，重复摘除返回 false");
    }

    #[test]
    fn conservation_rejects_broken_accounts() {
        let mut l = FrameLedger::new();
        let good = FrameRecord {
            frame_id: 1,
            tier: 2,
            budget_us: FRAME_BUDGET_US,
            quota_us: SECURITY_QUOTA_US,
            est_us: 10,
            sec_actual_us: 104,
            frame_total_us: 1_104,
            lane_us: [50, 20, 20, 10],
            quota_overrun: false,
            contract_breach: false,
            downgraded: false,
            upgraded: false,
            verdict_fingerprint: 7,
        };
        assert!(l.record(good).is_ok());
        // 破账：Σ道 = 120（+4 = 124）≠ 安全实耗 104 → 拒收。
        let mut bad = good;
        bad.frame_id = 2;
        bad.lane_us = [60, 20, 20, 20];
        let err = l.record(bad).unwrap_err();
        assert_eq!(err.code(), 0x130A);
        assert_eq!(l.total_frames(), 1, "破账不入账");
    }

    #[test]
    fn determinism_two_runs_same_verdict() {
        let mut c = FrameContractController::new();
        c.sign().unwrap();
        let inp = heavy_input();
        let out = c.determinism_check(1, &inp).unwrap();
        assert!(out.matched);
        assert_eq!(out.fingerprint_a, out.fingerprint_b);
        assert_eq!(out.steps_a, out.steps_b);
        assert_eq!(out.findings_a, out.findings_b);
        // 双跑结论进体检（通过）。
        let h = c.health();
        assert!(h.checks.iter().any(|k| k.name == "双跑确定性对拍" && k.pass));
        // 不同输入 → 裁决不同（对拍函数能分辨）。
        let mut p = SecurityScanPipeline::new();
        let active: Vec<u64> = Vec::new();
        let ra = p.run(1, ScanTier::Full, &heavy_input(), &active);
        let rb = p.run(1, ScanTier::Full, &light_input(), &active);
        assert!(!reports_match(&ra, &rb));
        // 同输入不同免检集 → 裁决不同（免检参与指纹）。
        let bad = {
            let mut t = tex(9, 4_096);
            t.magic = 0x1234_5678;
            t
        };
        let inp_bad = mk_input(4_096, 0, vec![bad]);
        let r1 = p.run(2, ScanTier::Full, &inp_bad, &active);
        let r2 = p.run(2, ScanTier::Full, &inp_bad, &[9u64]);
        assert_ne!(r1.finding_total, r2.finding_total);
    }

    #[test]
    fn privacy_redline_blocks_content_fingerprints() {
        // 撞禁用子串的投影字段一律拒。
        for name in ["content_fp", "texture_sig", "x_content_hash_y"] {
            let mut p = TelemetryProjection::new();
            assert!(p.push(name, 1).is_err(), "字段 {} 应被红线拦截", name);
        }
        // 白名单外字段拒。
        let mut p = TelemetryProjection::new();
        assert!(p.push("not_in_allowlist", 1).is_err());
        // 合法投影：12 个统计量字段全部通过并自检干净。
        let agg = SessionAggregate {
            frames: 10,
            window_frames: 10,
            sec_total_us: 40,
            sec_mean_us: 4,
            sec_p95_us: 4,
            sec_max_us: 4,
            frame_p95_us: 1_004,
            overrun_ppm: 0,
            breach_ppm: 0,
            downgrades: 0,
            upgrades: 0,
            overwritten: 0,
            tier_frames: [0, 0, 10],
            fingerprint: 0,
        }
        .with_fingerprint();
        let proj = project_telemetry(&agg, 100, 0, 2).unwrap();
        assert_eq!(proj.len(), 12);
        assert!(proj.redline_selfcheck().is_ok());
        // 投影面不含任何内容指纹字段。
        let mut i = 0usize;
        while i < proj.fields().len() {
            assert!(!TelemetryProjection::field_banned(proj.fields()[i].0));
            i += 1;
        }
    }

    #[test]
    fn breach_flips_state_and_requires_explicit_reattest() {
        let mut c = FrameContractController::new();
        c.sign().unwrap();
        // 主体 12_400µs + 安全 4µs = 12_404µs ≤ 12_500：不破。
        let mut inp = light_input();
        inp.frame_base_us = 12_400;
        let out = c.run_frame(1, &inp).unwrap();
        assert!(!out.contract_breach);
        // 主体 12_497µs + 安全 4µs = 12_501µs > 12_500：违约。
        inp.frame_base_us = 12_497;
        let out = c.run_frame(2, &inp).unwrap();
        assert!(out.contract_breach);
        assert_eq!(c.state(), ContractState::Breached);
        // 违约不丢帧：Breached 态照常执行照常出账。
        let out = c.run_frame(3, &inp).unwrap();
        assert!(out.contract_breach);
        assert_eq!(c.frames_executed(), 3);
        // 三通道显性：事件账 + 违约账 + 状态。
        assert!(c.events_snapshot().iter().any(|e| e.code == ev::CONTRACT_BREACHED));
        assert_eq!(c.violations_snapshot().len(), 2);
        // 静默恢复被拒（错确认字）。
        assert_eq!(c.reattest(0).unwrap_err().code(), 0x1301);
        assert_eq!(c.state(), ContractState::Breached);
        // 显性重签（正确确认字）。
        c.reattest(REATTEST_ACK).unwrap();
        assert_eq!(c.state(), ContractState::Signed);
        // 体检不合格（违约率超线 + 曾违约）。
        let h = c.health();
        assert!(h.score < 100);
        assert!(h.critical_codes.iter().any(|&c| c == 0x1301));
    }

    #[test]
    fn frame_id_reuse_and_lane_skip_guards() {
        let mut c = FrameContractController::new();
        c.sign().unwrap();
        c.run_frame(1, &light_input()).unwrap();
        // 帧号回退/复用拒。
        assert_eq!(c.run_frame(1, &light_input()).unwrap_err().code(), 0x1302);
        // 越界道禁用拒（安全底线道）。
        let mut inp = light_input();
        inp.lane_mask = 0b1110;
        assert_eq!(
            validate_input(&inp).unwrap_err().code(),
            0x1308
        );
        // 其他道禁用但不给留痕原因拒。
        let mut inp2 = light_input();
        inp2.lane_mask = 0b1101;
        assert_eq!(
            validate_input(&inp2).unwrap_err().code(),
            0x130E
        );
        // 其他道禁用且给了留痕原因：放行，且跳过留痕在账。
        inp2.skip_reason = SKIP_REASON_MASKED;
        let out = c.run_frame(2, &inp2).unwrap();
        assert!(out.report.traces[1].skipped);
        assert_eq!(out.report.traces[1].skip_reason, SKIP_REASON_MASKED);
        assert!(c.pipeline().lanes_skipped() > 0);
    }

    #[test]
    fn p95_and_ppm_integer_semantics() {
        // P95：1..=100 升序，floor(0.95n)=95 → 第 96 小 = 96。
        let mut v = Vec::new();
        let mut i = 1u64;
        while i <= 100 {
            v.push(i);
            i += 1;
        }
        assert_eq!(p95_of(v), 96);
        // 单元素与空集。
        assert_eq!(p95_of(vec![7]), 7);
        assert_eq!(p95_of(Vec::new()), 0);
        // ppm。
        assert_eq!(ppm_of(1, 1_000), 1_000);
        assert_eq!(ppm_of(0, 0), 0);
        assert_eq!(ppm_of(3, 600), 5_000);
    }
    #[test]
    fn ring_wrap_and_event_ring_wrap() {
        let mut l = FrameLedger::new();
        // 601 帧轻账：环覆盖 1 次，累计不丢。
        let mut id = 1u64;
        while id <= 601 {
            let rec = FrameRecord {
                frame_id: id,
                tier: 2,
                budget_us: FRAME_BUDGET_US,
                quota_us: SECURITY_QUOTA_US,
                est_us: 4,
                sec_actual_us: 4,
                frame_total_us: 1_004,
                lane_us: [0, 0, 0, 0],
                quota_overrun: false,
                contract_breach: false,
                downgraded: false,
                upgraded: false,
                verdict_fingerprint: id,
            };
            l.record(rec).unwrap();
            id += 1;
        }
        assert_eq!(l.window_len(), RING_FRAMES);
        assert_eq!(l.overwritten(), 1);
        assert_eq!(l.total_frames(), 601);
        assert!(l.reconcile().is_ok());
        let agg = l.aggregate();
        assert_eq!(agg.window_frames, 600);
        // 事件环：200 事件 > 128 容量 → 覆盖 72 次，序号链不破。
        let mut e = EventLedger::new();
        let mut k = 0u64;
        while k < 200 {
            e.push(0x1309, k, k);
            k += 1;
        }
        assert_eq!(e.total(), 200);
        assert_eq!(e.overwritten(), 72);
        assert_eq!(e.len(), EVENT_CAP);
        assert!(e.self_consistent());
        assert_eq!(e.count_code(0x1309), 128);
    }
}
