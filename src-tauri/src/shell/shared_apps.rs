//! 任务46（AI-V）· 分级登记写 SHARED apps.json ＋ 通道标记数据源。
//!
//! 总案 369/392：SHARED/apps.json 是「软件登记总表」单一事实源——
//! 每软件记录（名称/通道 wine|engine|native-only/分级 ok|partial|blocked
//! /缺失 API 清单）。分级语义（总案 369）：
//! - ✅ ok      可跑（办公/工具/多数桌面）
//! - 🔶 partial 逐步补（缺哪个 API 补哪个，登记缺失清单）
//! - ❌ blocked 不可跑（内核驱动类、反作弊——永远走 native-only 通道）
//!
//! 与任务9（Init-Shared.ps1 schema 定版）/任务44（内核 compatdb 适配
//! 数据库）对齐：本模块是 Windows 侧登记桥——宿主登记表（launcher.rs
//! apps.json）→ SHARED 分区 apps.json 导出，schema 与 Init-Shared 校验
//! 规则逐条同源（version≥1 / channel 枚举 / tier 枚举 / id 唯一）。
//!
//! 零 FFI：SHARED 根发现走「env → 便携根兄弟目录 → 盘符契约标记扫描」，
//! 与 cli_rescue::discover_data_roots 同一判据哲学（文件存在性，不查卷标）。

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{AppError, CmdResult};
use crate::state::AppState;

/// 通道枚举（总案 137：wine | engine | native-only）。
pub const CHANNEL_WINE: &str = "wine";
pub const CHANNEL_ENGINE: &str = "engine";
pub const CHANNEL_NATIVE: &str = "native-only";
pub const CHANNELS: [&str; 3] = [CHANNEL_WINE, CHANNEL_ENGINE, CHANNEL_NATIVE];

/// 分级枚举（总案 369：ok | partial | blocked）。
pub const TIER_OK: &str = "ok";
pub const TIER_PARTIAL: &str = "partial";
pub const TIER_BLOCKED: &str = "blocked";
pub const TIERS: [&str; 3] = [TIER_OK, TIER_PARTIAL, TIER_BLOCKED];

/// SHARED/apps.json schema version（与 Init-Shared.ps1 定版一致）。
pub const SHARED_APPS_VERSION: i64 = 1;

/// 单条分级登记（SHARED/apps.json apps[] 项；缺失清单=任务392 缺失 API 排期数据源）。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SharedAppEntry {
    pub id: String,
    pub name: String,
    pub channel: String,
    pub tier: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing: Vec<String>,
}

/// SHARED/apps.json 顶层（version 字段必填——升版迁移走 Init-Shared 契约）。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SharedAppsFile {
    pub version: i64,
    pub apps: Vec<SharedAppEntry>,
}

/// 通道合法性（与 Init-Shared `allowedChannel` 同表）。
pub fn channel_valid(c: &str) -> bool {
    CHANNELS.contains(&c)
}

/// 分级合法性（与 Init-Shared `allowedTier` 同表）。
pub fn tier_valid(t: &str) -> bool {
    TIERS.contains(&t)
}

/// 分级派生（诚实语义——未证实不虚报）：
/// - native-only：Windows 原生通道全兼容 → ok；
/// - engine：隐形 Windows 引擎=完整 Win32 → ok；
/// - wine：内核 compatdb（任务44）未给出 Full 证据前，缺省 partial
///   （「逐步补」是缺省态）；blocked 显式登记（反作弊/驱动类）。
pub fn derive_tier(channel: &str, wine_tier: &str) -> &'static str {
    if wine_tier == TIER_BLOCKED {
        // 反作弊/驱动类：无论通道，如实登记不可跑 wine（分级描述软件本身）
        return TIER_BLOCKED;
    }
    match channel {
        CHANNEL_WINE => {
            if wine_tier == TIER_OK {
                TIER_OK
            } else {
                TIER_PARTIAL
            }
        }
        _ => TIER_OK,
    }
}

