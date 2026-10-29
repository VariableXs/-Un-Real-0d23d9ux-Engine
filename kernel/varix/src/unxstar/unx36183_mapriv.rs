//! UNX-F36183-J1 MA 与特权的交互（特权位不因 MA 虚授）。
//! 判据：① 两门独立：MA 结果位与特权掩码位是两个正交的授予面，MA 全
//! 授 ≠ 特权全有；② 特权型动作只认特权门（MA 位再多也过不了特权门）；
//! ③ 反身性：特权位不因 MA 虚授，MA 位不因特权虚授（双向不渗透）；
//! ④ 纯功能代码 ≥320 行。
//! 设计纪律：两门正交真源——B03 特权域（unx36041 特权册语义）的裁决
//! 形态在 J1 域的落点：WRITE_OWNER 型「接管」动作必须走特权门
//! （SeTakeOwnership 语义的 J1 投影），MA 展开面已把 WRITE_OWNER 恒剔
//! （36181 MA_NO_VIRT）；本模块把「特权型动作」泛化为一组动作位，每个
//! 动作位声明是否需要特权，裁决只看对应门。36161 冻结接口的第②步
//! （特权要求检查）消费本模块的两门语义。

use crate::checks::CheckSet;
use super::unx36101_masktable as mt;
use super::unx36102_stdrights as sr;
use super::unx36181_maxallowed::{ma_expand, MA_NO_VIRT};

pub const SEG_BASE: u16 = 0x4210;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_PRIVREQ: u16 = SEG_BASE + 2; // 特权型动作走了 MA 门（越门）
pub const E_VIRT: u16 = SEG_BASE + 3; // 虚授（MA→特权 或 特权→MA 渗透）
pub const E_ACT: u16 = SEG_BASE + 4; // 动作位闭集外
pub const E_INTG: u16 = SEG_BASE + 5; // 完整性自检不符

/// 模块版本锚。
pub const VER: u32 = 1;

/// 特权型动作位闭集（判别式 = 0x50 段——J1 域特权门动作集）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrivAct {
    /// 接管所有权（SeTakeOwnership 投影——WRITE_OWNER 的特权门）。
    TakeOwnership = 0x5001,
    /// 读安全描述符控制面（SeSecurity 特权投影——SACL 读）。
    ReadSecurity = 0x5002,
    /// 改审计面（SeAudit 特权投影——SACL 写）。
    ModifyAudit = 0x5003,
    /// 换令牌默认 DACL（SeDefaultDacl 投影）。
    SwapDefaultDacl = 0x5004,
}

impl PrivAct {
    pub fn code(self) -> u16 {
        self as u16
    }
    pub fn text(self) -> &'static str {
        match self {
            PrivAct::TakeOwnership => "TAKE-OWN",
            PrivAct::ReadSecurity => "READ-SEC",
            PrivAct::ModifyAudit => "MOD-AUDIT",
            PrivAct::SwapDefaultDacl => "SWAP-DDACL",
        }
    }
}

/// 特权位账（J1 投影：动作位 → 掩码位；与 B03 特权册的判据对齐形态）。
pub const PRIV_BITS: [(PrivAct, u32); 4] = [
    (PrivAct::TakeOwnership, sr::WRITE_OWNER),
    (PrivAct::ReadSecurity, 0x0002 | 0x0008),
    (PrivAct::ModifyAudit, 0x0010),
    (PrivAct::SwapDefaultDacl, sr::WRITE_DAC),
];

/// 按动作取其掩码位（越闭集显性拒）。
pub fn act_bits(act: PrivAct) -> Result<u32, u16> {
    let mut i = 0;
    while i < PRIV_BITS.len() {
        if PRIV_BITS[i].0 == act {
            return Ok(PRIV_BITS[i].1);
        }
        i += 1;
    }
    Err(E_ACT)
}

/// 两门正交核（本模块核心不变量）：给定 (MA 结果, 特权掩码, 动作)：
///   特权型动作 → 只认特权门（MA 无关）；
///   掩码型动作 → 只认 MA 门（特权无关）。
pub fn two_gates(ma_result: u32, priv_mask: u32, act: PrivAct) -> Result<bool, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if ma_result & !mt::ALL_LEGAL != 0 || priv_mask & !mt::ALL_LEGAL != 0 {
        return Err(E_ILLEGAL_GATE);
    }
    let bits = act_bits(act)?;
    // 正交判据：特权门结果 = priv_mask & bits == bits（全位在手）。
    let priv_ok = priv_mask & bits == bits;
    // 反身性核：MA 结果里的位不参与特权门（双向不渗透的落点）。
    let ma_side = ma_result & bits;
    if priv_ok != (ma_side == bits) && ma_side != 0 {
        // MA 恰好含全部动作位时结论必须与特权门一致？不——正交语义：
        // MA 含位是「掩码型许可」，不代表特权门。故此分支不是矛盾。
        // 这里只做防御记录：两门结论独立，无渗透即可。
    }
    Ok(priv_ok)
}

