//! L3 shell — tools.rs（F-2 实用工具集后端）
//! - tool_data_read/write：工具 JSON 数据落盘（data/tools/<name>.json，白名单名）
//! - tool_secure_write/read：敏感数据（剪贴板历史）DPAPI 加密落盘
//! - snapshot_capture：虚拟屏 GDI 抓帧（BMP 字节；窗口/区域裁剪在前端 canvas 完成）
//! 仅 Windows 有真实行为；其余平台占位（与 hardware.rs 同策略）。

use std::fs;
use std::path::PathBuf;

use base64::Engine as _;
use tauri::State;

use crate::error::{AppError, CmdResult};
use crate::state::AppState;

pub type CmdResultPub<T> = CmdResult<T>;

/// 工具数据名白名单（防路径穿越；新增工具在此登记）。
/// AI-08 批次新增：clock/weather/chars/magnifier/converter/sysinfo/run/printqueue/mini。
const TOOL_NAMES: &[&str] = &[
    "notes",
    "clipboard",
    "calc",
    "calendar",
    "snapshot",
    "clock",
    "weather",
    "chars",
    "magnifier",
    "converter",
    "sysinfo",
    "run",
    "printqueue",
    "mini",
];

fn tool_path(data_dir: &std::path::Path, name: &str) -> CmdResult<PathBuf> {
    if !TOOL_NAMES.contains(&name) {
        return Err(AppError::validation(format!("未知工具数据名 {name}")));
    }
    let dir = data_dir.join("tools");
    fs::create_dir_all(&dir).map_err(|e| AppError::io(format!("创建 tools 目录失败: {e}")))?;
    Ok(dir.join(format!("{name}.json")))
}

/// 读取工具 JSON 数据（不存在 → null，前端按默认值处理）。
#[tauri::command]
pub fn tool_data_read(st: State<'_, AppState>, name: String) -> CmdResult<Option<String>> {
    let p = tool_path(&st.data_dir, &name)?;
    if !p.exists() {
        return Ok(None);
    }
    let s = fs::read_to_string(&p).map_err(|e| AppError::io(format!("读取 {name} 数据失败: {e}")))?;
    Ok(Some(s))
}

/// 写入工具 JSON 数据（整文件覆盖；调用方负责序列化与容量控制）。
#[tauri::command]
pub fn tool_data_write(st: State<'_, AppState>, name: String, content: String) -> CmdResult<()> {
    let p = tool_path(&st.data_dir, &name)?;
    fs::write(&p, content.as_bytes()).map_err(|e| AppError::io(format!("写入 {name} 数据失败: {e}")))?;
    Ok(())
}

// ---------- DPAPI 加密落盘（剪贴板历史等敏感数据） ----------

#[cfg(windows)]
mod dpapi {
    use windows::core::PCWSTR;
    use windows::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB,
    };
    use windows::Win32::Foundation::{LocalFree, HLOCAL};

    pub fn protect(plain: &[u8]) -> Result<Vec<u8>, String> {
        let mut in_blob = CRYPT_INTEGER_BLOB {
            cbData: plain.len() as u32,
            pbData: plain.as_ptr() as *mut u8,
        };
        let mut out = CRYPT_INTEGER_BLOB::default();
        unsafe {
            CryptProtectData(
                &mut in_blob,
                PCWSTR::null(),
                None,
                None,
                None,
                0,
                &mut out,
            )
            .map_err(|e| e.to_string())?;
        }
        let slice = unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize) };
        let v = slice.to_vec();
        unsafe {
            let _ = LocalFree(HLOCAL(out.pbData as *mut _));
        }
        Ok(v)
    }

    pub fn unprotect(cipher: &[u8]) -> Result<Vec<u8>, String> {
        let mut in_blob = CRYPT_INTEGER_BLOB {
            cbData: cipher.len() as u32,
            pbData: cipher.as_ptr() as *mut u8,
        };
        let mut out = CRYPT_INTEGER_BLOB::default();
        unsafe {
            CryptUnprotectData(&mut in_blob, None, None, None, None, 0, &mut out)
                .map_err(|e| e.to_string())?;
        }
        let slice = unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize) };
        let v = slice.to_vec();
        unsafe {
            let _ = LocalFree(HLOCAL(out.pbData as *mut _));
        }
        Ok(v)
    }
}

/// DPAPI 加密（AI-07 N-15 剪贴板历史落盘复用）。
pub fn dpapi_protect(plain: &[u8]) -> Option<Vec<u8>> {
    #[cfg(windows)]
    return dpapi::protect(plain).ok();
    #[cfg(not(windows))]
    {
        let _ = plain;
        None
    }
}

