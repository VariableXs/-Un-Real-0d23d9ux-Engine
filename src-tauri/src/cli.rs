//! CLI 应急通道（B-33 M3 半包 + 任务63 U 盘自救援适配）：GUI 之外的抢救入口。
//!
//! 用法（任意一台电脑，无需 GUI；容器/输出可省略走自动发现）：
//! - `Variable.exe --export-rescue [容器.uxv] [输出目录] [口令]`
//!   文件级救援失败自动降级 chunk 级；U 盘形态输出缺省写回 U 盘 rescue/；
//! - `Variable.exe --repair [容器.uxv] [口令]`
//!   journal 重放 + checkpoint 固化；
//! - `Variable.exe --force-raster`
//!   写入软件渲染标记文件后正常进入 GUI（黑屏演练的降级开关）；
//! - `Variable.exe --revoke-list [输出]`
//!   吊销清单导出（缺省落数据根同级 rescue/）。
//!
//! 任务63 自动发现（cli_rescue::discover_data_roots）：环境变量 → exe 便携标记
//! → 全盘符判据扫描（.portable / data\data.uxv / Variable\.portable）。
//! U 盘自救援语义：可救援自身，不强依赖宿主——数据根、容器、输出全部
//! 优先落 U 盘；宿主上零部署也能从插着的 U 盘救援。
//!
//! 设计原则：CLI 与 GUI 共用同一套 container crate 实现，零逻辑分叉。