/// 登记表 → SHARED 文件构建（id 去重保序；tier 走派生规则）。
pub fn build_shared_apps(apps: &[(String, String, String, String)]) -> SharedAppsFile {
    let mut out: Vec<SharedAppEntry> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for (id, name, channel, wine_tier) in apps {
        if id.is_empty() || seen.contains(id) {
            continue;
        }
        seen.push(id.clone());
        out.push(SharedAppEntry {
            id: id.clone(),
            name: name.clone(),
            channel: channel.to_string(),
            tier: derive_tier(channel, wine_tier).to_string(),
            missing: Vec::new(),
        });
    }
    SharedAppsFile { version: SHARED_APPS_VERSION, apps: out }
}

/// schema 校验（与 Init-Shared Test-AppsJsonSchema 同规则，宿主侧复检）。
pub fn validate(f: &SharedAppsFile) -> Vec<String> {
    let mut errs = Vec::new();
    if f.version < 1 {
        errs.push(format!("version 非法：{}", f.version));
    }
    let mut ids: Vec<String> = Vec::new();
    for a in &f.apps {
        if a.id.is_empty() {
            errs.push("存在缺 id 的条目".to_string());
            continue;
        }
        if ids.contains(&a.id) {
            errs.push(format!("id 重复：{}", a.id));
        }
        ids.push(a.id.clone());
        if a.name.is_empty() {
            errs.push(format!("{} 缺 name", a.id));
        }
        if !channel_valid(&a.channel) {
            errs.push(format!("{} channel 非法（允许 wine/engine/native-only）", a.id));
        }
        if !tier_valid(&a.tier) {
            errs.push(format!("{} tier 非法（允许 ok/partial/blocked）", a.id));
        }
    }
    errs
}

/// SHARED 根候选发现（优先级序）：①env ②便携根兄弟目录 ③盘符契约标记。
/// 判据=文件存在性（契约骨架 apps.json+handoff），零 FFI 不查卷标。
pub fn discover_shared_roots(data_roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let push = |p: PathBuf, out: &mut Vec<PathBuf>| {
        if !out.contains(&p) {
            out.push(p);
        }
    };
    // ① 显式环境变量（最高优先级）。
    if let Ok(root) = std::env::var("VARIABLE_SHARED_ROOT") {
        let root = PathBuf::from(root.trim());
        if !root.as_os_str().is_empty() {
            push(root, &mut out);
        }
    }
    // ② 便携数据根的 SHARED 兄弟目录（<usb>/SHARED，五分区目录形态）。
    for r in data_roots {
        // data_roots 元素是数据目录（…/data）；SHARED 与其父同级或与根同级
        if let Some(parent) = r.parent() {
            push(parent.join("SHARED"), &mut out);
        }
        push(r.join("SHARED"), &mut out);
    }
    // ③ 盘符扫描：根下有 apps.json + handoff/ 即视为 SHARED 卷（契约标记）。
    for letter in b'A'..=b'Z' {
        let drive = PathBuf::from(format!("{}:\\", letter as char));
        let marker_apps = drive.join("apps.json");
        let marker_handoff = drive.join("handoff");
        if marker_apps.is_file() && marker_handoff.is_dir() {
            push(drive, &mut out);
        }
    }
    out
}

/// SHARED 根有效性：契约骨架存在（apps.json 或 handoff/ 任一）。
pub fn shared_root_valid(p: &Path) -> bool {
    p.join("apps.json").is_file() || p.join("handoff").is_dir()
}

/// 原子导出：tmp 写入 → 旧文件留 .bak → rename 落位（断电不写半文件）。
pub fn export_atomic(path: &Path, file: &SharedAppsFile) -> CmdResult<u64> {
    let json = serde_json::to_string_pretty(file)
        .map_err(|e| AppError::io(format!("apps.json 序列化失败：{}", e)))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| AppError::io(format!("SHARED 目录创建失败：{}", e)))?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, json.as_bytes())
        .map_err(|e| AppError::io(format!("apps.json 临时文件写入失败：{}", e)))?;
    if path.exists() {
        let bak = path.with_extension("json.bak");
        let _ = fs::rename(path, &bak); // 备份失败不阻断（.bak 尽力而为）
    }
    fs::rename(&tmp, path).map_err(|e| AppError::io(format!("apps.json 落位失败：{}", e)))?;
    Ok(json.len() as u64)
}

