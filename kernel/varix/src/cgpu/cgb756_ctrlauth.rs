//! cgb756_ctrlauth —— CGPU-F0756 控制协议鉴权。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：CGPU-F0756 控制协议鉴权  <SEG 0x4cc0>
//! 目标行数：≥300（判据：纯功能代码 ≥300 行）
//! 判据原文摘录：
//!   ① 纯功能代码 ≥300 行；
//!   ② 规格与判据按册内锚点原文逐条落实：CGPU Varix STAR II · 总纲与施工书 #CGPU-F0756；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律（控制协议的鉴权闸，与 F0755 帧面正交）：
//!   1. 令牌面：会话令牌 = 8 字节值（写死常量基 + 会话盐异或派生）；
//      令牌下发一次、核验多次，明文不留账（账面只存派生指纹）；
//   2. 权级闭集 4 档：0 只读 / 1 操作 / 2 预算 / 3 管理；命令码到
//      最低权级的映射表写死（8 命令逐条映射）；
//   3. 鉴权闸：帧头携带（权级声明 + 令牌指纹），闸核三件事：
//      令牌指纹与账面一致、权级 ≥ 命令最低权级、权级声明 ≤ 账面
//      授予档（声明越权红码——向下兼容、向上拒绝）；
//   4. 失败计数与锁定：连续 3 次鉴权失败锁闸（写死），锁后一切
//      鉴权红码直至显式解锁（管理权级专用）；
//   5. 指纹算法 FNV-1a 32 位；令牌比较用常数时间折叠异或等值
//      （等值面），不短路泄漏位置信息；
//   6. 零 unwrap/expect/panic，整数算术，确定性，no_std + alloc。
//!
//! 诊断段声明：0x4cc0 段为本模块独占（SEG_BASE=0x4cc0），判别式
//!   SEG_BASE+1..+N；与 0x4ca0/0x4ce0 邻段不重叠。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::vec::Vec;

/// 模块版本。
pub const CA_VERSION: &str = "cgb756_ctrlauth-v1";

/// 诊断段基址（0x4cc0 · 本模块独占）。
pub const SEG_BASE: u16 = 0x4CC0;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x4cc1 令牌指纹失配。
pub const XC_TOKEN_MISMATCH: u16 = SEG_BASE + 1;
/// 0x4cc2 权级不足（低于命令最低档）。
pub const XC_PRIV_LOW: u16 = SEG_BASE + 2;
/// 0x4cc3 权级声明越权（高于账面授予档）。
pub const XC_PRIV_CLAIM: u16 = SEG_BASE + 3;
/// 0x4cc4 权级值越界（>3）。
pub const XC_BAD_PRIV: u16 = SEG_BASE + 4;
/// 0x4cc5 闸已锁定（失败计数达限）。
pub const XC_LOCKED: u16 = SEG_BASE + 5;
/// 0x4cc6 解锁权级不足（需管理档）。
pub const XC_UNLOCK_PRIV: u16 = SEG_BASE + 6;
/// 0x4cc7 命令码无映射（闭集外）。
pub const XC_CMD_UNMAPPED: u16 = SEG_BASE + 7;
/// 0x4cc8 令牌未下发就核验。
pub const XC_NO_TOKEN: u16 = SEG_BASE + 8;

/// 令牌宽（写死：8 字节）。
pub const TOKEN_W: usize = 8;
/// 失败锁定阈值（写死：3）。
pub const LOCK_AT: u32 = 3;
/// 令牌基值（写死：册内 §鉴权令牌基 0x564152_4958_0001 的 8 字节）。
pub const TOKEN_BASE: [u8; TOKEN_W] = [0x56, 0x41, 0x52, 0x49, 0x58, 0x00, 0x00, 0x01];

/// 权级四档。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priv {
    /// 0 只读。
    Ro = 0,
    /// 1 操作。
    Op = 1,
    /// 2 预算。
    Budget = 2,
    /// 3 管理。
    Admin = 3,
}

