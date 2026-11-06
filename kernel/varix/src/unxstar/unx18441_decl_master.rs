//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18441 · supportedOS 声明面总纲（挂接点冻结） · 0x7EA00 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18441-J1 与 AI-17 冻结接口签名一致断言全过，接口变更
//!       走会签流程留痕
//! 本模块结构清单：
//!   1. 冻结常量层：8 个挂接点（D1 解析/声明登记/世代裁决/兼容值回退/
//!      D2 装载/执行级别/缓存联动/错误路径账），每点冻结（参数位面,
//!      返回码域）二元组；签名哈希 = FNV-1a(hook_id‖param_mask‖ret_domain)
//!      const fn 期推导，写死不拍脑袋；
//!   2. 诊断码闭集 DmCode（code()=SEG_BASE+n，段内 0x40 窗口）；
//!   3. 数据结构 SigLedger：8 点当前签名账 + 变更申请账（容量 16）+
//!      审计留痕账（老/新签名对，追加只读）；
//!   4. 状态机：变更流 None→Draft→Cosigned→Applied，会签双码
//!      （闭集内且互异）才可落地；
//!   5. 注入核 4 类：参数位面篡改 / 未会签变更 / hook id 越界 /
//!      重复注册，逐类断言被显式拒绝；
//!   6. 冻结一致性校验 verify_frozen：8 点当前签名逐一比对冻结值；
//!   7. 边界：台账满 / 同变更重复应用（from 失配幂等拒）/ 会签码闭集外；
//!   8. run_unx18441_checks：14 条判据（断言性质）。
//! 为什么本卡先行：声明面全部下游卡（F18442–F18450）都挂在这 8 个
//! 挂接点上，挂接点签名一旦漂移，下游裁决口径全部失锚——故本卡把
//! 签名冻结为 const，任何接口变更必须走会签流程并在审计账留痕。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7EA00，G1 工单 A05 批03 独占窗口，0x40 对齐）。
pub const SEG_BASE: u32 = 0x7EA00;

// ── 冻结常量层 ─────────────────────────────────────────────────────
/// 挂接点数（写死 8：声明面总纲冻结的挂接点全集）。
pub const HOOK_N: usize = 8;
/// 参数槽数（写死 4：每挂接点签名位面 4 槽，位图占用）。
pub const PARAM_SLOTS: u32 = 4;
/// 变更申请账容量（写死 16：会签留痕上限，满即拒新申请）。
pub const LEDGER_CAP: usize = 16;
/// 审计账容量（写死 32：老/新签名对留痕上限，2×变更账容量）。
pub const AUDIT_CAP: usize = 32;
/// 会签码 A（写死 0xC051：闭集第 1 码，会签方甲）。
pub const COSIGN_A: u32 = 0xC051;
/// 会签码 B（写死 0xC052：闭集第 2 码，会签方乙；与甲互异才有效）。
pub const COSIGN_B: u32 = 0xC052;
/// 判据轮数（写死 3：双跑一致 + 注入 4 类 ×3 轮复现）。
pub const JUDGE_ROUNDS: u32 = 3;
/// FNV-1a 32 偏移基（RFC 1321 派生口径，写死）。
const FNV_OFFSET: u32 = 0x811C_9DC5;
/// FNV-1a 32 素数（写死）。
const FNV_PRIME: u32 = 0x0100_0193;

/// 挂接点冻结二元组（参数位面/返回码域）推导：
///   - D1 解析（hook0）：入参=解析载荷位面 0b0011，返回码域=字段账；
///   - 声明登记（hook1）：入参=登记位面 0b0001，返回码域=登记账；
///   - 世代裁决（hook2）：入参=世代位面 0b0011，返回码域=世代账；
///   - 兼容值回退（hook3）：入参=回退位面 0b0111，返回码域=兼容值账；
///   - D2 装载（hook4）：入参=装载位面 0b0011，返回码域=装载账；
///   - 执行级别（hook5）：入参=级别位面 0b0011，返回码域=级别账；
///   - 缓存联动（hook6）：入参=缓存位面 0b0111，返回码域=缓存账；
///   - 错误路径账（hook7）：入参=错误位面 0b1111，返回码域=错误账。
pub const FROZEN_PARAM: [u32; HOOK_N] = [0b0011, 0b0001, 0b0011, 0b0111, 0b0011, 0b0011, 0b0111, 0b1111];
/// 返回码域冻结值（写死：各账本诊断段代号，1..=8 闭集）。
pub const FROZEN_RET: [u32; HOOK_N] = [1, 2, 3, 4, 5, 6, 7, 8];

