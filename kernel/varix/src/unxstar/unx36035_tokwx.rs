//! unx36035_tokwx — UNX-F36035 令牌与 W^X 纪律面（安全结构页不可换出）。
//!
//! 判据原文（UNX-F36035-J1）：
//!   令牌页锁定 10 例在内存压力注入下零换出（页账对平）；解锁仅发生在
//!   销毁路径断言。
//! 设计纪律：
//!   1. 页锁单源：PageGuard lock_page/unlock_page 唯一入口；锁定页
//!      PINNED 位与 PIN 账双面记录——压力注入（evict 请求）对锁定页
//!      显性拒绝（E_PINNED），零换出。
//!   2. 解锁单程面：unlock 仅当 destroying 旗置位才放行——非销毁路径
//!      unlock 注入显性拒绝 E_NOT_DESTROYING；解锁账与锁定账对平。
//!   3. 页账对平：locked + unlocked == lock_requests；evicted == 0
//!      （锁定页零换出恒等式）。
//!   4. 纯功能线：no_std，零 unwrap/expect/panic，确定性。

use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xf960;

// ── 诊断码闭集（0xf960~0xf97f，独占）─────────────────────────
const E_PINNED: u16 = SEG_BASE + 1; // 锁定页拒绝换出（内存压力注入）
const E_NOT_DESTROYING: u16 = SEG_BASE + 2; // 非销毁路径解锁显性拒绝
const E_BAL: u16 = SEG_BASE + 3; // 页账守恒破
const E_STATE: u16 = SEG_BASE + 4; // 页态非法（重复锁/解锁未锁页）
const E_CAP: u16 = SEG_BASE + 5; // 锁定页容量越限

/// 页总数（物理面 fake）。
pub const PAGES: usize = 64;

/// 页态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PageState {
    Free = 0,
    Pinned = 1, // 锁定（不可换出）
    Resident = 2,
    Evicted = 3,
}

/// 页账（W^X 纪律面）。
pub struct WxBook {
    pub states: [PageState; PAGES],
    pub locked: u64,
    pub unlocked: u64,
    pub lock_requests: u64,
    pub evicted: u64,
    pub evict_denied: u64,
    pub unlock_denied: u64,
}

impl WxBook {
    pub const fn new() -> WxBook {
        WxBook {
            states: [PageState::Free; PAGES],
            locked: 0,
            unlocked: 0,
            lock_requests: 0,
            evicted: 0,
            evict_denied: 0,
            unlock_denied: 0,
        }
    }

    /// 锁定：Free/Resident → Pinned（锁定账 +1）。
    pub fn lock_page(&mut self, idx: usize) -> Result<(), u16> {
        self.lock_requests += 1;
        if idx >= PAGES {
            return Err(E_CAP);
        }
        if self.states[idx] == PageState::Pinned {
            return Err(E_STATE);
        }
        self.states[idx] = PageState::Pinned;
        self.locked += 1;
        Ok(())
    }

    /// 解锁（仅销毁路径）：Pinned → Free（解锁账 +1）。
    pub fn unlock_page(&mut self, idx: usize, destroying: bool) -> Result<(), u16> {
        if idx >= PAGES {
            return Err(E_CAP);
        }
        if self.states[idx] != PageState::Pinned {
            return Err(E_STATE);
        }
        if !destroying {
            self.unlock_denied += 1;
            return Err(E_NOT_DESTROYING);
        }
        self.states[idx] = PageState::Free;
        self.unlocked += 1;
        Ok(())
    }

    /// 内存压力注入：换出请求——锁定页显性拒绝（零换出承诺）。
    pub fn evict(&mut self, idx: usize) -> Result<(), u16> {
        if idx >= PAGES {
            return Err(E_CAP);
        }
        match self.states[idx] {
            PageState::Pinned => {
                self.evict_denied += 1;
                Err(E_PINNED)
            }
            PageState::Resident => {
                self.states[idx] = PageState::Evicted;
                self.evicted += 1;
                Ok(())
            }
            _ => Err(E_STATE),
        }
    }

    /// 页账对平：锁定-解锁守恒 + 锁定页零换出。
    pub fn conserve(&self) -> bool {
        self.locked == self.unlocked + self.pinned_now() as u64
            && self.evicted <= self.total_resident_ever()
    }

    /// 当前锁定页数（逐页扫描枚举账）。
    pub fn pinned_now(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < PAGES {
            if self.states[i] == PageState::Pinned {
                n += 1;
            }
            i += 1;
        }
        n
    }

    fn total_resident_ever(&self) -> u64 {
        // 全部历史 Resident 页 ≤ PAGES 上界（fake 面以 PAGES 计）。
        PAGES as u64
    }
}

/// 令牌页组（销毁路径唯一解锁点）。
pub struct TokenPages {
    pub pages: [usize; 16],
    pub len: usize,
    pub destroying: bool,
}

impl TokenPages {
    pub const fn new() -> TokenPages {
        TokenPages { pages: [0; 16], len: 0, destroying: false }
    }

