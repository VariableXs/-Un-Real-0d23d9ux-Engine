
// ---------------------------------------------------------------------------
// F018 · 深化批次八：DoDragDrop 终态返回码钉值（OLE 拖放会话的三出口）+
// 文件拖放数据格式登记面（拖文件的数据对象必须供 CF_HDROP——缺席 = 缺陷，
// 与批次三「预览不缺席」同纪律）。
// ---------------------------------------------------------------------------

/// DoDragDrop 返回码（winuser/ole 钉值）。
pub const DRAGDROP_S_DROP: u32 = 0x0004_0100;
pub const DRAGDROP_S_CANCEL: u32 = 0x0004_0101;

/// 拖放会话终态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DragOutcome {
    /// 用户在合法落点松手 → DRAGDROP_S_DROP。
    Dropped,
    /// Esc / 焦点丢失等取消 → DRAGDROP_S_CANCEL。
    Cancelled,
    /// 无合法落点松手 → 0（无效果落点，同 S_OK 无操作）。
    NoTarget,
}

/// 终态 → 返回码（三出口全覆盖，无第四出口）。
pub fn drag_outcome_code(o: DragOutcome) -> u32 {
    match o {
        DragOutcome::Dropped => DRAGDROP_S_DROP,
        DragOutcome::Cancelled => DRAGDROP_S_CANCEL,
        DragOutcome::NoTarget => 0,
    }
}

/// 数据对象格式登记（文件拖放的最小契约：必须登记 CF_HDROP）。
pub struct DragDataRegistry {
    formats: [Option<u32>; 4],
    n: usize,
}

impl DragDataRegistry {
    pub fn new() -> DragDataRegistry {
        DragDataRegistry { formats: [None; 4], n: 0 }
    }
    pub fn register(&mut self, cf: u32) -> bool {
        if self.n >= 4 || self.formats[..self.n].contains(&Some(cf)) {
            return false;
        }
        self.formats[self.n] = Some(cf);
        self.n += 1;
        true
    }
    /// 文件拖放资格：CF_HDROP 在登记面中（1 值 = winuser CF_HDROP 钉值）。
    pub fn qualifies_for_file_drag(&self) -> bool {
        self.formats[..self.n].contains(&Some(1))
    }
    pub fn len(&self) -> usize {
        self.n
    }
}

/// F018 深化批次八自检。
fn run_dragdrop_deep7_checks() -> CheckSet {
    let mut cs = CheckSet::new("F018-dragdrop-deep7");
    // 1) 三出口返回码钉值 + 覆盖完整性（任何枚举值都有码）。
    cs.add(
        "drag_outcome_codes_pinned",
        drag_outcome_code(DragOutcome::Dropped) == 0x0004_0100
            && drag_outcome_code(DragOutcome::Cancelled) == 0x0004_0101
            && drag_outcome_code(DragOutcome::NoTarget) == 0,
        "",
    );
    // 2) CF_HDROP(1) 登记后才有文件拖放资格；重复登记拒绝（计数不涨）。
    let mut reg = DragDataRegistry::new();
    let before = reg.qualifies_for_file_drag();
    let ok = reg.register(1);
    let dup = reg.register(1);
    cs.add(
        "file_drag_requires_cfhdrop",
        !before && ok && !dup && reg.len() == 1 && reg.qualifies_for_file_drag(),
        "",
    );
    // 3) 容量 4 纪律：第 5 格如实拒绝（登记面不静默溢出）。
    let mut full = DragDataRegistry::new();
    let mut all_ok = true;
    for cf in 1..=5u32 {
        all_ok &= full.register(cf) == (cf <= 4);
    }
    cs.add(
        "registry_capacity_four",
        all_ok && full.len() == 4,
        "",
    );
    cs
}
