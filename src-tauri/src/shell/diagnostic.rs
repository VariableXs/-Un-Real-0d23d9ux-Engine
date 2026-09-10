//! B-34：诊断包导出 + 演示胶囊（脱敏，绝不含数据 chunk / 凭据 / token）。

use std::path::{Path, PathBuf};

use container::StorageBackend as _;

use serde::Serialize;

use crate::error::AppError;
use crate::state::AppState;

type CmdResult<T> = Result<T, AppError>;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagReport {
    pub out: String,
    pub sections: usize,
}

fn scrub(s: &str) -> String {
    // 脱敏：用户名路径段、token 样式的长串
    let mut out = s.to_string();
    if let Some(home) = std::env::var("USERPROFILE").ok() {
        let user = home
            .rsplit(['\\', '/'])
            .next()
            .unwrap_or("")
            .to_string();
        if !user.is_empty() {
            out = out.replace(&user, "<user>");
        }
        out = out.replace(&home, "<home>");
    }
    out
}

#[tauri::command(async)]
pub fn diagnostic_export(st: tauri::State<AppState>, out: String) -> CmdResult<DiagReport> {
    diagnostic_export_inner(&st, Path::new(&out))
}

pub(crate) fn diagnostic_export_inner(st: &AppState, out: &Path) -> CmdResult<DiagReport> {
    let mut md = String::from("# Variable 诊断包\n\n");
    md.push_str("> 脱敏规则：用户名/主目录路径已替换；不含数据 chunk、凭据、token。\n\n");
    let mut sections = 0usize;

    // 1) 宿主环境
    md.push_str("## 宿主环境\n\n");
    md.push_str(&format!(
        "- OS: {} {}\n",
        std::env::consts::OS,
        std::env::consts::ARCH
      ));
    md.push_str(&format!(
        "- 管理员: {}（探测见 设置 → 编码/安全 各 probe）\n",
        "见能力矩阵"
    ));
    sections += 1;

    // 2) 容器元信息（若存在）
    let container = st.data_dir.join("data.uxv");
    if container.is_file() {
        md.push_str("\n## 容器\n\n");
        md.push_str(&format!(
            "- 文件: {} · {} B\n",
            scrub(&container.to_string_lossy()),
            std::fs::metadata(&container).map(|m| m.len()).unwrap_or(0)
        ));
        match container::schema_probe(&container) {
            Ok(info) => {
                md.push_str(&format!(
                    "- schemaVersion: {}（引擎 {}）· needs_migration: {}\n",
                    info.container_version, info.engine_version, info.needs_migration
                ));
            }
            Err(e) => md.push_str(&format!("- 探测失败: {e}\n")),
        }
        sections += 1;
    }

    // 3) 网络层状态
    if let Ok(s) = crate::shell::network::net_status_snapshot(st) {
        md.push_str("\n## 网络\n\n");
        md.push_str(&format!(
            "- 代理运行: {} · kill-switch: {} · 规则 {} 条\n",
            s.proxy_running, s.kill_switch, s.rule_count
        ));
        md.push_str(&format!(
            "- 放行 {} · 拒绝 {} · 转发 {} B\n",
            s.conns_allowed, s.conns_denied, s.bytes_relayed
        ));
        sections += 1;
    }

    // 4) 数据目录清单（不含内容）
    md.push_str("\n## 数据目录结构\n\n");
    fn walk(dir: &Path, depth: usize, md: &mut String) {
        if depth > 2 {
            return;
        }
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let is_dir = e.path().is_dir();
                md.push_str(&format!(
                    "- {}{} ({} B)\n",
                    "  ".repeat(depth),
                    e.file_name().to_string_lossy(),
                    e.metadata().map(|m| m.len()).unwrap_or(0)
                ));
                if is_dir {
                    walk(&e.path(), depth + 1, md);
                }
            }
        }
    }
    walk(&st.data_dir, 0, &mut md);
    sections += 1;

    md.push_str("\n## 隐私声明\n\n");
    md.push_str("- 本包不含：数据 chunk、凭据/token、文档内容、媒体内容。\n");
    md.push_str("- 目的：故障排查与社区支持（用户自愿提供）。\n");

    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::io(e.to_string()))?;
    }
    std::fs::write(out, md).map_err(|e| AppError::io(e.to_string()))?;
    Ok(DiagReport {
        out: out.to_string_lossy().into_owned(),
        sections,
    })
}

/// 演示胶囊：构建空容器模板（OOBE 前置物；build-windows.bat demo 目标调用的内核）。
pub fn demo_capsule_build(data_dir: &Path) -> CmdResult<PathBuf> {
    let out = data_dir.join("demo-capsule.uxv");
    if out.exists() && std::fs::metadata(&out).map(|m| m.len()).unwrap_or(0) > 0 {
        return Ok(out);
    }
    let mut be = container::UxvBackend::new();
    use container::StorageBackend;
    be.open(&container::OpenCfg {
        root: out.clone(),
        extra_volumes: Vec::new(),
    })
    .map_err(|e| AppError::new("CONTAINER", e.to_string()))?;
    use container::StorageBackend as _;
    be.seal().map_err(|e| AppError::new("CONTAINER", e.to_string()))?;
    Ok(out)
}

#[tauri::command(async)]
pub fn demo_capsule(st: tauri::State<AppState>) -> CmdResult<String> {
    demo_capsule_build(&st.data_dir).map(|p| p.to_string_lossy().into_owned())
}