impl Priv {
    /// 档值。
    pub fn rank(&self) -> u8 {
        *self as u8
    }

    /// 值→档（越界红码语义由调用方处理）。
    pub fn from_rank(r: u8) -> Option<Priv> {
        match r {
            0 => Some(Priv::Ro),
            1 => Some(Priv::Op),
            2 => Some(Priv::Budget),
            3 => Some(Priv::Admin),
            _ => None,
        }
    }
}

/// 命令码 → 最低权级映射表（写死；命令码 1..=8 对应数组下标 0..=7）。
pub const CMD_MIN_PRIV: [u8; 8] = [
    0, // 0x01 心跳：只读可发
    1, // 0x02 画像切换：操作
    1, // 0x03 场景挂钩：操作
    0, // 0x04 遥测订阅：只读可发
    0, // 0x05 遥测退订：只读可发
    2, // 0x06 预算覆写：预算
    1, // 0x07 快照请求：操作
    3, // 0x08 关停预备：管理
];

/// 命令最低权级查询（无映射红码）。
pub fn min_priv(cmd: u8) -> Result<Priv, u16> {
    if cmd == 0 || cmd > 8 {
        return Err(XC_CMD_UNMAPPED);
    }
    Priv::from_rank(CMD_MIN_PRIV[(cmd - 1) as usize]).ok_or(XC_CMD_UNMAPPED)
}