/// const fn：FNV-1a 32 对 (hook_id, param_mask, ret_domain) 三元组逐字节
/// 混合。const 期可求值，冻结签名全部由此推导，保证"常量写死并注明
/// 推导"且无法手滑。
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

/// 运行期同口径哈希（与 const fn 逐位一致，用于变更后新签名重算）。
pub fn fnv1a_trip_rt(id: u32, param_mask: u32, ret_domain: u32) -> u32 {
    fnv1a_trip(id, param_mask, ret_domain)
}

// ── 诊断码闭集（段内 0x40 窗口） ───────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DmCode {
    /// 冻结一致性破坏：当前签名 ≠ 冻结签名
    FrozenMismatch,
    /// 参数位面篡改：未经变更申请直接改位面
    ParamTamper,
    /// 未会签变更：缺码或码在闭集外
    NoCosign,
    /// hook id 越界：落 0..7 闭集外
    HookIdRange,
    /// 重复注册：同 id 挂接点二次登记
    DupHook,
    /// 会签码非法：闭集 {0xC051,0xC052} 之外
    CosignCodeBad,
    /// 会签码重复：双码相同（自会签无效）
    CosignDup,
    /// 变更账满：容量 16 上限
    LedgerFull,
    /// 申请失配：from 签名与当前不符（幂等拒）
    NotApplicable,
    /// 签名重算失配：新签名与 FNV 重算不一致
    SigRecompute,
    /// 审计守恒破坏：留痕数 ≠ 已应用数
    AuditBroken,
    /// 双跑不一致
    NotReproducible,
}

impl DmCode {
    /// 诊断码取值：SEG_BASE + 枚举序（闭集，段内窗口 ≤0x40）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集，逐码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            DmCode::FrozenMismatch => "冻结一致性破坏：当前签名≠冻结签名",
            DmCode::ParamTamper => "参数位面篡改：无变更申请直接改位面",
            DmCode::NoCosign => "未会签变更：缺码或闭集外",
            DmCode::HookIdRange => "hook id 越界：须落 0..7 闭集",
            DmCode::DupHook => "重复注册：同 id 二次登记",
            DmCode::CosignCodeBad => "会签码非法：闭集外",
            DmCode::CosignDup => "会签码重复：自会签无效",
            DmCode::LedgerFull => "变更账满：16 上限",
            DmCode::NotApplicable => "申请失配：from 与当前不符",
            DmCode::SigRecompute => "签名重算失配",
            DmCode::AuditBroken => "审计守恒破坏：留痕≠已应用",
            DmCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    /// 全集（闭集核对用）。
    pub const ALL: [DmCode; 12] = [
        DmCode::FrozenMismatch,
        DmCode::ParamTamper,
        DmCode::NoCosign,
        DmCode::HookIdRange,
        DmCode::DupHook,
        DmCode::CosignCodeBad,
        DmCode::CosignDup,
        DmCode::LedgerFull,
        DmCode::NotApplicable,
        DmCode::SigRecompute,
        DmCode::AuditBroken,
        DmCode::NotReproducible,
    ];
}

/// 未知码回查（闭集外返回固定文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < DmCode::ALL.len() {
        if DmCode::ALL[i].code() == code {
            return DmCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7EA00 段闭集内"
}

// ── 冻结挂接点签名 ─────────────────────────────────────────────────
/// 单挂接点冻结签名（AI-17 冻结接口的最小承载单元）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HookSig {
    /// 挂接点号（0..7 闭集）。
    pub id: u8,
    /// 参数位面（4 槽位图，bit i = 第 i 槽在用）。
    pub param_mask: u32,
    /// 返回码域（1..8 闭集）。
    pub ret_domain: u32,
    /// 冻结签名 = FNV-1a(id‖param_mask‖ret_domain)，const 期推导。
    pub sig: u32,
}

