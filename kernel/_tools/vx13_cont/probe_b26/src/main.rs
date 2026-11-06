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
#[path = "../../../../varix/src/unxstar/unx10122_t10122.rs"]
mod unx10122;
#[path = "../../../../varix/src/unxstar/unx10123_readdir.rs"]
mod unx10123;
#[path = "../../../../varix/src/unxstar/unx10124_link_unlink.rs"]
mod unx10124;
#[path = "../../../../varix/src/unxstar/unx10125_symlink_readlink.rs"]
mod unx10125;
#[path = "../../../../varix/src/unxstar/unx10126_rename.rs"]
mod unx10126;

#[cfg(test)]
mod dbg {
    use super::*;

    #[test]
    fn debug_link_faces() {
        let mut l = unx10124::InoLedger::new();
        let ino = l.create("a", unx10124::DEV_A, b"DATA").unwrap_or(0);
        println!("f1 nlink={:?}", l.nlink_of("a"));
        let r = l.link("a", "b");
        println!("f2 link={:?} nlink={:?}", r, l.nlink_of("a"));
        let ba = l.body_of("a").unwrap_or_default();
        let bb = l.body_of("b").unwrap_or_default();
        println!("f3 same={}", ba == bb);
        let u = l.unlink("b");
        println!("f4 unlink={:?} nlink={:?}", u, l.nlink_of("a"));
        let u2 = l.unlink("a");
        println!("f5 unlink={:?} name_gone={} rel={}", u2, l.name_ino("a").is_none(), l.releases);
        println!("f6 conserved={}", l.conserved());
    }

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
        unx10122::run_unx10122_checks(),
        unx10123::run_unx10123_checks(),
        unx10124::run_unx10124_checks(),
        unx10125::run_unx10125_checks(),
        unx10126::run_unx10126_checks(),
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
    println!("MODULES=12 TOTAL={} RED={} PANIC=0", total, red);
    if red > 0 {
        std::process::exit(1);
    }
}
