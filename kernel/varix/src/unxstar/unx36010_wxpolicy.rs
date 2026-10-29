//! unx36010_wxpolicy — UNX-F36010 SID 不可变纪律与 W^X 页面置位
//! （接管 security.rs 纪律面锚）。
//!
//! 判据原文：
//!   J1 已发射 SID 落只读页 10 例写注入全部页 fault 拦截；W^X 状态
//!      切换账与 AI-04 纪律面逐条对平。
//! 设计纪律：
//!   1. 页模型：16 页 × 每页 8 SID 槽；页状态机 Writable→ReadOnly
//!      单向（W^X 纪律：只读页永不复写——RO→W 迁移在 API 层不存在）。
//!   2. 发射纪律：SID 落页即封页（emit+seal 原子序），封页后任何写
//!      注入（改指纹/改子权）→ 页 fault 拦截 + fault 账落痕。
//!   3. 对平面：切换账（每次 W→RO 迁移一行）与发射账逐条对平——
//!      封页数 == 切换行数 == 只读页数；违规迁移恒 0。
//!   4. 纯功能线：no_std，零 unwrap/expect/panic，确定性。

pub const SEG_BASE: u16 = 0x7390;

// ── 诊断码闭集（0x7390~0x73AF，独占）─────────────────────────
const E_FAULT: u16 = SEG_BASE + 1; // 写注入未被拦截（纪律破）
const E_RO2W: u16 = SEG_BASE + 2; // 只读页被改写（不可容忍迁移）
const E_PAGE: u16 = SEG_BASE + 3; // 页空间耗尽（显性拒）
const E_LEDGER: u16 = SEG_BASE + 4; // 切换账与发射账对平失配
const E_ANCHOR: u16 = SEG_BASE + 5; // 落页指纹对平失配
const E_STATE: u16 = SEG_BASE + 6; // 页状态机非法态

use crate::checks::CheckSet;
use super::unx36001_sidcore::Sid;

/// 页数。
pub const WX_PAGES: usize = 16;
/// 每页 SID 槽数。
pub const PAGE_SLOTS: usize = 8;

/// 页状态（单向：Writable → ReadOnly）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PageState {
    Writable = 0,
    ReadOnly = 1,
}

/// 单页：指纹槽 + 状态。
pub struct WxPage {
    pub state: PageState,
    fps: [u64; PAGE_SLOTS],
    used: [bool; PAGE_SLOTS],
}

