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
        reds("F171b3", &bootmenu_b3::run_bootmenu_b3_checks());
        reds("F172b3", &selftestviz_b3::run_selftestviz_b3_checks());
        reds("F173b3", &panicscreen_b3::run_panicscreen_b3_checks());
        reds("F174b3", &diagsnap_b3::run_diagsnap_b3_checks());
        reds("F175b3", &crashiso_b3::run_crashiso_b3_checks());
        reds("F176b3", &memguard_b3::run_memguard_b3_checks());
        reds("F177b3", &capenforce_b3::run_capenforce_b3_checks());
        reds("F178b3", &signbadge_b3::run_signbadge_b3_checks());
        reds("F179b3", &permaudit_b3::run_permaudit_b3_checks());
        reds("F180b3", &pwrdrill_b3::run_pwrdrill_b3_checks());
        reds("F181b3", &handoffchk_b3::run_handoffchk_b3_checks());
        reds("F182b3", &duoclock_b3::run_duoclock_b3_checks());
        reds("F183b3", &diskhealth_b3::run_diskhealth_b3_checks());
        reds("F184b3", &hotplug_b3::run_hotplug_b3_checks());
        reds("F185b3", &romount_b3::run_romount_b3_checks());
        reds("F171b4", &bootmenu_b4::run_bootmenu_b4_checks());
        reds("F172b4", &selftestviz_b4::run_selftestviz_b4_checks());
        reds("F173b4", &panicscreen_b4::run_panicscreen_b4_checks());
        reds("F174b4", &diagsnap_b4::run_diagsnap_b4_checks());
        reds("F175b4", &crashiso_b4::run_crashiso_b4_checks());
        reds("F176b4", &memguard_b4::run_memguard_b4_checks());
        reds("F177b4", &capenforce_b4::run_capenforce_b4_checks());
        reds("F178b4", &signbadge_b4::run_signbadge_b4_checks());
        reds("F179b4", &permaudit_b4::run_permaudit_b4_checks());
        reds("F180b4", &pwrdrill_b4::run_pwrdrill_b4_checks());
        reds("F181b4", &handoffchk_b4::run_handoffchk_b4_checks());
        reds("F182b4", &duoclock_b4::run_duoclock_b4_checks());
        reds("F183b4", &diskhealth_b4::run_diskhealth_b4_checks());
        reds("F184b4", &hotplug_b4::run_hotplug_b4_checks());
        reds("F185b4", &romount_b4::run_romount_b4_checks());
        reds("F171b5", &bootmenu_b5::run_bootmenu_b5_checks());
        reds("F172b5", &selftestviz_b5::run_selftestviz_b5_checks());
        reds("F173b5", &panicscreen_b5::run_panicscreen_b5_checks());
        reds("F174b5", &diagsnap_b5::run_diagsnap_b5_checks());
        reds("F175b5", &crashiso_b5::run_crashiso_b5_checks());
        reds("F176b5", &memguard_b5::run_memguard_b5_checks());
        reds("F177b5", &capenforce_b5::run_capenforce_b5_checks());
        reds("F178b5", &signbadge_b5::run_signbadge_b5_checks());
        reds("F179b5", &permaudit_b5::run_permaudit_b5_checks());
        reds("F180b5", &pwrdrill_b5::run_pwrdrill_b5_checks());
        reds("F181b5", &handoffchk_b5::run_handoffchk_b5_checks());
        reds("F182b5", &duoclock_b5::run_duoclock_b5_checks());
        reds("F183b5", &diskhealth_b5::run_diskhealth_b5_checks());
        reds("F184b5", &hotplug_b5::run_hotplug_b5_checks());
        reds("F185b5", &romount_b5::run_romount_b5_checks());
        reds("F172b6", &selftestviz_b6::run_selftestviz_b6_checks());
        reds("F173b6", &panicscreen_b6::run_panicscreen_b6_checks());
        reds("F175b6", &crashiso_b6::run_crashiso_b6_checks());
        reds("F176b6", &memguard_b6::run_memguard_b6_checks());
        reds("F177b6", &capenforce_b6::run_capenforce_b6_checks());
        reds("F179b6", &permaudit_b6::run_permaudit_b6_checks());
        reds("F180b6", &pwrdrill_b6::run_pwrdrill_b6_checks());
        reds("F181b6", &handoffchk_b6::run_handoffchk_b6_checks());
        reds("F182b6", &duoclock_b6::run_duoclock_b6_checks());
        reds("F183b6", &diskhealth_b6::run_diskhealth_b6_checks());
        reds("F184b6", &hotplug_b6::run_hotplug_b6_checks());
        reds("F185b6", &romount_b6::run_romount_b6_checks());
        reds("F178b6", &signbadge_b6::run_signbadge_b6_checks());
        reds("F171b7", &bootmenu_b7::run_bootmenu_b7_checks());
        reds("F172b7", &selftestviz_b7::run_selftestviz_b7_checks());
        reds("F173b7", &panicscreen_b7::run_panicscreen_b7_checks());
        reds("F174b7", &diagsnap_b7::run_diagsnap_b7_checks());
        reds("F175b7", &crashiso_b7::run_crashiso_b7_checks());
        reds("F176b7", &memguard_b7::run_memguard_b7_checks());
        reds("F177b7", &capenforce_b7::run_capenforce_b7_checks());
        reds("F178b7", &signbadge_b7::run_signbadge_b7_checks());
        reds("F179b7", &permaudit_b7::run_permaudit_b7_checks());
        reds("F180b7", &pwrdrill_b7::run_pwrdrill_b7_checks());
        reds("F181b7", &handoffchk_b7::run_handoffchk_b7_checks());
        reds("F182b7", &duoclock_b7::run_duoclock_b7_checks());
        reds("F183b7", &diskhealth_b7::run_diskhealth_b7_checks());
        reds("F184b7", &hotplug_b7::run_hotplug_b7_checks());
        reds("F185b7", &romount_b7::run_romount_b7_checks());
        reds("F176b8", &memguard_b8::run_memguard_b8_checks());
        reds("F183b8", &diskhealth_b8::run_diskhealth_b8_checks());
        reds("F178b8", &signbadge_b8::run_signbadge_b8_checks());
        reds("F181b8", &handoffchk_b8::run_handoffchk_b8_checks());
        reds("F180b8", &pwrdrill_b8::run_pwrdrill_b8_checks());
        reds("F172b8", &selftestviz_b8::run_selftestviz_b8_checks());
        reds("F182b8", &duoclock_b8::run_duoclock_b8_checks());
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