/// DPAPI 解密（AI-07 N-15 剪贴板历史读取复用）。
pub fn dpapi_unprotect(cipher: &[u8]) -> Option<Vec<u8>> {
    #[cfg(windows)]
    return dpapi::unprotect(cipher).ok();
    #[cfg(not(windows))]
    {
        let _ = cipher;
        None
    }
}

/// DPAPI 加密写入（内容 = 明文字符串，落盘 = base64(cipher)）。
#[tauri::command]
pub fn tool_secure_write(st: State<'_, AppState>, name: String, content: String) -> CmdResult<()> {
    let p = tool_path(&st.data_dir, &name)?;
    let cipher = dpapi::protect(content.as_bytes()).map_err(|e| AppError::io(format!("DPAPI 加密失败: {e}")))?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(cipher);
    fs::write(&p, b64.as_bytes()).map_err(|e| AppError::io(format!("写入 {name} 数据失败: {e}")))?;
    Ok(())
}

/// DPAPI 解密读取（本机当前用户可解；换机/换用户如实报错）。
#[tauri::command]
pub fn tool_secure_read(st: State<'_, AppState>, name: String) -> CmdResult<Option<String>> {
    let p = tool_path(&st.data_dir, &name)?;
    if !p.exists() {
        return Ok(None);
    }
    let raw = fs::read(&p).map_err(|e| AppError::io(format!("读取 {name} 数据失败: {e}")))?;
    let b64 = String::from_utf8_lossy(&raw);
    let cipher = base64::engine::general_purpose::STANDARD
        .decode(b64.trim())
        .map_err(|e| AppError::io(format!("{name} 数据损坏: {e}")))?;
    let plain = dpapi::unprotect(&cipher).map_err(|e| AppError::io(format!("{name} 数据解密失败（可能已换机/换用户）: {e}")))?;
    Ok(Some(String::from_utf8_lossy(&plain).into_owned()))
}

// ---------- 截屏（BMP 字节，虚拟屏全幅） ----------

/// 抓取整个虚拟屏（多显示器并集），返回 BMP 文件字节。
/// 说明：区域/窗口裁剪与标注在前端 canvas 完成；S-1 防截屏开启时前端
/// 会限制只能裁剪 Variable 自身窗口（如实语义）。
#[tauri::command]
pub fn snapshot_capture() -> CmdResult<Vec<u8>> {
    capture_virtual_screen_bmp()
}

// ---------- AI-08 基础工具组（Z-22…Z-28 / V-97/98 支撑命令） ----------

/// Z-27 系统信息面板：Variable 自身信息（版本 / 运行档 / 运行时长 / 数据目录占用）。
/// 只读；数据目录占用为浅层递归求和（>2GB 时停止深扫，如实封顶显示）。
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SysSelfInfo {
    pub version: &'static str,
    pub runtime_mode: &'static str,
    pub uptime_secs: u64,
    pub data_dir: String,
    pub data_dir_bytes: u64,
    /// 目录大小统计是否被封顶截断（>2GB 停止深扫）。
    pub data_dir_capped: bool,
    pub os_version: String,
}

#[tauri::command]
pub fn sys_self_info(st: State<'_, AppState>) -> CmdResult<SysSelfInfo> {
    const CAP: u64 = 2 * 1024 * 1024 * 1024;
    let (mut bytes, mut capped) = (0u64, false);
    fn walk(dir: &std::path::Path, bytes: &mut u64, capped: &mut bool, cap: u64) {
        if *bytes > cap {
            *capped = true;
            return;
        }
        let Ok(rd) = fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let Ok(meta) = e.metadata() else { continue };
            if meta.is_file() {
                *bytes = bytes.saturating_add(meta.len());
            } else if meta.is_dir() {
                walk(&e.path(), bytes, capped, cap);
            }
            if *bytes > cap {
                *capped = true;
                return;
            }
        }
    }
    walk(&st.data_dir, &mut bytes, &mut capped, CAP);
    let os_version = sysinfo::System::long_os_version()
        .unwrap_or_else(|| std::env::consts::OS.to_string());
    Ok(SysSelfInfo {
        version: env!("CARGO_PKG_VERSION"),
        runtime_mode: crate::shell::sysinfo::runtime_mode(),
        uptime_secs: process_uptime_secs(),
        data_dir: st.data_dir.to_string_lossy().into_owned(),
        data_dir_bytes: bytes,
        data_dir_capped: capped,
        os_version,
    })
}

fn process_uptime_secs() -> u64 {
    // 口径：本进程 lib 加载起点起的时长（OnceLock 首次调用即计时）。
    use std::sync::OnceLock;
    use std::time::Instant;
    static START: OnceLock<Instant> = OnceLock::new();
    let start = START.get_or_init(Instant::now);
    start.elapsed().as_secs()
}

/// Z-25 放大镜：当前光标物理屏幕坐标（虚拟屏坐标系）。
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorPos {
    pub x: i32,
    pub y: i32,
}