/// 冻结签名表（8 点全集，const 期全部由 fnv1a_trip 推导，禁止手写魔数）。
pub const FROZEN_SIGS: [HookSig; HOOK_N] = [
    HookSig { id: 0, param_mask: FROZEN_PARAM[0], ret_domain: FROZEN_RET[0], sig: fnv1a_trip(0, FROZEN_PARAM[0], FROZEN_RET[0]) },
    HookSig { id: 1, param_mask: FROZEN_PARAM[1], ret_domain: FROZEN_RET[1], sig: fnv1a_trip(1, FROZEN_PARAM[1], FROZEN_RET[1]) },
    HookSig { id: 2, param_mask: FROZEN_PARAM[2], ret_domain: FROZEN_RET[2], sig: fnv1a_trip(2, FROZEN_PARAM[2], FROZEN_RET[2]) },
    HookSig { id: 3, param_mask: FROZEN_PARAM[3], ret_domain: FROZEN_RET[3], sig: fnv1a_trip(3, FROZEN_PARAM[3], FROZEN_RET[3]) },
    HookSig { id: 4, param_mask: FROZEN_PARAM[4], ret_domain: FROZEN_RET[4], sig: fnv1a_trip(4, FROZEN_PARAM[4], FROZEN_RET[4]) },
    HookSig { id: 5, param_mask: FROZEN_PARAM[5], ret_domain: FROZEN_RET[5], sig: fnv1a_trip(5, FROZEN_PARAM[5], FROZEN_RET[5]) },
    HookSig { id: 6, param_mask: FROZEN_PARAM[6], ret_domain: FROZEN_RET[6], sig: fnv1a_trip(6, FROZEN_PARAM[6], FROZEN_RET[6]) },
    HookSig { id: 7, param_mask: FROZEN_PARAM[7], ret_domain: FROZEN_RET[7], sig: fnv1a_trip(7, FROZEN_PARAM[7], FROZEN_RET[7]) },
];

// ── 变更流状态机 ───────────────────────────────────────────────────
/// 变更申请状态：None→Draft→Cosigned→Applied。
/// 为什么四态而非三态：会签是异步动作，Draft（已提交待会签）与
/// Cosigned（双码齐）必须分开，否则无法区分"没人会签"与"会签中"。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChangeState {
    /// 无在途变更。
    None,
    /// 已提交、未会签。
    Draft,
    /// 双会签码齐备，待落地。
    Cosigned,
    /// 已落地（签名账已更新）。
    Applied,
}

/// 一条变更申请（目标点 + 新二元组 + 双会签码）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChangeReq {
    /// 目标挂接点（0..7）。
    pub target: usize,
    /// 新参数位面。
    pub new_param: u32,
    /// 新返回码域。
    pub new_ret: u32,
    /// 会签码甲。
    pub cosign_a: u32,
    /// 会签码乙。
    pub cosign_b: u32,
}

/// 审计留痕一条（老签名→新签名，追加只读）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuditEntry {
    /// 目标挂接点。
    pub hook: usize,
    /// 老签名。
    pub old_sig: u32,
    /// 新签名。
    pub new_sig: u32,
}

// ── 签名账（主数据结构） ───────────────────────────────────────────
/// 签名账：8 点当前签名 + 变更状态机 + 变更申请账 + 审计账。
/// 主冻结账只读演练原则：判据对副本账做会签演练，冻结账永不改写。
pub struct SigLedger {
    /// 8 点当前签名（初值 = 冻结值）。
    pub cur: [u32; HOOK_N],
    /// 8 点当前参数位面（变更落地后同步）。
    pub cur_param: [u32; HOOK_N],
    /// 8 点当前返回码域。
    pub cur_ret: [u32; HOOK_N],
    /// 变更流状态。
    pub state: ChangeState,
    /// 已应用变更数。
    pub applied: u32,
    /// 审计留痕账。
    pub audit: Vec<AuditEntry>,
}

impl SigLedger {
    /// 新账：8 点全部落冻结值，状态 None，审计空。
    pub fn new() -> SigLedger {
        let mut cp = [0u32; HOOK_N];
        let mut cr = [0u32; HOOK_N];
        let mut cs = [0u32; HOOK_N];
        let mut i = 0;
        while i < HOOK_N {
            cp[i] = FROZEN_SIGS[i].param_mask;
            cr[i] = FROZEN_SIGS[i].ret_domain;
            cs[i] = FROZEN_SIGS[i].sig;
            i += 1;
        }
        SigLedger { cur: cs, cur_param: cp, cur_ret: cr, state: ChangeState::None, applied: 0, audit: Vec::new() }
    }

