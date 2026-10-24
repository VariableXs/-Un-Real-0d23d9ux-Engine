//! UNX-F0129 · 恢复引导菜单（失败后自动进入）· 诊断段 0x2470
//! 验收锚点：UNX-F0129-J1 连败计数达阈自动进恢复菜单，人工选择
//! 正常/恢复/单次，三路分支全账。零浮点、零 unwrap、零 expect。


use alloc::vec::Vec;
use alloc::format;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x2470;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_CHOICE: u16 = SEG_BASE + 1; // 选项越纲（三路闭集外）
const E_TWICE: u16 = SEG_BASE + 2;  // 同次引导重复决策
const E_RDONLY: u16 = SEG_BASE + 3; // 菜单已闭，再选被拒
const E_BAL: u16 = SEG_BASE + 4;    // 分支账不平
const E_NONE: u16 = SEG_BASE + 5;   // 未决策出报告
const E_NOMENU: u16 = SEG_BASE + 6; // 菜单未开即选择

pub fn detail(code: u16) -> &'static str {
    match code {
        E_CHOICE => "选项越纲：[正常, 恢复, 单次恢复] 三路外",
        E_TWICE => "重复决策：一次引导一次选择",
        E_RDONLY => "菜单已闭：决策窗口关闭",
        E_BAL => "分支账不平：三路计数与总决策数不符",
        E_NONE => "未决策出报告：菜单开了没人选",
        E_NOMENU => "菜单未开：未触发即选择",
        _ => "未知诊断码",
    }
}

/// 引导决策三路。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootChoice {
    Normal,       // 正常引导
    Recovery,     // 恢复模式（持久）
    RecoveryOnce, // 单次恢复（下回回正常）
}

pub fn choice_name(c: BootChoice) -> &'static str {
    match c {
        BootChoice::Normal => "正常",
        BootChoice::Recovery => "恢复",
        BootChoice::RecoveryOnce => "单次恢复",
    }
}

/// 失败账：连败计数 + 阈值触发恢复菜单。
pub struct RecoveryMenu {
    pub threshold: u64,
    pub fail_streak: u64,
    pub menu_open: bool,
    pub chosen: Option<BootChoice>,
    pub closed: bool,
    pub counts: [u64; 3], // 三路分支账
    pub total_choices: u64,
    pub auto_enters: u64, // 自动触发次数
}

impl RecoveryMenu {
    pub fn new(threshold: u64) -> Result<RecoveryMenu, u16> {
        if threshold == 0 {
            return Err(E_BAL);
        }
        Ok(RecoveryMenu {
            threshold,
            fail_streak: 0,
            menu_open: false,
            chosen: None,
            closed: false,
            counts: [0; 3],
            total_choices: 0,
            auto_enters: 0,
        })
    }

    /// 记一次引导失败：连败 +1；达阈自动开菜单。
    pub fn record_fail(&mut self) -> Result<bool, u16> {
        if self.closed {
            return Err(E_RDONLY);
        }
        self.fail_streak += 1;
        if self.fail_streak >= self.threshold && !self.menu_open {
            self.menu_open = true;
            self.auto_enters += 1;
            return Ok(true); // 自动进入恢复菜单
        }
        Ok(false)
    }

    /// 记一次成功：连败清零（菜单若开着不关，等决策）。
    pub fn record_ok(&mut self) -> Result<(), u16> {
        if self.closed {
            return Err(E_RDONLY);
        }
        self.fail_streak = 0;
        Ok(())
    }

    /// 人工决策：菜单开着才可选；一引一选。
    pub fn choose(&mut self, c: BootChoice) -> Result<(), u16> {
        if self.closed {
            return Err(E_RDONLY);
        }
        if !self.menu_open {
            return Err(E_NOMENU);
        }
        if self.chosen.is_some() {
            return Err(E_TWICE);
        }
        self.chosen = Some(c);
        self.total_choices += 1;
        Ok(())
    }

    /// 闭菜单：决策落地生效（未决策禁止闭）。
    pub fn close(&mut self) -> Result<BootChoice, u16> {
        if self.closed {
            return Err(E_RDONLY);
        }
        match self.chosen {
            None => Err(E_NONE),
            Some(c) => {
                let idx = match c {
                    BootChoice::Normal => 0,
                    BootChoice::Recovery => 1,
                    BootChoice::RecoveryOnce => 2,
                };
                self.counts[idx] += 1;
                self.closed = true;
                // 单次恢复落地后连败账清（恢复引导本身算干预成功）。
                if c == BootChoice::Recovery || c == BootChoice::RecoveryOnce {
                    self.fail_streak = 0;
                }
                Ok(c)
            }
        }
    }

