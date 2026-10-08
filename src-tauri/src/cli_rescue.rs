//! 任务63：救援 CLI 四命令的 U 盘自救援适配层（总案 545/546）。
//!
//! 目标：U 盘形态「可救援自身，不强依赖宿主」——
//! - 容器/输出零参数时自动发现数据根（env → 便携标记 → 全盘符判据扫描）；
//! - 输出缺省写回 U 盘自身（数据根同级 `rescue/`），不散落宿主目录；
//! - 复用 B-33 应急包实现语义（container::salvage_* / UxvBackend journal 重放 /
//!   recovery::revocation_list_export_inner），CLI 与设置页共享同一服务层，零逻辑分叉。
//!
//! 自动发现判据（对齐 state.rs 便携语义）：
//! ① `VARIABLE_DATA_ROOT` 环境变量（B-25 嵌套/救援显式指定，优先级最高）；
//! ② exe 便携根（`.portable` 标记或 `VARIABLE_PORTABLE=1`，与 state::portable_base 同源）；
//! ③ 全盘符扫描：`X:\.portable`（根部署）/ `X:\data\data.uxv`（数据盘形态）/
//!    `X:\Variable\.portable`（常规便携部署名）。
//!
//! 判据取文件存在性而非卷标查询——零 FFI 依赖，且对任意部署命名鲁棒
//! （「不强依赖宿主」同样意味着不强依赖特定卷标约定）。

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use container::StorageBackend as _;

use crate::state::AppState;

/// 单个数据根候选：根目录（data 目录的父目录）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootCandidate {
    pub root: PathBuf,      // 数据根父目录（如 X:\Variable）
    pub data_dir: PathBuf,  // 数据目录（如 X:\Variable\data）
    pub source: &'static str, // 发现来源（env / portable-exe / marker / scan）
}

/// 数据根候选发现（优先级序，去重后返回）。
pub fn discover_data_roots() -> Vec<RootCandidate> {
    let mut out: Vec<RootCandidate> = Vec::new();
    let push = |c: RootCandidate, out: &mut Vec<RootCandidate>| {
        if !out.iter().any(|e| e.data_dir == c.data_dir) {
            out.push(c);
        }
    };
    // ① 显式环境变量（救援通道显式指定，最高优先级）。
    if let Ok(root) = std::env::var("VARIABLE_DATA_ROOT") {
        let root = PathBuf::from(root.trim());
        if !root.as_os_str().is_empty() {
            push(
                RootCandidate { data_dir: root.clone(), root, source: "env" },
                &mut out,
            );
        }
    }
    // ② exe 便携根（与 state::portable_base 同源判定）。
    if let Some(exe_dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())) {
        if exe_dir.join(".portable").exists()
            || std::env::var("VARIABLE_PORTABLE").map(|v| v == "1" || v.eq_ignore_ascii_case("true")).unwrap_or(false)
        {
            push(
                RootCandidate {
                    data_dir: exe_dir.join("data"),
                    root: exe_dir,
                    source: "portable-exe",
                },
                &mut out,
            );
        }
    }
    // ③ 全盘符判据扫描（固定盘符 A..Z，存在性检查均为 O(1)）。
    for b in b'A'..=b'Z' {
        let letter = b as char;
        let drive = PathBuf::from(format!("{letter}:\\"));
        if !drive.exists() {
            continue;
        }
        // 3a 根部署：X:\.portable（五分区 VARIX_SYS 或根目录便携）。
        if drive.join(".portable").exists() {
            push(
                RootCandidate { data_dir: drive.join("data"), root: drive.clone(), source: "marker" },
                &mut out,
            );
        }
        // 3b 数据盘形态：X:\data\data.uxv。
        let data = drive.join("data");
        if data.join("data.uxv").is_file() {
            push(
                RootCandidate { data_dir: data.clone(), root: data.parent().unwrap_or(&drive).to_path_buf(), source: "marker" },
                &mut out,
            );
        }
        // 3c 常规便携部署名：X:\Variable\.portable（E 盘实测布局）。
        let var_dir = drive.join("Variable");
        if var_dir.join(".portable").exists() {
            push(
                RootCandidate { data_dir: var_dir.join("data"), root: var_dir, source: "marker" },
                &mut out,
            );
        }
    }
    out
}

/// 便携部署标记（暴露给测试：3a/3c 共用判据）。
pub fn portable_marker(dir: &Path) -> bool {
    dir.join(".portable").exists()
}

