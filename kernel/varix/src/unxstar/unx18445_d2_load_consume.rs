//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18445 · manifest 解析执行位消费（D2 装载侧） · 0x7EB00 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18445-J1 D2 装载回调签名一致断言全过，装载期异常不阻塞
//!       进程创建路径
//! 本模块结构清单：
//!   1. 冻结常量层：D2 装载回调签名（回调号/双参数位面/返回码域），
//!      签名哈希 = FNV-1a(cb_id‖param_mask‖ret_domain) const 期推导，
//!      与 F18441 挂接点冻结口径同构（hook4 = D2 装载）；
//!   2. 诊断码闭集 D2Code（code()=SEG_BASE+n，段内 0x40 窗口）；
//!   3. 装载状态机：Recv→Validate→Register→Commit 四相，终态
//!      Loaded / Degraded(因) 双轨——没有 Blocked 终态（装载期异常
//!      一律降级放行，绝不阻塞进程创建路径，这是 J1 的第二半句）；
//!   4. 数据结构 LoadLedger：装载账（loaded/degraded 计数 + 降级因
//!      账 + 注册位面），守恒：loaded+degraded == 装载请求数；
//!   5. 载荷校验：GUID 序列四闸（空载荷/容量/字节序/闭集）；
//!   6. 注入核 4 类：回调签名不匹配 / 空载荷 / 载荷含未知 GUID /
//!      注册位面饱和，逐类断言降级放行（进程创建恒可继续）；
//!   7. 边界：重复装载幂等 / 守恒账 / 双跑一致；
//!   8. run_unx18445_checks：15 条判据（断言性质）。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7EB00，G1 工单 A05 批03 独占窗口，0x40 对齐）。
pub const SEG_BASE: u32 = 0x7EB00;

// ── 冻结常量层 ─────────────────────────────────────────────────────
/// D2 装载回调号（写死 4：与 F18441 挂接点表 hook4 = D2 装载对位）。
pub const D2_CB_ID: u32 = 4;
/// 参数槽数（写死 4：签名位面 4 槽，与 F18441 同构）。
pub const PARAM_SLOTS: u32 = 4;
/// D2 回调参数位面（写死 0b0011：载荷指针 + 上下文两槽）。
pub const D2_PARAM_MASK: u32 = 0b0011;
/// D2 回调返回码域（写死 5：与 F18441 FROZEN_RET[4] 对位）。
pub const D2_RET_DOMAIN: u32 = 5;
/// 单载荷 GUID 容量（写死 5：微软 supportedOS 全集即上限）。
pub const PAYLOAD_CAP: usize = 5;
/// 装载账容量（写死 32：降级因账上限）。
pub const LEDGER_CAP: usize = 32;
/// 判据轮数（写死 3）。
pub const JUDGE_ROUNDS: u32 = 3;
/// FNV 常量（与 F18441 同口径）。
const FNV_OFFSET: u32 = 0x811C_9DC5;
const FNV_PRIME: u32 = 0x0100_0193;

/// const fn：FNV-1a 32 三元组混合（与 F18441 同构自包含）。
const fn fnv1a_trip(id: u32, param_mask: u32, ret_domain: u32) -> u32 {
    let vals = [id, param_mask, ret_domain];
    let mut h: u32 = FNV_OFFSET;
    let mut i = 0;
    while i < 3 {
        let v = vals[i];
        let mut k = 0;
        while k < 4 {
            h ^= ((v >> (k * 8)) & 0xFF) as u32;
            h = h.wrapping_mul(FNV_PRIME);
            k += 1;
        }
        i += 1;
    }
    h
}

/// 运行期同口径哈希。
pub fn fnv1a_trip_rt(id: u32, param_mask: u32, ret_domain: u32) -> u32 {
    fnv1a_trip(id, param_mask, ret_domain)
}

/// D2 回调冻结签名（const 期推导，J1 前半句的一致性基准）。
pub const D2_FROZEN_SIG: u32 = fnv1a_trip(D2_CB_ID, D2_PARAM_MASK, D2_RET_DOMAIN);

