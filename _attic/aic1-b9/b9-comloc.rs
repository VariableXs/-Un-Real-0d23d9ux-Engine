
// ---------------------------------------------------------------------------
// F019 · 深化批次九：IClassFactory::LockServer 锁计数面（OLE 生存期第二轴：
// 工厂锁——TRUE +1 / FALSE −1 / 下溢如实拒；canUnload = 零对象且零锁）。
// ---------------------------------------------------------------------------

/// 工厂锁操作结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LockServerResult {
    Locked,
    Unlocked,
    /// FALSE 时无锁可解（下溢——调用方失配，如实拒）。
    UnderflowDenied,
}

/// LockServer 语义核（与既有 ClassFactory 的引用计数正交——对象计数另走
/// AddRef/Release 面）。
pub struct FactoryLock {
    pub locks: u32,
}

impl FactoryLock {
    pub fn new() -> FactoryLock {
        FactoryLock { locks: 0 }
    }

    pub fn lock_server(&mut self, f_lock: bool) -> LockServerResult {
        if f_lock {
            self.locks += 1;
            LockServerResult::Locked
        } else if self.locks == 0 {
            LockServerResult::UnderflowDenied
        } else {
            self.locks -= 1;
            LockServerResult::Unlocked
        }
    }

    /// 可卸载判据：锁归零（对象计数的联动由调用方合流——本面只答锁轴）。
    pub fn can_unload_by_locks(&self) -> bool {
        self.locks == 0
    }
}

/// F019 深化批次九自检。
fn run_comloc_deep8_checks() -> CheckSet {
    let mut cs = CheckSet::new("F019-comloc-deep8");
    let mut f = FactoryLock::new();
    // 1) 锁两升一降：计数 2→1；可卸载判据随计数翻转。
    let _ = f.lock_server(true);
    let _ = f.lock_server(true);
    let busy = f.can_unload_by_locks();
    let r = f.lock_server(false);
    cs.add(
        "lockserver_counting",
        !busy && f.locks == 1 && r == LockServerResult::Unlocked,
        "",
    );
    // 2) 下溢如实拒：无锁时 FALSE 不减不炸。
    let mut g = FactoryLock::new();
    let under = g.lock_server(false);
    cs.add(
        "lockserver_underflow_denied",
        under == LockServerResult::UnderflowDenied && g.locks == 0,
        "",
    );
    // 3) 归零后可卸载：N 锁 N 解对称。
    for _ in 0..3 {
        let _ = g.lock_server(true);
    }
    let mut balanced = true;
    for _ in 0..3 {
        balanced &= g.lock_server(false) == LockServerResult::Unlocked;
    }
    cs.add(
        "lockserver_balanced_to_unloadable",
        balanced && g.locks == 0 && g.can_unload_by_locks(),
        "",
    );
    cs
}