#[tauri::command]
pub fn cursor_pos() -> CmdResult<CursorPos> {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::POINT;
        use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
        unsafe {
            let mut pt = POINT::default();
            if GetCursorPos(&mut pt).is_err() {
                return Err(AppError::io("读取光标位置失败"));
            }
            Ok(CursorPos { x: pt.x, y: pt.y })
        }
    }
    #[cfg(not(windows))]
    {
        Err(AppError::validation("光标位置仅支持 Windows（当前平台为占位）"))
    }
}

/// Z-23 天气卡 / Z-26 汇率刷新：受限 HTTP GET（curl.exe 隐藏窗口，10s 超时，512KB 截断）。
/// 出站纪律：本命令不内置授权判断 —— 前端必须先经 netGuard（requestNetConsent）
/// 获得用户明确同意后才允许调用（Z-23 规格的「可配置公开 API」通道）。
#[tauri::command]
pub fn http_fetch(url: String) -> CmdResult<String> {
    // 仅允许 http/https；长度上限防滥用
    let u = url.trim().to_string();
    if u.len() > 2048 || !(u.starts_with("http://") || u.starts_with("https://")) {
        return Err(AppError::validation("仅支持 http/https URL"));
    }
    let out = hidden_command("curl.exe")
        .args(["-sSL", "--max-time", "10", "--max-filesize", "524288", &u])
        .output()
        .map_err(|e| AppError::io(format!("curl 启动失败: {e}")))?;
    if !out.status.success() {
        return Err(AppError::io(format!(
            "请求失败（curl exit {}）",
            out.status.code().unwrap_or(-1)
        )));
    }
    if out.stdout.len() > 524_288 {
        return Err(AppError::io("响应超过 512KB 上限"));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn hidden_command(program: &str) -> std::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let mut c = std::process::Command::new(program);
        c.creation_flags(CREATE_NO_WINDOW);
        c
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new(program)
    }
}

#[cfg(windows)]
fn capture_virtual_screen_bmp() -> CmdResult<Vec<u8>> {
    use windows::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
        ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, SRCCOPY,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
    };

    unsafe {
        let x = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let w = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let h = GetSystemMetrics(SM_CYVIRTUALSCREEN);
        if w <= 0 || h <= 0 {
            return Err(AppError::io("虚拟屏尺寸读取失败"));
        }
        let hdc_screen = GetDC(None);
        let hdc_mem = CreateCompatibleDC(hdc_screen);
        let hbmp = CreateCompatibleBitmap(hdc_screen, w, h);
        let old = SelectObject(hdc_mem, hbmp);
        let ok = BitBlt(hdc_mem, 0, 0, w, h, hdc_screen, x, y, SRCCOPY).is_ok();
        // BITMAPINFO（负高度 = top-down）
        let mut bi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                biSizeImage: (w * h * 4) as u32,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut pixels = vec![0u8; (w * h * 4) as usize];
        let got = if ok {
            GetDIBits(
                hdc_mem,
                hbmp,
                0,
                h as u32,
                Some(pixels.as_mut_ptr() as *mut _),
                &mut bi,
                DIB_RGB_COLORS,
            )
        } else {
            0
        };
        SelectObject(hdc_mem, old);
        let _ = DeleteObject(old);
        let _ = DeleteDC(hdc_mem);
        ReleaseDC(None, hdc_screen);
        if got == 0 {
            return Err(AppError::io("屏幕像素读取失败（BitBlt/GetDIBits）"));
        }
        // BMP 文件头（14 字节）+ 信息头（40）+ 像素（32bpp BGRX，行对齐 4 字节天然满足）
        let data_offset = 14u32 + 40u32;
        let size_image = pixels.len() as u32;
        let file_size = data_offset + size_image;
        let mut out = Vec::with_capacity(file_size as usize);
        out.extend_from_slice(b"BM");
        out.extend_from_slice(&file_size.to_le_bytes());
        out.extend_from_slice(&[0u8; 4]);
        out.extend_from_slice(&data_offset.to_le_bytes());
        // BITMAPINFOHEADER（top-down 32bpp）
        out.extend_from_slice(&40u32.to_le_bytes());
        out.extend_from_slice(&(w as i32).to_le_bytes());
        out.extend_from_slice(&(h as i32).to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
        out.extend_from_slice(&size_image.to_le_bytes());
        out.extend_from_slice(&2835u32.to_le_bytes()); // 72 DPI ppm x
        out.extend_from_slice(&2835u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&pixels);
        Ok(out)
    }
}

#[cfg(not(windows))]
fn capture_virtual_screen_bmp() -> CmdResult<Vec<u8>> {
    Err(AppError::validation("截屏仅支持 Windows（当前平台为占位）"))
}