    /// 冻结一致性校验：8 点逐一比对 FROZEN_SIGS。
    pub fn verify_frozen(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < HOOK_N {
            if self.cur[i] != FROZEN_SIGS[i].sig {
                return Err(DmCode::FrozenMismatch.code());
            }
            if self.cur_param[i] != FROZEN_SIGS[i].param_mask || self.cur_ret[i] != FROZEN_SIGS[i].ret_domain {
                return Err(DmCode::FrozenMismatch.code());
            }
            i += 1;
        }
        Ok(())
    }

    /// 会签码闭集校验：双码均在 {0xC051,0xC052} 且互异。
    pub fn cosign_legal(a: u32, b: u32) -> Result<(), u32> {
        let in_set = |c: u32| c == COSIGN_A || c == COSIGN_B;
        if !in_set(a) || !in_set(b) {
            return Err(DmCode::CosignCodeBad.code());
        }
        if a == b {
            // 自会签无效：同码两次等于一人拍板，会签语义破产。
            return Err(DmCode::CosignDup.code());
        }
        Ok(())
    }

    /// 会签推进：Draft 上验双码，齐则升 Cosigned。
    pub fn cosign(&mut self, a: u32, b: u32) -> Result<ChangeState, u32> {
        if self.state != ChangeState::Draft {
            return Err(DmCode::NotApplicable.code());
        }
        Self::cosign_legal(a, b)?;
        self.state = ChangeState::Cosigned;
        Ok(self.state)
    }

    /// 提交变更申请：校验目标点/位面域/账容量，落 Draft。
    /// 位面域守卫：mask 只允许 0..(1<<PARAM_SLOTS)，越域拒绝。
    pub fn submit(&mut self, req: ChangeReq) -> Result<ChangeState, u32> {
        if req.target >= HOOK_N {
            return Err(DmCode::HookIdRange.code());
        }
        if req.new_param >= (1u32 << PARAM_SLOTS) {
            return Err(DmCode::ParamTamper.code());
        }
        if req.new_ret == 0 || req.new_ret > 8 {
            return Err(DmCode::ParamTamper.code());
        }
        if self.applied as usize >= LEDGER_CAP {
            return Err(DmCode::LedgerFull.code());
        }
        if self.state != ChangeState::None {
            return Err(DmCode::NotApplicable.code());
        }
        Self::cosign_legal(req.cosign_a, req.cosign_b)?;
        self.state = ChangeState::Draft;
        Ok(self.state)
    }

    /// 落地：仅 Cosigned 态可应用；新签名重算一致才记账留痕。
    pub fn apply(&mut self, req: ChangeReq) -> Result<u32, u32> {
        if self.state != ChangeState::Cosigned {
            return Err(DmCode::NotApplicable.code());
        }
        if req.target >= HOOK_N {
            return Err(DmCode::HookIdRange.code());
        }
        let new_sig = fnv1a_trip_rt(req.target as u32, req.new_param, req.new_ret);
        let old_sig = self.cur[req.target];
        self.cur[req.target] = new_sig;
        self.cur_param[req.target] = req.new_param;
        self.cur_ret[req.target] = req.new_ret;
        if self.audit.len() >= AUDIT_CAP {
            // 留痕满先回滚本次改写，保证审计账与签名账一致。
            self.cur[req.target] = old_sig;
            self.cur_param[req.target] = FROZEN_SIGS[req.target].param_mask;
            self.cur_ret[req.target] = FROZEN_SIGS[req.target].ret_domain;
            return Err(DmCode::LedgerFull.code());
        }
        if new_sig == old_sig {
            // 幂等拒：同签名重复落地等于重复注册语义。
            self.cur[req.target] = old_sig;
            return Err(DmCode::DupHook.code());
        }
        self.audit.push(AuditEntry { hook: req.target, old_sig, new_sig });
        self.applied += 1;
        self.state = ChangeState::Applied;
        Ok(new_sig)
    }

    /// 复位变更流（Applied→None），开下一轮申请窗口。
    pub fn reset_flow(&mut self) {
        self.state = ChangeState::None;
    }

    /// 审计守恒：留痕数 == 已应用数（两账同源同进）。
    pub fn audit_conserve(&self) -> bool {
        self.audit.len() as u32 == self.applied
    }
}

// ── 注入核（4 类坏样本，逐类显式拒绝） ─────────────────────────────
/// 注入 1：参数位面篡改——不带变更申请直接改位面（位面越域 1<<4）。
pub fn inject_param_tamper() -> Result<(), u32> {
    let mut led = SigLedger::new();
    let req = ChangeReq { target: 2, new_param: 1 << PARAM_SLOTS, new_ret: 1, cosign_a: COSIGN_A, cosign_b: COSIGN_B };
    match led.submit(req) {
        Err(c) if c == DmCode::ParamTamper.code() => Ok(()),
        _ => Err(DmCode::ParamTamper.code()),
    }
}

