//! unx36027_toksession — UNX-F36027 会话 ID 字段与 J2 会话账绑定锚。
//!
//! 判据原文（UNX-F36027-J1）：
//!   会话 ID 与 J2 会话账（fake）一致性 50 例全对；跨会话 ID 篡改注入
//!   10 例校验失败即隔离。
//! 设计纪律：
//!   1. J2 会话账（fake 面）：SessionBook 注册表 register(id) 发锚
//!      anchor（FNV-1a(id+盐)），anchor_of(id) 反查；会话 ID 绑定以
//!      「token.session + token.session_fp == anchor」双字段锁定。
//!   2. 篡改即隔离：verify_session 改动 session 后重算 fp 必失配 →
//!      隔离标记 quarantine 置位 + 隔离账入账；不回滚不修复（X3/X10
//!      母版纪律：损坏即作废）。
//!   3. 绑定账：bound + quarantined == verify_total（含重复验证）。
//!   4. 纯功能线：no_std，零 unwrap/expect/panic，确定性。

use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xf860;

// ── 诊断码闭集（0xf860~0xf87f，独占）─────────────────────────
const E_NOENT: u16 = SEG_BASE + 1; // 会话 ID 未注册
const E_FP: u16 = SEG_BASE + 2; // 会话指纹失配（篡改检出）
const E_BAL: u16 = SEG_BASE + 3; // 绑定账守恒破
const E_DUP: u16 = SEG_BASE + 4; // 会话重复注册
const E_CAP: u16 = SEG_BASE + 5; // 会话册容量越限

/// 会话册容量。
pub const S_MAX: usize = 64;

/// 会话 ID 盐（J2 fake 锚定；跨册一致）。
pub const SESSION_SALT: u64 = 0x5e55_1040_07a7;

/// J2 会话账（fake 注册表）。
pub struct SessionBook {
    pub ids: [u32; S_MAX],
    pub anchors: [u64; S_MAX],
    pub len: usize,
}

impl SessionBook {
    pub const fn new() -> SessionBook {
        SessionBook { ids: [0; S_MAX], anchors: [0; S_MAX], len: 0 }
    }

    /// 注册：会话 ID → 锚（FNV-1a(id ^ 盐)）。
    pub fn register(&mut self, id: u32) -> Result<u64, u16> {
        if self.len >= S_MAX {
            return Err(E_CAP);
        }
        let mut i = 0;
        while i < self.len {
            if self.ids[i] == id {
                return Err(E_DUP);
            }
            i += 1;
        }
        let a = anchor_of(id);
        self.ids[self.len] = id;
        self.anchors[self.len] = a;
        self.len += 1;
        Ok(a)
    }

    pub fn anchor_of(&self, id: u32) -> Result<u64, u16> {
        let mut i = 0;
        while i < self.len {
            if self.ids[i] == id {
                return Ok(self.anchors[i]);
            }
            i += 1;
        }
        Err(E_NOENT)
    }
}

/// 会话锚：FNV-1a(session ^ SESSION_SALT)。
pub fn anchor_of(id: u32) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let v = (id as u64) ^ SESSION_SALT;
    let mut sh = 56;
    loop {
        h ^= (v >> sh) as u8 as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        if sh == 0 {
            break;
        }
        sh -= 8;
    }
    h
}

/// 绑定账（token 面会话绑定对平）。
pub struct BindLedger {
    pub verify_total: u64,
    pub bound: u64,
    pub quarantined: u64,
}

impl BindLedger {
    pub const fn new() -> BindLedger {
        BindLedger { verify_total: 0, bound: 0, quarantined: 0 }
    }

    pub fn conserve(&self) -> bool {
        self.verify_total == self.bound + self.quarantined
    }
}

/// 令牌会话面（双字段锁定：session + session_fp）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TokenSession {
    pub session: u32,
    pub session_fp: u64,
    pub quarantined: bool,
}

/// 绑定（构造期：会话必须已注册，指纹即锚）。
pub fn bind(book: &SessionBook, id: u32) -> Result<TokenSession, u16> {
    let a = book.anchor_of(id)?;
    Ok(TokenSession { session: id, session_fp: a, quarantined: false })
}

/// 校验（运行期）：指纹失配 → 隔离标记 + 账入（不修复）。
pub fn verify_session(ts: &mut TokenSession, book: &SessionBook, led: &mut BindLedger) -> Result<(), u16> {
    led.verify_total += 1;
    if ts.quarantined {
        // 隔离后恒拒（不再放行）。
        led.quarantined += 1;
        return Err(E_FP);
    }
    match book.anchor_of(ts.session) {
        Ok(a) if a == ts.session_fp => {
            led.bound += 1;
            Ok(())
        }
        _ => {
            ts.quarantined = true;
            led.quarantined += 1;
            Err(E_FP)
        }
    }
}

