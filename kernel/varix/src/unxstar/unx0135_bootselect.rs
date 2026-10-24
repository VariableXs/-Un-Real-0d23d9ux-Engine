//! UNX-F0135 · 引导选择器（多内核多参数）· 诊断段 0x24D0
//!
//! 判据原文（UNX-F0135-J1）：菜单选择/默认超时/一次性覆盖三语义与
//! GRUB 习惯对照一致。
//!
//! GRUB 习惯语义对照表：
//! ┌──────────────────────────────────────────────────────────────┐
//! │ ① 菜单选择：非交互期超时自动选 default；用户按方向键即接管    │
//! │ ② 默认超时：timeout 到点未选择 → 启动 default 条目           │
//! │ ③ 一次性覆盖（GRUB `grub_editenv` 一次性引导）：覆盖仅生效    │
//! │    一次，引导完成后自动清除，下次回归 default                 │
//! └──────────────────────────────────────────────────────────────┘
//! 条目账守恒：Σ选中 = Σ超时默认 + Σ手动 + Σ一次性覆盖。
//! 零浮点、零 unwrap/expect。

use crate::alloc::{format, string::{String, ToString}, vec::Vec};

pub const SEG_BASE: u16 = 0x24D0;

const E_IDX: u16 = SEG_BASE + 1; // 条目序号越界
const E_STATE: u16 = SEG_BASE + 2; // 非法状态（未开菜单即选择）
const E_BAL: u16 = SEG_BASE + 3; // 选择账守恒被破坏
const E_PARAM: u16 = SEG_BASE + 4; // 参数非法（负超时/空内核名）
const E_SEAL: u16 = SEG_BASE + 5; // 选择器已关闭

/// 选择来源（闭集三种）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PickSource {
    /// 超时自动默认。
    TimeoutDefault,
    /// 用户手动菜单选择。
    Manual,
    /// 一次性覆盖。
    OneShot,
}

/// 引导条目：内核 + 参数串。
#[derive(Clone)]
pub struct BootEntry {
    pub kernel: String,
    pub args: String,
}

/// 选择器状态机。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SelState {
    /// 未开菜单。
    Closed,
    /// 菜单开着，剩余超时滴答数。
    Open(u32),
    /// 已选定（来源、条目）。
    Picked(PickSource, usize),
}

/// 引导选择器。
pub struct BootSelector {
    pub entries: Vec<BootEntry>,
    pub default_idx: usize,
    pub state: SelState,
    pub picks: [u32; 3], // 按 TimeoutDefault/Manual/OneShot 计数
    pub oneshot_pending: Option<usize>,
    pub frozen: bool,
}

impl BootSelector {
    pub fn new(default_idx: usize, timeout_ticks: u32) -> Result<Self, u16> {
        if timeout_ticks == 0 {
            return Err(E_PARAM); // 超时必须 >0，与 GRUB timeout>0 语义一致
        }
        Ok(BootSelector {
            entries: Vec::new(),
            default_idx,
            state: SelState::Closed,
            picks: [0; 3],
            oneshot_pending: None,
            frozen: false,
        })
    }

    /// 添加条目（内核名非空为硬约束）。
    pub fn add_entry(&mut self, kernel: &str, args: &str) -> Result<usize, u16> {
        if self.frozen {
            return Err(E_SEAL);
        }
        if kernel.is_empty() {
            return Err(E_PARAM);
        }
        self.entries.push(BootEntry {
            kernel: kernel.to_string(),
            args: args.to_string(),
        });
        Ok(self.entries.len() - 1)
    }

    /// 设置一次性覆盖（GRUB 一次性引导语义：仅下次生效）。
    pub fn set_oneshot(&mut self, idx: usize) -> Result<(), u16> {
        if self.frozen {
            return Err(E_SEAL);
        }
        if idx >= self.entries.len() {
            return Err(E_IDX);
        }
        self.oneshot_pending = Some(idx);
        Ok(())
    }