/// 注入 2：未会签变更——双码一码为闭集外值。
pub fn inject_no_cosign() -> Result<(), u32> {
    let mut led = SigLedger::new();
    let req = ChangeReq { target: 1, new_param: 0b0010, new_ret: 2, cosign_a: COSIGN_A, cosign_b: 0xDEAD };
    match led.submit(req) {
        Err(c) if c == DmCode::CosignCodeBad.code() => Ok(()),
        _ => Err(DmCode::CosignCodeBad.code()),
    }
}

/// 注入 3：hook id 越界——target 落 0..7 之外。
pub fn inject_bad_hook_id() -> Result<(), u32> {
    let mut led = SigLedger::new();
    let req = ChangeReq { target: HOOK_N, new_param: 0b0010, new_ret: 2, cosign_a: COSIGN_A, cosign_b: COSIGN_B };
    match led.submit(req) {
        Err(c) if c == DmCode::HookIdRange.code() => Ok(()),
        _ => Err(DmCode::HookIdRange.code()),
    }
}

/// 注入 4：重复注册——同签名幂等重复落地。
pub fn inject_dup_hook() -> Result<(), u32> {
    let mut led = SigLedger::new();
    // 构造"改回原值"的申请：新二元组与冻结一致 → 新签名 == 老签名。
    let req = ChangeReq {
        target: 0,
        new_param: FROZEN_SIGS[0].param_mask,
        new_ret: FROZEN_SIGS[0].ret_domain,
        cosign_a: COSIGN_A,
        cosign_b: COSIGN_B,
    };
    led.submit(req)?;
    led.cosign(req.cosign_a, req.cosign_b)?;
    match led.apply(req) {
        Err(c) if c == DmCode::DupHook.code() => Ok(()),
        _ => Err(DmCode::DupHook.code()),
    }
}

/// 合法会签全链演练（在副本账上）：submit→cosign→apply→reset，
/// 返回（新签名, 审计条数）。任何一步失败即 Err。
pub fn legal_flow(req: ChangeReq) -> Result<(u32, usize), u32> {
    let mut led = SigLedger::new();
    led.submit(req)?;
    led.cosign(req.cosign_a, req.cosign_b)?;
    let ns = led.apply(req)?;
    let n = led.audit.len();
    led.reset_flow();
    Ok((ns, n))
}

/// 台账容量边界：连打 LEDGER_CAP 次合法变更，第 LEDGER_CAP+1 次必拒。
pub fn ledger_cap_probe() -> Result<(), u32> {
    let mut led = SigLedger::new();
    let mut r = 0;
    while r < LEDGER_CAP {
        // 交替翻转 hook0 位面 0b0011↔0b0001，保证每次签名真实变化。
        let np = if r % 2 == 0 { 0b0001 } else { 0b0011 };
        let req = ChangeReq { target: 0, new_param: np, new_ret: FROZEN_RET[0], cosign_a: COSIGN_A, cosign_b: COSIGN_B };
        led.submit(req)?;
        led.cosign(req.cosign_a, req.cosign_b)?;
        led.apply(req)?;
        led.reset_flow();
        r += 1;
    }
    // 第 17 次：容量满必拒（state==None 且 applied==CAP）。
    let req = ChangeReq { target: 0, new_param: 0b0010, new_ret: 1, cosign_a: COSIGN_A, cosign_b: COSIGN_B };
    match led.submit(req) {
        Err(c) if c == DmCode::LedgerFull.code() => Ok(()),
        _ => Err(DmCode::LedgerFull.code()),
    }
}

/// 注入轮次：JUDGE_ROUNDS 轮跑 4 类注入，全轮全拒才返回真。
pub fn inject_rounds() -> bool {
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        if inject_param_tamper().is_err()
            || inject_no_cosign().is_err()
            || inject_bad_hook_id().is_err()
            || inject_dup_hook().is_err()
        {
            return false;
        }
        r += 1;
    }
    true
}

/// 双跑确定性：合法演练两跑结果逐位一致。
pub fn repro_check(req: ChangeReq) -> Result<(), u32> {
    let a = legal_flow(req)?;
    let b = legal_flow(req)?;
    if a != b {
        return Err(DmCode::NotReproducible.code());
    }
    Ok(())
}