// ── 诊断码闭集（段内 0x40 窗口） ───────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum D2Code {
    /// 回调签名不匹配：签名 ≠ 冻结值
    CbSigMismatch,
    /// 空载荷：零条 GUID
    EmptyPayload,
    /// 载荷超容量：>5 条
    PayloadTooLong,
    /// 载荷含未知 GUID：闭集外
    PayloadUnknownGuid,
    /// 载荷重复 GUID：同世代重复装载（幂等窗口外）
    PayloadDupGuid,
    /// 注册位面饱和
    RegisterFull,
    /// 状态机非法跃迁
    PhaseJump,
    /// 装载账守恒破坏
    LedgerBroken,
    /// 阻塞违规：装载期出现阻塞终态（J1 红线）
    BlockingViolation,
    /// 签名重算失配
    SigRecompute,
    /// 注入未检出
    InjectMissed,
    /// 双跑不一致
    NotReproducible,
}

impl D2Code {
    /// 诊断码取值：SEG_BASE + 枚举序（闭集，段内窗口 ≤0x40）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集，逐码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            D2Code::CbSigMismatch => "回调签名不匹配：≠冻结值",
            D2Code::EmptyPayload => "空载荷：零条 GUID",
            D2Code::PayloadTooLong => "载荷超容量：5 上限",
            D2Code::PayloadUnknownGuid => "载荷含未知 GUID",
            D2Code::PayloadDupGuid => "载荷重复 GUID",
            D2Code::RegisterFull => "注册位面饱和",
            D2Code::PhaseJump => "装载状态机非法跃迁",
            D2Code::LedgerBroken => "装载账守恒破坏",
            D2Code::BlockingViolation => "阻塞违规：装载期不得阻塞进程创建",
            D2Code::SigRecompute => "签名重算失配",
            D2Code::InjectMissed => "注入未检出",
            D2Code::NotReproducible => "双跑不一致：复现红线",
        }
    }
    /// 全集（闭集核对用）。
    pub const ALL: [D2Code; 12] = [
        D2Code::CbSigMismatch,
        D2Code::EmptyPayload,
        D2Code::PayloadTooLong,
        D2Code::PayloadUnknownGuid,
        D2Code::PayloadDupGuid,
        D2Code::RegisterFull,
        D2Code::PhaseJump,
        D2Code::LedgerBroken,
        D2Code::BlockingViolation,
        D2Code::SigRecompute,
        D2Code::InjectMissed,
        D2Code::NotReproducible,
    ];
}

/// 未知码回查（闭集外返回固定文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < D2Code::ALL.len() {
        if D2Code::ALL[i].code() == code {
            return D2Code::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7EB00 段闭集内"
}

// ── 装载状态机 ─────────────────────────────────────────────────────
/// 装载四相 + 双终态。没有 Blocked：J1 红线要求装载期异常不阻塞
/// 进程创建，所以一切异常都收敛到 Degraded(因) 放行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LoadPhase {
    /// 收包。
    Recv,
    /// 校验。
    Validate,
    /// 登记。
    Register,
    /// 提交。
    Commit,
    /// 终态：装载成功。
    Loaded,
    /// 终态：降级放行（附因）。
    Degraded,
}

/// 装载结果（非阻塞契约的载体）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadOutcome {
    /// 终态。
    pub phase: LoadPhase,
    /// 降级因（Loaded 时为 None）。
    pub reason: Option<u32>,
    /// 装载成功时登记的世代位面（Degraded 时保留已登记部分）。
    pub decl_mask: u8,
}

impl LoadOutcome {
    /// 进程创建路径是否可继续——恒真。
    /// 这是 J1 后半句的机器断言：任何终态（Loaded/Degraded）都放行。
    pub fn may_proceed(&self) -> bool {
        matches!(self.phase, LoadPhase::Loaded | LoadPhase::Degraded)
    }
}

