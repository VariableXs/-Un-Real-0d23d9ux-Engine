//! L3 shell — wallpaper.rs（批次E-6 壁纸细节）：
//! - 多显示器独立壁纸：IDesktopWallpaper（Windows 原生，每个显示器一个 device path）
//! - 每日自动换：从本地缓存目录（Bing 壁纸缓存等用户自备图片）按日期确定性取一张，
//!   或随机换一张 —— 零网络，纯本地文件选择
//! - 仅 Windows 有真实行为；其余平台返回空/报错（与 hardware.rs 同策略）

use serde::Serialize;

use crate::error::{AppError, CmdResult};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WpMonitor {
    /// IDesktopWallpaper 的 monitor device path（SetWallpaper 直接回传使用）
    pub id: String,
    pub primary: bool,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[cfg(windows)]
fn with_wallpaper<T>(
    f: impl FnOnce(&windows::Win32::UI::Shell::IDesktopWallpaper) -> windows::core::Result<T>,
) -> CmdResult<T> {
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED};
    use windows::Win32::UI::Shell::{DesktopWallpaper, IDesktopWallpaper};

    let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    let need_uninit = hr.is_ok();
    let result = (|| -> CmdResult<T> {
        let wp: IDesktopWallpaper = unsafe { CoCreateInstance(&DesktopWallpaper, None, CLSCTX_INPROC_SERVER) }
            .map_err(|e| AppError::io(format!("IDesktopWallpaper 创建失败 / create failed: {e}")))?;
        f(&wp).map_err(|e| AppError::io(format!("壁纸操作失败 / wallpaper failed: {e}")))
    })();
    if need_uninit {
        unsafe { CoUninitialize() };
    }
    result
}

/// 枚举显示器（IDesktopWallpaper 视角，id 可直接用于 SetWallpaper）。
#[tauri::command(async)]
#[cfg(windows)]
pub fn wp_monitors() -> CmdResult<Vec<WpMonitor>> {
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::IDesktopWallpaper;
    with_wallpaper(|wp: &IDesktopWallpaper| -> windows::core::Result<Vec<WpMonitor>> {
        unsafe {
            let n = wp.GetMonitorDevicePathCount()?;
            let mut out = Vec::new();
            for i in 0..n {
                let id = wp.GetMonitorDevicePathAt(i)?;
                let id_s = id.to_string()?;
                let r = wp.GetMonitorRECT(PCWSTR(id.as_ptr()))?;
                out.push(WpMonitor {
                    id: id_s,
                    primary: i == 0,
                    x: r.left,
                    y: r.top,
                    width: (r.right - r.left).max(0) as u32,
                    height: (r.bottom - r.top).max(0) as u32,
                });
            }
            Ok(out)
        }
    })
}

#[tauri::command(async)]
#[cfg(not(windows))]
pub fn wp_monitors() -> CmdResult<Vec<WpMonitor>> {
    Ok(Vec::new())
}

/// 设置某显示器（monitor 为空 = 全部显示器）的桌面壁纸为 path 指向的图片。
#[tauri::command(async)]
#[cfg(windows)]
pub fn wp_set_monitor(monitor: String, path: String) -> CmdResult<()> {
    use windows::core::{HSTRING, PCWSTR};
    with_wallpaper(|wp| {
        unsafe {
            let mon: Option<HSTRING> = if monitor.is_empty() { None } else { Some(HSTRING::from(monitor.as_str())) };
            let img = HSTRING::from(path.as_str());
            let mon_pcw = mon
                .as_ref()
                .map(|m| PCWSTR(m.as_ptr()))
                .unwrap_or(PCWSTR::null());
            wp.SetWallpaper(mon_pcw, PCWSTR(img.as_ptr()))
        }
    })
}

#[tauri::command(async)]
#[cfg(not(windows))]
pub fn wp_set_monitor(_monitor: String, _path: String) -> CmdResult<()> {
    Err(AppError::validation("仅 Windows 支持 / Windows only"))
}