/// 容器解析：显式路径优先；缺省 = 候选根里第一个存在 `data/data.uxv` 的。
/// 返回 (容器文件路径, 所属数据根)。找不到时 Err 带候选诊断。
pub fn resolve_container(explicit: Option<&str>) -> Result<(PathBuf, Option<RootCandidate>), String> {
    if let Some(c) = explicit {
        let c = c.trim();
        if c.is_empty() {
            return Err("容器路径为空".into());
        }
        let p = PathBuf::from(c);
        if !p.is_file() {
            return Err(format!("容器不存在: {}", p.display()));
        }
        return Ok((p, None));
    }
    let roots = discover_data_roots();
    for cand in &roots {
        let container = cand.data_dir.join("data.uxv");
        if container.is_file() {
            return Ok((container, Some(cand.clone())));
        }
    }
    let seen: Vec<String> = roots.iter().map(|c| format!("{}（{}）", c.data_dir.display(), c.source)).collect();
    Err(if seen.is_empty() {
        "未发现任何 VARIX 数据根（无环境变量/便携标记/盘符判据命中）".to_string()
    } else {
        format!("候选数据根均无 data.uxv 容器: {}", seen.join("; "))
    })
}

/// 输出目录缺省：<数据根父>/rescue/rescue-<epoch 秒>（写回 U 盘自身）。
pub fn default_out_dir(owned: Option<&RootCandidate>, container: &Path) -> PathBuf {
    if let Some(c) = owned {
        let ts = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        return c.root.join("rescue").join(format!("rescue-{ts}"));
    }
    // 显式容器路径：取容器目录（保守，不越级猜测布局）。
    container.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| PathBuf::from("."))
}

/// `--export-rescue` 执行体（纯函数：显式参数，返回退出码；打印走 stdout/stderr）。
pub fn export_rescue_run(container: &Path, out: &Path, pass: Option<&str>) -> i32 {
    let result = match container::salvage_files(container, out, pass.map(|p| p.as_bytes())) {
        Ok(r) => r,
        Err(files_err) => {
            println!("文件级救援失败（{files_err}），降级 chunk 级…");
            match container::salvage_chunks(container, out) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("chunk 级救援也失败: {e}");
                    return 1;
                }
            }
        }
    };
    println!("模式: {}", result.mode);
    println!("文件救回: {} 个", result.files_rescued.len());
    println!("chunk 救回: {}", result.chunks_rescued);
    println!("字节: {}", result.bytes_rescued);
    for e in &result.errors {
        println!("错误: {e}");
    }
    0
}

/// `--repair` 执行体（journal 重放 + checkpoint 固化；纯函数）。
pub fn repair_run(container: &Path, pass: Option<&str>) -> i32 {
    let mut be = container::UxvBackend::new();
    let opened = match pass.map(|p| p.as_bytes().to_vec()).as_deref() {
        Some(p) => be.open_with_passphrase(
            &container::OpenCfg { root: container.to_path_buf(), extra_volumes: Vec::new() },
            p,
        ),
        None => be.open(&container::OpenCfg {
            root: container.to_path_buf(),
            extra_volumes: Vec::new(),
        }),
    };
    match opened.and_then(|_| be.seal()) {
        Ok(()) => {
            println!("journal 重放完成并已固化（--repair）");
            0
        }
        Err(e) => {
            eprintln!("修复失败: {e}");
            1
        }
    }
}

/// `--force-raster` 执行体（软件渲染标记；base = 数据根目录）。
pub fn force_raster_run(base: &Path) -> i32 {
    let flag = base.join("force-raster.flag");
    if std::fs::write(&flag, b"software rendering requested").is_ok() {
        println!("软件渲染标记已写入：{}（下次启动生效）", flag.display());
        0
    } else {
        eprintln!("标记写入失败");
        1
    }
}