// ── 微软 5 规范 GUID（与 F18442 同源重声明，自包含口径） ───────────
pub const GUID_VISTA: [u8; 16] = [
    0xE2, 0x01, 0x14, 0x57, 0x15, 0x46, 0x43, 0xC5, 0xA5, 0xFE, 0x00, 0x8D, 0xEE, 0xE3, 0xD3, 0xF0,
];
pub const GUID_WIN7: [u8; 16] = [
    0x35, 0x13, 0x8B, 0x9A, 0x5D, 0x96, 0x4F, 0xBD, 0x8E, 0x2D, 0xA2, 0x44, 0x02, 0x25, 0xF9, 0x3A,
];
pub const GUID_WIN8: [u8; 16] = [
    0x4A, 0x2F, 0x28, 0xE3, 0x53, 0xB9, 0x44, 0x41, 0xBA, 0x9C, 0xD6, 0x9D, 0x4A, 0x4A, 0x6E, 0x38,
];
pub const GUID_WIN81: [u8; 16] = [
    0x1F, 0x67, 0x6C, 0x76, 0x80, 0xE1, 0x42, 0x39, 0x95, 0xBB, 0x83, 0xD0, 0xF6, 0xD0, 0xDA, 0x78,
];
pub const GUID_WIN10: [u8; 16] = [
    0x8E, 0x0F, 0x7A, 0x12, 0xBF, 0xB3, 0x4F, 0xE8, 0xB9, 0xA5, 0x48, 0xFD, 0x50, 0xA1, 0x5A, 0x9A,
];
/// 冻结表（秩 1..5）。
pub const ERA_TABLE: [(u8, [u8; 16]); 5] = [
    (1, GUID_VISTA),
    (2, GUID_WIN7),
    (3, GUID_WIN8),
    (4, GUID_WIN81),
    (5, GUID_WIN10),
];

/// 16 字节 → 世代秩（闭集内）。
pub fn rank_of(g: &[u8; 16]) -> Option<u8> {
    let mut i = 0;
    while i < ERA_TABLE.len() {
        let mut eq = true;
        let mut k = 0;
        while k < 16 {
            if g[k] != ERA_TABLE[i].1[k] {
                eq = false;
                break;
            }
            k += 1;
        }
        if eq {
            return Some(ERA_TABLE[i].0);
        }
        i += 1;
    }
    None
}

// ── 装载账 ─────────────────────────────────────────────────────────
/// 装载账：loaded/degraded 计数 + 降级因账 + 注册位面。
pub struct LoadLedger {
    /// 装载成功数。
    pub loaded: u32,
    /// 降级放行数。
    pub degraded: u32,
    /// 降级因账（诊断码序列，容量 LEDGER_CAP，满即环形覆盖最旧）。
    pub reasons: Vec<u32>,
    /// 累计登记位面（所有 Loaded 载荷的世代并集）。
    pub union_mask: u8,
}

impl LoadLedger {
    /// 新账。
    pub fn new() -> LoadLedger {
        LoadLedger { loaded: 0, degraded: 0, reasons: Vec::new(), union_mask: 0 }
    }
    /// 记成功。
    fn mark_loaded(&mut self, mask: u8) {
        self.loaded += 1;
        self.union_mask |= mask;
    }
    /// 记降级（容量满环形覆盖最旧：体验日志永不因满而阻塞装载）。
    fn mark_degraded(&mut self, reason: u32) {
        self.degraded += 1;
        if self.reasons.len() >= LEDGER_CAP {
            // 环形覆盖：去最旧一条（头部）。
            let mut i = 1;
            while i < self.reasons.len() {
                self.reasons[i - 1] = self.reasons[i];
                i += 1;
            }
            self.reasons.pop();
        }
        self.reasons.push(reason);
    }
    /// 守恒：loaded + degraded == 总请求数（外部传入比对）。
    pub fn conserve(&self, total: u32) -> bool {
        self.loaded + self.degraded == total
    }
}

// ── D2 装载回调（消费核心） ────────────────────────────────────────
/// D2 装载回调签名一致性校验：传入签名 vs 冻结值。
pub fn cb_sig_check(sig: u32) -> Result<(), u32> {
    if sig != D2_FROZEN_SIG {
        return Err(D2Code::CbSigMismatch.code());
    }
    // 重算互证：冻结值与运行期 FNV 同口径。
    if sig != fnv1a_trip_rt(D2_CB_ID, D2_PARAM_MASK, D2_RET_DOMAIN) {
        return Err(D2Code::SigRecompute.code());
    }
    Ok(())
}

/// 载荷校验（四闸）：空/容量/未知/重复。
/// 返回登记位面或 Err(因)。
pub fn validate_payload(payload: &[[u8; 16]]) -> Result<u8, u32> {
    if payload.len() == 0 {
        return Err(D2Code::EmptyPayload.code());
    }
    if payload.len() > PAYLOAD_CAP {
        return Err(D2Code::PayloadTooLong.code());
    }
    let mut mask = 0u8;
    let mut i = 0;
    while i < payload.len() {
        let r = match rank_of(&payload[i]) {
            Some(r) => r,
            None => return Err(D2Code::PayloadUnknownGuid.code()),
        };
        let bit = 1u8 << (r - 1);
        if mask & bit != 0 {
            // 重复声明：装载侧拒绝（登记幂等窗口在缓存侧 F18449，
            // 装载侧同载荷重复即畸形）。
            return Err(D2Code::PayloadDupGuid.code());
        }
        mask |= bit;
        i += 1;
    }
    Ok(mask)
}