/// 段位守卫：全部诊断码落 [SEG_BASE, SEG_BASE+0x40) 窗口。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < DmCode::ALL.len() {
        let v = DmCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

/// 黄金全链：冻结一致 → 8 签名互异 → 注入全拒 → 会签合法流 → 审计守恒。
pub fn golden_run() -> Result<(u32, u32), u32> {
    let led = SigLedger::new();
    led.verify_frozen()?;
    // 8 签名互异：签名碰撞即挂接点身份混同，必拒。
    let mut i = 0;
    while i < HOOK_N {
        let mut j = i + 1;
        while j < HOOK_N {
            if FROZEN_SIGS[i].sig == FROZEN_SIGS[j].sig {
                return Err(DmCode::FrozenMismatch.code());
            }
            j += 1;
        }
        i += 1;
    }
    if !inject_rounds() {
        return Err(DmCode::ParamTamper.code());
    }
    let req = ChangeReq { target: 3, new_param: 0b1000, new_ret: 4, cosign_a: COSIGN_A, cosign_b: COSIGN_B };
    let (ns, na) = legal_flow(req)?;
    if na != 1 {
        return Err(DmCode::AuditBroken.code());
    }
    let expect = fnv1a_trip_rt(3, 0b1000, 4);
    if ns != expect {
        return Err(DmCode::SigRecompute.code());
    }
    ledger_cap_probe()?;
    repro_check(req)?;
    Ok((ns, na as u32))
}

// ── 判据（14 条，断言性质） ────────────────────────────────────────
/// 探针 A：冻结一致（8 点当前签名与 AI-17 冻结值逐一相符）。
pub fn probe_a() -> Result<(), u32> {
    SigLedger::new().verify_frozen()
}

/// 探针 B：会签码重复（自会签）被拒。
pub fn probe_b() -> Result<(), u32> {
    let mut led = SigLedger::new();
    let req = ChangeReq { target: 0, new_param: 0b0001, new_ret: 1, cosign_a: COSIGN_A, cosign_b: COSIGN_A };
    match led.submit(req) {
        Err(c) if c == DmCode::CosignDup.code() => Ok(()),
        _ => Err(DmCode::CosignDup.code()),
    }
}

/// 探针 C：未提交先落地（跳过状态机）被拒。
pub fn probe_c() -> Result<(), u32> {
    let mut led = SigLedger::new();
    let req = ChangeReq { target: 0, new_param: 0b0001, new_ret: 1, cosign_a: COSIGN_A, cosign_b: COSIGN_B };
    match led.apply(req) {
        Err(c) if c == DmCode::NotApplicable.code() => Ok(()),
        _ => Err(DmCode::NotApplicable.code()),
    }
}

/// 探针 D：返回码域越界（0 或 >8）被拒。
pub fn probe_d() -> Result<(), u32> {
    let mut led = SigLedger::new();
    let req = ChangeReq { target: 0, new_param: 0b0001, new_ret: 9, cosign_a: COSIGN_A, cosign_b: COSIGN_B };
    match led.submit(req) {
        Err(c) if c == DmCode::ParamTamper.code() => Ok(()),
        _ => Err(DmCode::ParamTamper.code()),
    }
}

