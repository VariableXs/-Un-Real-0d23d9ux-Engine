//! AI-C1 隔离验证壳（_attic · 非功能物品）：整库被其他分队在途代码阻塞时，
//! compatstar 二十域在此独立编译与测试。判据面与主仓逐字节一致（复制自
//! kernel/varix/src/compatstar）。域收官后本壳仅存档于 _attic。
// no_std 档位对齐主仓 crate 根（alloc 宏/类型在两档位同源）。
extern crate alloc;

pub mod checks;
pub mod compatstar;

#[cfg(test)]
mod checkup {
    use crate::checks::CheckSet;
    type F = fn() -> CheckSet;
    const FNS: [(&str, F); 20] = [
        ("F001", crate::compatstar::dblrun::run_dblrun_checks),
        ("F002", crate::compatstar::peblend::run_peblend_checks),
        ("F003", crate::compatstar::pebind::run_pebind_checks),
        ("F004", crate::compatstar::wow64::run_wow64_checks),
        ("F005", crate::compatstar::winmgr::run_winmgr_checks),
        ("F006", crate::compatstar::gdiface::run_gdiface_checks),
        ("F007", crate::compatstar::gdiplus::run_gdiplus_checks),
        ("F008", crate::compatstar::comdlg::run_comdlg_checks),
        ("F009", crate::compatstar::reghive::run_reghive_checks),
        ("F010", crate::compatstar::fsredir::run_fsredir_checks),
        ("F011", crate::compatstar::envsess::run_envsess_checks),
        ("F012", crate::compatstar::condrv::run_condrv_checks),
        ("F013", crate::compatstar::lnkfile::run_lnkfile_checks),
        ("F014", crate::compatstar::persrc::run_persrc_checks),
        ("F015", crate::compatstar::mlangres::run_mlangres_checks),
        ("F016", crate::compatstar::fontchain::run_fontchain_checks),
        ("F017", crate::compatstar::clipfmt::run_clipfmt_checks),
        ("F018", crate::compatstar::dragdrop::run_dragdrop_checks),
        ("F019", crate::compatstar::comloc::run_comloc_checks),
        ("F020", crate::compatstar::excface::run_excface_checks),
    ];

    #[test]
    fn domain_count_matches() {
        assert_eq!(FNS.len(), crate::compatstar::DOMAIN_NAMES.len());
    }