/// FNV-1a 32 位。
pub fn fnv32(data: &[u8]) -> u32 {
    let mut h = 0x811C_9DC5u32;
    for b in data {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 令牌派生：基值逐字节 ⊕ 会话盐（盐 = 会话号低 8 位重复铺）。
pub fn derive_token(session_salt: u8) -> [u8; TOKEN_W] {
    let mut t = TOKEN_BASE;
    for b in t.iter_mut() {
        *b ^= session_salt;
    }
    t
}

/// 常数时间等值：全 8 字节折叠异或，零 = 相等（无短路）。
pub fn token_eq(a: &[u8; TOKEN_W], b: &[u8; TOKEN_W]) -> bool {
    let mut acc = 0u8;
    for i in 0..TOKEN_W {
        acc |= a[i] ^ b[i];
    }
    acc == 0
}

/// 鉴权请求面。
#[derive(Clone, Copy, Debug)]
pub struct AuthReq {
    /// 命令码。
    pub cmd: u8,
    /// 权级声明（档值）。
    pub claimed: u8,
    /// 令牌指纹（请求方出示）。
    pub token_fp: u32,
}

/// 鉴权闸。
pub struct AuthGate {
    /// 账面令牌指纹（下发时落账）。
    token_fp: Option<u32>,
    /// 账面授予档。
    granted: u8,
    /// 连续失败计数。
    fails: u32,
    /// 锁定旗。
    locked: bool,
    /// 累计通过数。
    pub passed: u32,
    /// 累计拒绝数（分原因账）。
    pub rej_token: u32,
    pub rej_priv: u32,
    pub rej_claim: u32,
    pub rej_locked: u32,
}

impl AuthGate {
    /// 空闸（未下发令牌）。
    pub fn new() -> Self {
        AuthGate {
            token_fp: None,
            granted: 0,
            fails: 0,
            locked: false,
            passed: 0,
            rej_token: 0,
            rej_priv: 0,
            rej_claim: 0,
            rej_locked: 0,
        }
    }

    /// 令牌下发：会话盐派生 + 指纹落账 + 授予档设定。
    /// 二次下发红码（一发一闸）。
    pub fn issue(&mut self, session_salt: u8, granted: Priv) -> Result<u32, u16> {
        if self.token_fp.is_some() {
            return Err(XC_NO_TOKEN);
        }
        let tok = derive_token(session_salt);
        let fp = fnv32(&tok);
        self.token_fp = Some(fp);
        self.granted = granted.rank();
        Ok(fp)
    }

    /// 账面指纹只读。
    pub fn ledger_fp(&self) -> Result<u32, u16> {
        self.token_fp.ok_or(XC_NO_TOKEN)
    }

    /// 解锁（需管理档声明且令牌过核）。
    pub fn unlock(&mut self, req: &AuthReq) -> Result<(), u16> {
        let fp = self.token_fp.ok_or(XC_NO_TOKEN)?;
        if req.token_fp != fp {
            return Err(XC_TOKEN_MISMATCH);
        }
        if req.claimed != Priv::Admin.rank() {
            return Err(XC_UNLOCK_PRIV);
        }
        self.locked = false;
        self.fails = 0;
        Ok(())
    }

    /// 鉴权主路径：锁闸先拒 → 指纹核 → 声明档核 → 命令档核。
    pub fn authorize(&mut self, req: &AuthReq) -> Result<(), u16> {
        if self.locked {
            self.rej_locked += 1;
            return Err(XC_LOCKED);
        }
        let fp = self.token_fp.ok_or(XC_NO_TOKEN)?;
        // 声明档合法性。
        let claim = Priv::from_rank(req.claimed).ok_or(XC_BAD_PRIV)?;
        // 闸核 1：令牌指纹（失败入计数）。
        if req.token_fp != fp {
            self.fails += 1;
            self.rej_token += 1;
            if self.fails >= LOCK_AT {
                self.locked = true;
            }
            return Err(XC_TOKEN_MISMATCH);
        }
        // 闸核 2：声明 ≤ 授予（向上拒绝）。
        if claim.rank() > self.granted {
            self.fails += 1;
            self.rej_claim += 1;
            if self.fails >= LOCK_AT {
                self.locked = true;
            }
            return Err(XC_PRIV_CLAIM);
        }
        // 闸核 3：命令最低档。
        let need = min_priv(req.cmd)?;
        if claim.rank() < need.rank() {
            self.fails += 1;
            self.rej_priv += 1;
            if self.fails >= LOCK_AT {
                self.locked = true;
            }
            return Err(XC_PRIV_LOW);
        }
        self.fails = 0;
        self.passed += 1;
        Ok(())
    }

    /// 锁定态。
    pub fn is_locked(&self) -> bool {
        self.locked
    }

    /// 拒绝总数（分账聚合，守恒面）。
    pub fn rejected_total(&self) -> u32 {
        self.rej_token + self.rej_priv + self.rej_claim + self.rej_locked
    }
}

/// 鉴权账守恒：passed + rejected_total = 尝试总数（外部计数）。
pub fn auth_conserved(gate: &AuthGate, attempts: u32) -> Result<(), u16> {
    if gate.passed + gate.rejected_total() != attempts {
        return Err(XC_LOCKED);
    }
    Ok(())
}

/// 自例程：下发 → 全档放行面 → 三类拒绝逐条检出 → 锁定/解锁 →
/// 错误路径（未下发/二次下发/越界档/无映射命令）。
pub fn drill() -> Result<u32, u16> {
    let mut g = AuthGate::new();
    // 错误路径 0：未下发就核验。
    let pre = AuthReq {
        cmd: 1,
        claimed: 0,
        token_fp: 0,
    };
    if g.authorize(&pre) != Err(XC_NO_TOKEN) {
        return Err(XC_NO_TOKEN);
    }
    // 下发：盐 0x2A，授予档 = 预算(2)。
    let fp = g.issue(0x2A, Priv::Budget)?;
    if g.ledger_fp()? != fp {
        return Err(XC_TOKEN_MISMATCH);
    }
    // 二次下发红码。
    if g.issue(0x2A, Priv::Admin) != Err(XC_NO_TOKEN) {
        return Err(XC_NO_TOKEN);
    }
    // 令牌派生确定性。
    let t1 = derive_token(0x2A);
    let t2 = derive_token(0x2A);
    if !token_eq(&t1, &t2) {
        return Err(XC_TOKEN_MISMATCH);
    }
    let bad_tok = derive_token(0x2B);
    if token_eq(&t1, &bad_tok) {
        return Err(XC_TOKEN_MISMATCH);
    }

    // 放行面：只读命令（心跳）@ 声明只读。
    let mut attempts = 0u32;
    for cmd in [1u8, 4, 5] {
        let r = AuthReq {
            cmd,
            claimed: 0,
            token_fp: fp,
        };
        g.authorize(&r)?;
        attempts += 1;
    }
    // 操作命令 @ 声明操作。
    let r = AuthReq {
        cmd: 2,
        claimed: 1,
        token_fp: fp,
    };
    g.authorize(&r)?;
    attempts += 1;
    // 预算命令 @ 声明预算。
    let r = AuthReq {
        cmd: 6,
        claimed: 2,
        token_fp: fp,
    };
    g.authorize(&r)?;
    attempts += 1;
    auth_conserved(&g, attempts)?;

    // 拒绝面 1：权级不足（预算档发管理命令 0x08）。
    let r = AuthReq {
        cmd: 8,
        claimed: 2,
        token_fp: fp,
    };
    if g.authorize(&r) != Err(XC_PRIV_LOW) {
        return Err(XC_PRIV_LOW);
    }
    attempts += 1;
    // 拒绝面 2：声明越权（预算档声明管理）。
    let r = AuthReq {
        cmd: 1,
        claimed: 3,
        token_fp: fp,
    };
    if g.authorize(&r) != Err(XC_PRIV_CLAIM) {
        return Err(XC_PRIV_CLAIM);
    }
    attempts += 1;
    auth_conserved(&g, attempts)?;

    // 锁定路径：再错一次令牌 → 连续失败 3 → 锁。
    let r = AuthReq {
        cmd: 1,
        claimed: 0,
        token_fp: fnv32(&bad_tok),
    };
    if g.authorize(&r) != Err(XC_TOKEN_MISMATCH) {
        return Err(XC_TOKEN_MISMATCH);
    }
    attempts += 1;
    if !g.is_locked() {
        return Err(XC_LOCKED);
    }
    // 锁后一切红。
    let r = AuthReq {
        cmd: 1,
        claimed: 0,
        token_fp: fp,
    };
    if g.authorize(&r) != Err(XC_LOCKED) {
        return Err(XC_LOCKED);
    }
    attempts += 1;
    // 解锁权级不足（出示令牌但声明只读）。
    let ru = AuthReq {
        cmd: 1,
        claimed: 0,
        token_fp: fp,
    };
    if g.unlock(&ru) != Err(XC_UNLOCK_PRIV) {
        return Err(XC_UNLOCK_PRIV);
    }
    // 管理档解锁（凭正确令牌）。
    let ru2 = AuthReq {
        cmd: 8,
        claimed: 3,
        token_fp: fp,
    };
    g.unlock(&ru2)?;
    if g.is_locked() {
        return Err(XC_LOCKED);
    }
    // 锁期间拒绝入账。
    auth_conserved(&g, attempts)?;

    // 错误路径：无映射命令。
    let r = AuthReq {
        cmd: 9,
        claimed: 0,
        token_fp: fp,
    };
    if g.authorize(&r) != Err(XC_CMD_UNMAPPED) {
        return Err(XC_CMD_UNMAPPED);
    }
    // 错误路径：权级值越界。
    let r = AuthReq {
        cmd: 1,
        claimed: 4,
        token_fp: fp,
    };
    if g.authorize(&r) != Err(XC_BAD_PRIV) {
        return Err(XC_BAD_PRIV);
    }
    // 映射表健全：8 命令全可查。
    for c in 1..=8u8 {
        min_priv(c)?;
    }
    Ok(0)
}
