//! VX9 尾批 W6 探针：卡 12758/12759/12760/12762 四模块本地编译与判据收口自检。
//! 跑 4 个 run_unx<id>_checks()，统计 total/red，打印 TOTAL=4 RED=0
//! PANIC=0；有红项则逐条打明细并以非零码退出。
extern crate alloc;

mod checks;

#[path = "../../../../varix/src/unxstar/unx12758_b39_preview.rs"]
mod unx12758;
#[path = "../../../../varix/src/unxstar/unx12759_asset_sha.rs"]
mod unx12759;
#[path = "../../../../varix/src/unxstar/unx12760_seh_chain_decl.rs"]
mod unx12760;
#[path = "../../../../varix/src/unxstar/unx12762_semantic_confirm.rs"]
mod unx12762;

fn main() {
    let sets = [
        unx12758::run_unx12758_checks(),
        unx12759::run_unx12759_checks(),
        unx12760::run_unx12760_checks(),
        unx12762::run_unx12762_checks(),
    ];
    let mut total = 0usize;
    let mut red = 0usize;
    let panicked = 0usize;
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
    // 判据谓词双跑一致抽验（对账一致语义）。
    let a = unx12758::extra_checks().len();
    let b = unx12758::extra_checks().len();
    if a != b || a == 0 {
        println!("RED: unx12758 双跑条数不一致 {} vs {}", a, b);
        red += 1;
    }
    let c = unx12759::extra_checks().len();
    let d = unx12759::extra_checks().len();
    if c != d || c == 0 {
        println!("RED: unx12759 双跑条数不一致 {} vs {}", c, d);
        red += 1;
    }
    let e = unx12760::extra_checks().len();
    let f = unx12760::extra_checks().len();
    if e != f || e == 0 {
        println!("RED: unx12760 双跑条数不一致 {} vs {}", e, f);
        red += 1;
    }
    let g = unx12762::extra_checks().len();
    let h = unx12762::extra_checks().len();
    if g != h || g == 0 {
        println!("RED: unx12762 双跑条数不一致 {} vs {}", g, h);
        red += 1;
    }
    println!(
        "TOTAL={} RED={} PANIC={}",
        total + red,
        red,
        panicked
    );
    if red > 0 {
        std::process::exit(1);
    }
}