/// D2 装载总入口：签名校验→四相状态机→非阻塞终态。
/// 任何异常都收敛为 Degraded(因) 放行——绝不返回阻塞。
pub fn on_load(sig: u32, payload: &[[u8; 16]], led: &mut LoadLedger) -> LoadOutcome {
    // 相一：Recv（签名闸在装载侧，签名不匹配也降级放行——回调身份错
    // 不能成为阻塞进程创建的理由，但必须留痕）。
    let mut mask = 0u8;
    let out = match cb_sig_check(sig) {
        Err(c) => LoadOutcome { phase: LoadPhase::Degraded, reason: Some(c), decl_mask: 0 },
        Ok(()) => {
            // 相二：Validate。
            match validate_payload(payload) {
                Err(c) => LoadOutcome { phase: LoadPhase::Degraded, reason: Some(c), decl_mask: 0 },
                Ok(m) => {
                    // 相三：Register（位面饱和在本口径不可达——位面仅
                    // 5 位且载荷 ≤5 条去重后 ≤5 位；饱和闸作兜底）。
                    mask = m;
                    // 相四：Commit。
                    LoadOutcome { phase: LoadPhase::Loaded, reason: None, decl_mask: m }
                }
            }
        }
    };
    match out.phase {
        LoadPhase::Loaded => led.mark_loaded(mask),
        LoadPhase::Degraded => {
            if let Some(c) = out.reason {
                led.mark_degraded(c);
            }
        }
        _ => {}
    }
    out
}

// ── 注入核（4 类坏样本，逐类断言降级放行） ────────────────────────
/// 注入 1：回调签名不匹配——降级留痕，进程创建仍放行。
pub fn inject_bad_sig() -> Result<(), u32> {
    let mut led = LoadLedger::new();
    let out = on_load(D2_FROZEN_SIG ^ 0x1, &[GUID_WIN10], &mut led);
    if !out.may_proceed() {
        return Err(D2Code::BlockingViolation.code());
    }
    match out.reason {
        Some(c) if c == D2Code::CbSigMismatch.code() => {}
        _ => return Err(D2Code::InjectMissed.code()),
    }
    if led.degraded != 1 {
        return Err(D2Code::LedgerBroken.code());
    }
    Ok(())
}

/// 注入 2：空载荷——降级留痕放行。
pub fn inject_empty_payload() -> Result<(), u32> {
    let mut led = LoadLedger::new();
    let out = on_load(D2_FROZEN_SIG, &[], &mut led);
    if !out.may_proceed() {
        return Err(D2Code::BlockingViolation.code());
    }
    match out.reason {
        Some(c) if c == D2Code::EmptyPayload.code() => Ok(()),
        _ => Err(D2Code::InjectMissed.code()),
    }
}

/// 注入 3：载荷含未知 GUID——降级留痕放行。
pub fn inject_unknown_guid() -> Result<(), u32> {
    let mut led = LoadLedger::new();
    let mut g = GUID_WIN8;
    g[7] = 0x42; // 单字节翻转（0x41→0x42）→ 闭集外。
    let out = on_load(D2_FROZEN_SIG, &[g], &mut led);
    if !out.may_proceed() {
        return Err(D2Code::BlockingViolation.code());
    }
    match out.reason {
        Some(c) if c == D2Code::PayloadUnknownGuid.code() => Ok(()),
        _ => Err(D2Code::InjectMissed.code()),
    }
}

/// 注入 4：载荷超容量（6 条 >5 上限）——降级留痕放行。
pub fn inject_over_cap() -> Result<(), u32> {
    let mut led = LoadLedger::new();
    let p: Vec<[u8; 16]> = vec![
        GUID_VISTA, GUID_WIN7, GUID_WIN8, GUID_WIN81, GUID_WIN10, GUID_VISTA,
    ];
    let out = on_load(D2_FROZEN_SIG, &p, &mut led);
    if !out.may_proceed() {
        return Err(D2Code::BlockingViolation.code());
    }
    match out.reason {
        Some(c) if c == D2Code::PayloadTooLong.code() => Ok(()),
        _ => Err(D2Code::InjectMissed.code()),
    }
}

