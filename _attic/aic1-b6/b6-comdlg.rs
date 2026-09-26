
// ---------------------------------------------------------------------------
// F008 · 深化批次六：OFN_READONLY 复选框状态面（打开对话框的只读选项）
//
// 主册依据（G-A-08【功能定义】）：「打开/保存（GetOpenFileNameW …）」——
// OFN_READONLY 标志 → 只读复选框的显示/预勾选/回传语义（Windows 同语义）。
// ---------------------------------------------------------------------------

/// OFN_READONLY 标志（commdlg.h 钉值）。
pub const OFN_READONLY: u32 = 0x0000_0001;

/// 只读复选框状态机（打开时按标志预勾选；用户可切换；确认时回传标志）。
#[derive(Clone, Copy, Debug)]
pub struct ReadonlyCheckbox {
    pub shown: bool,
    pub checked: bool,
}

impl ReadonlyCheckbox {
    /// 按调用方标志初始化（OFN_READONLY → 预勾选；无标志 → 显示但不勾）。
    pub fn from_flags(flags: u32) -> ReadonlyCheckbox {
        ReadonlyCheckbox { shown: true, checked: flags & OFN_READONLY != 0 }
    }

    pub fn toggle(&mut self) {
        self.checked = !self.checked;
    }

    /// 确认回传：勾选 → 标志置位（程序读到的 flags 与用户所见一致）。
    pub fn apply_to_flags(&self, mut flags: u32) -> u32 {
        if self.checked {
            flags |= OFN_READONLY;
        } else {
            flags &= !OFN_READONLY;
        }
        flags
    }
}

/// F008 深化批次六自检。
pub fn run_comdlg_deep5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F008-comdlg-deep5");
    // 1) 标志预勾选：OFN_READONLY → 勾；无标志 → 显示未勾。
    let pre = ReadonlyCheckbox::from_flags(OFN_READONLY);
    let plain = ReadonlyCheckbox::from_flags(0);
    cs.add(
        "readonly_checkbox_prefill",
        pre.checked && !plain.checked && pre.shown && plain.shown,
        "",
    );
    // 2) 切换与回传：勾 → 标志置位；取消勾 → 标志清除（回传与所见一致）。
    let mut box1 = ReadonlyCheckbox::from_flags(0);
    box1.toggle();
    let f1 = box1.apply_to_flags(0);
    box1.toggle();
    let f2 = box1.apply_to_flags(OFN_READONLY);
    cs.add(
        "readonly_checkbox_roundtrip",
        f1 == OFN_READONLY && f2 == 0,
        "",
    );
    // 3) 其他标志位不动（只读写回本位——不破坏调用方 flags 的其余位）。
    let mut box2 = ReadonlyCheckbox::from_flags(0x0000_0002);
    box2.toggle();
    let f3 = box2.apply_to_flags(0x0000_0002);
    cs.add(
        "readonly_checkbox_other_flags_preserved",
        f3 == 0x0000_0003,
        "",
    );
    cs
}