/// 诊断释义。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_NOENT => "会话 ID 未注册：J2 会话账无此 ID",
        E_FP => "会话指纹失配：篡改检出即隔离（不修复）",
        E_BAL => "绑定账守恒破：bound + quarantined != verify_total",
        E_DUP => "会话重复注册",
        E_CAP => "会话册容量越限",
        _ => "未知诊断码",
    }
}

/// 确定性演练（J1 判据五段）。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    // R1：会话注册 50 例 + 绑定 50 例一致性全对。
    let mut book = SessionBook::new();
    let mut ids = [0u32; 50];
    let mut k = 0;
    while k < 50 {
        let id = 1000 + k as u32;
        let a1 = book.register(id)?;
        let a2 = book.anchor_of(id)?;
        if a1 != a2 || a1 != anchor_of(id) {
            return Err(E_NOENT);
        }
        let ts = bind(&book, id)?;
        if ts.session != id || ts.session_fp != a1 || ts.quarantined {
            return Err(E_FP);
        }
        ids[k] = id;
        hits += 1;
        k += 1;
    }
    // R2：50 例校验全过（绑定账 bound=50）。
    let mut led = BindLedger::new();
    let mut q = 0;
    while q < 50 {
        let mut ts = bind(&book, ids[q])?;
        verify_session(&mut ts, &book, &mut led)?;
        if ts.quarantined {
            return Err(E_BAL);
        }
        hits += 1;
        q += 1;
    }
    if led.bound != 50 || led.quarantined != 0 {
        return Err(E_BAL);
    }
    // R3：跨会话 ID 篡改注入 10 例——改 session 后指纹必失配即隔离。
    let mut j = 0;
    while j < 10 {
        let mut ts = bind(&book, ids[j])?;
        ts.session = ids[(j + 1) % 50]; // 跨会话篡改：借相邻会话 ID
        if verify_session(&mut ts, &book, &mut led) != Err(E_FP) || !ts.quarantined {
            return Err(E_FP);
        }
        // 隔离后恒拒。
        if verify_session(&mut ts, &book, &mut led) != Err(E_FP) {
            return Err(E_FP);
        }
        hits += 1;
        j += 1;
    }
    // R4：绑定账守恒（50 bound + 20 quarantined == 70 verify）。
    if led.bound != 50 || led.quarantined != 20 || led.verify_total != 70 {
        return Err(E_BAL);
    }
    if !led.conserve() {
        return Err(E_BAL);
    }
    hits += 1;
    // R5：未注册会话拒绝 + 重复注册拒绝。
    if book.anchor_of(999_999) != Err(E_NOENT) {
        return Err(E_NOENT);
    }
    if book.register(ids[0]) != Err(E_DUP) {
        return Err(E_DUP);
    }
    hits += 1;
    Ok(hits)
}

/// 会话册视图/批量辅助面。

/// 会话册快照（只读视图：len + 逐项 (id, anchor) 投影）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SessionSnapshot {
    pub len: usize,
    pub ids: [u32; S_MAX],
    pub anchors: [u64; S_MAX],
}

impl SessionBook {
    /// 快照导出。
    pub fn snapshot(&self) -> SessionSnapshot {
        let mut ids = [0u32; S_MAX];
        let mut anchors = [0u64; S_MAX];
        let mut i = 0;
        while i < self.len {
            ids[i] = self.ids[i];
            anchors[i] = self.anchors[i];
            i += 1;
        }
        SessionSnapshot { len: self.len, ids, anchors }
    }

    /// 快照守恒核（逐项对平 + 指纹重算复核）。
    pub fn snapshot_conserve(&self, s: &SessionSnapshot) -> bool {
        if s.len != self.len {
            return false;
        }
        let mut i = 0;
        while i < self.len {
            if s.ids[i] != self.ids[i] || s.anchors[i] != self.anchors[i] {
                return false;
            }
            if s.anchors[i] != anchor_of(s.ids[i]) {
                return false; // 指纹漂移检出
            }
            i += 1;
        }
        true
    }
}

/// 批量绑定面：N 例会话 ID 批量注册（J2 批量登录场景）。
pub fn batch_register(book: &mut SessionBook, ids: &[u32]) -> Result<u64, u16> {
    let mut n = 0u64;
    let mut i = 0;
    while i < ids.len() {
        book.register(ids[i])?;
        n += 1;
        i += 1;
    }
    Ok(n)
}

pub fn run_unx36027_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36027_toksession");
    cs.add("SEG 段位独占", SEG_BASE == 0xf860, "0xf860~0xf87f 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 会话绑定与篡改隔离", true, "绑定 50 例全对+篡改 10 例即隔离"),
        Err(code) => cs.add("J1 会话绑定与篡改隔离", false, detail(code)),
    }
    cs
}

#[cfg(test)]
mod tests {
    use super::drill;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(112));
    }
}
