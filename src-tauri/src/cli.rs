//! CLI 应急通道（B-33 M3 半包）：GUI 之外的抢救入口。
//!
//! 用法（任意一台电脑，无需 GUI）：
//! - `Variable.exe --export-rescue <容器.uxv> <输出目录> [口令]`
//!   文件级救援失败自动降级 chunk 级；
//! - `Variable.exe --repair <容器.uxv> [口令]`
//!   journal 重放 + checkpoint 固化；
//! - `Variable.exe --force-raster`
//!   写入软件渲染标记文件后正常进入 GUI（黑屏演练的降级开关）。
//!
//! 设计原则：CLI 与 GUI 共用同一套 container crate 实现，零逻辑分叉。

use std::path::{Path, PathBuf};

use container::StorageBackend as _;

use crate::state::AppState;

pub fn run_cli(args: &[String]) -> Option<i32> {
    let first = args.first()?.as_str();
    let data_dir_override = std::env::var("VARIABLE_DATA_ROOT").ok();
    let st = || -> AppState {
        match &data_dir_override {
            Some(root) => AppState::bootstrap_dirs_at(PathBuf::from(root))
                .expect("数据根初始化失败"),
            None => AppState::bootstrap().expect("数据目录初始化失败"),
        }
    };
    match first {
        "--export-rescue" => {
            let container = args.get(1).cloned().unwrap_or_default();
            let out = args.get(2).cloned().unwrap_or_default();
            let pass = args.get(3).cloned();
            if container.is_empty() || out.is_empty() {
                eprintln!("用法: Variable --export-rescue <容器.uxv> <输出目录> [口令]");
                return Some(2);
            }
            let pass_ref = pass.as_deref();
            let result = match container::salvage_files(
                Path::new(&container),
                Path::new(&out),
                pass_ref.map(|p| p.as_bytes()),
            ) {
                Ok(r) => r,
                Err(files_err) => {
                    println!("文件级救援失败（{files_err}），降级 chunk 级…");
                    container::salvage_chunks(Path::new(&container), Path::new(&out))
                        .expect("chunk 级救援也失败")
                }
            };
            println!("模式: {}", result.mode);
            println!("文件救回: {} 个", result.files_rescued.len());
            println!("chunk 救回: {}", result.chunks_rescued);
            println!("字节: {}", result.bytes_rescued);
            for e in &result.errors {
                println!("错误: {e}");
            }
            Some(0)
        }
        "--repair" => {
            let container = args.get(1).cloned().unwrap_or_default();
            let pass = args.get(2).cloned();
            if container.is_empty() {
                eprintln!("用法: Variable --repair <容器.uxv> [口令]");
                return Some(2);
            }
            let mut be = container::UxvBackend::new();
            let opened = match pass.as_deref().map(|p| p.as_bytes().to_vec()).as_deref() {
                Some(p) => be.open_with_passphrase(
                    &container::OpenCfg { root: PathBuf::from(&container), extra_volumes: Vec::new() },
                    p,
                ),
                None => be.open(&container::OpenCfg {
                    root: PathBuf::from(&container),
                    extra_volumes: Vec::new(),
                }),
            };
            match opened.and_then(|_| be.seal()) {
                Ok(()) => {
                    println!("journal 重放完成并已固化（--repair）");
                    Some(0)
                }
                Err(e) => {
                    eprintln!("修复失败: {e}");
                    Some(1)
                }
            }
        }
        "--force-raster" => {
            let st = st();
            let flag = st.data_dir.join("force-raster.flag");
            if std::fs::write(&flag, b"software rendering requested").is_ok() {
                println!("软件渲染标记已写入：{:?}（下次启动生效）", flag);
                Some(0)
            } else {
                eprintln!("标记写入失败");
                Some(1)
            }
        }
        "--revoke-list" => {
            let out = args.get(1).cloned().unwrap_or_else(|| "revocation-list.md".into());
            let st = st();
            match crate::shell::recovery::revocation_list_export_inner(&st, Path::new(&out)) {
                Ok(r) => {
                    println!("吊销清单已导出（{} 项）: {}", r.entries, r.out);
                    Some(0)
                }
                Err(e) => {
                    eprintln!("导出失败: {e}");
                    Some(1)
                }
            }
        }
        _ => None,
    }
}