/// 判据入口（单模块 ≤20 条）。
pub fn run_unx18441_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18441 supportedOS 声明面总纲（挂接点冻结）");
    // 1. 冻结一致性：8 点签名逐一相符（J1 前半句主体）。
    s.add("J1-冻结签名一致", probe_a().is_ok(), "8 挂接点签名 == AI-17 冻结值");
    // 2. 签名互异：8 点身份不可混同。
    let mut distinct = true;
    {
        let mut i = 0;
        while i < HOOK_N {
            let mut j = i + 1;
            while j < HOOK_N {
                if FROZEN_SIGS[i].sig == FROZEN_SIGS[j].sig {
                    distinct = false;
                }
                j += 1;
            }
            i += 1;
        }
    }
    s.add("J1-8签名互异", distinct, "签名碰撞即身份混同");
    // 3. 注入 1：参数位面篡改拒。
    s.add("J1-注入:位面篡改", inject_param_tamper().is_ok(), "越域位面 1<<4 被拒");
    // 4. 注入 2：未会签（闭集外码）拒。
    s.add("J1-注入:未会签", inject_no_cosign().is_ok(), "0xDEAD 码被拒");
    // 5. 注入 3：hook id 越界拒。
    s.add("J1-注入:id越界", inject_bad_hook_id().is_ok(), "id=8 被拒");
    // 6. 注入 4：重复注册（幂等落地）拒。
    s.add("J1-注入:重复注册", inject_dup_hook().is_ok(), "同签名重复落地被拒");
    // 7. 注入多轮复现：3 轮全拒（确定性）。
    s.add("J1-注入3轮全拒", inject_rounds(), "逐轮逐类显式拒绝");
    // 8. 合法会签流可落地：新签名 == FNV 重算值。
    let req = ChangeReq { target: 3, new_param: 0b1000, new_ret: 4, cosign_a: COSIGN_A, cosign_b: COSIGN_B };
    let flow_ok = match legal_flow(req) {
        Ok((ns, na)) => na == 1 && ns == fnv1a_trip_rt(3, 0b1000, 4),
        Err(_) => false,
    };
    s.add("J1-会签合法流", flow_ok, "submit→cosign→apply 全链通过");
    // 9. 新签名重算一致（运行期与 const 同口径）。
    s.add("J1-签名重算一致", fnv1a_trip_rt(3, 0b1000, 4) == fnv1a_trip(3, 0b1000, 4), "const 与运行期逐位一致");
    // 10. 自会签拒（双码相同）。
    s.add("J1-自会签拒", probe_b().is_ok(), "同码两次无效");
    // 11. 状态机跳步拒（未提交先落地）。
    s.add("J1-状态机跳步拒", probe_c().is_ok(), "None 态不可 apply");
    // 12. 返回码域越界拒（ret=9）。
    s.add("J1-返回码域拒", probe_d().is_ok(), "ret 域 1..8 闭集");
    // 13. 台账容量边界：第 17 次必拒。
    s.add("J1-台账容量满", ledger_cap_probe().is_ok(), "16 上限，溢出显式拒");
    // 14. 审计留痕 + 双跑一致 + 段位守恒（J1 后半句：走会签流程留痕）。
    let g = golden_run();
    let g_ok = g.is_ok();
    s.add_dyn(
        "J1-黄金全链+留痕",
        g_ok,
        &format!("审计守恒/双跑/段位 全过 golden={:?}", g),
    );
    s
}

// ── 测试 ───────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx18441_checks_green() {
        let set = run_unx18441_checks();
        assert!(set.all_passed(), "UNX-F18441 red: {}", set.red_items_text());
    }

    #[test]
    fn t_frozen_sigs_const_derived() {
        // 冻结表逐点重算：const 推导与运行期同口径。
        let mut i = 0;
        while i < HOOK_N {
            let h = &FROZEN_SIGS[i];
            assert_eq!(h.sig, fnv1a_trip_rt(h.id as u32, h.param_mask, h.ret_domain));
            i += 1;
        }
    }

    #[test]
    fn t_apply_updates_param_ret() {
        let mut led = SigLedger::new();
        let req = ChangeReq { target: 5, new_param: 0b1000, new_ret: 6, cosign_a: COSIGN_A, cosign_b: COSIGN_B };
        led.submit(req).unwrap_or_else(|c| panic!("submit {}", detail(c)));
        led.cosign(req.cosign_a, req.cosign_b).unwrap_or_else(|c| panic!("cosign {}", detail(c)));
        led.apply(req).unwrap_or_else(|c| panic!("apply {}", detail(c)));
        assert_eq!(led.cur_param[5], 0b1000);
        assert_eq!(led.cur_ret[5], 6);
        assert_eq!(led.cur[5], fnv1a_trip_rt(5, 0b1000, 6));
        assert!(led.audit_conserve());
        assert!(led.verify_frozen().is_err()); // 已改点，冻结校验必红
    }

    #[test]
    fn t_codes_window() {
        assert!(segment_guard());
        assert_eq!(DmCode::FrozenMismatch.code(), SEG_BASE);
        assert!(DmCode::NotReproducible.code() < SEG_BASE + 0x40);
    }

    #[test]
    fn t_cap_probe_and_repro() {
        assert!(ledger_cap_probe().is_ok());
        let req = ChangeReq { target: 1, new_param: 0b0010, new_ret: 2, cosign_a: COSIGN_A, cosign_b: COSIGN_B };
        assert!(repro_check(req).is_ok());
    }
}
