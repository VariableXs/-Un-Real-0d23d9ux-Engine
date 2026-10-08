//! 测试串行门：给「共用进程级全局状态」的测试一把同一把锁。
//!
//! 背景：进程内单例（如跨域同步的上一帧快照 `LAST_SNAPSHOT`）在 cargo 的
//! 默认并行测试下会被多个测试同时读写——一个测试 `reset_baseline()`，
//! 另一个测试的基线就凭空消失，表现为**单跑全绿、全量随机失败**。
//! 这类失败极具误导性：看起来像被测逻辑有问题，实际只是测试互相踩。
//!
//! 用法：凡触及进程级单例的测试，开头取一次锁即可：
//! ```
//! let _g = crate::testgate::lock();
//! ```
//! 锁只在测试进程内生效（`#[cfg(test)]` 视角下无成本），不影响生产路径。
//! 池化：`Mutex` 中毒时直接取内层值——一个测试 panic 不该让其余测试全部
//! 报 poisoning 失败，那会把真正的失败原因埋掉。

static GATE: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 取测试串行锁。返回的 guard 存活期间，其他调用者会等待。
pub fn lock() -> std::sync::MutexGuard<'static, ()> {
    match GATE.lock() {
        Ok(g) => g,
        // 前一个持有者 panic 过：把中毒当成「已释放」，让后续测试照常跑。
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_is_reentrant_across_calls() {
        // 顺序取两次必须都能拿到（不能自锁死）。
        {
            let _g = lock();
        }
        let _g2 = lock();
    }

    #[test]
    fn lock_survives_poisoning() {
        // 模拟另一个测试在持锁时 panic：后续取锁仍然成功。
        let h = std::thread::spawn(|| {
            let _g = lock();
            panic!("intentional test panic to poison the gate");
        });
        let _ = h.join(); // 吞掉 panic，只留下中毒状态
        let _g = lock(); // 必须仍能取到
    }
}
