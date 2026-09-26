//! AI-S1 隔离校验 crate（_attic 临时工程——收口后以主仓全量套件为准）。
//! 真实文件原样拷贝：checks / secstar（F171-F185 全十五模块）。
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
pub mod secstar;

#[cfg(test)]
mod ais1reds {
    use crate::secstar::*;
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
    fn dump_reds_batch1() {
        reds("F171", &bootmenu::run_bootmenu_checks());
        reds("F172", &selftestviz::run_selftestviz_checks());
        reds("F173", &panicscreen::run_panicscreen_checks());
        reds("F174", &diagsnap::run_diagsnap_checks());
        reds("F175", &crashiso::run_crashiso_checks());
        reds("F176", &memguard::run_memguard_checks());
        reds("F177", &capenforce::run_capenforce_checks());
        reds("F178", &signbadge::run_signbadge_checks());
    }
    #[test]
    fn dump_reds_batch2() {
        reds("F179", &permaudit::run_permaudit_checks());
        reds("F180", &pwrdrill::run_pwrdrill_checks());
        reds("F181", &handoffchk::run_handoffchk_checks());
        reds("F182", &duoclock::run_duoclock_checks());
        reds("F183", &diskhealth::run_diskhealth_checks());
        reds("F184", &hotplug::run_hotplug_checks());
        reds("F185", &romount::run_romount_checks());
    }
    #[test]
    fn dump_reds_deep() {
        reds("F171d", &bootmenu::run_bootmenu_deep_checks());
        reds("F172d", &selftestviz::run_selftestviz_deep_checks());
        reds("F173d", &panicscreen::run_panicscreen_deep_checks());
        reds("F174d", &diagsnap::run_diagsnap_deep_checks());
        reds("F175d", &crashiso::run_crashiso_deep_checks());
        reds("F176d", &memguard::run_memguard_deep_checks());
        reds("F177d", &capenforce::run_capenforce_deep_checks());
        reds("F178d", &signbadge::run_signbadge_deep_checks());
        reds("F179d", &permaudit::run_permaudit_deep_checks());
        reds("F180d", &pwrdrill::run_pwrdrill_deep_checks());
        reds("F181d", &handoffchk::run_handoffchk_deep_checks());
        reds("F182d", &duoclock::run_duoclock_deep_checks());
        reds("F183d", &diskhealth::run_diskhealth_deep_checks());
        reds("F184d", &hotplug::run_hotplug_deep_checks());
        reds("F185d", &romount::run_romount_deep_checks());
    }
    #[test]
    fn domain_aggregate() {
        let set = run_secstar_checks();
        let (passed, failed) = set.tally();
        println!("SECSTAR-S1 aggregate: {passed} passed, {failed} failed");
        assert!(set.all_passed(), "域聚合存在红项：{passed}/{} 绿", passed + failed);
        let deep = run_secstar_deep_checks();
        let (dp, df) = deep.tally();
        println!("SECSTAR-S1 deep aggregate: {dp} passed, {df} failed");
        assert!(deep.all_passed(), "深化自检存在红项：{dp}/{} 绿", dp + df);
    }
}
