//! AI-S2 隔离校验 crate（_attic 临时工程——收口后以主仓全量套件为准）。
//! 真实文件原样拷贝：checks / ksha256 / star::sbase / secstar2。
//! cfg 语义镜像真实 lib.rs：宿主测试走 std，非测试走 no_std（镜像口径）。
#![cfg_attr(not(test), no_std)]
#[cfg(all(not(test), not(feature = "kernel-image")))]
extern crate std;
extern crate alloc;

// kernel-image 口径：no_std 编译验证用桩分配器（永不真正分配——check-only）。
#[cfg(feature = "kernel-image")]
struct ScratchAlloc;
#[cfg(feature = "kernel-image")]
unsafe impl core::alloc::GlobalAlloc for ScratchAlloc {
    unsafe fn alloc(&self, _l: core::alloc::Layout) -> *mut u8 {
        core::ptr::null_mut()
    }
    unsafe fn dealloc(&self, _p: *mut u8, _l: core::alloc::Layout) {}
}
#[cfg(feature = "kernel-image")]
#[global_allocator]
static SCRATCH_ALLOC: ScratchAlloc = ScratchAlloc;

pub mod checks;
#[path = "security/ksha256.rs"]
pub mod ksha256;
pub mod star;
pub mod secstar2;

#[cfg(test)]
mod ais2debug {
    use crate::secstar2::*;
    fn reds(tag: &str, set: &crate::checks::CheckSet) {
        for i in 0..set.len() {
            if let Some(c) = set.get(i) {
                if !c.passed {
                    println!("RED {tag} :: {} :: {}", c.name, c.detail);
                }
            }
        }
    }
    #[test]
    fn dump_reds() {
        reds("F186", &syspart::run_syspart_checks());
        reds("F187", &clockguard::run_clockguard_checks());
        reds("F188", &logring::run_logring_checks());
        reds("F189", &selfheal2::run_selfheal2_checks());
        reds("F190", &slotview::run_slotview_checks());
        reds("F191", &bootaudit::run_bootaudit_checks());
        reds("F192", &paramwl::run_paramwl_checks());
        reds("F193", &safemode::run_safemode_checks());
    }
}

#[cfg(test)]
mod ais2debug2 {
    use crate::secstar2::*;
    fn reds(tag: &str, set: &crate::checks::CheckSet) {
        for i in 0..set.len() {
            if let Some(c) = set.get(i) {
                if !c.passed { println!("RED {tag} :: {} :: {}", c.name, c.detail); }
            }
        }
    }
    #[test]
    fn dump2() {
        reds("F194", &auditchain::run_auditchain_checks());
        reds("F195", &resquota::run_resquota_checks());
        reds("F196", &batguard::run_batguard_checks());
        reds("F197", &thermgov::run_thermgov_checks());
    }
}

#[cfg(test)]
mod ais2debug3 {
    use crate::secstar2::batguard::*;
    #[test]
    fn trace_recover() {
        let mut g = BatteryGuard::new();
        g.set_levels(20, 10);
        let mut level = 400u64;
        let mut got = false;
        while level > 100 {
            level -= 2;
            if g.report_level(level) == Some(GuardEvent::WarnToast) { got = true; }
        }
        println!("first toast: {got}, level={level}");
        println!("160: {:?}", g.report_level(160));
        println!("162: {:?}", g.report_level(162));
        while level < 320 { level += 2; let _ = g.report_level(level); }
        for _ in 0..30 { let _ = g.report_level(320); }
        
        g.recover();
        
        let mut got2 = false;
        let mut k = 0;
        while level > 100 {
            level -= 2; k += 1;
            if g.report_level(level) == Some(GuardEvent::WarnToast) { got2 = true; println!("toast at level={level} k={k}"); break; }
        }
        println!("got2: {got2}");
    }
}

#[cfg(test)]
mod ais2debug4 {
    use crate::secstar2::*;
    fn reds(tag: &str, set: &crate::checks::CheckSet) {
        for i in 0..set.len() {
            if let Some(c) = set.get(i) {
                if !c.passed { println!("RED {tag} :: {} :: {}", c.name, c.detail); }
            }
        }
    }
    #[test]
    fn dump4() {
        reds("F186d", &syspart::run_syspart_deep_checks());
        reds("F187d", &clockguard::run_clockguard_deep_checks());
        reds("F188d", &logring::run_logring_deep_checks());
        reds("F189d", &selfheal2::run_selfheal2_deep_checks());
    }
}

#[cfg(test)]
mod ais2debug5 {
    use crate::secstar2::*;
    fn reds(tag: &str, set: &crate::checks::CheckSet) {
        for i in 0..set.len() {
            if let Some(c) = set.get(i) {
                if !c.passed { println!("RED {tag} :: {} :: {}", c.name, c.detail); }
            }
        }
    }
    #[test]
    fn dump5() {
        reds("F190d", &slotview::run_slotview_deep_checks());
        reds("F191d", &bootaudit::run_bootaudit_deep_checks());
        reds("F192d", &paramwl::run_paramwl_deep_checks());
        reds("F195d", &resquota::run_resquota_deep_checks());
        reds("F197d", &thermgov::run_thermgov_deep_checks());
        reds("F198d", &recenv::run_recenv_deep_checks());
        reds("F199d", &lineage::run_lineage_deep_checks());
        reds("F200d", &walkall::run_walkall_deep_checks());
    }
}

#[cfg(test)]
mod ais2debug6 {
    use crate::secstar2::thermgov::*;
    #[test]
    fn trace_attrib() {
        let mut t4 = ThermoGovernor::new();
        let mut at = 0u64;
        let mut att = ReleaseAttributor::new();
        for temp in [60i64, 86, 80, 78] {
            at += SAMPLE_PERIOD_S;
            let ev = t4.sample(temp, at);
            println!("temp={} at={} level={:?} ev={:?}", temp, at, t4.level(), ev);
            if let Some(e) = ev {
                println!("  feed -> {:?}", att.feed(&e).err());
            }
        }
        println!("records={}", att.len());
    }
}
