//! VX13 续作 B26 探针：unx10115~unx10116 两模块本地编译与判据收口自检。
//! 打印 MODULES=n TOTAL RED PANIC；有红项则逐条打明细并以非零码退出。
extern crate alloc;

#[path = "../../../../varix/src/checks.rs"]
mod checks;

#[path = "../../../../varix/src/unxstar/unx10115_unlink_rename.rs"]
mod unx10115;
#[path = "../../../../varix/src/unxstar/unx10116_mmap.rs"]
mod unx10116;
#[path = "../../../../varix/src/unxstar/unx10117_munmap_mprotect.rs"]
mod unx10117;
#[path = "../../../../varix/src/unxstar/unx10118_brk.rs"]
mod unx10118;
#[path = "../../../../varix/src/unxstar/unx10119_exit_wait.rs"]
mod unx10119;
#[path = "../../../../varix/src/unxstar/unx10120_b26.rs"]
mod unx10120;
#[path = "../../../../varix/src/unxstar/unx10121_open_o__o_excl.rs"]
mod unx10121;

#[cfg(test)]
mod dbg {
    use super::*;

    /// B26 六单判据全绿回归锚（防止后续批次改动破坏 B26）。
    #[test]
    fn b26_all_green() {
        let sets = [
            unx10115::run_unx10115_checks(),
            unx10116::run_unx10116_checks(),
            unx10117::run_unx10117_checks(),
            unx10118::run_unx10118_checks(),
            unx10119::run_unx10119_checks(),
            unx10120::run_unx10120_checks(),
        ];
        for s in sets.iter() {
            assert!(s.all_passed(), "{} red: {}", s.domain, s.red_items_text());
        }
    }
}

fn main() {
    let sets = [
        unx10115::run_unx10115_checks(),
        unx10116::run_unx10116_checks(),
        unx10117::run_unx10117_checks(),
        unx10118::run_unx10118_checks(),
        unx10119::run_unx10119_checks(),
        unx10120::run_unx10120_checks(),
        unx10121::run_unx10121_checks(),
    ];
    let mut total = 0usize;
    let mut red = 0usize;
    for s in sets.iter() {
        let (passed, failed) = s.tally();
        total += passed + failed;
        red += failed;
        print!("{}: {}/{} pass", s.domain, passed, passed + failed);
        if failed > 0 {
            println!("  红项: {}", s.red_items_text());
        } else {
            println!();
        }
    }
    println!("MODULES=7 TOTAL={} RED={} PANIC=0", total, red);
    if red > 0 {
        std::process::exit(1);
    }
}
