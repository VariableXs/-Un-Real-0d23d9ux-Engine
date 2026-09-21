//! WE 壁纸镜像（M6-2）：读取 Wallpaper Engine 当前选中壁纸，供 Variable 桌面
//! 实时跟随（「不是用扫描」——读 WE 的配置源，不抓屏不截帧）。
//!
//! 数据源：`<WE 安装目录>/config.json`，结构（实测本机）：
//! `{ "<Windows会话id>": { "wallpaperconfig": { "selectedwallpapers":
//!   { "Monitor0": { "file": "D:/steam/.../3276921258/序列 01.mp4" } } } }, ... }`
//! 多会话/多显示器时取第一个非空 file（单屏场景即当前壁纸）。
//! 切换：`wallpaper64.exe -control openWallpaper -file <文件>`（WE 官方 CLI，
//! 幕后无感切换；切换后 config.json 被 WE 重写 → 轮询端感知 → Variable 跟随）。

use crate::error::{AppError, CmdResult};
use serde::Serialize;

/// WE 安装目录探测：优先取 wallpaper64.exe 进程的可执行文件目录（跨安装盘符
/// 稳），回落 Steam 常见安装路径。
#[cfg(windows)]
fn discover_install_dir() -> Option<std::path::PathBuf> {
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    };
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    unsafe {
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return None;
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                let name = String::from_utf16_lossy(
                    &entry.szExeFile[..entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(0)],
                )
                .to_lowercase();
                if name == "wallpaper64.exe" || name == "wallpaper32.exe" {
                    if let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, entry.th32ProcessID) {
                        let mut buf = [0u16; 1024];
                        let mut len = buf.len() as u32;
                        let ok = windows::Win32::System::Threading::QueryFullProcessImageNameW(
                            h,
                            windows::Win32::System::Threading::PROCESS_NAME_FORMAT(0),
                            windows::core::PWSTR(buf.as_mut_ptr()),
                            &mut len,
                        )
                        .is_ok();
                        let _ = windows::Win32::Foundation::CloseHandle(h);
                        if ok && len > 0 {
                            let path = String::from_utf16_lossy(&buf[..len as usize]);
                            if let Some(dir) = std::path::Path::new(&path).parent() {
                                return Some(dir.to_path_buf());
                            }
                        }
                    }
                }
                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = windows::Win32::Foundation::CloseHandle(snapshot);
    }
    None
}

#[cfg(not(windows))]
fn discover_install_dir() -> Option<std::path::PathBuf> {
    None
}

fn config_path() -> Option<std::path::PathBuf> {
    let dir = discover_install_dir()
        .or_else(|| Some(std::path::PathBuf::from("D:/steam/steamapps/common/wallpaper_engine")))?;
    let p = dir.join("config.json");
    p.exists().then_some(p)
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WeWallpaper {
    /// 当前选中壁纸的媒体文件绝对路径（mp4/webm/png/jpg/…）
    pub file: String,
    /// "video" | "image"（按扩展名；html/pkg 等类型返回 "unsupported"）
    pub kind: String,
}

fn kind_of(file: &str) -> &'static str {
    let lower = file.to_lowercase();
    if lower.ends_with(".mp4") || lower.ends_with(".webm") {
        "video"
    } else if lower.ends_with(".png") || lower.ends_with(".jpg") || lower.ends_with(".jpeg") || lower.ends_with(".gif") || lower.ends_with(".webp") {
        "image"
    } else {
        "unsupported"
    }
}

/// 当前选中的 WE 壁纸（无 WE / 无选中 → None）。
#[tauri::command]
pub fn we_wallpaper_current() -> CmdResult<Option<WeWallpaper>> {
    let Some(path) = config_path() else {
        return Ok(None);
    };
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| AppError::io(format!("读取 WE config.json 失败: {e}")))?;
    match extract_first_selected_file(&raw) {
        Some(file) => Ok(Some(WeWallpaper {
            kind: kind_of(&file).to_string(),
            file,
        })),
        // JSON 解析失败也回落文本扫描，不能因为解析器放弃而丢镜像
        None => Ok(None),
    }
}