    #[test]
    fn f001() { let cs = FNS[0].1(); assert!(cs.all_passed(), "domain failed:
{}", {
        let mut buf = [0u8; 2048];
        let n = cs.render(&mut buf);
        String::from_utf8_lossy(&buf[..n]).to_string()
    }); }
    #[test]
    fn f002() { let cs = FNS[1].1(); assert!(cs.all_passed(), "domain failed:
{}", {
        let mut buf = [0u8; 2048];
        let n = cs.render(&mut buf);
        String::from_utf8_lossy(&buf[..n]).to_string()
    }); }
    #[test]
    fn f003() { let cs = FNS[2].1(); assert!(cs.all_passed(), "domain failed:
{}", {
        let mut buf = [0u8; 2048];
        let n = cs.render(&mut buf);
        String::from_utf8_lossy(&buf[..n]).to_string()
    }); }
    #[test]
    fn f004() { let cs = FNS[3].1(); assert!(cs.all_passed(), "domain failed:
{}", {
        let mut buf = [0u8; 2048];
        let n = cs.render(&mut buf);
        String::from_utf8_lossy(&buf[..n]).to_string()
    }); }
    #[test]
    fn f005() { let cs = FNS[4].1(); assert!(cs.all_passed(), "domain failed:
{}", {
        let mut buf = [0u8; 2048];
        let n = cs.render(&mut buf);
        String::from_utf8_lossy(&buf[..n]).to_string()
    }); }
    #[test]
    fn f006() { let cs = FNS[5].1(); assert!(cs.all_passed(), "domain failed:
{}", {
        let mut buf = [0u8; 2048];
        let n = cs.render(&mut buf);
        String::from_utf8_lossy(&buf[..n]).to_string()
    }); }
    #[test]
    fn f007() { let cs = FNS[6].1(); assert!(cs.all_passed(), "domain failed:
{}", {
        let mut buf = [0u8; 2048];
        let n = cs.render(&mut buf);
        String::from_utf8_lossy(&buf[..n]).to_string()
    }); }
    #[test]
    fn f008() { let cs = FNS[7].1(); assert!(cs.all_passed(), "domain failed:
{}", {
        let mut buf = [0u8; 2048];
        let n = cs.render(&mut buf);
        String::from_utf8_lossy(&buf[..n]).to_string()
    }); }
    #[test]
    fn f009() { let cs = FNS[8].1(); assert!(cs.all_passed(), "domain failed:
{}", {
        let mut buf = [0u8; 2048];
        let n = cs.render(&mut buf);
        String::from_utf8_lossy(&buf[..n]).to_string()
    }); }
    #[test]
    fn f010() { let cs = FNS[9].1(); assert!(cs.all_passed(), "domain failed:
{}", {
        let mut buf = [0u8; 2048];
        let n = cs.render(&mut buf);
        String::from_utf8_lossy(&buf[..n]).to_string()
    }); }
    #[test]
    fn f011() { let cs = FNS[10].1(); assert!(cs.all_passed(), "domain failed:
{}", {
        let mut buf = [0u8; 2048];
        let n = cs.render(&mut buf);
        String::from_utf8_lossy(&buf[..n]).to_string()
    }); }
    #[test]
    fn f012() { let cs = FNS[11].1(); assert!(cs.all_passed(), "domain failed:
{}", {
        let mut buf = [0u8; 2048];
        let n = cs.render(&mut buf);
        String::from_utf8_lossy(&buf[..n]).to_string()
    }); }
    #[test]
    fn f013() { let cs = FNS[12].1(); assert!(cs.all_passed(), "domain failed:
{}", {
        let mut buf = [0u8; 2048];
        let n = cs.render(&mut buf);
        String::from_utf8_lossy(&buf[..n]).to_string()
    }); }
    #[test]
    fn f014() { let cs = FNS[13].1(); assert!(cs.all_passed(), "domain failed:
{}", {
        let mut buf = [0u8; 2048];
        let n = cs.render(&mut buf);
        String::from_utf8_lossy(&buf[..n]).to_string()
    }); }
    #[test]
    fn f015() { let cs = FNS[14].1(); assert!(cs.all_passed(), "domain failed:
{}", {
        let mut buf = [0u8; 2048];
        let n = cs.render(&mut buf);
        String::from_utf8_lossy(&buf[..n]).to_string()
    }); }
    #[test]
    fn f016() { let cs = FNS[15].1(); assert!(cs.all_passed(), "domain failed:
{}", {
        let mut buf = [0u8; 2048];
        let n = cs.render(&mut buf);
        String::from_utf8_lossy(&buf[..n]).to_string()
    }); }
    #[test]
    fn f017() { let cs = FNS[16].1(); assert!(cs.all_passed(), "domain failed:
{}", {
        let mut buf = [0u8; 2048];
        let n = cs.render(&mut buf);
        String::from_utf8_lossy(&buf[..n]).to_string()
    }); }
    #[test]
    fn f018() { let cs = FNS[17].1(); assert!(cs.all_passed(), "domain failed:
{}", {
        let mut buf = [0u8; 2048];
        let n = cs.render(&mut buf);
        String::from_utf8_lossy(&buf[..n]).to_string()
    }); }
    #[test]
    fn f019() { let cs = FNS[18].1(); assert!(cs.all_passed(), "domain failed:
{}", {
        let mut buf = [0u8; 2048];
        let n = cs.render(&mut buf);
        String::from_utf8_lossy(&buf[..n]).to_string()
    }); }
    #[test]
    fn f020() { let cs = FNS[19].1(); assert!(cs.all_passed(), "domain failed:
{}", {
        let mut buf = [0u8; 2048];
        let n = cs.render(&mut buf);
        String::from_utf8_lossy(&buf[..n]).to_string()
    }); }

    #[test]
    fn check_item_stats() {
        let mut total = 0usize;
        for (tag, f) in FNS {
            let cs = f();
            let (passed, failed) = cs.tally();
            let items = passed + failed;
            total += items;
            println!("{}: {} items (passed {})", tag, items, passed);
            assert!(!cs.truncated(), "{} CheckSet truncated", tag);
            assert_eq!(cs.dropped(), 0, "{} CheckSet dropped items", tag);
        }
        println!("TOTAL CheckSet items: {}", total);
    }
}

pub mod diag;
pub mod diag3;
pub mod diag4;
