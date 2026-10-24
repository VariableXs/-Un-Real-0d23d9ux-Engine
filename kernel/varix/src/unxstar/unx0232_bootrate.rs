//! UNX-F0232 · 引导成功率统计账（域出口）
//!
//! 锚点判据（施工书原文）：UNX-F0232-J1 连续 50 次真机引导成功
//! 50/50（出口判据），失败分类账齐备。
//!
//! 设计：引导流水账（成功/失败 + 失败四分类：介质/签名/内存/
//! 其他）→ 出口统计（连续成功游标 + 总成功率千分比）→ 红线：
//! ①出口判据要求游标 ≥50 且窗口内零失败；②失败必分类（未分类
//! 失败 = 静默，即红）；③统计数字与流水对账一致。
//!
//! 错误码段：0x2D80（全仓普查后独占分配，AI-WBX1022）。

use alloc::vec::Vec;

/// 模块版本。
pub const F0232_VERSION: &str = "F0232-bootrate-v1";

/// 诊断码段基址（UNX-F0232 独占）。
pub const SEG_BASE: u16 = 0x2D80;

/// 出口游标阈值（判据口径 50）。
pub const EXIT_CURSOR: usize = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum E {
    /// 未分类失败（静默）。
    UnclassifiedFail = SEG_BASE + 1,
    /// 出口游标未达。
    CursorShort = SEG_BASE + 2,
    /// 对账不一致。
    ReconcileFail = SEG_BASE + 3,
    /// 游标断裂未清零。
    CursorStale = SEG_BASE + 4,
    /// 空账出统计。
    EmptyLedger = SEG_BASE + 5,
}

impl E {
    pub const fn code(self) -> u16 {
        self as u16
    }
}

/// 失败分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailKind {
    /// 介质类。
    Media,
    /// 签名类。
    Signature,
    /// 内存类。
    Memory,
    /// 其他（必须带自由文本因）。
    Other,
}

/// 一笔引导流水。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootEntry {
    /// 成功否。
    pub ok: bool,
    /// 失败分类（成功笔为 None）。
    pub fail: Option<(FailKind, &'static str)>,
}

/// 出口统计账。
#[derive(Debug, Default)]
pub struct ExitLedger {
    /// 流水。
    pub entries: Vec<BootEntry>,
    /// 连续成功游标。
    pub cursor: usize,
}

impl ExitLedger {
    /// 记一笔（失败必分类，游标按成败推进/清零）。
    pub fn record(&mut self, e: BootEntry) -> Result<(), u16> {
        if !e.ok && e.fail.is_none() {
            return Err(E::UnclassifiedFail.code());
        }
        if e.ok {
            self.cursor += 1;
        } else {
            self.cursor = 0;
        }
        self.entries.push(e);
        Ok(())
    }

    /// 出口判据：游标 ≥ 50。
    pub fn exit_ok(&self) -> Result<(), u16> {
        if self.entries.is_empty() {
            return Err(E::EmptyLedger.code());
        }
        if self.cursor < EXIT_CURSOR {
            return Err(E::CursorShort.code());
        }
        Ok(())
    }

    /// 统计：总成功率千分比。
    pub fn permille(&self) -> Result<u64, u16> {
        if self.entries.is_empty() {
            return Err(E::EmptyLedger.code());
        }
        let ok = self.entries.iter().filter(|e| e.ok).count();
        Ok((ok as u64 * 1000) / self.entries.len() as u64)
    }

    /// 对账：重算游标与统计。
    pub fn reconcile(&self) -> Result<(), u16> {
        if self.entries.is_empty() {
            return Err(E::EmptyLedger.code());
        }
        let mut cursor = 0usize;
        for e in self.entries.iter() {
            if e.ok {
                cursor += 1;
            } else {
                cursor = 0;
            }
        }
        if cursor != self.cursor {
            return Err(E::CursorStale.code());
        }
        Ok(())
    }
}

/// J1 自证（判据原文：50 连成 50/50 出口 + 失败分类账齐备）。
pub fn self_audit() -> Result<(), u16> {
    // 一、J1 主体：50 连成 → 出口绿。
    let mut led = ExitLedger::default();
    for _ in 0..EXIT_CURSOR {
        led.record(BootEntry {
            ok: true,
            fail: None,
        })?;
    }
    led.exit_ok()?;
    led.reconcile()?;
    if led.permille()? != 1000 {
        return Err(E::ReconcileFail.code());
    }

    // 二、游标断裂清零：50 成 + 1 败 → 游标归零 → 出口红。
    let mut broken = ExitLedger::default();
    for _ in 0..EXIT_CURSOR {
        broken.record(BootEntry {
            ok: true,
            fail: None,
        })?;
    }
    broken.record(BootEntry {
        ok: false,
        fail: Some((FailKind::Media, "usb0-nomedia")),
    })?;
    if broken.cursor != 0 {
        return Err(E::CursorStale.code());
    }
    if broken.exit_ok().is_ok() {
        return Err(E::CursorShort.code());
    }

    // 三、失败分类账齐备：四分类各一例全落账。
    let mut cls = ExitLedger::default();
    for (k, note) in [
        (FailKind::Media, "no-media"),
        (FailKind::Signature, "bad-sign"),
        (FailKind::Memory, "map-hole"),
        (FailKind::Other, "fw-weird"),
    ] {
        cls.record(BootEntry {
            ok: false,
            fail: Some((k, note)),
        })?;
    }
    if cls.entries.len() != 4 {
        return Err(E::UnclassifiedFail.code());
    }
    for e in cls.entries.iter() {
        if e.ok || e.fail.is_none() {
            return Err(E::UnclassifiedFail.code());
        }
    }

    // 四、红线逐项：未分类失败拒 / 空账 / 游标对账破。
    let mut unclass = ExitLedger::default();
    if unclass
        .record(BootEntry {
            ok: false,
            fail: None,
        })
        .is_ok()
    {
        return Err(E::UnclassifiedFail.code());
    }
    if ExitLedger::default().exit_ok().is_ok() {
        return Err(E::EmptyLedger.code());
    }
    let mut stale = ExitLedger::default();
    stale.record(BootEntry {
        ok: true,
        fail: None,
    })?;
    stale.cursor = 99; // 手工污染。
    if stale.reconcile().is_ok() {
        return Err(E::CursorStale.code());
    }

    // 五、判别式段位对拍。
    if E::UnclassifiedFail.code() != SEG_BASE + 1 || E::EmptyLedger.code() != SEG_BASE + 5 {
        return Err(E::UnclassifiedFail.code());
    }
    Ok(())
}
