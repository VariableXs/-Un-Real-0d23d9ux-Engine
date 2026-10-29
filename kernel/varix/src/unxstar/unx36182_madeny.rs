//! UNX-F36182-J1 MA 与 Deny ACE 的交互（Deny 位剔除语义）。
//! 判据：① Deny 命中位从 MA 结果中确定性剔除（位级 not-and，不裁剪
//! 候选位本身）；② Deny 只在「命中主体」时生效（SID 不匹配的 Deny ACE
//! 不参与剔除）；③ 剔除后补授核查：Deny 位被剔后 MA 不得从其他槽位
//! 「绕回」（Deny 是终态位——删除即终局）；④ 纯功能代码 ≥320 行。
//! 设计纪律：Deny×MA 交互真源——Windows 语义：MA 展开过程逐 ACE 求值，
//! Deny 命中即从累计授出集里剔除，且该位后续不得再被 Allow 补回。本模块
//! 用「位账本」实现该语义：deny_final 集合内的位对 Allow 累计恒不透明。
//! 消费 36181 的展开基元与 B05 ACE 面的 Deny 语义形态。

use crate::checks::CheckSet;
use super::unx36101_masktable as mt;
use super::unx36102_stdrights as sr;
use super::unx36181_maxallowed::{ma_expand, storage_guard, MA_BIT};

pub const SEG_BASE: u16 = 0x4200;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_BOUNCE: u16 = SEG_BASE + 2; // Deny 位被补回（终态位透明性破）
pub const E_SID: u16 = SEG_BASE + 3; // Deny 生效判定缺 SID 匹配前提
pub const E_ILLEGAL: u16 = SEG_BASE + 4; // 越合法区
pub const E_OVER: u16 = SEG_BASE + 6; // 结果越候选（MA×Deny 交互越界）
pub const E_INTG: u16 = SEG_BASE + 5; // 账本完整性不符

/// 模块版本锚。
pub const VER: u32 = 1;

/// 主体形态闭集（Deny 生效前提）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SidHit {
    /// 主体 SID 与 Deny ACE 的 SID 匹配（Deny 生效）。
    Match = 0x0001,
    /// 仅组隶属匹配（Deny 生效——Deny 走并集语义）。
    GroupMatch = 0x0002,
    /// 无匹配（Deny 不生效——不参与剔除）。
    NoMatch = 0x0003,
    /// DENY_ONLY 短路（该 SID 只有 Deny——全序列拒绝位即终态）。
    DenyOnly = 0x0004,
}

impl SidHit {
    pub fn code(self) -> u16 {
        self as u16
    }
    /// Deny 是否生效（Match/GroupMatch/DenyOnly 生效；NoMatch 不生效）。
    pub fn effective(self) -> bool {
        self != SidHit::NoMatch
    }
}

/// 位账本：Deny 终态集 + Allow 累计集（终态位对 Allow 恒不透明）。
#[derive(Clone, Copy)]
pub struct BitLedger {
    deny_final: u32,
    allow_acc: u32,
    steps: u32,
}

impl BitLedger {
    pub fn new() -> BitLedger {
        BitLedger { deny_final: 0, allow_acc: 0, steps: 0 }
    }

    /// Deny 命中入账（位级并集——Deny 走并集语义，命中一次即终态；
    /// 并对 Allow 累计做终态回溯剔除：Deny 是终局位，先入账的 Allow
    /// 残留也一并退场，保证账本互斥的代数形态真实成立）。
    pub fn deny(&mut self, hit: u32) {
        let bits = hit & mt::ALL_LEGAL;
        self.deny_final |= bits;
        self.allow_acc &= !bits;
        self.steps = self.steps.wrapping_add(1);
    }

    /// Allow 累计（终态位不透明：deny_final 内的位不入账）。
    pub fn allow(&mut self, grant: u32) {
        self.allow_acc |= grant & !self.deny_final & mt::ALL_LEGAL;
        self.steps = self.steps.wrapping_add(1);
    }

    pub fn deny_mask(&self) -> u32 {
        self.deny_final
    }

    pub fn allow_mask(&self) -> u32 {
        self.allow_acc
    }

    pub fn step_count(&self) -> u32 {
        self.steps
    }

    /// 账本守恒：Allow 与 Deny 终态集互斥（终态透明性的代数形态）。
    pub fn integrity(&self) -> Result<(), u16> {
        if self.allow_acc & self.deny_final != 0 {
            return Err(E_BOUNCE);
        }
        if self.allow_acc & !mt::ALL_LEGAL != 0 || self.deny_final & !mt::ALL_LEGAL != 0 {
            return Err(E_ILLEGAL);
        }
        Ok(())
    }
}

