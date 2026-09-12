//! Code Analysis 壳A 入口（W1 骨架）：`CodeAnalysis.exe <项目路径>`。
//!
//! 完整窗口体系属后续波次（P2）；本入口验证 core 契约 open_project 可用。

use ca_core::ir::open_project;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).map(|s| s.as_str()).unwrap_or(".");
    match open_project(std::path::Path::new(path)) {
        Ok(ir) => {
            println!("项目: {}  文件: {}  行数: {}", ir.name, ir.file_count, ir.loc);
            for (lang, n) in &ir.lang_stats {
                println!("  {lang}: {n}");
            }
            println!("调用边: {}", ir.calls.len());
            for s in ca_core::run_ca_checks() {
                if !s.all_pass() {
                    eprintln!("{}", s.render());
                    std::process::exit(1);
                }
            }
            println!("自检 64/64 通过");
        }
        Err(e) => {
            eprintln!("无法打开项目 {path}: {e}");
            std::process::exit(1);
        }
    }
}
