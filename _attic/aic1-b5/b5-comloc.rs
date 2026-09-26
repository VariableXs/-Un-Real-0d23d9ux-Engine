
// ---------------------------------------------------------------------------
// F019 · 深化批次五：AddRef/Release 配平审计（引用计数的封闭核对）
//
// 主册依据（G-A-19【数据与存储】）：「COM 对象生命周期随进程（引用计数语义
// 按 Windows：AddRef/Release 对拍）」——配平判据：N 对 AddRef/Release 后
/// 计数回 1（构造引用）且泄漏审计为 0；不配平路径被既有 RefLeakAudit 捕获。
// ---------------------------------------------------------------------------

/// 引用计数配平审计（对 ClassFactory 既有 AddRef/Release 的封闭核对面）。
pub struct RefBalance {
    pub acquired: u32,
    pub released: u32,
}

impl RefBalance {
    pub const fn new() -> RefBalance {
        RefBalance { acquired: 0, released: 0 }
    }

    pub fn note_addref(&mut self) {
        self.acquired += 1;
    }

    pub fn note_release(&mut self) {
        self.released = self.released.saturating_add(1);
    }

    /// 配平判据：在途引用 = acquired − released；配平 = 在途与实际存活计数
    /// 一致（构造引用 1 + N 对 → 在途 1）。
    pub fn balanced_with_one_live(&self) -> bool {
        self.acquired > 0 && self.acquired - self.released == 1
    }
}

/// F019 深化批次五自检。
pub fn run_comloc_deep4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F019-comloc-deep4");
    // 1) 封闭配平：工厂初始 1（构造）+ 5 对 AddRef/Release → 计数回 1、
    //    配平审计绿、泄漏 0（既有 session_exit 消费口径）。
    let mut factory = ClassFactory::new(Clsid::ShellLink);
    let mut bal = RefBalance::new();
    let mut live = factory.refcount(); // 构造引用 1
    for _ in 0..5u32 {
        live += factory.add_ref();
        bal.note_addref();
        live -= factory.release();
        bal.note_release();
    }
    cs.add(
        "refbalance_closed_pairing",
        live == 1 && factory.refcount() == 1 && bal.balanced_with_one_live(),
        "",
    );
    // 2) 不配平（漏 Release）→ 配平判据如实红 + 泄漏告警联动（既有面）。
    let mut factory2 = ClassFactory::new(Clsid::Clipboard);
    let mut bal2 = RefBalance::new();
    let _ = factory2.add_ref();
    bal2.note_addref();
    let _ = factory2.add_ref();
    bal2.note_addref();
    let mut audit = RefLeakAudit::new();
    let warned = audit.session_exit(factory2.refcount() - 1); // 构造引用外多 1
    cs.add(
        "refbalance_leak_detected",
        !bal2.balanced_with_one_live() && warned,
        "",
    );
    // 3) Release 超额如实钳制（saturating——计数不为负，异常路径可见）。
    let mut bal3 = RefBalance::new();
    bal3.note_addref();
    bal3.note_release();
    bal3.note_release();
    cs.add(
        "refbalance_over_release_clamped",
        bal3.released == 2 && bal3.acquired == 1 && !bal3.balanced_with_one_live(),
        "",
    );
    cs
}