/// 命令：设置登记项通道（wine|engine|native-only）＋wine 分级（可选）。
/// 通道语义约束（总案 369）：blocked 分级只允许 native-only 通道。
#[tauri::command(async)]
pub fn tp_set_channel(
    st: tauri::State<AppState>,
    id: String,
    channel: String,
    wine_tier: Option<String>,
) -> CmdResult<crate::shell::launcher::ThirdApp> {
    if !channel_valid(&channel) {
        return Err(AppError::validation(format!(
            "channel 非法：{}（允许 wine/engine/native-only）",
            channel
        )));
    }
    if let Some(t) = &wine_tier {
        if !t.is_empty() && !tier_valid(t) {
            return Err(AppError::validation(format!(
                "tier 非法：{}（允许 ok/partial/blocked）",
                t
            )));
        }
    }
    let mut reg = crate::shell::launcher::load_registry(&st);
    if channel != CHANNEL_NATIVE && wine_tier.as_deref() == Some(TIER_BLOCKED) {
        return Err(AppError::validation(
            "blocked 分级仅允许 native-only 通道（反作弊/驱动类永不走 wine/engine）".to_string(),
        ));
    }
    let idx = reg
        .iter()
        .position(|a| a.id == id)
        .ok_or_else(|| AppError::validation(format!("登记项不存在：{}", id)))?;
    let app = &mut reg[idx];
    app.channel = channel;
    if let Some(t) = wine_tier {
        if !t.is_empty() {
            app.wine_tier = t;
        }
    }
    let updated = app.clone();
    crate::shell::launcher::save_registry(&st, &reg)?;
    Ok(updated)
}

/// 命令：分级登记同步 SHARED/apps.json（发现→构建→校验→原子导出）。
/// 返回（落盘路径, 登记条数, 字节数）；未发现 SHARED 根时如实报错，
/// 绝不静默写错位置（单一事实源污染比缺文件更糟）。
#[tauri::command(async)]
pub fn tp_sync_shared_apps(st: tauri::State<AppState>) -> CmdResult<(String, usize, u64)> {
    let reg = crate::shell::launcher::load_registry(&st);
    let apps: Vec<(String, String, String, String)> = reg
        .iter()
        .map(|a| {
            (
                a.id.clone(),
                a.name.clone(),
                if a.channel.is_empty() { CHANNEL_NATIVE.to_string() } else { a.channel.clone() },
                a.wine_tier.clone(),
            )
        })
        .collect();
    let file = build_shared_apps(&apps);
    let errs = validate(&file);
    if !errs.is_empty() {
        return Err(AppError::validation(format!(
            "分级登记未过 schema 校验：{}",
            errs.join("；")
        )));
    }
    let data_roots = crate::cli_rescue::discover_data_roots()
        .into_iter()
        .map(|c| c.data_dir)
        .collect::<Vec<_>>();
    let roots = discover_shared_roots(&data_roots);
    let root = roots
        .iter()
        .find(|p| shared_root_valid(p))
        .ok_or_else(|| AppError::validation(
            "未发现 SHARED 契约根（env VARIABLE_SHARED_ROOT / 便携根 SHARED/ / 盘符契约标记均未命中）".to_string(),
        ))?;
    let path = root.join("apps.json");
    let bytes = export_atomic(&path, &file)?;
    Ok((path.display().to_string(), file.apps.len(), bytes))
}