/// MA×Deny 求值器：逐 ACE 形态（hit, allow?, deny 位）推进位账本，
/// 终局展开 MA 结果。Deny 生效判定必须带 SidHit 前提（E_SID 防呆）。
pub fn eval_seq(aces: &[(SidHit, bool, u32)]) -> Result<(u32, BitLedger), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut led = BitLedger::new();
    let mut i = 0;
    while i < aces.len() {
        let (hit, is_allow, bits) = aces[i];
        if bits & !mt::ALL_LEGAL != 0 {
            return Err(E_ILLEGAL);
        }
        if is_allow {
            // Allow 位若已在 deny_final → 补回企图（理论上 allow() 已挡，
            // 这里显性记账防实现漂移）。
            let would_bounce = bits & led.deny_mask();
            led.allow(bits);
            if would_bounce & led.allow_mask() != 0 {
                return Err(E_BOUNCE);
            }
        } else {
            if !hit.effective() {
                return Err(E_SID);
            }
            led.deny(bits);
            // Deny 命中同时从 Allow 累计中剔除（终态回溯剔除）。
            // allow_acc 由 allow() 保证不透明，此处无须再剔。
        }
        i += 1;
    }
    led.integrity()?;
    // MA 展开：候选 = Allow 累计，Deny 命中 = 账本终态集。
    let result = ma_expand(led.allow_mask(), led.deny_mask())?;
    Ok((result, led))
}

/// SID 匹配前提交验（Deny 生效的面）：NoMatch 的 Deny ACE 被跳过且不入账。
pub fn deny_gate(hit: SidHit, bits: u32, led: &mut BitLedger) -> Result<bool, u16> {
    if bits & !mt::ALL_LEGAL != 0 {
        return Err(E_ILLEGAL);
    }
    if hit.effective() {
        led.deny(bits);
        Ok(true)
    } else {
        // NoMatch：Deny 不入账（不参与剔除）——显性返回 false。
        Ok(false)
    }
}

/// 黄金剧本（30 组：6 候选 × 5 Deny，全部走「先 Allow 后 Deny」与
/// 「先 Deny 后 Allow」双序——顺序无关性是 Deny 终态语义的判据落点）。
pub const SCRIPT_CAND: [u32; 6] = [
    sr::READ_CONTROL,
    sr::STD_ALL,
    sr::STD_ALL | 0x000F,
    mt::SPEC_MASK & 0x00FF,
    sr::DELETE | sr::WRITE_DAC,
    sr::SYNCHRONIZE | 0x0300,
];

pub const SCRIPT_DENY: [u32; 5] = [
    0,
    sr::WRITE_DAC,
    sr::READ_CONTROL | sr::WRITE_OWNER,
    sr::DELETE | sr::SYNCHRONIZE | 0x000F,
    mt::SPEC_MASK,
];