/// `--revoke-list` 执行体（st 由调用方构造；U 盘形态 data_dir 已自动指向 U 盘）。
pub fn revoke_list_run(st: &AppState, out: &Path) -> i32 {
    match crate::shell::recovery::revocation_list_export_inner(st, out) {
        Ok(r) => {
            println!("吊销清单已导出（{} 项）: {}", r.entries, r.out);
            0
        }
        Err(e) => {
            eprintln!("导出失败: {e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use container::{OpenCfg, UxvBackend, VPath};
    use std::path::PathBuf;

    /// "系统半损坏"模拟根：临时目录造 data/ + 容器，返回 (数据根, 容器路径)。
    fn half_broken_root(tag: &str, round: u32) -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("rescue63-{tag}-{}-r{round}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let data = root.join("data");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(root.join(".portable"), b"").unwrap();
        (root.clone(), data.join("data.uxv"))
    }

    fn write_files(container: &Path, seal: bool) {
        let mut be = UxvBackend::new();
        be.open(&OpenCfg { root: container.to_path_buf(), extra_volumes: Vec::new() }).unwrap();
        be.write(&VPath::new("docs/note.txt").unwrap(), b"rescue-me-63").unwrap();
        be.write(&VPath::new("docs/keep.bin").unwrap(), &[7u8; 64]).unwrap();
        if seal {
            be.seal().unwrap();
        }
    }

    /// 形态A（journal 未固化/Footer 缺失）：export-rescue 自动降级 chunk 级 ×3。
    #[test]
    fn export_rescue_survives_footer_missing_x3() {
        for round in 0..3 {
            let (root, container) = half_broken_root("footerless", round);
            write_files(&container, false); // 不 seal = 半损坏形态 A
            let out = root.join("rescue").join(format!("a{round}"));
            let code = export_rescue_run(&container, &out, None);
            assert_eq!(code, 0, "round {round} 导出应成功");
            let rescued = std::fs::read_dir(&out).unwrap().count();
            assert!(rescued >= 1, "round {round} 至少救回 1 个文件");
            let _ = std::fs::remove_dir_all(&root);
        }
    }

    /// 形态B（journal 半写=尾部撕裂）：repair 重放已提交事务并固化 ×3。
    /// 语义：seal 过的容器继续写（journal 挂起）→ 掉电撕裂 journal 尾部 →
    /// open 自动重放已提交事务 + seal 固化新 checkpoint（--repair 真实用武之地）。
    #[test]
    fn repair_recovers_truncated_journal_x3() {
        for round in 0..3 {
            let (root, container) = half_broken_root("truncated", round);
            write_files(&container, true); // A,B 已固化
            {
                // 追加挂起事务（journal PUT，未 seal）。
                let mut be = UxvBackend::new();
                be.open(&OpenCfg { root: container.clone(), extra_volumes: Vec::new() }).unwrap();
                be.write(&VPath::new("docs/late.txt").unwrap(), b"torn-or-not").unwrap();
            }
            // 模拟掉电撕裂：截掉 journal 尾部（COMMIT 帧可能被撕）。
            let len = std::fs::metadata(&container).unwrap().len();
            let f = std::fs::OpenOptions::new().read(true).write(true).open(&container).unwrap();
            f.set_len(len.saturating_sub(8)).unwrap();
            drop(f);
            assert_eq!(repair_run(&container, None), 0, "round {round} repair 应成功");
            // 固化后容器应可再次打开并读回数据。
            let mut be = UxvBackend::new();
            be.open(&OpenCfg { root: container.clone(), extra_volumes: Vec::new() })
                .unwrap_or_else(|e| panic!("round {round} repair 后应可打开: {e}"));
            // checkpoint 区数据必在；挂起事务两态皆合法（COMMIT 完整→重放，撕裂→丢弃）。
            let vp = VPath::new("docs/note.txt").unwrap();
            assert_eq!(be.read(&vp).unwrap(), b"rescue-me-63", "round {round} 固化区数据读回");
            let _ = std::fs::remove_dir_all(&root);
        }
    }

    /// 形态B'（Footer 副本损坏）：副本 B 翻转 → open 走副本 A 兜底 → repair 固化 ×3。
    #[test]
    fn repair_survives_footer_b_corruption_x3() {
        for round in 0..3 {
            let (root, container) = half_broken_root("footerb", round);
            write_files(&container, true);
            // 就地破坏副本 B 的魔数区（uxv.rs：FOOTER_LEN=96，MAGIC @ [52..60]；
            // 副本 A = [off, off+96)，副本 B = [off+96, off+192)），不改变文件长度。
            {
                use std::io::{Read, Seek, SeekFrom, Write};
                let mut f = std::fs::OpenOptions::new().read(true).write(true).open(&container).unwrap();
                f.seek(SeekFrom::Start(12)).unwrap(); // SuperBlock footer_offset @ [12..20]
                let mut off = [0u8; 8];
                f.read_exact(&mut off).unwrap();
                let footer_offset = u64::from_le_bytes(off);
                assert!(footer_offset > 0, "round {round} 已 seal 必有 checkpoint");
                f.seek(SeekFrom::Start(footer_offset + 96 + 52)).unwrap(); // 副本 B 魔数
                f.write_all(&[0xAAu8; 8]).unwrap();
            }
            assert_eq!(repair_run(&container, None), 0, "round {round} repair 应走副本 A 兜底");
            let mut be = UxvBackend::new();
            be.open(&OpenCfg { root: container.clone(), extra_volumes: Vec::new() })
                .unwrap_or_else(|e| panic!("round {round} repair 后应可打开: {e}"));
            assert_eq!(
                be.read(&VPath::new("docs/keep.bin").unwrap()).unwrap(),
                [7u8; 64],
                "round {round} 数据完好"
            );
            let _ = std::fs::remove_dir_all(&root);
        }
    }

    /// 形态C（记录流中段破坏）：export-rescue chunk 级跳过损坏救回完好 ×3。
    #[test]
    fn export_rescue_skips_corrupt_records_x3() {
        for round in 0..3 {
            let (root, container) = half_broken_root("corrupt", round);
            write_files(&container, true);
            // 破坏记录流中段。
            {
                use std::io::{Seek, SeekFrom, Write};
                let mut f = std::fs::OpenOptions::new().read(true).write(true).open(&container).unwrap();
                f.seek(SeekFrom::Start(64 + 10)).unwrap();
                f.write_all(&[0xFFu8; 8]).unwrap();
            }
            let out = root.join("rescue").join(format!("c{round}"));
            assert_eq!(export_rescue_run(&container, &out, None), 0, "round {round} 导出应成功");
            assert!(out.exists(), "round {round} chunk 级应产出");
            let _ = std::fs::remove_dir_all(&root);
        }
    }

    /// force-raster ×3：临时数据根三次写标记。
    #[test]
    fn force_raster_writes_flag_x3() {
        for round in 0..3 {
            let (root, _container) = half_broken_root("raster", round);
            assert_eq!(force_raster_run(&root.join("data")), 0, "round {round}");
            assert_eq!(
                std::fs::read(root.join("data").join("force-raster.flag")).unwrap(),
                b"software rendering requested",
                "round {round} 标记内容"
            );
            let _ = std::fs::remove_dir_all(&root);
        }
    }

    /// revoke-list ×3：空身份库导出走通（entries=0 合法）。
    #[test]
    fn revoke_list_exports_x3() {
        for round in 0..3 {
            let (root, _container) = half_broken_root("revoke", round);
            let st = AppState::bootstrap_at(root.join("data")).unwrap();
            let out = root.join("rescue").join(format!("revoke{round}.md"));
            assert_eq!(revoke_list_run(&st, &out), 0, "round {round}");
            let md = std::fs::read_to_string(&out).unwrap();
            assert!(md.contains("紧急吊销清单"), "round {round} 清单头");
            let _ = std::fs::remove_dir_all(&root);
        }
    }

    /// 自动发现：env 指向优先；判据函数对 .portable 部署命中。
    #[test]
    fn discover_respects_env_and_markers() {
        let (root, container) = half_broken_root("discover", 0);
        write_files(&container, true);
        // 判据：便携标记命中。
        assert!(portable_marker(&root));
        // env 优先：设到临时根后 discover 首位应指它（盘符扫描可能命中真盘，env 排最前）。
        std::env::set_var("VARIABLE_DATA_ROOT", root.join("data"));
        let roots = discover_data_roots();
        std::env::remove_var("VARIABLE_DATA_ROOT");
        assert_eq!(roots.first().unwrap().source, "env");
        assert_eq!(roots.first().unwrap().data_dir, root.join("data"));
        // 容器解析走 env 候选命中。
        std::env::set_var("VARIABLE_DATA_ROOT", root.join("data"));
        let (c, owner) = resolve_container(None).unwrap();
        std::env::remove_var("VARIABLE_DATA_ROOT");
        assert_eq!(c, container);
        assert_eq!(owner.unwrap().source, "env");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 输出缺省落 <数据根父>/rescue/（U 盘自身，不强依赖宿主）。
    #[test]
    fn default_out_lands_inside_root() {
        let (root, container) = half_broken_root("outdir", 0);
        write_files(&container, true);
        let cand = RootCandidate {
            root: root.clone(),
            data_dir: root.join("data"),
            source: "portable-exe",
        };
        let out = default_out_dir(Some(&cand), &container);
        assert!(out.starts_with(&root), "输出必须落在数据根内");
        assert!(out.to_string_lossy().contains("rescue"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