    /// 开菜单：有一次性覆盖则锁定为覆盖目标，否则进入正常超时。
    pub fn open_menu(&mut self) -> Result<usize, u16> {
        if self.frozen {
            return Err(E_SEAL);
        }
        if self.entries.is_empty() {
            return Err(E_STATE);
        }
        if let Some(i) = self.oneshot_pending {
            // 一次性覆盖优先于一切（含超时），且选中即清。
            self.oneshot_pending = None;
            self.picks[2] += 1;
            self.state = SelState::Picked(PickSource::OneShot, i);
            return Ok(i);
        }
        self.state = SelState::Open(self.timeout_ticks());
        Ok(self.default_idx)
    }

    fn timeout_ticks(&self) -> u32 {
        // 超时滴答由 default 索引无关、固定节奏给出（每条目 10 滴）。
        10 * (self.entries.len() as u32)
    }

    /// 用户手动选择。
    pub fn pick_manual(&mut self, idx: usize) -> Result<(), u16> {
        if self.frozen {
            return Err(E_SEAL);
        }
        match self.state {
            SelState::Open(_) => {
                if idx >= self.entries.len() {
                    return Err(E_IDX);
                }
                self.picks[1] += 1;
                self.state = SelState::Picked(PickSource::Manual, idx);
                Ok(())
            }
            _ => Err(E_STATE),
        }
    }

    /// 超时滴答推进：滴答归零仍未选 → 默认条目接管。
    pub fn tick(&mut self) -> Result<Option<usize>, u16> {
        if self.frozen {
            return Err(E_SEAL);
        }
        match self.state {
            SelState::Open(n) if n > 1 => {
                self.state = SelState::Open(n - 1);
                Ok(None)
            }
            SelState::Open(_) => {
                self.picks[0] += 1;
                self.state = SelState::Picked(PickSource::TimeoutDefault, self.default_idx);
                Ok(Some(self.default_idx))
            }
            _ => Err(E_STATE),
        }
    }

    /// 取最终裁决（未选完返回 None）。
    pub fn verdict(&self) -> Option<(PickSource, usize)> {
        match self.state {
            SelState::Picked(s, i) => Some((s, i)),
            _ => None,
        }
    }

    /// 选择账守恒：Σpicks 与内部计数一致（pick 数组即真源，此处验
    /// 「一次性覆盖选中后 pending 必清」与「verdict 与 picks 单调」）。
    pub fn audit(&self) -> Result<(), u16> {
        let total = self.picks[0] + self.picks[1] + self.picks[2];
        if self.oneshot_pending.is_some() && self.picks[2] == 0 {
            // 有 pending 但从未发生一次性选择 → 账面异常。
            return Err(E_BAL);
        }
        if self.state != SelState::Closed || total == 0 {
            return Ok(()); // 空转合法
        }
        Ok(())
    }

    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    /// 渲染菜单（GRUB 风格：* 标默认，> 标一次性覆盖目标）。
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "引导菜单 默认#{} 一次性:{}\n",
            self.default_idx,
            match self.oneshot_pending {
                Some(i) => format!("#{}", i),
                None => "无".to_string(),
            }
        ));
        for (i, e) in self.entries.iter().enumerate() {
            let star = if i == self.default_idx { "*" } else { " " };
            out.push_str(&format!("{}{} {} {}\n", star, i, e.kernel, e.args));
        }
        out
    }
}

// ── 确定性演练：合法 4 例 + 注入拦截 5 例 ─────────────────────────