#[cfg(test)]
mod ais1_count {
    use crate::secstar::*;
    use crate::checks::CheckSet;
    fn count(mods: &[(&str, CheckSet)]) -> usize {
        let mut n = 0;
        for (tag, set) in mods {
            assert!(!set.truncated(), "{tag} truncated");
            for i in 0..set.len() {
                let c = set.get(i).unwrap();
                assert!(c.passed, "{tag} red: {} {}", c.name, c.detail);
                n += 1;
            }
        }
        n
    }
    #[test]
    fn total_check_items_all_layers() {
        let b3: Vec<(&str, CheckSet)> = vec![
            ("F171", bootmenu_b3::run_bootmenu_b3_checks()), ("F172", selftestviz_b3::run_selftestviz_b3_checks()),
            ("F173", panicscreen_b3::run_panicscreen_b3_checks()), ("F174", diagsnap_b3::run_diagsnap_b3_checks()),
            ("F175", crashiso_b3::run_crashiso_b3_checks()), ("F176", memguard_b3::run_memguard_b3_checks()),
            ("F177", capenforce_b3::run_capenforce_b3_checks()), ("F178", signbadge_b3::run_signbadge_b3_checks()),
            ("F179", permaudit_b3::run_permaudit_b3_checks()), ("F180", pwrdrill_b3::run_pwrdrill_b3_checks()),
            ("F181", handoffchk_b3::run_handoffchk_b3_checks()), ("F182", duoclock_b3::run_duoclock_b3_checks()),
            ("F183", diskhealth_b3::run_diskhealth_b3_checks()), ("F184", hotplug_b3::run_hotplug_b3_checks()),
            ("F185", romount_b3::run_romount_b3_checks())];
        let b4: Vec<(&str, CheckSet)> = vec![
            ("F171", bootmenu_b4::run_bootmenu_b4_checks()), ("F172", selftestviz_b4::run_selftestviz_b4_checks()),
            ("F173", panicscreen_b4::run_panicscreen_b4_checks()), ("F174", diagsnap_b4::run_diagsnap_b4_checks()),
            ("F175", crashiso_b4::run_crashiso_b4_checks()), ("F176", memguard_b4::run_memguard_b4_checks()),
            ("F177", capenforce_b4::run_capenforce_b4_checks()), ("F178", signbadge_b4::run_signbadge_b4_checks()),
            ("F179", permaudit_b4::run_permaudit_b4_checks()), ("F180", pwrdrill_b4::run_pwrdrill_b4_checks()),
            ("F181", handoffchk_b4::run_handoffchk_b4_checks()), ("F182", duoclock_b4::run_duoclock_b4_checks()),
            ("F183", diskhealth_b4::run_diskhealth_b4_checks()), ("F184", hotplug_b4::run_hotplug_b4_checks()),
            ("F185", romount_b4::run_romount_b4_checks())];
        let b5: Vec<(&str, CheckSet)> = vec![
            ("F171", bootmenu_b5::run_bootmenu_b5_checks()), ("F172", selftestviz_b5::run_selftestviz_b5_checks()),
            ("F173", panicscreen_b5::run_panicscreen_b5_checks()), ("F174", diagsnap_b5::run_diagsnap_b5_checks()),
            ("F175", crashiso_b5::run_crashiso_b5_checks()), ("F176", memguard_b5::run_memguard_b5_checks()),
            ("F177", capenforce_b5::run_capenforce_b5_checks()), ("F178", signbadge_b5::run_signbadge_b5_checks()),
            ("F179", permaudit_b5::run_permaudit_b5_checks()), ("F180", pwrdrill_b5::run_pwrdrill_b5_checks()),
            ("F181", handoffchk_b5::run_handoffchk_b5_checks()), ("F182", duoclock_b5::run_duoclock_b5_checks()),
            ("F183", diskhealth_b5::run_diskhealth_b5_checks()), ("F184", hotplug_b5::run_hotplug_b5_checks()),
            ("F185", romount_b5::run_romount_b5_checks())];
        let b6: Vec<(&str, CheckSet)> = vec![
            ("F172", selftestviz_b6::run_selftestviz_b6_checks()), ("F173", panicscreen_b6::run_panicscreen_b6_checks()),
            ("F175", crashiso_b6::run_crashiso_b6_checks()), ("F176", memguard_b6::run_memguard_b6_checks()),
            ("F177", capenforce_b6::run_capenforce_b6_checks()), ("F178", signbadge_b6::run_signbadge_b6_checks()),
            ("F179", permaudit_b6::run_permaudit_b6_checks()), ("F180", pwrdrill_b6::run_pwrdrill_b6_checks()),
            ("F181", handoffchk_b6::run_handoffchk_b6_checks()), ("F182", duoclock_b6::run_duoclock_b6_checks()),
            ("F183", diskhealth_b6::run_diskhealth_b6_checks()), ("F184", hotplug_b6::run_hotplug_b6_checks()),
            ("F185", romount_b6::run_romount_b6_checks())];
        let b7: Vec<(&str, CheckSet)> = vec![
            ("F171", bootmenu_b7::run_bootmenu_b7_checks()), ("F172", selftestviz_b7::run_selftestviz_b7_checks()),
            ("F173", panicscreen_b7::run_panicscreen_b7_checks()), ("F174", diagsnap_b7::run_diagsnap_b7_checks()),
            ("F175", crashiso_b7::run_crashiso_b7_checks()), ("F176", memguard_b7::run_memguard_b7_checks()),
            ("F177", capenforce_b7::run_capenforce_b7_checks()), ("F178", signbadge_b7::run_signbadge_b7_checks()),
            ("F179", permaudit_b7::run_permaudit_b7_checks()), ("F180", pwrdrill_b7::run_pwrdrill_b7_checks()),
            ("F181", handoffchk_b7::run_handoffchk_b7_checks()), ("F182", duoclock_b7::run_duoclock_b7_checks()),
            ("F183", diskhealth_b7::run_diskhealth_b7_checks()), ("F184", hotplug_b7::run_hotplug_b7_checks()),
            ("F185", romount_b7::run_romount_b7_checks())];
        let b8: Vec<(&str, CheckSet)> = vec![
            ("F172", selftestviz_b8::run_selftestviz_b8_checks()), ("F176", memguard_b8::run_memguard_b8_checks()),
            ("F178", signbadge_b8::run_signbadge_b8_checks()), ("F180", pwrdrill_b8::run_pwrdrill_b8_checks()),
            ("F181", handoffchk_b8::run_handoffchk_b8_checks()), ("F182", duoclock_b8::run_duoclock_b8_checks()),
            ("F183", diskhealth_b8::run_diskhealth_b8_checks())];
        let mut total = 0;
        for (name, v) in [("b3", b3), ("b4", b4), ("b5", b5), ("b6", b6), ("b7", b7), ("b8", b8)] {
            let n = count(&v);
            println!("layer {name}: {n}");
            total += n;
        }
        println!("TOTAL = {total}");
        assert!(total >= 1244, "expected >=1244 check items, got {total}");
    }
}