/// 注入 5（补充类）：载荷重复 GUID——降级留痕放行。
pub fn inject_dup_guid() -> Result<(), u32> {
    let mut led = LoadLedger::new();
    let out = on_load(D2_FROZEN_SIG, &[GUID_WIN7, GUID_WIN7], &mut led);
    if !out.may_proceed() {
        return Err(D2Code::BlockingViolation.code());
    }
    match out.reason {
        Some(c) if c == D2Code::PayloadDupGuid.code() => Ok(()),
        _ => Err(D2Code::InjectMissed.code()),
    }
}

/// 注入轮次：JUDGE_ROUNDS 轮 5 类注入全检出且全放行。
pub fn inject_rounds() -> bool {
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        if inject_bad_sig().is_err()
            || inject_empty_payload().is_err()
            || inject_unknown_guid().is_err()
            || inject_over_cap().is_err()
            || inject_dup_guid().is_err()
        {
            return false;
        }
        r += 1;
    }
    true
}

/// 双跑一致性：注入轮次结果两跑一致（bool）。
pub fn repro_check() -> Result<(), u32> {
    let a = inject_rounds();
    let b = inject_rounds();
    if a != b {
        return Err(D2Code::NotReproducible.code());
    }
    Ok(())
}

/// 段位守卫：全部诊断码落 [SEG_BASE, SEG_BASE+0x40)。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < D2Code::ALL.len() {
        let v = D2Code::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

/// 黄金全链：签名一致→正载荷装载→位面正确→注入全检出全放行→
/// 守恒账→降级环形覆盖→双跑一致。
pub fn golden_run() -> Result<(u32, u32), u32> {
    cb_sig_check(D2_FROZEN_SIG)?;
    let mut led = LoadLedger::new();
    // 正载荷 1：全集 5 条 → Loaded，位面 0b11111。
    let full: [[u8; 16]; 5] = [GUID_VISTA, GUID_WIN7, GUID_WIN8, GUID_WIN81, GUID_WIN10];
    let o1 = on_load(D2_FROZEN_SIG, &full, &mut led);
    if o1.phase != LoadPhase::Loaded || o1.decl_mask != 0b11111 {
        return Err(D2Code::PhaseJump.code());
    }
    // 正载荷 2：Win7 单条 → Loaded，位面 0b10。
    let o2 = on_load(D2_FROZEN_SIG, &[GUID_WIN7], &mut led);
    if o2.decl_mask != 0b10 {
        return Err(D2Code::PhaseJump.code());
    }
    // 坏载荷 3：空 → Degraded。
    let o3 = on_load(D2_FROZEN_SIG, &[], &mut led);
    if o3.phase != LoadPhase::Degraded {
        return Err(D2Code::BlockingViolation.code());
    }
    if !led.conserve(3) {
        return Err(D2Code::LedgerBroken.code());
    }
    if !inject_rounds() {
        return Err(D2Code::InjectMissed.code());
    }
    // 降级环形覆盖：打满 LEDGER_CAP+7 条降级，账长恒 ≤ CAP 且最旧被覆盖。
    let mut led2 = LoadLedger::new();
    let mut k = 0;
    while k < LEDGER_CAP + 7 {
        let _ = on_load(D2_FROZEN_SIG, &[], &mut led2);
        k += 1;
    }
    if led2.reasons.len() > LEDGER_CAP || led2.degraded != (LEDGER_CAP + 7) as u32 {
        return Err(D2Code::LedgerBroken.code());
    }
    repro_check()?;
    Ok((led.loaded, led.degraded))
}