/// 从本地缓存目录挑一张壁纸（零网络）：
/// - mode="date"：按当天日期确定性取一张（每日自动换，同一天内稳定）
/// - mode="next"：随机取一张（右键"下一张壁纸"）
/// 目录无可用图片时返回 None（前端如实提示，不伪造）。
#[tauri::command(async)]
pub fn wp_pick_daily(dir: String, mode: String) -> CmdResult<Option<String>> {
    let root = std::path::PathBuf::from(&dir);
    if !root.is_dir() {
        return Err(AppError::not_found(format!(
            "壁纸缓存目录不存在 / cache dir not found: {dir}"
        )));
    }
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&root)
        .map_err(|e| AppError::io(format!("读取目录失败 / read_dir failed: {e}")))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.extension()
                    .map(|e| {
                        let e = e.to_string_lossy().to_lowercase();
                        e == "jpg" || e == "jpeg" || e == "png" || e == "webp" || e == "bmp"
                    })
                    .unwrap_or(false)
        })
        .collect();
    files.sort();
    if files.is_empty() {
        return Ok(None);
    }
    let idx: usize = if mode == "next" {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos() as usize ^ d.as_secs() as usize)
            .unwrap_or(0);
        nanos % files.len()
    } else {
        // 每日：以 UTC 日期天数为种子，同一天稳定命中同一张
        let days = (std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            / 86_400) as usize;
        days % files.len()
    };
    Ok(files
        .get(idx)
        .map(|p| p.to_string_lossy().to_string()))
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WpEngineItem {
    /// 项目目录名（workshop 为内容 id）
    pub id: String,
    pub title: String,
    /// Wallpaper Engine 类型（scene/video/web/image/application，小写）
    pub kind: String,
    /// 可打开的入口文件绝对路径（video/image 媒体、web 的 index.html）
    pub file: Option<String>,
    /// 项目目录绝对路径（"用 Wallpaper Engine 打开"需要）
    pub dir: String,
    /// 预览图绝对路径（preview.jpg，可能没有）
    pub preview: Option<String>,
    /// Variable 能否直接渲染（video/image = 媒体壁纸；web = 内嵌 iframe；
    /// scene = 着色器型走前端 WebGL 本地渲染；其余回退预览图静态壁纸）
    pub supported: bool,
    /// 来源目录（workshop / myprojects）
    pub source: String,
}

/// 实机反馈：scene 项目常缺 preview.jpg —— 回退扫描项目目录里最大的图片
/// （WE scene 的 textures/meshes 贴图几乎必有图片），保证"本地打开"不再报错。
/// 跳过 <20KB 的小图（图标/角标）；递归限深防巨型目录拖慢扫描。
fn find_fallback_preview(project_dir: &std::path::Path) -> Option<String> {
    let mut best: Option<(u64, std::path::PathBuf)> = None;
    let mut stack = vec![project_dir.to_path_buf()];
    let mut visited = 0usize;
    while let Some(dir) = stack.pop() {
        if visited > 4000 {
            break;
        }
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            visited += 1;
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            let is_img = p
                .extension()
                .map(|e| {
                    let e = e.to_string_lossy().to_lowercase();
                    matches!(e.as_str(), "jpg" | "jpeg" | "png" | "webp" | "bmp" | "gif")
                })
                .unwrap_or(false);
            if !is_img {
                continue;
            }
            let size = e.metadata().map(|m| m.len()).unwrap_or(0);
            if size < 20 * 1024 {
                continue;
            }
            if best.as_ref().map(|(s, _)| size > *s).unwrap_or(true) {
                best = Some((size, p));
            }
        }
    }
    best.map(|(_, p)| p.to_string_lossy().to_string())
}