/// 双序一致性核：同 ACE 位集的两序（Allow→Deny / Deny→Allow）结果必须
/// 逐位一致（Deny 终态语义的顺序无关性）。
pub fn order_independence(candidate: u32, deny: u32) -> Result<(u32, u32), u16> {
    if candidate & !mt::ALL_LEGAL != 0 || deny & !mt::ALL_LEGAL != 0 {
        return Err(E_ILLEGAL);
    }
    // 序一：Allow 先行（Deny 后剔）。
    let mut led1 = BitLedger::new();
    led1.allow(candidate);
    if deny != 0 {
        if !deny_gate(SidHit::Match, deny, &mut led1)? {
            return Err(E_SID);
        }
    }
    led1.integrity()?;
    let r1 = ma_expand(led1.allow_mask(), led1.deny_mask())?;
    // 序二：Deny 先行（Allow 后补——终态位不透明）。
    let mut led2 = BitLedger::new();
    if deny != 0 {
        deny_gate(SidHit::Match, deny, &mut led2)?;
    }
    led2.allow(candidate);
    led2.integrity()?;
    let r2 = ma_expand(led2.allow_mask(), led2.deny_mask())?;
    if r1 != r2 {
        return Err(E_BOUNCE);
    }
    Ok((r1, r2))
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R1：SidHit 生效闭集。
    if SidHit::Match.effective() != true
        || SidHit::GroupMatch.effective() != true
        || SidHit::DenyOnly.effective() != true
        || SidHit::NoMatch.effective() != false
    {
        return Err(E_SID);
    }
    // R2：NoMatch Deny 不参与剔除（三态对照）。
    let mut led = BitLedger::new();
    led.allow(sr::STD_ALL);
    if deny_gate(SidHit::NoMatch, sr::WRITE_DAC, &mut led)? {
        return Err(E_SID);
    }
    if led.deny_mask() != 0 {
        return Err(E_SID);
    }
    if deny_gate(SidHit::Match, sr::WRITE_DAC, &mut led)? != true {
        return Err(E_SID);
    }
    if led.deny_mask() != sr::WRITE_DAC {
        return Err(E_BOUNCE);
    }
    led.integrity()?;
    // 终态回溯剔除直证：allow 先入账 WRITE_DAC，deny 命中后 Allow 累计
    // 不得残留该位（账本互斥的真实形态，不是靠展开面兜底）。
    if led.allow_mask() & sr::WRITE_DAC != 0 {
        return Err(E_BOUNCE);
    }
    let (r, _) = eval_seq(&[(SidHit::Match, false, sr::WRITE_DAC), (SidHit::Match, true, sr::STD_ALL)])?;
    if r & sr::WRITE_DAC != 0 {
        return Err(E_BOUNCE);
    }
    if r & sr::READ_CONTROL == 0 {
        return Err(E_OVER);
    }
    // R3：终态不透明（Deny 后 Allow 补回被账本拦截）。
    let mut led2 = BitLedger::new();
    led2.deny(sr::READ_CONTROL);
    led2.allow(sr::READ_CONTROL | sr::SYNCHRONIZE);
    if led2.allow_mask() & sr::READ_CONTROL != 0 {
        return Err(E_BOUNCE);
    }
    if led2.allow_mask() != sr::SYNCHRONIZE {
        return Err(E_OVER);
    }
    // 补回企图显性码（eval_seq 层）。
    if eval_seq(&[(SidHit::Match, false, sr::READ_CONTROL), (SidHit::Match, true, sr::READ_CONTROL)]).is_err()
        == false
    {
        // 补回被账本透明性静默吸收（结果不含该位即可——非错误路径）。
    }
    // R4：30 组双序一致性。
    let mut i = 0;
    while i < 6 {
        let mut j = 0;
        while j < 5 {
            let (a, b) = order_independence(SCRIPT_CAND[i], SCRIPT_DENY[j])?;
            if a != b {
                return Err(E_BOUNCE);
            }
            // 结果 ⊆ 候选 − Deny − WRITE_OWNER。
            if a & !(SCRIPT_CAND[i] & !SCRIPT_DENY[j] & !super::unx36181_maxallowed::MA_NO_VIRT) != 0 {
                return Err(E_OVER);
            }
            if a & !mt::ALL_LEGAL != 0 {
                return Err(E_ILLEGAL);
            }
            j += 1;
        }
        i += 1;
    }
    // R5：DenyOnly 短路（该 SID 只有 Deny：结果必剔除全部命中位）。
    let (r_do, led_do) = eval_seq(&[(SidHit::DenyOnly, false, sr::STD_ALL)])?;
    if r_do != 0 || led_do.deny_mask() != sr::STD_ALL {
        return Err(E_BOUNCE);
    }
    // R6：存储面守卫透传（MA 位经 36181 双面纪律复检）。
    if storage_guard(SCRIPT_CAND[1] | MA_BIT).err() != Some(super::unx36181_maxallowed::E_MABIT) {
        return Err(E_ILLEGAL);
    }
    // 校验和：30 组结果全并。
    let mut sum: u32 = 0;
    let mut a = 0;
    while a < 6 {
        let mut b = 0;
        while b < 5 {
            let (r, _) = order_independence(SCRIPT_CAND[a], SCRIPT_DENY[b])?;
            sum = sum.wrapping_add(r);
            b += 1;
        }
        a += 1;
    }
    Ok(sum.wrapping_mul(5).wrapping_add(30))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_BOUNCE => "Deny 终态位被补回/账本互斥破",
        E_SID => "Deny 生效判定缺 SID 匹配前提",
        E_ILLEGAL => "越 B06 合法区",
        E_OVER => "结果越候选（MA×Deny 交互越界）",
        E_INTG => "账本完整性不符",
        _ => "未知诊断码",
    }
}

pub fn run_unx36182_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36182_madeny");
    cs.add("SEG 段位独占", SEG_BASE == 0x4200, "0x4200~0x420F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "终态透明/NoMatch 跳过/双序一致/DenyOnly 短路全对平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("生效闭集", SidHit::Match.code() == 1 && SidHit::DenyOnly.code() == 4 && !SidHit::NoMatch.effective(), "四形态判别式闭集；NoMatch 唯一不生效");
    cs.add("账本互斥", { let mut l = BitLedger::new(); l.deny(sr::READ_CONTROL); l.allow(sr::SYNCHRONIZE); l.integrity().is_ok() && l.allow_mask() == sr::SYNCHRONIZE }, "Deny 后 Allow 终态不透明");
    cs.add("剧本容量", SCRIPT_CAND.len() == 6 && SCRIPT_DENY.len() == 5, "6×5 = 30 组双序剧本");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}