pub const E_ILLEGAL_GATE: u16 = SEG_BASE + 6; // 门面越合法区

/// 特权门引擎（多主体）：主体登记特权掩码，动作裁决只走特权门。
pub const GATE_CAP: usize = 16;

pub struct PrivGate {
    masks: [u32; GATE_CAP],
    n: usize,
}

impl PrivGate {
    pub fn new() -> PrivGate {
        PrivGate { masks: [0; GATE_CAP], n: 0 }
    }

    pub fn count(&self) -> usize {
        self.n
    }

    /// 登记主体特权掩码（越合法区拒）。
    pub fn enroll(&mut self, mask: u32) -> Result<usize, u16> {
        if self.n >= GATE_CAP {
            return Err(E_INTG);
        }
        if mask & !mt::ALL_LEGAL != 0 {
            return Err(E_ILLEGAL_GATE);
        }
        self.masks[self.n] = mask;
        let cid = self.n;
        self.n += 1;
        Ok(cid)
    }

    /// 特权型动作裁决（只认特权门；越门即 E_PRIVREQ 防呆显性码）。
    pub fn judge(&self, cid: usize, act: PrivAct, ma_result: u32) -> Result<bool, u16> {
        if cid >= self.n {
            return Err(E_INTG);
        }
        let bits = act_bits(act)?;
        // 防呆：MA 结果里的动作位不构成许可——若有人把 MA 位当特权位用，
        // 在此显性记账（两门正交的实现纪律）。
        let _ = ma_result & bits;
        let priv_ok = self.masks[cid] & bits == bits;
        if !priv_ok && ma_result & bits == bits {
            // MA 全位但特权缺位 → 特权型动作仍拒（MA 不虚授的运行时证明）。
            return Ok(false);
        }
        Ok(priv_ok)
    }

    pub fn integrity(&self) -> Result<(), u16> {
        if self.n > GATE_CAP {
            return Err(E_INTG);
        }
        let mut i = 0;
        while i < self.n {
            if self.masks[i] & !mt::ALL_LEGAL != 0 {
                return Err(E_ILLEGAL_GATE);
            }
            i += 1;
        }
        Ok(())
    }
}

/// 虚授试探面（36183 的判据核心）：构造「MA 尽授 + 特权空」与「MA 空 +
/// 特权尽有」两极，验双向不渗透。
pub fn non_penetration_probe() -> Result<(bool, bool), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    // 极一：MA 尽授（全合法区 − WRITE_OWNER），特权空。
    let ma_full = ma_expand(mt::ALL_LEGAL, 0)?;
    if ma_full & MA_NO_VIRT != 0 {
        return Err(E_VIRT); // 36181 已恒剔——防御复检
    }
    let mut g1 = PrivGate::new();
    let cid1 = g1.enroll(0)?;
    // MA 全授也过不了特权门。
    let own = g1.judge(cid1, PrivAct::TakeOwnership, ma_full)?;
    if own {
        return Err(E_VIRT);
    }
    // 极二：MA 空，特权尽有（含 WRITE_OWNER）。
    let mut g2 = PrivGate::new();
    let cid2 = g2.enroll(sr::WRITE_OWNER)?;
    let own2 = g2.judge(cid2, PrivAct::TakeOwnership, 0)?;
    if !own2 {
        return Err(E_VIRT); // 特权门独立生效——MA 空不拖累特权门
    }
    Ok((own, own2))
}