    /// 分支账守恒：三路之和 == 总决策数。
    pub fn audit(&self) -> Result<(), u16> {
        let sum: u64 = self.counts.iter().sum();
        if sum != self.total_choices {
            return Err(E_BAL);
        }
        Ok(())
    }

    pub fn render(&self) -> Result<String, u16> {
        if self.total_choices == 0 {
            return Err(E_NONE);
        }
        Ok(format!(
            "恢复菜单：正常{} 恢复{} 单次{}，自动触发 {} 次",
            self.counts[0], self.counts[1], self.counts[2], self.auto_enters
        ))
    }
}

// ── 确定性演练：合法 4 例 + 注入拦截 5 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：阈 3 → 连败三回自动开菜单 → 选恢复 → 闭账连败清零。
    let mut m = RecoveryMenu::new(3)?;
    if !m.record_fail()? || m.record_fail()? {
        return Err(E_BAL);
    }
    if !m.record_fail()? {
        return Err(E_BAL);
    }
    if !m.menu_open || m.auto_enters != 1 {
        return Err(E_BAL);
    }
    m.choose(BootChoice::Recovery)?;
    if m.close()? != BootChoice::Recovery {
        return Err(E_BAL);
    }
    if m.fail_streak != 0 {
        return Err(E_BAL);
    }
    m.audit()?;
    hits += 1;

    // R2：成功重置连败——两败一成再两败不触发（阈 3）。
    let mut m2 = RecoveryMenu::new(3)?;
    m2.record_fail()?;
    m2.record_fail()?;
    if m2.menu_open {
        return Err(E_BAL);
    }
    m2.record_ok()?;
    m2.record_fail()?;
    m2.record_fail()?;
    if m2.menu_open {
        return Err(E_BAL);
    }
    m2.record_fail()?;
    if !m2.menu_open {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：三路分支账各计一笔 + 渲染。
    let mut m3 = RecoveryMenu::new(1)?;
    m3.record_fail()?;
    m3.choose(BootChoice::Normal)?;
    m3.close()?;
    let mut m3b = RecoveryMenu::new(1)?;
    m3b.record_fail()?;
    m3b.choose(BootChoice::RecoveryOnce)?;
    m3b.close()?;
    m3.counts[2] = m3b.counts[2]; // 合账演示
    m3.total_choices += 1;
    m3.audit()?;
    let rep = m3.render()?;
    if !rep.contains("正常1") || !rep.contains("单次1") {
        return Err(E_BAL);
    }
    hits += 1;

    // R4：阈 1 极值——一败即开菜单。
    let mut m4 = RecoveryMenu::new(1)?;
    if !m4.record_fail()? {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：阈零建账 → E_BAL。
    if RecoveryMenu::new(0).is_ok() {
        return Err(E_BAL);
    }
    hits += 1;

    // I2：菜单未开即选择 → E_NOMENU。
    let mut i2 = RecoveryMenu::new(5)?;
    if i2.choose(BootChoice::Normal) != Err(E_NOMENU) {
        return Err(E_NOMENU);
    }
    hits += 1;

    // I3：重复决策 → E_TWICE；未决策闭账 → E_NONE。
    m4.choose(BootChoice::RecoveryOnce)?;
    if m4.choose(BootChoice::Normal) != Err(E_TWICE) {
        return Err(E_TWICE);
    }
    let mut i3b = RecoveryMenu::new(1)?;
    i3b.record_fail()?;
    if i3b.close() != Err(E_NONE) {
        return Err(E_NONE);
    }
    hits += 1;

    // I4：闭账后再操作（记败/选择/闭账）→ E_RDONLY。
    let mut i4 = RecoveryMenu::new(1)?;
    i4.record_fail()?;
    i4.choose(BootChoice::Recovery)?;
    i4.close()?;
    if i4.record_fail() != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    if i4.choose(BootChoice::Normal) != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    if i4.close() != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    hits += 1;

    // I5：分支账破注入 → E_BAL；空账渲染 → E_NONE。
    let mut i5 = RecoveryMenu::new(1)?;
    i5.record_fail()?;
    i5.choose(BootChoice::Normal)?;
    i5.close()?;
    i5.total_choices = 9;
    if i5.audit() != Err(E_BAL) {
        return Err(E_BAL);
    }
    let i5b = RecoveryMenu::new(1)?;
    if i5b.render() != Err(E_NONE) {
        return Err(E_NONE);
    }
    hits += 1;

    Ok(hits)
}