/// 从 project.json 所在目录提取一条可导入项（解析失败返回 None）。
fn wp_engine_item(project_dir: &std::path::Path, source: &str) -> Option<WpEngineItem> {
    let raw = std::fs::read(project_dir.join("project.json")).ok()?;
    // Wallpaper Engine 的 json 可能带 BOM
    let text = String::from_utf8(raw).ok()?;
    let text = text.trim_start_matches('\u{feff}');
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    let kind = v.get("type").and_then(|t| t.as_str()).unwrap_or("").to_lowercase();
    let title = v
        .get("title")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string();
    let rel_file = v.get("file").and_then(|f| f.as_str()).unwrap_or("");
    let file_path = if rel_file.is_empty() {
        None
    } else {
        let p = project_dir.join(rel_file);
        p.is_file().then(|| p.to_string_lossy().to_string())
    };
    let ext = file_path
        .as_ref()
        .and_then(|f| std::path::Path::new(f).extension())
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let supported = match kind.as_str() {
        // video/image：媒体壁纸；web：入口是 html，Variable 内嵌 iframe 渲染；
        // scene：着色器型（project.json 的 file 直接指向 GLSL 片元着色器）
        // 由前端 WebGL 引擎本地渲染（实机反馈：必须全本地，不靠 WE 本体）
        "video" | "image" => matches!(
            ext.as_str(),
            "mp4" | "webm" | "ogv" | "mov" | "m4v" | "jpg" | "jpeg" | "png" | "webp" | "bmp" | "gif"
        ),
        "web" => matches!(ext.as_str(), "html" | "htm"),
        "scene" => matches!(ext.as_str(), "frag" | "glsl" | "fs" | "fsh"),
        _ => false,
    };
    // 实机反馈（动态壁纸）：预览图 GIF 优先 —— WE scene/video 项目常带 preview.gif
    // 动图预览，导入 living/image 模式后 <img> 原生播放动图，不再是死静态。
    let preview = project_dir
        .join("preview.gif")
        .is_file()
        .then(|| project_dir.join("preview.gif").to_string_lossy().to_string())
        .or_else(|| {
            project_dir
                .join("preview.jpg")
                .is_file()
                .then(|| project_dir.join("preview.jpg").to_string_lossy().to_string())
        })
        .or_else(|| find_fallback_preview(project_dir));
    Some(WpEngineItem {
        id: project_dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default(),
        title: if title.is_empty() {
            project_dir
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default()
        } else {
            title
        },
        kind,
        file: file_path,
        dir: project_dir.to_string_lossy().to_string(),
        preview,
        supported,
        source: source.to_string(),
    })
}

/// 收集 root 下可能包含 wallpaper 项目（project.json）的目录。
/// 兼容三种 root：Steam 库根 / wallpaper_engine 根 / workshop 431960 / 直接项目父目录。
fn wp_engine_candidates(root: &std::path::Path) -> Vec<(std::path::PathBuf, &'static str)> {
    let mut out = Vec::new();
    let mut push_dir = |d: std::path::PathBuf, src: &'static str| {
        if d.join("project.json").is_file() {
            out.push((d, src));
        }
    };
    // Steam 库根 → workshop 创意工坊 + 本地 projects
    let ws = root.join("steamapps").join("workshop").join("content").join("431960");
    if ws.is_dir() {
        if let Ok(rd) = std::fs::read_dir(&ws) {
            for e in rd.flatten() {
                push_dir(e.path(), "workshop");
            }
        }
    }
    let we = root.join("steamapps").join("common").join("wallpaper_engine");
    let proj_dirs = [we.join("projects").join("myprojects"), we.join("projects")];
    for pd in proj_dirs {
        if pd.is_dir() {
            if let Ok(rd) = std::fs::read_dir(&pd) {
                for e in rd.flatten() {
                    push_dir(e.path(), "myprojects");
                }
            }
        }
    }
    // root 本身就是项目父目录（用户手动选择 myprojects / 431960 等）
    if let Ok(rd) = std::fs::read_dir(root) {
        for e in rd.flatten() {
            push_dir(e.path(), "custom");
        }
    }
    out
}

/// 探测 Steam 库根：注册表 HKCU\Software\Valve\Steam\SteamPath 优先，
/// 再补常见安装位置；每个根再解析 libraryfolders.vdf 里的其余库。
#[cfg(windows)]
pub(crate) fn steam_library_roots() -> Vec<std::path::PathBuf> {
    let mut roots: Vec<std::path::PathBuf> = Vec::new();
    // 注册表（Steam 正装路径，含非 C 盘安装）
    use winreg::enums::HKEY_CURRENT_USER;
    if let Ok(hk) = winreg::RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey("Software\\Valve\\Steam")
    {
        if let Ok(path) = hk.get_value::<String, _>("SteamPath") {
            let pb = std::path::PathBuf::from(path.replace('/', "\\"));
            if pb.is_dir() {
                roots.push(pb);
            }
        }
    }
    roots.push(std::path::PathBuf::from("C:\\Program Files (x86)\\Steam"));
    roots.push(std::path::PathBuf::from("C:\\Program Files\\Steam"));
    let mut out: Vec<std::path::PathBuf> = Vec::new();
    for base in roots {
        if !base.is_dir() || out.iter().any(|r| r == &base) {
            continue;
        }
        out.push(base.clone());
        let vdf = base.join("steamapps").join("libraryfolders.vdf");
        if let Ok(text) = std::fs::read_to_string(&vdf) {
            for line in text.lines() {
                let line = line.trim();
                if let Some(rest) = line.strip_prefix("\"path\"") {
                    let rest = rest.trim();
                    if let Some(p) = rest.strip_prefix('"').and_then(|r| r.strip_suffix('"')) {
                        let p = p.replace("\\\\", "\\");
                        let pb = std::path::PathBuf::from(&p);
                        if pb.is_dir() && !out.iter().any(|r| r == &pb) {
                            out.push(pb);
                        }
                    }
                }
            }
        }
    }
    out
}