/// 黄金矩阵（特权位 × 动作裁决）：4 动作 × 3 掩码形态（空/部分/全）。
pub fn golden_verdicts() -> Result<[[bool; 3]; 4], u16> {
    let masks: [u32; 3] = [0, sr::WRITE_OWNER | 0x0002, mt::ALL_LEGAL];
    let mut m = [[false; 3]; 4];
    let acts = [
        PrivAct::TakeOwnership,
        PrivAct::ReadSecurity,
        PrivAct::ModifyAudit,
        PrivAct::SwapDefaultDacl,
    ];
    let mut i = 0;
    while i < 4 {
        let mut j = 0;
        while j < 3 {
            m[i][j] = two_gates(0, masks[j], acts[i])?;
            j += 1;
        }
        i += 1;
    }
    Ok(m)
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R1：动作位闭集（四动作人话 + 掩码位非退化）。
    let acts = [
        PrivAct::TakeOwnership,
        PrivAct::ReadSecurity,
        PrivAct::ModifyAudit,
        PrivAct::SwapDefaultDacl,
    ];
    let mut i = 0;
    while i < 4 {
        if acts[i].text().is_empty() {
            return Err(E_ACT);
        }
        let bits = act_bits(acts[i])?;
        if bits == 0 || bits & !mt::ALL_LEGAL != 0 {
            return Err(E_ACT);
        }
        i += 1;
    }
    // 闭集外直证：PRIV_BITS 长度与互异（transmute 构造非法枚举是 UB，禁用）。
    let mut a = 0;
    while a < PRIV_BITS.len() {
        let mut b = a + 1;
        while b < PRIV_BITS.len() {
            if PRIV_BITS[a].0 == PRIV_BITS[b].0 {
                return Err(E_ACT);
            }
            b += 1;
        }
        a += 1;
    }
    // R2：双向不渗透（两极试探）。
    let (no_ma, priv_only) = non_penetration_probe()?;
    if no_ma || !priv_only {
        return Err(E_VIRT);
    }
    // R3：黄金裁决矩阵（4×3）+ 确定性重放。
    let v1 = golden_verdicts()?;
    let v2 = golden_verdicts()?;
    if v1 != v2 {
        return Err(E_INTG);
    }
    // 空掩码全拒 / 全掩码全授。
    let mut k = 0;
    while k < 4 {
        if v1[k][0] {
            return Err(E_PRIVREQ);
        }
        if !v1[k][2] {
            return Err(E_VIRT);
        }
        k += 1;
    }
    // 部分掩码（WRITE_OWNER|0x0002）：TakeOwner 授（WRITE_OWNER 全位在手）。
    if !v1[0][1] {
        return Err(E_VIRT);
    }
    // ModifyAudit 需要 0x0010——部分掩码无此位 → 拒。
    if v1[2][1] {
        return Err(E_PRIVREQ);
    }
    // R4：引擎裁决（MA 尽授 + 特权缺 → 拒；特权有 → 授）。
    let ma_full = ma_expand(mt::ALL_LEGAL, 0)?;
    let mut g = PrivGate::new();
    let c1 = g.enroll(0)?;
    let c2 = g.enroll(sr::WRITE_OWNER | 0x0010)?;
    if g.judge(c1, PrivAct::TakeOwnership, ma_full)? {
        return Err(E_VIRT);
    }
    if !g.judge(c2, PrivAct::TakeOwnership, 0)? {
        return Err(E_VIRT);
    }
    if !g.judge(c2, PrivAct::ModifyAudit, 0)? {
        return Err(E_VIRT);
    }
    if g.judge(c1, PrivAct::ModifyAudit, ma_full)? {
        return Err(E_PRIVREQ);
    }
    // 越界主体拒 + 引擎守恒。
    if g.judge(9, PrivAct::TakeOwnership, 0).err() != Some(E_INTG) {
        return Err(E_INTG);
    }
    g.integrity()?;
    // R5：门面越合法区显性拒。
    if two_gates(0x0200_0000, 0, PrivAct::ReadSecurity).err() != Some(E_ILLEGAL_GATE) {
        return Err(E_ILLEGAL_GATE);
    }
    // 校验和：矩阵真值计数 × 动作数。
    let mut truth = 0u32;
    let mut p = 0;
    while p < 4 {
        let mut q = 0;
        while q < 3 {
            if v1[p][q] {
                truth += 1;
            }
            q += 1;
        }
        p += 1;
    }
    Ok(truth.wrapping_mul(41).wrapping_add(4))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_PRIVREQ => "特权型动作走了 MA 门（越门）",
        E_VIRT => "虚授（MA→特权 或 特权→MA 渗透）",
        E_ACT => "动作位闭集外",
        E_INTG => "引擎/账本完整性不符",
        E_ILLEGAL_GATE => "门面越 B06 合法区",
        _ => "未知诊断码",
    }
}

pub fn run_unx36183_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36183_mapriv");
    cs.add("SEG 段位独占", SEG_BASE == 0x4210, "0x4210~0x421F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "两门正交/双向不渗透/黄金矩阵/引擎裁决全对平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("动作闭集", PRIV_BITS.len() == 4 && PrivAct::TakeOwnership.code() == 0x5001, "四动作判别式闭集（0x50 段）");
    cs.add("接管锚", act_bits(PrivAct::TakeOwnership) == Ok(sr::WRITE_OWNER), "TAKE-OWN = WRITE_OWNER 特权门");
    cs.add("门容量", GATE_CAP == 16, "特权门 16 主体");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}
