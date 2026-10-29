//! unx36029_tokdestroy — UNX-F36029 令牌销毁与页回收（令牌页锁定分配消费）。
//!
//! 判据原文（UNX-F36029-J1）：
//!   销毁后敏感字段清零（覆写断言）100 例全过；页回收与 AI-04 锁存账对平；
//!   use-after-free 注入 10 例被隔离检测拦截。
//! 设计纪律：
//!   1. 清零覆写：destroy() 把敏感区（session_fp/luid 区/auth 包引用）
//!      逐字节置 0 后才降代币态——覆写断言读回全零；清零次序不可逆
//!      （代币态先死，结构体仍留清零残骸供审计）。
//!   2. 页账对平：AI-04 锁存账（fake 面 PageLedger）alloc/+1、free/-1；
//!      销毁路径 free 页必须与分配页一一对应（leak 即账破）。
//!   3. use-after-free：代币态 DEAD 后任何 access 显性 E_UAF 隔离拦截
//!      + 拦截账入账；拦截即隔离（不放行、不复活）。
//!   4. 纯功能线：no_std，零 unwrap/expect/panic，确定性。

use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xf8a0;

// ── 诊断码闭集（0xf8a0~0xf8bf，独占）─────────────────────────
const E_UAF: u16 = SEG_BASE + 1; // use-after-free：DEAD 后访问被隔离拦截
const E_LEAK: u16 = SEG_BASE + 2; // 页泄漏：free 数与 alloc 数不平
const E_BAL: u16 = SEG_BASE + 3; // 销毁账/清零账守恒破
const E_STATE: u16 = SEG_BASE + 4; // 代币态非法（重复销毁等）
const E_WIPE: u16 = SEG_BASE + 5; // 清零覆写失败：敏感区残留非零

/// 代币态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TokState {
    Live = 1,
    Dead = 2,
}

/// 敏感区定容（字节）。
pub const SENS_LEN: usize = 32;

/// AI-04 锁存账（fake 页账）。
pub struct PageLedger {
    pub allocated: u64,
    pub freed: u64,
    pub live: i64,
}

impl PageLedger {
    pub const fn new() -> PageLedger {
        PageLedger { allocated: 0, freed: 0, live: 0 }
    }

    pub fn alloc(&mut self) {
        self.allocated += 1;
        self.live += 1;
    }

    pub fn free(&mut self) -> Result<(), u16> {
        if self.live <= 0 {
            return Err(E_LEAK);
        }
        self.freed += 1;
        self.live -= 1;
        Ok(())
    }

    /// 对平：free 数与 alloc 数一致（零泄漏）。
    pub fn drained(&self) -> bool {
        self.allocated == self.freed && self.live == 0
    }
}

/// 可销毁令牌壳（敏感面 + 代币态 + 页锚）。
pub struct DestructToken {
    pub id: u32,
    pub state: TokState,
    pub session_fp: u64,
    pub luid_area: [u32; 4],
    pub auth_pkg_ref: u32,
    pub sensitive: [u8; SENS_LEN],
    pub pages: u32,
}

impl DestructToken {
    /// 样本构造（页锚 1 页）。
    pub fn sample(id: u32, led: &mut PageLedger) -> DestructToken {
        led.alloc();
        let mut s = [0u8; SENS_LEN];
        let mut i = 0;
        while i < SENS_LEN {
            s[i] = (i as u8) ^ 0x5a;
            i += 1;
        }
        DestructToken {
            id,
            state: TokState::Live,
            session_fp: 0xdead_beef_cafe_f00d ^ (id as u64),
            luid_area: [0x1111_1111, 0x2222_2222, 0x3333_3333, 0x4444_4444],
            auth_pkg_ref: 9,
            sensitive: s,
            pages: 1,
        }
    }

    /// 敏感区清零覆写（逐字节，读回断言面）。
    fn wipe(&mut self) {
        self.session_fp = 0;
        self.auth_pkg_ref = 0;
        let mut i = 0;
        while i < self.luid_area.len() {
            self.luid_area[i] = 0;
            i += 1;
        }
        let mut j = 0;
        while j < SENS_LEN {
            self.sensitive[j] = 0;
            j += 1;
        }
    }

    /// 敏感区读回全零断言。
    fn wiped_clean(&self) -> bool {
        if self.session_fp != 0 || self.auth_pkg_ref != 0 {
            return false;
        }
        let mut i = 0;
        while i < self.luid_area.len() {
            if self.luid_area[i] != 0 {
                return false;
            }
            i += 1;
        }
        let mut j = 0;
        while j < SENS_LEN {
            if self.sensitive[j] != 0 {
                return false;
            }
            j += 1;
        }
        true
    }
}

/// 销毁账（清零账 + 拦截账）。
pub struct DestroyLedger {
    pub destroyed: u64,
    pub wiped_ok: u64,
    pub uaf_intercepted: u64,
}

impl DestroyLedger {
    pub const fn new() -> DestroyLedger {
        DestroyLedger { destroyed: 0, wiped_ok: 0, uaf_intercepted: 0 }
    }

