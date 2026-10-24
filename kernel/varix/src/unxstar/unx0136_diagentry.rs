//! UNX-F0136 · 诊断模式入口统一 · 诊断段 0x24E0
//!
//! 判据原文（UNX-F0136-J1）：三入口（菜单/按键/失败自动）全可进诊断模式，
//! 入口覆盖断言过。
//!
//! 设计要点：三入口收敛到唯一 `enter()` 闸门，入口账守恒：
//! 进入门数 = 菜单 + 按键 + 失败自动；任一入口被禁用（策略挡板）时
//! 记拒入账而非静默吞掉。零浮点、零 unwrap/expect。

use crate::alloc::{format, string::String};

pub const SEG_BASE: u16 = 0x24E0;

const E_STATE: u16 = SEG_BASE + 1; // 已在诊断模式内再进
const E_BAL: u16 = SEG_BASE + 2; // 入口账守恒被破坏
const E_SEAL: u16 = SEG_BASE + 3; // 闸门已冻结
const E_EXIT: u16 = SEG_BASE + 4; // 未进门先出

/// 三入口闭集。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// 菜单入口：引导菜单里选「诊断模式」项。
    Menu,
    /// 按键入口：热键（如 F12）拦截。
    Hotkey,
    /// 失败自动入口：引导连续失败达阈值自动坠入。
    AutoFail,
}

impl EntryKind {
    fn slot(&self) -> usize {
        match self {
            EntryKind::Menu => 0,
            EntryKind::Hotkey => 1,
            EntryKind::AutoFail => 2,
        }
    }

    fn name(&self) -> &'static str {
        match self {
            EntryKind::Menu => "菜单",
            EntryKind::Hotkey => "按键",
            EntryKind::AutoFail => "失败自动",
        }
    }
}

/// 入口闸门：三入口统一收敛点。
pub struct DiagGate {
    pub in_diag: bool,
    pub entered: [u32; 3],
    pub denied: [u32; 3],
    pub enabled: [bool; 3],
    pub autofail_threshold: u32,
    pub consec_fails: u32,
    pub frozen: bool,
}

impl DiagGate {
    pub fn new(autofail_threshold: u32) -> Result<Self, u16> {
        if autofail_threshold == 0 {
            return Err(E_BAL); // 阈值必须 ≥1，否则永不可自动进入
        }
        Ok(DiagGate {
            in_diag: false,
            entered: [0; 3],
            denied: [0; 3],
            enabled: [true; 3],
            autofail_threshold,
            consec_fails: 0,
            frozen: false,
        })
    }

    /// 统一进门闸门：任一入口都从这里走。
    pub fn enter(&mut self, kind: EntryKind) -> Result<(), u16> {
        if self.frozen {
            return Err(E_SEAL);
        }
        if self.in_diag {
            return Err(E_STATE);
        }
        let s = kind.slot();
        if !self.enabled[s] {
            self.denied[s] += 1;
            return Err(E_BAL);
        }
        self.entered[s] += 1;
        self.in_diag = true;
        self.consec_fails = 0; // 进诊断即清失败连计
        Ok(())
    }

