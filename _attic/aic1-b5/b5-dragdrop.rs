
// ---------------------------------------------------------------------------
// F018 · 深化批次五：拖放目标注册面（RegisterDragDrop/Revoke 语义）
//
// 主册依据（G-A-18【功能定义】）：「OLE 拖放（DoDragDrop/IDropSource/
// IDropTarget 语义）」——目标注册是 OLE 拖放的第一步：窗口 ↔ target 绑定表
// （注册/撤销/重复注册语义按 Windows：重复注册如实拒）。
// ---------------------------------------------------------------------------

/// 拖放目标绑定表（hwnd → target 槽位；容量 16）。
pub struct DropTargetRegistry {
    hwnds: [Option<u32>; 16],
    n: usize,
    /// 重复注册拒绝计数（Windows 语义：DRAGDROP_E_ALREADYREGISTERED）。
    pub duplicate_refusals: u32,
}

/// 重复注册错误（Windows HRESULT 钉值）。
pub const DRAGDROP_E_ALREADYREGISTERED: u32 = 0x8004_0101;

impl DropTargetRegistry {
    pub const fn new() -> DropTargetRegistry {
        DropTargetRegistry { hwnds: [None; 16], n: 0, duplicate_refusals: 0 }
    }

    /// RegisterDragDrop 语义：注册返回 Ok；已注册 hwnd → 拒 + 计数；满容拒。
    pub fn register(&mut self, hwnd: u32) -> Result<(), u32> {
        if self.hwnds[..self.n].contains(&Some(hwnd)) {
            self.duplicate_refusals += 1;
            return Err(DRAGDROP_E_ALREADYREGISTERED);
        }
        if self.n >= self.hwnds.len() {
            return Err(0x8000_0005); // E_OUTOFMEMORY 惯用值（满容路径）
        }
        self.hwnds[self.n] = Some(hwnd);
        self.n += 1;
        Ok(())
    }

    /// RevokeDragDrop 语义：撤销注册（未注册 → false 如实）。
    pub fn revoke(&mut self, hwnd: u32) -> bool {
        if let Some(pos) = (0..self.n).find(|&i| self.hwnds[i] == Some(hwnd)) {
            self.hwnds[pos] = self.hwnds[self.n - 1].take();
            self.n -= 1;
            true
        } else {
            false
        }
    }

    /// 查目标是否注册（拖拽落点分发的查询面）。
    pub fn is_registered(&self, hwnd: u32) -> bool {
        self.hwnds[..self.n].contains(&Some(hwnd))
    }

    pub fn len(&self) -> usize {
        self.n
    }
}

/// F018 深化批次五自检。
pub fn run_dragdrop_deep4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F018-dragdrop-deep4");
    // 1) 注册/查询/重复注册拒（Windows HRESULT 钉值）。
    let mut reg = DropTargetRegistry::new();
    let r1 = reg.register(100);
    let dup = reg.register(100);
    cs.add(
        "drop_target_register_semantics",
        r1.is_ok()
            && reg.is_registered(100)
            && dup == Err(DRAGDROP_E_ALREADYREGISTERED)
            && reg.duplicate_refusals == 1,
        "",
    );
    // 2) 撤销：已注册撤成功且查询失效；未注册撤如实 false（swap-remove 槽位
    //    复用——表不稀疏化）。
    let revoked = reg.revoke(100);
    let ghost = reg.revoke(100);
    cs.add(
        "drop_target_revoke_semantics",
        revoked && !ghost && !reg.is_registered(100) && reg.len() == 0,
        "",
    );
    // 3) 多目标并发注册与撤销后重注册（乱序路径——状态机完整性）。
    let mut reg2 = DropTargetRegistry::new();
    let _ = reg2.register(1);
    let _ = reg2.register(2);
    let _ = reg2.register(3);
    let mid = reg2.revoke(2);
    let re = reg2.register(2);
    cs.add(
        "drop_target_re_register_after_revoke",
        mid && re.is_ok() && reg2.len() == 3 && reg2.is_registered(2),
        "",
    );
    cs
}