#[cfg(not(windows))]
pub(crate) fn steam_library_roots() -> Vec<std::path::PathBuf> {
    Vec::new()
}

/// 扫描 Wallpaper Engine 壁纸项目。
/// root 为空 = 自动探测（默认 Steam 库 + libraryfolders.vdf 里的全部库）；
/// 否则 root 为 Steam 库根 / wallpaper_engine 目录 / 项目父目录。
/// scene 着色器型 / web / application 类型按实际能力返回 supported（其余回退预览图）。
#[tauri::command(async)]
pub fn wp_engine_scan(root: String) -> CmdResult<Vec<WpEngineItem>> {
    let roots: Vec<std::path::PathBuf> = if root.trim().is_empty() {
        steam_library_roots()
    } else {
        vec![std::path::PathBuf::from(root)]
    };
    let mut items: Vec<WpEngineItem> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for r in roots {
        for (dir, src) in wp_engine_candidates(&r) {
            if seen.insert(dir.clone()) {
                if let Some(item) = wp_engine_item(&dir, src) {
                    items.push(item);
                }
            }
        }
    }
    // 标题排序（稳定展示），video 优先于不可导入项
    items.sort_by(|a, b| {
        b.supported
            .cmp(&a.supported)
            .then_with(|| a.title.cmp(&b.title))
    });
    Ok(items)
}

/// （已移除 wp_engine_open：实机反馈场景/应用型壁纸交给 WE 本体 + 隐藏窗口
/// 会出现整屏黑屏，前端已改为全部本地打开 —— 预览图静态渲染；
/// 通道下线后宿主侧不再启动 wallpaper64.exe，攻击面同步收窄。）

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WpImageFile {
    pub name: String,
    pub path: String,
}

/// 枚举目录下的壁纸图片（平铺一层，不递归；按文件名排序）。
/// 壁纸中心「本地目录」库源；目录不存在报错，空目录返回空表（如实）。
#[tauri::command(async)]
pub fn wp_list_images(dir: String) -> CmdResult<Vec<WpImageFile>> {
    let root = std::path::PathBuf::from(&dir);
    let mut out: Vec<WpImageFile> = std::fs::read_dir(&root)
        .map_err(|e| AppError::io(format!("读取目录失败 / read_dir failed: {e}")))?
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            let is_img = p
                .extension()
                .map(|x| {
                    let x = x.to_string_lossy().to_lowercase();
                    matches!(
                        x.as_str(),
                        "jpg" | "jpeg" | "png" | "webp" | "bmp" | "gif"
                    )
                })
                .unwrap_or(false);
            if !p.is_file() || !is_img {
                return None;
            }
            Some(WpImageFile {
                name: e.file_name().to_string_lossy().into_owned(),
                path: p.to_string_lossy().into_owned(),
            })
        })
        .collect();
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(out)
}

/// 实机反馈：scene 着色器壁纸本地 WebGL 渲染 —— 读取主片元着色器并递归展开
/// `#include "x"`（相对主文件目录；PathBuf 拼接、限深 8、防环；只读零网络）。
/// 只允许读文件（拒绝目录/不存在），内容原样返回由前端编译。
#[tauri::command]
pub async fn wp_scene_shader(entry: String) -> CmdResult<String> {
    let root = std::path::PathBuf::from(&entry);
    if !root.is_file() {
        return Err(AppError::not_found("着色器文件不存在 / Shader file missing"));
    }
    let mut out = String::new();
    expand_shader_includes(&root, 0, &mut std::collections::HashSet::new(), &mut out)?;
    Ok(out)
}