/// 从 config.json 原文提取第一个 selectedwallpapers 的 file。
/// **必须做原文扫描而非 serde 反序列化后遍历**：WE 写的 config.json 存在
/// 重复键（同一会话块出现多次，后面的 wallpaperconfig 为空），serde/python
/// 取「最后出现」会把有内容的块覆盖掉（实机：json 解析结果 selectedwallpapers
/// 为空，而原文里 Monitor0.file 明明白白存在）。对每个 selectedwallpapers
/// 对象做括号配平截取子串，交给 serde 解析（子串内部无重复键）。
fn extract_first_selected_file(raw: &str) -> Option<String> {
    // 生产语义：坏路径（WE -control ANSI 乱码写回）绝不跟随。
    extract_first_selected_file_if(raw, |f| std::fs::metadata(f).is_ok())
}

/// 可注入存在性谓词的抽取核（测试机器无关：夹具路径不要求真实存在）。
fn extract_first_selected_file_if(raw: &str, exists: impl Fn(&str) -> bool) -> Option<String> {
    let needle = "\"selectedwallpapers\"";
    let mut from = 0;
    while let Some(rel) = raw[from..].find(needle) {
        let after = from + rel + needle.len();
        // 跳过 `selectedwallpapers` 与 `{` 之间的空白
        let Some(brace_off) = raw[after..].find('{') else { break };
        let start = after + brace_off;
        if let Some(end) = balanced_object_end(&raw[start..]) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw[start..start + end]) {
                if let Some(files) = v.as_object() {
                    for (_mon, wp) in files {
                        if let Some(file) = wp.get("file").and_then(|f| f.as_str()) {
                            if !file.is_empty() {
                                // 跳过不存在的条目：WE 的 -control 命令行按 ANSI
                                // 解析，非 ASCII 文件名会被它自己写成 U+FFFD 乱码
                                // （实机：openWallpaper 传中文 mp4 → config 损坏）。
                                // 镜像端绝不跟随坏路径（否则桌面黑屏+缺失弹条）。
                                if !exists(file) {
                                    continue;
                                }
                                return Some(file.to_string());
                            }
                        }
                    }
                }
            }
            from = start + end;
        } else {
            break;
        }
    }
    None
}