/// 命令：读取 SHARED/apps.json 现状（设置页展示对比用；缺失返回空）。
#[tauri::command(async)]
pub fn tp_shared_apps_snapshot(_st: tauri::State<AppState>) -> CmdResult<Option<SharedAppsFile>> {
    let data_roots = crate::cli_rescue::discover_data_roots()
        .into_iter()
        .map(|c| c.data_dir)
        .collect::<Vec<_>>();
    let roots = discover_shared_roots(&data_roots);
    let Some(root) = roots.iter().find(|p| shared_root_valid(p)) else {
        return Ok(None);
    };
    let path = root.join("apps.json");
    let Ok(bytes) = fs::read(&path) else {
        return Ok(None);
    };
    Ok(serde_json::from_slice::<SharedAppsFile>(&bytes).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(id: &str, name: &str, channel: &str, wine_tier: &str) -> (String, String, String, String) {
        (id.to_string(), name.to_string(), channel.to_string(), wine_tier.to_string())
    }

    #[test]
    fn task46_derive_tier_follows_honest_defaults() {
        assert_eq!(derive_tier(CHANNEL_NATIVE, ""), TIER_OK);
        assert_eq!(derive_tier(CHANNEL_ENGINE, ""), TIER_OK);
        // wine 未证实 → partial（缺省态=逐步补）
        assert_eq!(derive_tier(CHANNEL_WINE, ""), TIER_PARTIAL);
        assert_eq!(derive_tier(CHANNEL_WINE, TIER_OK), TIER_OK);
        assert_eq!(derive_tier(CHANNEL_WINE, TIER_BLOCKED), TIER_BLOCKED);
    }

    #[test]
    fn task46_schema_validation_matches_init_shared_rules() {
        let f = build_shared_apps(&[
            app("a1", "Notepad", CHANNEL_WINE, TIER_OK),
            app("a2", "Game", CHANNEL_NATIVE, ""),
            app("a3", "Tool", CHANNEL_ENGINE, ""),
        ]);
        assert!(validate(&f).is_empty());
        assert_eq!(f.version, 1);
        assert_eq!(f.apps[0].tier, TIER_OK);
        assert_eq!(f.apps[1].tier, TIER_OK);
        assert_eq!(f.apps[2].tier, TIER_OK);
        // 非法 channel/tier/重复 id 逐条报错
        let bad = SharedAppsFile {
            version: 1,
            apps: vec![
                SharedAppEntry { id: "x".into(), name: "X".into(), channel: "vmware".into(), tier: "ok".into(), missing: vec![] },
                SharedAppEntry { id: "y".into(), name: "Y".into(), channel: "wine".into(), tier: "maybe".into(), missing: vec![] },
                SharedAppEntry { id: "x".into(), name: "X2".into(), channel: "wine".into(), tier: "ok".into(), missing: vec![] },
            ],
        };
        let errs = validate(&bad);
        assert_eq!(errs.len(), 3, "channel/tier/重复id 各一条：{:?}", errs);
    }

    #[test]
    fn task46_atomic_export_roundtrip_and_backup() {
        let dir = std::env::temp_dir().join(format!("varix-t46-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("apps.json");
        let v1 = build_shared_apps(&[app("a1", "One", CHANNEL_WINE, TIER_PARTIAL)]);
        let n1 = export_atomic(&path, &v1).expect("first export");
        assert!(n1 > 0);
        let read: SharedAppsFile = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(read, v1);
        // 第二次导出：旧文件留 .bak
        let v2 = build_shared_apps(&[app("a1", "One", CHANNEL_ENGINE, "")]);
        export_atomic(&path, &v2).expect("second export");
        assert!(dir.join("apps.json.bak").is_file(), "旧文件必须留 .bak");
        let bak: SharedAppsFile = serde_json::from_slice(&fs::read(dir.join("apps.json.bak")).unwrap()).unwrap();
        assert_eq!(bak, v1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn task46_discovery_env_and_contract_markers() {
        // ① env 优先
        std::env::set_var("VARIABLE_SHARED_ROOT", r"D:\varix-t46-shared");
        let roots = discover_shared_roots(&[]);
        std::env::remove_var("VARIABLE_SHARED_ROOT");
        assert_eq!(roots.first(), Some(&PathBuf::from(r"D:\varix-t46-shared")));
        // ③ 盘符契约标记扫描（用进程可写的临时盘根不可行——直测判据函数）
        assert!(!shared_root_valid(Path::new(r"C:\definitely-not-varix-shared-zz")));
    }

    #[test]
    fn task46_blocked_semantics_only_native() {
        // blocked 登记（反作弊）走 native-only 通道由命令层强制；派生层如实透传
        let f = build_shared_apps(&[app("ac", "AntiCheat", CHANNEL_NATIVE, TIER_BLOCKED)]);
        assert!(validate(&f).is_empty());
        assert_eq!(f.apps[0].tier, TIER_BLOCKED, "native-only + 显式 blocked 如实保留");
    }
}