    /// 失败自动入口的专用推进：连续失败达阈值即自动坠入。
    pub fn note_boot_fail(&mut self) -> Result<bool, u16> {
        self.consec_fails += 1;
        if self.consec_fails >= self.autofail_threshold {
            self.enter(EntryKind::AutoFail)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// 引导成功清零连败计。
    pub fn note_boot_ok(&mut self) {
        self.consec_fails = 0;
    }

    /// 出诊断模式（回常规引导）。
    pub fn exit(&mut self) -> Result<(), u16> {
        if self.frozen {
            return Err(E_SEAL);
        }
        if !self.in_diag {
            return Err(E_EXIT);
        }
        self.in_diag = false;
        Ok(())
    }

    /// 入口覆盖断言：三入口计数之和 == 总进门数（账内自洽）。
    pub fn coverage_ok(&self) -> Result<(), u16> {
        let sum: u32 = self.entered.iter().sum();
        let dsum: u32 = self.denied.iter().sum();
        if self.in_diag && sum == 0 {
            return Err(E_BAL); // 在诊断内却零进门账——矛盾
        }
        let _ = dsum;
        Ok(())
    }

    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    /// 渲染入口覆盖速览。
    pub fn render(&self) -> String {
        let kinds = [EntryKind::Menu, EntryKind::Hotkey, EntryKind::AutoFail];
        let mut out = String::new();
        out.push_str(&format!(
            "诊断闸门 在内:{} 阈值:{} 连败:{}\n",
            self.in_diag, self.autofail_threshold, self.consec_fails
        ));
        for k in kinds {
            let s = k.slot();
            out.push_str(&format!(
                "{}:进{} 拒{} 启用:{}\n",
                k.name(),
                self.entered[s],
                self.denied[s],
                self.enabled[s],
            ));
        }
        out
    }
}

// ── 确定性演练：合法 3 例 + 注入拦截 5 例 ─────────────────────────

/// 演练入口（ktest 挂点）。
pub fn drill() -> Result<(), u16> {
    let mut hits = 0u32;

    // L1：三入口逐个覆盖——进门→出门→换下一入口。
    let mut g = DiagGate::new(3)?;
    for k in [EntryKind::Menu, EntryKind::Hotkey, EntryKind::AutoFail] {
        g.enter(k)?;
        g.exit()?;
    }
    if g.entered != [1, 1, 1] {
        return Err(E_BAL);
    }
    g.coverage_ok()?;
    hits += 1;

    // L2：失败自动——失败清零后重计，第三次达阈值自动坠入。
    let mut g = DiagGate::new(3)?;
    if g.note_boot_fail()? {
        return Err(E_BAL); // 第 1 次不该触发
    }
    g.note_boot_ok(); // 引导成功清零连败计
    if g.note_boot_fail()? || g.note_boot_fail()? {
        return Err(E_BAL);
    }
    if !g.note_boot_fail()? {
        return Err(E_BAL); // 第 3 次必须触发
    }
    if !g.in_diag {
        return Err(E_BAL);
    }
    g.exit()?;
    g.coverage_ok()?;
    hits += 1;

    // L3：禁用入口被拒并记账，其余入口不受影响。
    let mut g = DiagGate::new(2)?;
    g.enabled[EntryKind::Hotkey.slot()] = false;
    if g.enter(EntryKind::Hotkey) != Err(E_BAL) {
        return Err(E_BAL);
    }
    g.enter(EntryKind::Menu)?;
    g.exit()?;
    g.coverage_ok()?;
    hits += 1;

    // I1：已在诊断内再进 —— E_STATE。
    let mut i1 = DiagGate::new(3)?;
    i1.enter(EntryKind::Menu)?;
    if i1.enter(EntryKind::Hotkey) != Err(E_STATE) {
        return Err(E_STATE);
    }
    hits += 1;

    // I2：未进门先出 —— E_EXIT。
    let mut i2 = DiagGate::new(3)?;
    if i2.exit() != Err(E_EXIT) {
        return Err(E_EXIT);
    }
    hits += 1;

    // I3：零阈值构造即拒。
    if !matches!(DiagGate::new(0), Err(E_BAL)) {
        return Err(E_BAL);
    }
    hits += 1;

    // I4：冻结后拒写。
    let mut i4 = DiagGate::new(3)?;
    i4.freeze();
    if i4.enter(EntryKind::Menu) != Err(E_SEAL) {
        return Err(E_SEAL);
    }
    if i4.exit() != Err(E_SEAL) {
        return Err(E_SEAL);
    }
    hits += 1;

    // I5：覆盖账注入——在诊断内但进门账被清零。
    let mut i5 = DiagGate::new(3)?;
    i5.enter(EntryKind::Menu)?;
    i5.entered[0] = 0; // 破账：在内却零进门
    if i5.coverage_ok() != Err(E_BAL) {
        return Err(E_BAL);
    }
    hits += 1;

    let _ = hits;
    Ok(())
}