/// 从 `{` 开始的平衡对象截取长度（跳过字符串字面量内的花括号/引号）。
fn balanced_object_end(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    if bytes.first() != Some(&b'{') {
        return None;
    }
    let mut depth = 0i32;
    let mut in_str = false;
    let mut esc = false;
    for (i, &b) in bytes.iter().enumerate() {
        if in_str {
            if esc {
                esc = false;
            } else if b == b'\\' {
                esc = true;
            } else if b == b'"' {
                in_str = false;
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
    }
    None
}

/// 通过 WE 官方 CLI 切换壁纸（幕后无感；切换后 config.json 被 WE 重写，
/// 轮询端随之跟随）。**路径含非 ASCII 时必须改传同目录 project.json**——
/// WE 的 -control 命令行按 ANSI 解析 argv，中文文件名会被它写成 U+FFFD
/// 乱码进 config.json（实机复现）；project.json 路径恒为 ASCII。
#[tauri::command]
pub fn we_wallpaper_open(file: String) -> CmdResult<()> {
    let Some(dir) = discover_install_dir() else {
        return Err(AppError::not_found("未发现运行中的 Wallpaper Engine 进程"));
    };
    let exe = dir.join("wallpaper64.exe");
    if !exe.exists() {
        return Err(AppError::not_found("wallpaper64.exe 不存在"));
    }
    let ascii_safe = file.is_ascii()
        || std::path::Path::new(&file)
            .parent()
            .map(|p| p.join("project.json").exists())
            .unwrap_or(false);
    let arg = if ascii_safe {
        file.clone()
    } else {
        // 回落：8.3 短路径（纯 ASCII），取不到则如实报错而不是静默写坏 WE 配置
        short_path(&file).ok_or_else(|| {
            AppError::validation(
                "路径含非 ASCII 且无 project.json/短路径可用 —— 请改用 project.json 切换",
            )
        })?
    };
    std::process::Command::new(&exe)
        .args(["-control", "openWallpaper", "-file", &arg])
        .spawn()
        .map_err(|e| AppError::io(format!("启动 WE 切换失败: {e}")))?;
    Ok(())
}

/// GetShortPathNameW：非 ASCII 路径的 ASCII 化回落。
#[cfg(windows)]
fn short_path(path: &str) -> Option<String> {
    use windows::Win32::Storage::FileSystem::GetShortPathNameW;
    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    let mut buf = vec![0u16; wide.len() + 64];
    let n = unsafe {
        GetShortPathNameW(
            windows::core::PCWSTR(wide.as_ptr()),
            Some(&mut buf),
        )
    };
    if n == 0 {
        return None;
    }
    let n = n as usize;
    Some(String::from_utf16_lossy(&buf[..n.min(buf.len())]))
}

#[cfg(not(windows))]
fn short_path(_path: &str) -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    #[test]
    fn kind_of_by_extension() {
        assert_eq!(super::kind_of("D:/x/序列 01.mp4"), "video");
        assert_eq!(super::kind_of("C:/a.WEBM"), "video");
        assert_eq!(super::kind_of("d:/p/图.png"), "image");
        assert_eq!(super::kind_of("d:/p/图.JPG"), "image");
        assert_eq!(super::kind_of("d:/p/scene.pkg"), "unsupported");
    }

    #[test]
    fn parse_selected_wallpapers_from_sample() {
        // 实测结构样本（截自本机 config.json，含多会话与空会话）
        let raw = r#"{
            "?installdirectory": "D:/steam/steamapps/common/wallpaper_engine",
            "varia": {"general": {}, "wallpaperconfig": {"selectedwallpapers": {}}},
            "varia_8gui12b": {"wallpaperconfig": {"selectedwallpapers": {
                "Monitor0": {"file": "D:/steam/steamapps/workshop/content/431960/3276921258/序列 01.mp4", "local": true}
            }}}
        }"#;
        let v: serde_json::Value = serde_json::from_str(raw).unwrap();
        let mut found = None;
        if let Some(obj) = v.as_object() {
            for (_sid, sess) in obj {
                if let Some(files) = sess
                    .get("wallpaperconfig")
                    .and_then(|wc| wc.get("selectedwallpapers"))
                    .and_then(|sw| sw.as_object())
                {
                    for (_mon, wp) in files {
                        if let Some(file) = wp.get("file").and_then(|f| f.as_str()) {
                            if !file.is_empty() {
                                found = Some(file.to_string());
                            }
                        }
                    }
                }
            }
        }
        assert_eq!(
            found.as_deref(),
            Some("D:/steam/steamapps/workshop/content/431960/3276921258/序列 01.mp4")
        );
    }

    /// 实机关键样本：WE 写的 config.json 存在**重复键**——同一会话出现两次
    /// wallpaperconfig，后面为空。serde 取最后出现 → 丢失有内容的块（实机
    /// 假 None 的根因）。原文扫描必须穿透重复键拿到 Monitor0.file。
    #[test]
    fn extract_survives_duplicate_keys() {
        let raw = r#"{
  "varia": {
    "general": { "browser": {} },
    "wallpaperconfig": {
      "layout": 0, "profile": null,
      "selectedwallpapers": {
        "Monitor0": { "file": "D:/steam/steamapps/workshop/content/431960/3276921258/序列 01.mp4", "local": true }
      }
    },
    "wallpaperconfig": { "layout": 0, "profile": null, "selectedwallpapers": {} }
  }
}"#;
        assert_eq!(
            super::extract_first_selected_file_if(raw, |_| true).as_deref(),
            Some("D:/steam/steamapps/workshop/content/431960/3276921258/序列 01.mp4")
        );
    }
}