    /// 销毁：清零覆写 → 代币态 DEAD → 页回收（次序不可逆）。
    pub fn destroy(&mut self, t: &mut DestructToken, pages: &mut PageLedger) -> Result<(), u16> {
        if t.state != TokState::Live {
            return Err(E_STATE);
        }
        t.wipe();
        if !t.wiped_clean() {
            return Err(E_WIPE);
        }
        t.state = TokState::Dead;
        let mut p = 0;
        while p < t.pages {
            pages.free()?;
            p += 1;
        }
        self.destroyed += 1;
        self.wiped_ok += 1;
        Ok(())
    }

    /// 带账访问（use-after-free 拦截面）。
    pub fn access(&mut self, t: &DestructToken) -> Result<(u64, u32), u16> {
        if t.state != TokState::Live {
            self.uaf_intercepted += 1;
            return Err(E_UAF);
        }
        Ok((t.session_fp, t.auth_pkg_ref))
    }

    pub fn conserve(&self) -> bool {
        self.destroyed == self.wiped_ok
    }
}

/// 诊断释义。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_UAF => "use-after-free：DEAD 后访问被隔离拦截（不放行不复活）",
        E_LEAK => "页泄漏：free 数与 alloc 数不平（AI-04 锁存账破）",
        E_BAL => "销毁账守恒破",
        E_STATE => "代币态非法：重复销毁或态机漂移",
        E_WIPE => "清零覆写失败：敏感区残留非零",
        _ => "未知诊断码",
    }
}

/// 确定性演练（J1 判据五段）。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let mut pages = PageLedger::new();
    let mut led = DestroyLedger::new();
    // R1：销毁后敏感字段清零覆写断言 100 例全过。
    let mut k = 0;
    while k < 100 {
        let mut t = DestructToken::sample(100 + k, &mut pages);
        // 销毁前访问正常（带账访问面）。
        led.access(&t)?;
        led.destroy(&mut t, &mut pages)?;
        // 覆写断言：敏感区读回全零。
        if t.session_fp != 0 || t.auth_pkg_ref != 0 || t.luid_area != [0u32; 4] {
            return Err(E_WIPE);
        }
        if t.sensitive != [0u8; SENS_LEN] {
            return Err(E_WIPE);
        }
        hits += 1;
        k += 1;
    }
    // R2：页回收与 AI-04 锁存账对平（100 alloc == 100 free，live=0）。
    if pages.allocated != 100 || pages.freed != 100 || !pages.drained() {
        return Err(E_LEAK);
    }
    hits += 1;
    // R3：use-after-free 注入 10 例被隔离检测拦截。
    let mut q = 0;
    while q < 10 {
        let mut t = DestructToken::sample(900 + q, &mut pages);
        led.destroy(&mut t, &mut pages)?;
        if led.access(&t) != Err(E_UAF) {
            return Err(E_UAF);
        }
        // 拦截即隔离：重复访问仍拒（不复活）。
        if led.access(&t) != Err(E_UAF) {
            return Err(E_UAF);
        }
        hits += 1;
        q += 1;
    }
    if led.uaf_intercepted != 20 {
        return Err(E_BAL); // 10 例 × 2 次访问 = 20 拦截
    }
    // R4：重复销毁拒绝（态机单程）。
    let mut t2 = DestructToken::sample(999, &mut pages);
    led.destroy(&mut t2, &mut pages)?;
    if led.destroy(&mut t2, &mut pages) != Err(E_STATE) {
        return Err(E_STATE);
    }
    if !led.conserve() {
        return Err(E_BAL);
    }
    hits += 1;
    // R5：销毁账终值（100 + 10 + 1 destroyed == wiped_ok）。
    if led.destroyed != 111 || led.wiped_ok != 111 {
        return Err(E_BAL);
    }
    hits += 1;
    Ok(hits)
}

/// 销毁面视图/批量辅助。

/// 销毁账快照（只读视图）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DestroySnapshot {
    pub destroyed: u64,
    pub wiped_ok: u64,
    pub uaf_intercepted: u64,
}

impl DestroyLedger {
    /// 快照导出。
    pub fn snapshot(&self) -> DestroySnapshot {
        DestroySnapshot {
            destroyed: self.destroyed,
            wiped_ok: self.wiped_ok,
            uaf_intercepted: self.uaf_intercepted,
        }
    }

    /// 快照守恒核。
    pub fn snapshot_conserve(&self, s: &DestroySnapshot) -> bool {
        s.destroyed == self.destroyed
            && s.wiped_ok == self.wiped_ok
            && s.uaf_intercepted == self.uaf_intercepted
            && s.destroyed == s.wiped_ok
    }
}

impl PageLedger {
    /// 页账快照（AI-04 对平投影）。
    pub fn snapshot(&self) -> (u64, u64, i64) {
        (self.allocated, self.freed, self.live)
    }
}

pub fn run_unx36029_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36029_tokdestroy");
    cs.add("SEG 段位独占", SEG_BASE == 0xf8a0, "0xf8a0~0xf8bf 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 销毁清零与页回收", true, "清零 100 例全过+UAF 10 例拦截+页账对平"),
        Err(code) => cs.add("J1 销毁清零与页回收", false, detail(code)),
    }
    cs
}

#[cfg(test)]
mod tests {
    use super::drill;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(113));
    }
}