const SHADER_INCLUDE_MAX_DEPTH: u8 = 8;

fn expand_shader_includes(
    file: &std::path::Path,
    depth: u8,
    seen: &mut std::collections::HashSet<std::path::PathBuf>,
    out: &mut String,
) -> Result<(), AppError> {
    if depth > SHADER_INCLUDE_MAX_DEPTH {
        out.push_str("// [variable] include depth limit\n");
        return Ok(());
    }
    let key = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    if !seen.insert(key) {
        out.push_str("// [variable] include cycle skipped\n");
        return Ok(());
    }
    let text = std::fs::read_to_string(file)
        .map_err(|e| AppError::io(format!("读取着色器失败 / Read shader failed: {e}")))?;
    let text = text.trim_start_matches('\u{feff}');
    let dir = file.parent().map(|p| p.to_path_buf());
    for line in text.lines() {
        let trimmed = line.trim_start();
        let rel = trimmed
            .strip_prefix("#include")
            .or_else(|| trimmed.strip_prefix("#include ".trim_end()))
            .and_then(|rest| rest.trim().strip_prefix('"'))
            .and_then(|rest| rest.split('"').next());
        match rel {
            Some(rel) if !rel.is_empty() => {
                let rel_n = rel.replace('\\', "/");
                let target = dir.as_ref().map(|d| d.join(&rel_n)).unwrap_or_else(|| std::path::PathBuf::from(&rel_n));
                match target.is_file() {
                    true => {
                        out.push_str(&format!("// [variable] begin include {rel}\n"));
                        expand_shader_includes(&target, depth + 1, seen, out)?;
                        out.push_str(&format!("// [variable] end include {rel}\n"));
                    }
                    false => out.push_str(&format!("// [variable] include not found: {rel}\n")),
                }
            }
            _ => {
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_preview_picks_largest_image_and_skips_tiny() {
        let base = std::env::temp_dir().join(format!("variable-wp-fb-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let tex = base.join("textures");
        std::fs::create_dir_all(&tex).unwrap();
        // 小图标应被跳过；textures 里的最大贴图应胜出
        std::fs::write(base.join("icon.png"), vec![0u8; 1024]).unwrap();
        std::fs::write(tex.join("small.jpg"), vec![0u8; 30 * 1024]).unwrap();
        std::fs::write(tex.join("big.png"), vec![0u8; 200 * 1024]).unwrap();
        std::fs::write(tex.join("data.txt"), b"skip").unwrap();
        let picked = find_fallback_preview(&base).unwrap();
        assert!(picked.ends_with("big.png"));
        // 空目录 → None
        let empty = base.join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        assert!(find_fallback_preview(&empty).is_none());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn pick_daily_is_stable_per_day_and_next_is_random_path() {
        let base = std::env::temp_dir().join(format!("variable-wp-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join("a.jpg"), b"x").unwrap();
        std::fs::write(base.join("b.png"), b"y").unwrap();
        std::fs::write(base.join("note.txt"), b"skip").unwrap();

        let dir = base.to_string_lossy().to_string();
        // date 模式：同一天多次调用结果一致，且必是某张图片
        let a = wp_pick_daily(dir.clone(), "date".into()).unwrap();
        let b = wp_pick_daily(dir.clone(), "date".into()).unwrap();
        assert_eq!(a, b);
        let s = a.unwrap();
        assert!(s.ends_with("a.jpg") || s.ends_with("b.png"));
        // next 模式：返回的也是目录内图片
        let n = wp_pick_daily(dir, "next".into()).unwrap().unwrap();
        assert!(n.ends_with("a.jpg") || n.ends_with("b.png"));

        // 目录不存在 → 报错；空图片目录 → None
        assert!(wp_pick_daily(base.join("nope").to_string_lossy().to_string(), "date".into()).is_err());
        let empty = base.join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        assert!(wp_pick_daily(empty.to_string_lossy().to_string(), "date".into()).unwrap().is_none());

        let _ = std::fs::remove_dir_all(&base);
    }
}