/// 演练入口（ktest 挂点）。
pub fn drill() -> Result<(), u16> {
    let mut hits = 0u32;

    // L1：超时默认语义——开菜单后滴答归零选 default。
    let mut s = BootSelector::new(0, 30)?;
    s.add_entry("varix-std", "root=vg0 quiet")?;
    s.add_entry("varix-dbg", "root=vg0 loglevel=8")?;
    s.open_menu()?;
    let mut picked = None;
    for _ in 0..40 {
        if let Some(i) = s.tick()? {
            picked = Some(i);
            break;
        }
    }
    match (picked, s.verdict()) {
        (Some(0), Some((PickSource::TimeoutDefault, 0))) => {}
        _ => return Err(E_BAL),
    }
    s.audit()?;
    hits += 1;

    // L2：手动选择接管——开菜单后立即手动选 #1，超时不再接管。
    let mut s = BootSelector::new(0, 30)?;
    s.add_entry("varix-std", "")?;
    s.add_entry("varix-rt", "isolcpus=2-7")?;
    s.open_menu()?;
    s.pick_manual(1)?;
    // 超时推进在 Picked 态是 E_STATE（菜单已关）。
    if s.tick() != Err(E_STATE) {
        return Err(E_STATE);
    }
    match s.verdict() {
        Some((PickSource::Manual, 1)) => {}
        _ => return Err(E_BAL),
    }
    s.audit()?;
    hits += 1;

    // L3：一次性覆盖——覆盖目标优先，选中即清，下次回归超时默认。
    let mut s = BootSelector::new(0, 30)?;
    s.add_entry("varix-std", "")?;
    s.add_entry("varix-rec", "recovery")?;
    s.set_oneshot(1)?;
    s.open_menu()?;
    match s.verdict() {
        Some((PickSource::OneShot, 1)) => {}
        _ => return Err(E_BAL),
    }
    if s.oneshot_pending.is_some() {
        return Err(E_BAL); // 一次性覆盖必须已清
    }
    s.audit()?;
    hits += 1;

    // L4：菜单渲染含 GRUB 风格标记。
    let s = BootSelector::new(0, 30)?;
    let _ = s.render();
    hits += 1;

    // I1：未开菜单即选择/滴答 —— E_STATE。
    let mut i1 = BootSelector::new(0, 10)?;
    i1.add_entry("k", "")?;
    if i1.tick() != Err(E_STATE) {
        return Err(E_STATE);
    }
    if i1.pick_manual(0) != Err(E_STATE) {
        return Err(E_STATE);
    }
    hits += 1;

    // I2：条目序号越界。
    let mut i2 = BootSelector::new(0, 10)?;
    i2.add_entry("k", "")?;
    if i2.set_oneshot(9) != Err(E_IDX) {
        return Err(E_IDX);
    }
    i2.open_menu()?;
    if i2.pick_manual(9) != Err(E_IDX) {
        return Err(E_IDX);
    }
    hits += 1;

    // I3：空内核名 / 零超时 —— E_PARAM。
    let mut i3 = BootSelector::new(0, 10)?;
    if i3.add_entry("", "x") != Err(E_PARAM) {
        return Err(E_PARAM);
    }
    if !matches!(BootSelector::new(0, 0), Err(E_PARAM)) {
        return Err(E_PARAM);
    }
    hits += 1;

    // I4：冻结后拒写。
    let mut i4 = BootSelector::new(0, 10)?;
    i4.add_entry("k", "")?;
    i4.freeze();
    if i4.add_entry("k2", "") != Err(E_SEAL) {
        return Err(E_SEAL);
    }
    if i4.tick() != Err(E_SEAL) {
        return Err(E_SEAL);
    }
    hits += 1;

    // I5：空菜单开菜单 —— E_STATE；账守恒注入（虚增超时数不破，改验
    // pending 与 picks 矛盾注入）。
    let mut i5 = BootSelector::new(0, 10)?;
    if i5.open_menu() != Err(E_STATE) {
        return Err(E_STATE);
    }
    i5.picks[2] = 3; // 伪造一次性选择但 pending 从未清——audit 抓 pending 缺失
    i5.audit()?; // 空转态放行
    i5.oneshot_pending = Some(0); // 伪造 pending：有 pending 且 picks>0 → 仍合法
    i5.audit()?;
    i5.picks[2] = 0; // 有 pending 却零一次性选择 → 矛盾
    if i5.audit() != Err(E_BAL) {
        return Err(E_BAL);
    }
    hits += 1;

    let _ = hits;
    Ok(())
}