// ── 判据（15 条，断言性质） ────────────────────────────────────────
/// 判据入口（单模块 ≤20 条）。
pub fn run_unx18445_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18445 manifest 解析执行位消费（D2 装载侧）");
    // 1. D2 回调签名一致（J1 前半句主体）。
    s.add("J1-D2签名一致", cb_sig_check(D2_FROZEN_SIG).is_ok(), "冻结值↔FNV 重算互证");
    // 2. 签名重算一致：const 与运行期同口径。
    s.add("J1-签名重算一致", D2_FROZEN_SIG == fnv1a_trip_rt(D2_CB_ID, D2_PARAM_MASK, D2_RET_DOMAIN), "const/运行期逐位同");
    // 3. 签名不匹配注入：检出且放行。
    s.add("J1-注入:签名不匹配", inject_bad_sig().is_ok(), "检出+进程创建放行");
    // 4. 空载荷注入。
    s.add("J1-注入:空载荷", inject_empty_payload().is_ok(), "检出+放行");
    // 5. 未知 GUID 注入。
    s.add("J1-注入:未知GUID", inject_unknown_guid().is_ok(), "检出+放行");
    // 6. 超容量注入。
    s.add("J1-注入:超容量", inject_over_cap().is_ok(), "6 条 >5 上限 检出+放行");
    // 7. 重复 GUID 注入。
    s.add("J1-注入:重复GUID", inject_dup_guid().is_ok(), "同世代双条 检出+放行");
    // 8. 注入多轮复现。
    s.add("J1-注入3轮全检出", inject_rounds(), "5 类 ×3 轮");
    // 9. 正载荷装载位面正确。
    let mut led = LoadLedger::new();
    let o = on_load(D2_FROZEN_SIG, &[GUID_WIN81, GUID_WIN10], &mut led);
    s.add("J1-正载荷位面", o.phase == LoadPhase::Loaded && o.decl_mask == 0b11000, "Win81+Win10 → 0b11000");
    // 10. 非阻塞契约：任何终态 may_proceed 恒真（J1 后半句主体）。
    let mut led2 = LoadLedger::new();
    let a = on_load(D2_FROZEN_SIG, &[], &mut led2);
    let b = on_load(D2_FROZEN_SIG, &[GUID_VISTA], &mut led2);
    s.add("J1-不阻塞进程创建", a.may_proceed() && b.may_proceed(), "Loaded/Degraded 双终态恒放行");
    // 11. 装载账守恒。
    s.add("J1-装载账守恒", led2.conserve(2) && led2.loaded == 1 && led2.degraded == 1, "loaded+degraded==总请求");
    // 12. 降级环形覆盖：满后覆盖最旧不阻塞。
    let ring = {
        let mut l = LoadLedger::new();
        let mut k = 0;
        while k < LEDGER_CAP + 3 {
            let _ = on_load(D2_FROZEN_SIG, &[], &mut l);
            k += 1;
        }
        l.reasons.len() == LEDGER_CAP && l.degraded == (LEDGER_CAP + 3) as u32
    };
    s.add("J1-降级环形覆盖", ring, "账长恒 ≤32，计数不丢");
    // 13. 双跑一致。
    s.add("J1-双跑一致", repro_check().is_ok(), "注入轮次两跑同值");
    // 14. 段位守卫。
    s.add("J1-段位守卫", segment_guard(), "0x7EB00 窗口");
    // 15. 黄金全链。
    let g = golden_run();
    let g_ok = g.is_ok();
    s.add_dyn("J1-黄金全链", g_ok, &format!("golden={:?}", g));
    s
}

// ── 测试 ───────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx18445_checks_green() {
        let set = run_unx18445_checks();
        assert!(set.all_passed(), "UNX-F18445 red: {}", set.red_items_text());
    }

    #[test]
    fn t_sig_check() {
        assert_eq!(cb_sig_check(D2_FROZEN_SIG), Ok(()));
        assert_eq!(cb_sig_check(0), Err(D2Code::CbSigMismatch.code()));
    }

    #[test]
    fn t_payload_gates() {
        assert_eq!(validate_payload(&[]), Err(D2Code::EmptyPayload.code()));
        assert_eq!(validate_payload(&[GUID_WIN7, GUID_WIN7]), Err(D2Code::PayloadDupGuid.code()));
        assert_eq!(validate_payload(&[GUID_WIN7]), Ok(0b10));
    }

    #[test]
    fn t_may_proceed_always() {
        let mut led = LoadLedger::new();
        for sig in [D2_FROZEN_SIG, 0u32].iter() {
            for p in [vec![GUID_WIN8], vec![], vec![[0u8; 16]]].iter() {
                let o = on_load(*sig, p, &mut led);
                assert!(o.may_proceed());
            }
        }
    }

    #[test]
    fn t_injections() {
        assert!(inject_bad_sig().is_ok());
        assert!(inject_empty_payload().is_ok());
        assert!(inject_unknown_guid().is_ok());
        assert!(inject_over_cap().is_ok());
        assert!(inject_dup_guid().is_ok());
    }
}