impl WxPage {
    fn new() -> WxPage {
        WxPage { state: PageState::Writable, fps: [0; PAGE_SLOTS], used: [false; PAGE_SLOTS] }
    }
    fn free(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < PAGE_SLOTS {
            if !self.used[i] {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

/// W^X 纪律面：页表 + 切换账 + fault 账。
pub struct WxLedger {
    pages: [WxPage; WX_PAGES],
    /// 切换账：每次 W→RO 迁移登记页号（append-only）。
    switches: [u16; WX_PAGES],
    pub switch_count: usize,
    pub emitted: u32,
    pub faults: u32,
    pub sealed_pages: u32,
    pub violations: u32,
}

impl WxLedger {
    pub fn new() -> WxLedger {
        WxLedger {
            pages: [WxPage::new(), WxPage::new(), WxPage::new(), WxPage::new(),
                    WxPage::new(), WxPage::new(), WxPage::new(), WxPage::new(),
                    WxPage::new(), WxPage::new(), WxPage::new(), WxPage::new(),
                    WxPage::new(), WxPage::new(), WxPage::new(), WxPage::new()],
            switches: [0; WX_PAGES],
            switch_count: 0,
            emitted: 0,
            faults: 0,
            sealed_pages: 0,
            violations: 0,
        }
    }

    /// 发射：SID 独占一个新页（落页即封，原子序）——不可变纪律最强
    /// 形态：每个已发射 SID 自落页起恒在只读页上。
    pub fn emit(&mut self, s: &Sid) -> Result<(usize, usize), u16> {
        let fp = s.fingerprint();
        let mut p = 0;
        while p < WX_PAGES {
            if self.pages[p].state == PageState::Writable && self.pages[p].free() == PAGE_SLOTS {
                self.pages[p].fps[0] = fp;
                self.pages[p].used[0] = true;
                self.emitted += 1;
                self.seal(p)?;
                return Ok((p, 0));
            }
            p += 1;
        }
        Err(E_PAGE)
    }

    /// 封页：Writable → ReadOnly 单向迁移 + 切换账登记。
    pub fn seal(&mut self, page: usize) -> Result<(), u16> {
        if page >= WX_PAGES {
            return Err(E_STATE);
        }
        if self.pages[page].state == PageState::ReadOnly {
            return Ok(()); // 幂等：已只读页再封为空操作
        }
        self.pages[page].state = PageState::ReadOnly;
        if self.switch_count < WX_PAGES {
            self.switches[self.switch_count] = page as u16;
            self.switch_count += 1;
        }
        self.sealed_pages += 1;
        Ok(())
    }

    /// 写注入：对页槽改写指纹。只读页必须 fault 拦截（判据 J1 主面）。
    pub fn write_inject(&mut self, page: usize, slot: usize, new_fp: u64) -> Result<(), u16> {
        if page >= WX_PAGES || slot >= PAGE_SLOTS {
            return Err(E_STATE);
        }
        match self.pages[page].state {
            PageState::ReadOnly => {
                self.faults += 1;
                Err(E_FAULT) // 页 fault：只读页写注入拦截
            }
            PageState::Writable => {
                // 可写页写注入：纪律允许面（尚未封页），但本锚发射即封，
                // 此分支仅为 API 完整性保留。
                self.pages[page].fps[slot] = new_fp;
                Ok(())
            }
        }
    }

    /// 只读页读出口：指纹对平（页+槽 → 指纹）。
    pub fn read_fp(&self, page: usize, slot: usize) -> Result<u64, u16> {
        if page >= WX_PAGES || slot >= PAGE_SLOTS || !self.pages[page].used[slot] {
            return Err(E_ANCHOR);
        }
        Ok(self.pages[page].fps[slot])
    }

    /// 页状态读出口。
    pub fn state_of(&self, page: usize) -> Result<PageState, u16> {
        if page >= WX_PAGES {
            return Err(E_STATE);
        }
        Ok(self.pages[page].state)
    }

    /// 只读页计数。
    pub fn ro_pages(&self) -> u32 {
        let mut n = 0;
        let mut p = 0;
        while p < WX_PAGES {
            if self.pages[p].state == PageState::ReadOnly {
                n += 1;
            }
            p += 1;
        }
        n
    }

    /// 对平：切换行数 == 封页数 == 只读页数；违规迁移恒 0；
    /// 发射数与只读页占槽守恒。
    pub fn reconcile(&self) -> bool {
        if self.switch_count != self.sealed_pages as usize {
            return false;
        }
        if self.ro_pages() != self.sealed_pages {
            return false;
        }
        if self.violations != 0 {
            return false;
        }
        // 切换账逐行：登记页当前必须为只读。
        let mut i = 0;
        while i < self.switch_count {
            let p = self.switches[i] as usize;
            if self.pages[p].state != PageState::ReadOnly {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 诊断码释义。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_FAULT => "只读页写注入被页 fault 拦截",
        E_RO2W => "只读页被改写（不可容忍迁移）",
        E_PAGE => "页空间耗尽（显性拒绝）",
        E_LEDGER => "切换账与发射账对平失配",
        E_ANCHOR => "落页指纹对平失配",
        E_STATE => "页状态机非法态（页号/槽号越界）",
        _ => "未知诊断码",
    }
}

/// 确定性演练：发射落只读页 + 10 例写注入全拦截 + 切换账对平（判据 J1）。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut wx = WxLedger::new();

    // R1：10 例发射——逐例落页即封（逐发即封锚：每 SID 独占一页封一次），
    // 落页指纹与发射侧逐位对平。
    let mut seed: u32 = 0x360A_0001;
    let mut locs = [(0usize, 0usize); 10];
    let mut fps = [0u64; 10];
    let mut n = 0;
    while n < 10 {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let s = Sid::new(1, Sid::ia_from(5), &[21, seed])?;
        let fp = s.fingerprint();
        let (p, sl) = wx.emit(&s)?;
        if wx.read_fp(p, sl)? != fp {
            return Err(E_ANCHOR);
        }
        if wx.state_of(p)? != PageState::ReadOnly {
            return Err(E_STATE);
        }
        locs[n] = (p, sl);
        fps[n] = fp;
        n += 1;
    }
    if wx.emitted != 10 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R2：10 例写注入——对每个已发射槽注入伪指纹，全部页 fault 拦截，
    // fault 账逐例 +1，槽内指纹纹丝不动。
    let mut n = 0;
    while n < 10 {
        let (p, sl) = locs[n];
        let evil = 0xDEAD_BEEF_CAFE_0000u64 + n as u64;
        if wx.write_inject(p, sl, evil) != Err(E_FAULT) {
            return Err(E_FAULT);
        }
        if wx.read_fp(p, sl)? != fps[n] {
            return Err(E_RO2W);
        }
        n += 1;
    }
    if wx.faults != 10 {
        return Err(E_FAULT);
    }
    hits += 1;

    // R3：W^X 切换账对平——切换行数 == 封页数 == 只读页数 == 10
    //（逐发即封锚：10 发 10 封）；切换账逐行页号当前必为只读；
    // 违规迁移（RO→W）恒 0（API 层无迁移入口，violations 恒 0）。
    if !wx.reconcile() {
        return Err(E_LEDGER);
    }
    if wx.sealed_pages != 10 || wx.ro_pages() != 10 || wx.switch_count != 10 {
        return Err(E_LEDGER);
    }
    // 切换账页号唯一性（每页至多封一次）。
    let mut i = 0;
    while i < wx.switch_count {
        let mut j = i + 1;
        while j < wx.switch_count {
            if wx.switches[i] == wx.switches[j] {
                return Err(E_LEDGER);
            }
            j += 1;
        }
        i += 1;
    }
    hits += 1;

    // R4：页耗尽显性面——剩余 6 页逐发独占封页，第 17 发应显性 E_PAGE
    // 而非静默覆盖；对平面全程保持成立。
    let mut n = 0;
    while n < 6 {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let s = Sid::new(1, Sid::ia_from(5), &[99, seed])?;
        wx.emit(&s)?;
        n += 1;
    }
    seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    let s = Sid::new(1, Sid::ia_from(5), &[98, seed])?;
    if wx.emit(&s) != Err(E_PAGE) {
        return Err(E_PAGE);
    }
    if !wx.reconcile() {
        return Err(E_LEDGER);
    }
    if wx.sealed_pages != 16 || wx.ro_pages() != 16 || wx.emitted != 16 {
        return Err(E_LEDGER);
    }
    hits += 1;

    Ok(hits)
}

// ── 判据聚合（CheckSet 面 · 进 unxstar_blocks 检查表）────────
/// 判据 J1 聚合入口：SEG 锚 + 确定性演练全绿（红时回填诊断码）。
pub fn run_unx36010_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36010_wxpolicy");
    cs.add("SEG 段位独占", SEG_BASE == 0x7390, "0x7390~0x73AF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "演练段全对平、注入全拒"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs
}

#[cfg(test)]
mod tests {
    use super::drill;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(4));
    }
}
