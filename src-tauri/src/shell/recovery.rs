//! 存储恢复与应急命令（B-33 M2 半包 + B-17 仪表接线 + B-32 OOBE 建卷）。
//!
//! 命令面：
//! - `container_diag`：惰性诊断（版本/魔数/可开性），恢复模式入口的判定依据；
//! - `container_repair`：journal 重放 UI 化——打开（内含重放）→ 立即 checkpoint 固化；
//! - `container_rescue_export`：两级救援（文件级 → chunk 级）导出到普通目录；
//! - `container_init`：OOBE 建卷（可选口令加密）；
//! - `container_stats`：水位线/写放大仪表数据源；
//! - `vhdx_probe`：介质体检（管理员 + Mount-VHD 能力）。
//!
//! 错误约定：ContainerError → AppError("CONTAINER")，前端 errMessage 直读 message。

use std::path::PathBuf;

use serde::Serialize;

use crate::error::AppError;
use crate::state::AppState;
use container::StorageBackend as _;

type CmdResult<T> = Result<T, AppError>;

fn ce(e: container::ContainerError) -> AppError {
    AppError::new("CONTAINER", e.to_string())
}

fn parse_passphrase(passphrase: &Option<String>) -> Option<Vec<u8>> {
    passphrase
        .as_deref()
        .filter(|p| !p.is_empty())
        .map(|p| p.as_bytes().to_vec())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerDiag {
    pub exists: bool,
    pub magic_ok: bool,
    pub container_version: u32,
    pub engine_version: u32,
    pub needs_migration: bool,
    pub downgrade_required: bool,
    pub size_bytes: u64,
    pub openable: bool,
    pub open_error: Option<String>,
}

#[tauri::command]
pub fn container_diag(path: String) -> CmdResult<ContainerDiag> {
    let p = PathBuf::from(&path);
    let exists = p.exists();
    let mut diag = ContainerDiag {
        exists,
        magic_ok: false,
        container_version: 0,
        engine_version: container::SCHEMA_VERSION,
        needs_migration: false,
        downgrade_required: false,
        size_bytes: 0,
        openable: false,
        open_error: None,
    };
    if !exists {
        return Ok(diag);
    }
    diag.size_bytes = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
    match container::schema_probe(&p) {
        Ok(info) => {
            diag.magic_ok = true;
            diag.container_version = info.container_version;
            diag.needs_migration = info.needs_migration;
            diag.downgrade_required = info.downgrade_required;
        }
        Err(e) => {
            diag.open_error = Some(e.to_string());
            return Ok(diag);
        }
    }
    // 可开性判定：只读尝试打开（不落任何写）
    let mut be = container::UxvBackend::new();
    match be.open(&container::OpenCfg { root: p, extra_volumes: Vec::new() }) {
        Ok(()) => diag.openable = true,
        Err(e) => diag.open_error = Some(e.to_string()),
    }
    Ok(diag)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepairReport {
    pub repaired: bool,
    pub message: String,
    pub file_count: u64,
    pub chunk_count: u64,
}

#[tauri::command]
pub fn container_repair(path: String, passphrase: Option<String>) -> CmdResult<RepairReport> {
    let p = PathBuf::from(&path);
    let pass = parse_passphrase(&passphrase);
    let mut be = container::UxvBackend::new();
    match &pass {
        Some(pw) => be
            .open_with_passphrase(
                &container::OpenCfg { root: p.clone(), extra_volumes: Vec::new() },
                pw,
            )
            .map_err(ce)?,
        None => be
            .open(&container::OpenCfg { root: p.clone(), extra_volumes: Vec::new() })
            .map_err(ce)?,
    }
    // 打开即完成 journal 重放；立即 checkpoint 把恢复态固化（--repair 语义）。
    be.seal().map_err(ce)?;
    let stats = be.stats();
    Ok(RepairReport {
        repaired: true,
        message: "journal 重放完成并已 checkpoint 固化".into(),
        file_count: stats.file_count,
        chunk_count: stats.chunk_count,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RescueExportReport {
    pub mode: String,
    pub files_rescued: Vec<String>,
    pub chunks_rescued: u64,
    pub bytes_rescued: u64,
    pub errors: Vec<String>,
}

#[tauri::command]
pub fn container_rescue_export(
    path: String,
    out_dir: String,
    passphrase: Option<String>,
) -> CmdResult<RescueExportReport> {
    let pass = parse_passphrase(&passphrase);
    let pass_ref = pass.as_deref();
    // 第一级：文件级；失败（Footer 失效/结构损坏）→ 第二级：chunk 级裸走查。
    match container::salvage_files(std::path::Path::new(&path), std::path::Path::new(&out_dir), pass_ref) {
        Ok(r) => Ok(RescueExportReport {
            mode: r.mode.to_string(),
            files_rescued: r.files_rescued,
            chunks_rescued: r.chunks_rescued,
            bytes_rescued: r.bytes_rescued,
            errors: r.errors,
        }),
        Err(files_err) => {
            let r = container::salvage_chunks(std::path::Path::new(&path), std::path::Path::new(&out_dir))
                .map_err(|chunk_err| {
                    AppError::new(
                        "CONTAINER",
                        format!("文件级救援失败：{files_err}；chunk 级救援失败：{chunk_err}"),
                    )
                })?;
            Ok(RescueExportReport {
                mode: r.mode.to_string(),
                files_rescued: r.files_rescued,
                chunks_rescued: r.chunks_rescued,
                bytes_rescued: r.bytes_rescued,
                errors: r.errors,
            })
        }
    }
}

#[tauri::command]
pub fn container_init(path: String, passphrase: Option<String>) -> CmdResult<RepairReport> {
    let p = PathBuf::from(&path);
    if p.exists() && std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0) > 0 {
        return Err(AppError::validation("目标容器已存在且非空，拒绝覆盖"));
    }
    let pass = parse_passphrase(&passphrase);
    let mut be = container::UxvBackend::new();
    match &pass {
        Some(pw) => be
            .open_with_passphrase(
                &container::OpenCfg { root: p, extra_volumes: Vec::new() },
                pw,
            )
            .map_err(ce)?,
        None => be
            .open(&container::OpenCfg { root: p, extra_volumes: Vec::new() })
            .map_err(ce)?,
    }
    be.seal().map_err(ce)?;
    let stats = be.stats();
    Ok(RepairReport {
        repaired: true,
        message: if pass.is_some() { "加密容器已创建" } else { "容器已创建" }.into(),
        file_count: stats.file_count,
        chunk_count: stats.chunk_count,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerStatsView {
    pub volumes: Vec<VolumeView>,
    pub write_amplification: f64,
    pub file_count: u64,
    pub chunk_count: u64,
    pub logical_written: u64,
    pub physical_written: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VolumeView {
    pub path: String,
    pub used_bytes: u64,
    pub declared_capacity: u64,
}

#[tauri::command]
pub fn container_stats(path: String, passphrase: Option<String>) -> CmdResult<ContainerStatsView> {
    let pass = parse_passphrase(&passphrase);
    let mut be = container::UxvBackend::new();
    match &pass {
        Some(pw) => be
            .open_with_passphrase(
                &container::OpenCfg { root: PathBuf::from(&path), extra_volumes: Vec::new() },
                pw,
            )
            .map_err(ce)?,
        None => be
            .open(&container::OpenCfg { root: PathBuf::from(&path), extra_volumes: Vec::new() })
            .map_err(ce)?,
    }
    let s = be.stats();
    Ok(ContainerStatsView {
        volumes: s
            .per_volume
            .into_iter()
            .map(|v| VolumeView {
                path: v.path.to_string_lossy().into_owned(),
                used_bytes: v.used_bytes,
                declared_capacity: v.declared_capacity,
            })
            .collect(),
        write_amplification: s.write_amplification,
        file_count: s.file_count,
        chunk_count: s.chunk_count,
        logical_written: s.logical_written,
        physical_written: s.physical_written,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VhdxProbeView {
    pub is_admin: bool,
    pub mount_vhd_available: bool,
    pub usable: bool,
}

#[tauri::command]
pub fn vhdx_probe() -> CmdResult<VhdxProbeView> {
    let p = container::vhdx_probe();
    Ok(VhdxProbeView {
        is_admin: p.is_admin,
        mount_vhd_available: p.mount_vhd_available,
        usable: p.usable,
    })
}