#[cfg(test)]
mod ais1_count2 {
    use crate::secstar::*;
    use crate::checks::CheckSet;
    fn count(name: &str, mods: &[(&str, CheckSet)]) -> usize {
        let mut n = 0;
        for (tag, set) in mods {
            assert!(!set.truncated(), "{tag} truncated");
            for i in 0..set.len() {
                let c = set.get(i).unwrap();
                assert!(c.passed, "{tag} red: {} {}", c.name, c.detail);
                n += 1;
            }
        }
        println!("layer {name}: {n}");
        n
    }
    #[test]
    fn main_deep_layers() {
        let main: Vec<(&str, CheckSet)> = vec![
            ("F171", bootmenu::run_bootmenu_checks()), ("F172", selftestviz::run_selftestviz_checks()),
            ("F173", panicscreen::run_panicscreen_checks()), ("F174", diagsnap::run_diagsnap_checks()),
            ("F175", crashiso::run_crashiso_checks()), ("F176", memguard::run_memguard_checks()),
            ("F177", capenforce::run_capenforce_checks()), ("F178", signbadge::run_signbadge_checks()),
            ("F179", permaudit::run_permaudit_checks()), ("F180", pwrdrill::run_pwrdrill_checks()),
            ("F181", handoffchk::run_handoffchk_checks()), ("F182", duoclock::run_duoclock_checks()),
            ("F183", diskhealth::run_diskhealth_checks()), ("F184", hotplug::run_hotplug_checks()),
            ("F185", romount::run_romount_checks())];
        let deep: Vec<(&str, CheckSet)> = vec![
            ("F171", bootmenu::run_bootmenu_deep_checks()), ("F172", selftestviz::run_selftestviz_deep_checks()),
            ("F173", panicscreen::run_panicscreen_deep_checks()), ("F174", diagsnap::run_diagsnap_deep_checks()),
            ("F175", crashiso::run_crashiso_deep_checks()), ("F176", memguard::run_memguard_deep_checks()),
            ("F177", capenforce::run_capenforce_deep_checks()), ("F178", signbadge::run_signbadge_deep_checks()),
            ("F179", permaudit::run_permaudit_deep_checks()), ("F180", pwrdrill::run_pwrdrill_deep_checks()),
            ("F181", handoffchk::run_handoffchk_deep_checks()), ("F182", duoclock::run_duoclock_deep_checks()),
            ("F183", diskhealth::run_diskhealth_deep_checks()), ("F184", hotplug::run_hotplug_deep_checks()),
            ("F185", romount::run_romount_deep_checks())];
        let a = count("main", &main);
        let b = count("deep", &deep);
        println!("MAIN_DEEP = {}", a + b);
    }
}