use std::path::{Path, PathBuf};

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
    /// 显式容器参数是否为空（空 = 走自动发现，打印发现路径提示）。
    fn ownerless(explicit: &Option<String>) -> bool {
        explicit.as_deref().map(|c| c.trim().is_empty()).unwrap_or(true)
    }
    match first {
        "--export-rescue" => {
            // 任务63：容器/输出均可选——缺省自动发现数据根（U 盘自救援，
            // 不强依赖宿主）；输出缺省写回数据根同级 rescue/。
            let explicit_container = args.get(1).cloned();
            let explicit_out = args.get(2).cloned();
            // 口令槽位：容器缺省时上移一格（--export-rescue [输出] [口令]）。
            let pass = if explicit_container.as_deref().map(|c| !c.trim().is_empty()).unwrap_or(false) {
                args.get(3).cloned().filter(|s| !s.is_empty())
            } else {
                args.get(2).cloned().filter(|s| !s.is_empty())
            };
            let (container, owner) = match crate::cli_rescue::resolve_container(
                explicit_container.as_deref().filter(|c| !c.trim().is_empty()),
            ) {
                Ok(v) => {
                    if ownerless(&explicit_container) {
                        println!("自动发现容器: {}", v.0.display());
                    }
                    v
                }
                Err(e) => {
                    eprintln!("{e}");
                    eprintln!("用法: Variable --export-rescue [容器.uxv] [输出目录] [口令]");
                    return Some(2);
                }
            };
            let out = match explicit_out.as_deref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
                Some(o) => PathBuf::from(o),
                None => {
                    let d = crate::cli_rescue::default_out_dir(owner.as_ref(), &container);
                    println!("输出缺省（U 盘自救援落盘）: {}", d.display());
                    d
                }
            };
            Some(crate::cli_rescue::export_rescue_run(&container, &out, pass.as_deref()))
        }
        "--repair" => {
            // 任务63：容器可选——缺省自动发现（U 盘自救援）。
            let explicit_container = args.get(1).cloned();
            let pass = if explicit_container.as_deref().map(|c| !c.trim().is_empty()).unwrap_or(false) {
                args.get(2).cloned()
            } else {
                args.get(1).cloned().filter(|s| !s.trim().is_empty())
            };
            let (container, owner) = match crate::cli_rescue::resolve_container(
                explicit_container.as_deref().filter(|c| !c.trim().is_empty()),
            ) {
                Ok(v) => {
                    if ownerless(&explicit_container) {
                        println!("自动发现容器: {}", v.0.display());
                    }
                    v
                }
                Err(e) => {
                    eprintln!("{e}");
                    eprintln!("用法: Variable --repair [容器.uxv] [口令]");
                    return Some(2);
                }
            };
            let _ = owner;
            Some(crate::cli_rescue::repair_run(&container, pass.as_deref()))
        }
        "--force-raster" => {
            // 任务63：数据根自动发现（U 盘形态自动指向 U 盘，无需宿主目录）。
            let base = crate::cli_rescue::discover_data_roots()
                .into_iter()
                .next()
                .map(|c| c.data_dir);
            match base {
                Some(base) => Some(crate::cli_rescue::force_raster_run(&base)),
                None => {
                    let st = st();
                    Some(crate::cli_rescue::force_raster_run(&st.data_dir))
                }
            }
        }
        // ---- AI-14 N-29 variable-cli：离线子命令族（在线族经 N-28 网关，见 --help）----
        "--doctor" => {
            let st = st();
            let ok = std::fs::create_dir_all(&st.data_dir).is_ok();
            let container = st.data_dir.join("data.uxv");
            println!("data-dir: {}", st.data_dir.display());
            println!("writable: {ok}");
            println!(
                "container: {}",
                if container.is_file() { "present" } else { "absent (fresh)" }
            );
            println!(
                "engine: {} / os: {}-{}",
                env!("CARGO_PKG_VERSION"),
                std::env::consts::OS,
                std::env::consts::ARCH
            );
            if args.iter().any(|a| a == "--json") {
                println!(
                    "{}",
                    serde_json::json!({
                        "dataDir": st.data_dir.to_string_lossy(),
                        "writable": ok,
                        "container": container.is_file(),
                        "engine": env!("CARGO_PKG_VERSION"),
                    })
                );
            }
            Some(if ok { 0 } else { 1 })
        }
        "--api-token-gen" => {
            let st = st();
            let token = uuid::Uuid::new_v4().simple().to_string();
            // 写入 openhub.json（与网关共用 token 字段）
            let path = st.data_dir.join("openhub.json");
            let mut cfg: serde_json::Value = std::fs::read_to_string(&path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or(serde_json::json!({}));
            cfg["gatewayToken"] = serde_json::json!(token);
            match serde_json::to_string_pretty(&cfg)
                .map_err(|e| e.to_string())
                .and_then(|s| std::fs::write(&path, s).map_err(|e| e.to_string()))
            {
                Ok(()) => {
                    println!("token: {token}");
                    println!("（写入 openhub.json；网关开启后即生效）");
                    if args.iter().any(|a| a == "--json") {
                        println!("{}", serde_json::json!({ "token": token }));
                    }
                    Some(0)
                }
                Err(e) => {
                    eprintln!("写入失败: {e}");
                    Some(1)
                }
            }
        }
        // ---- AI-15 开放工具组 CLI ----
        // V-88 CLI 交互式教程：--tour [章节|list]
        "--tour" => {
            let topic = args.get(1).cloned().unwrap_or_else(|| "list".into());
            Some(crate::shell::opentools::cli_tour(&topic))
        }
        // M-58 插件开发热重载（CLI 侧）：校验插件目录/清单 + 打印热重载工作流指引
        "--plugin-dev" => {
            let dir = args.get(1).cloned().unwrap_or_default();
            if dir.is_empty() {
                eprintln!("用法: Variable --plugin-dev <插件目录>");
                return Some(2);
            }
            match crate::shell::opentools::cli_plugin_dev(Path::new(&dir)) {
                Ok(msg) => {
                    println!("{msg}");
                    Some(0)
                }
                Err(e) => {
                    eprintln!("校验失败: {e}");
                    Some(1)
                }
            }
        }
        // M-60 测试钩子规范（CLI 侧）：--test-ready 扫描 data-testid 覆盖与规范文件
        "--test-ready" => {
            let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            match crate::shell::opentools::cli_test_ready(&root) {
                Ok((ok, msg)) => {
                    println!("{msg}");
                    Some(if ok { 0 } else { 1 })
                }
                Err(e) => {
                    eprintln!("扫描失败: {e}");
                    Some(1)
                }
            }
        }
        "--help" | "-h" => {
            // 三段式帮助：应急 | 控制 | 生态（N-29 统一入口）
            println!("应急: --export-rescue [容器] [输出] [口令] | --repair [容器] [口令] | --force-raster | --revoke-list [out]");
            println!("     （任务63 U 盘自救援：容器/输出可省略，自动发现便携数据根，输出写回 U 盘 rescue/）");
            println!("生态: --doctor [--json] | --api-token-gen [--json] | --tour [章节|list] | --plugin-dev <插件目录> | --test-ready");
            println!("控制: 在线命令族经本地网关（设置 → 开放接口 → 网关，默认关闭）");
            Some(0)
        }
        "--revoke-list" => {
            // 任务63：输出缺省落数据根同级 rescue/（U 盘形态写回 U 盘自身）。
            let out = match args.get(1).cloned().filter(|s| !s.trim().is_empty()) {
                Some(o) => PathBuf::from(o),
                None => match crate::cli_rescue::discover_data_roots().into_iter().next() {
                    Some(c) => {
                        let d = c.root.join("rescue");
                        let _ = std::fs::create_dir_all(&d);
                        d.join("revocation-list.md")
                    }
                    None => PathBuf::from("revocation-list.md"),
                },
            };
            let st = st();
            Some(crate::cli_rescue::revoke_list_run(&st, &out))
        }
        _ => None,
    }
}