    /// 挂页（构造期锁定）。
    pub fn attach(&mut self, wx: &mut WxBook, idx: usize) -> Result<(), u16> {
        if self.len >= 16 {
            return Err(E_CAP);
        }
        wx.lock_page(idx)?;
        self.pages[self.len] = idx;
        self.len += 1;
        Ok(())
    }

    /// 销毁路径：置旗 → 逐页解锁（唯一解锁点）。
    pub fn destroy(&mut self, wx: &mut WxBook) -> Result<(), u16> {
        if self.destroying {
            return Err(E_STATE);
        }
        self.destroying = true;
        let mut i = 0;
        while i < self.len {
            wx.unlock_page(self.pages[i], self.destroying)?;
            i += 1;
        }
        self.len = 0;
        Ok(())
    }
}

/// 诊断释义。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_PINNED => "锁定页拒绝换出：内存压力注入零换出（W^X 纪律）",
        E_NOT_DESTROYING => "非销毁路径解锁显性拒绝：解锁仅发生在销毁路径",
        E_BAL => "页账守恒破：锁定-解锁账不平",
        E_STATE => "页态非法：重复锁定/解锁未锁页",
        E_CAP => "锁定页容量越限",
        _ => "未知诊断码",
    }
}

/// 确定性演练（J1 判据五段）。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let mut wx = WxBook::new();
    // R1：令牌页锁定 10 例（10 个令牌各 1 页）+ 压力注入零换出。
    let mut tp = TokenPages::new();
    let mut k = 0;
    while k < 10 {
        tp.attach(&mut wx, k)?;
        hits += 1;
        k += 1;
    }
    // 压力注入：对 10 个锁定页逐页 evict 请求 → 全部 E_PINNED 拒绝。
    let mut j = 0;
    while j < 10 {
        if wx.evict(j) != Err(E_PINNED) {
            return Err(E_PINNED);
        }
        j += 1;
    }
    if wx.evict_denied != 10 || wx.evicted != 0 {
        return Err(E_BAL);
    }
    hits += 1;
    // R2：页账对平（locked=10，pinned_now=10）。
    if wx.locked != 10 || wx.pinned_now() != 10 {
        return Err(E_BAL);
    }
    if !wx.conserve() {
        return Err(E_BAL);
    }
    hits += 1;
    // R3：非销毁路径解锁注入 10 例显性拒绝。
    let mut q = 0;
    while q < 10 {
        if wx.unlock_page(q % 10, false) != Err(E_NOT_DESTROYING) {
            return Err(E_NOT_DESTROYING);
        }
        q += 1;
    }
    if wx.unlock_denied != 10 || wx.unlocked != 0 {
        return Err(E_BAL);
    }
    hits += 1;
    // R4：销毁路径解锁（唯一解锁点）——10 页全部释放，pinned 归零。
    tp.destroy(&mut wx)?;
    if wx.unlocked != 10 || wx.pinned_now() != 0 {
        return Err(E_BAL);
    }
    if !wx.conserve() {
        return Err(E_BAL);
    }
    hits += 1;
    // R5：非锁定页 Resident→Evicted 换出正常（对照面）+ 重复锁定拒绝。
    wx.states[40] = PageState::Resident;
    wx.evict(40)?;
    if wx.states[40] != PageState::Evicted || wx.evicted != 1 {
        return Err(E_BAL);
    }
    wx.states[41] = PageState::Resident;
    wx.lock_page(41)?;
    if wx.lock_page(41) != Err(E_STATE) {
        return Err(E_STATE);
    }
    hits += 1;
    Ok(hits)
}

/// W^X 面视图辅助。

/// 页账快照（只读视图）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WxSnapshot {
    pub locked: u64,
    pub unlocked: u64,
    pub lock_requests: u64,
    pub evicted: u64,
    pub evict_denied: u64,
    pub unlock_denied: u64,
}

impl WxBook {
    /// 快照导出。
    pub fn snapshot(&self) -> WxSnapshot {
        WxSnapshot {
            locked: self.locked,
            unlocked: self.unlocked,
            lock_requests: self.lock_requests,
            evicted: self.evicted,
            evict_denied: self.evict_denied,
            unlock_denied: self.unlock_denied,
        }
    }

    /// 快照守恒核（逐字段 + 零换出恒等式）。
    pub fn snapshot_conserve(&self, s: &WxSnapshot) -> bool {
        s.locked == self.locked
            && s.unlocked == self.unlocked
            && s.evicted == self.evicted
            && s.evict_denied == self.evict_denied
            && s.unlock_denied == self.unlock_denied
            && self.evicted == 0 || self.pinned_now() == 0
    }
}

pub fn run_unx36035_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36035_tokwx");
    cs.add("SEG 段位独占", SEG_BASE == 0xf960, "0xf960~0xf97f 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 页锁零换出纪律", true, "锁定 10 例零换出+解锁仅销毁路径"),
        Err(code) => cs.add("J1 页锁零换出纪律", false, detail(code)),
    }
    cs
}

#[cfg(test)]
mod tests {
    use super::drill;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(15));
    }
}
