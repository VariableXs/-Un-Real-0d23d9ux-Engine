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
#[tauri::command(async)]
pub fn tool_data_read(st: State<'_, AppState>, name: String) -> CmdResult<Option<String>> {
    let p = tool_path(&st.data_dir, &name)?;
    if !p.exists() {
        return Ok(None);
    }
    let s = fs::read_to_string(&p).map_err(|e| AppError::io(format!("读取 {name} 数据失败: {e}")))?;
    Ok(Some(s))
}

/// 写入工具 JSON 数据（整文件覆盖；调用方负责序列化与容量控制）。
#[tauri::command(async)]
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
#[tauri::command(async)]
pub fn tool_secure_write(st: State<'_, AppState>, name: String, content: String) -> CmdResult<()> {
    let p = tool_path(&st.data_dir, &name)?;
    let cipher = dpapi::protect(content.as_bytes()).map_err(|e| AppError::io(format!("DPAPI 加密失败: {e}")))?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(cipher);
    fs::write(&p, b64.as_bytes()).map_err(|e| AppError::io(format!("写入 {name} 数据失败: {e}")))?;
    Ok(())
}

/// DPAPI 解密读取（本机当前用户可解；换机/换用户如实报错）。
#[tauri::command(async)]
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
#[tauri::command(async)]
pub fn snapshot_capture() -> CmdResult<Vec<u8>> {
    close_snipping_overlays();
    capture_virtual_screen_bmp()
}

/// 截图前清场：关闭 Windows 自带截图浮层（Snipping Tool / Win+Shift+S 抢注
/// 生效前已打开的残留）。浮层挡在屏幕最上层会让 BitBlt 抓到它而非 Variable。
#[cfg(windows)]
fn close_snipping_overlays() {
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};
    const SNIP_IMAGES: &[&str] = &[
        "screenclippinghost.exe", // Win11 Snipping Tool 覆盖层
        "screenclipping.exe",     // Win10 截图覆盖层
        "snippingtool.exe",       // 旧版截图工具
        "screensketch.exe",       // Screen Sketch
    ];
    let mut closed = 0usize;
    for (hwnd, _pid, image) in crate::shell::embed::watch_scan_windows() {
        let name = image.rsplit(['\\', '/']).next().unwrap_or("").to_lowercase();
        if SNIP_IMAGES.contains(&name.as_str()) {
            unsafe {
                let _ = PostMessageW(
                    HWND(hwnd as *mut core::ffi::c_void),
                    WM_CLOSE,
                    WPARAM(0),
                    LPARAM(0),
                );
            }
            closed += 1;
        }
    }
    if closed > 0 {
        crate::shell::applog::log("capture", format!("截图前清场：已关闭 {closed} 个 Snipping 浮层窗口"));
    }
}

#[cfg(not(windows))]
fn close_snipping_overlays() {}

// ---------- Variable 相册：截图落盘共享 ----------
//
// 「在 Variable 里独立截图，截完的图全系统共享」的落点：前端 canvas 已编码好
// PNG（data URL），这里只做解码 + 原子写盘，写进一个**固定共享目录**——
// 而不是浏览器下载目录。于是资源管理器、编辑器、便签引用的是同一份文件。

/// 截图共享目录决策（纯逻辑 + 可写探测，可单测）：
/// 优先 `<程序目录>\Screenshots` —— 用户打开环境所在文件夹即见，截图天然对整个
/// Variable 共享；程序目录不可写（安装到 Program Files 等）→ 回退 `<数据目录>\Screenshots`。
fn shots_dir_with(exe_dir: Option<&std::path::Path>, data_dir: &std::path::Path) -> PathBuf {
    if let Some(dir) = exe_dir {
        let candidate = dir.join("Screenshots");
        if candidate.exists() || fs::create_dir_all(&candidate).is_ok() {
            return candidate;
        }
    }
    data_dir.join("Screenshots")
}

/// Variable 相册目录（共享位置；程序目录优先，回退数据目录）。
pub fn shots_dir(st: &AppState) -> PathBuf {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()));
    shots_dir_with(exe_dir.as_deref(), &st.data_dir)
}

/// 图片 data URL → 字节（纯逻辑，可单测）。
/// 只接受 base64 图片载荷；非图片 data URL 或非法 base64 如实报错，绝不落半截文件。
pub fn decode_image_data_url(url: &str) -> CmdResult<Vec<u8>> {
    let (meta, payload) = url
        .split_once(',')
        .ok_or_else(|| AppError::validation("截图数据不是 data URL / not a data URL"))?;
    if !meta.starts_with("data:image/") || !meta.contains("base64") {
        return Err(AppError::validation(
            "截图数据必须是 base64 图片 data URL / expected base64 image data URL",
        ));
    }
    base64::engine::general_purpose::STANDARD
        .decode(payload.trim())
        .map_err(|e| AppError::validation(format!("截图数据 base64 解码失败 / bad base64: {e}")))
}

/// 落盘文件名（纯逻辑，可单测）。
pub fn shot_file_name(ts_ms: u64, seq: u32) -> String {
    if seq == 0 {
        format!("variable-shot-{ts_ms}.png")
    } else {
        format!("variable-shot-{ts_ms}-{seq}.png")
    }
}

/// 保存截图到 Variable 相册（共享目录），返回落盘后的绝对路径。
#[tauri::command(async)]
pub fn shot_save(st: State<'_, AppState>, data_url: String) -> CmdResult<String> {
    let bytes = decode_image_data_url(&data_url)?;
    if bytes.is_empty() {
        return Err(AppError::validation("截图为空 / empty image"));
    }
    let dir = shots_dir(&st);
    fs::create_dir_all(&dir).map_err(|e| AppError::io(format!("相册目录创建失败 / mkdir: {e}")))?;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    // 同毫秒内重复保存不覆盖（追加序号；上限保护避免病态目录下死循环）
    let mut path = dir.join(shot_file_name(ts, 0));
    let mut seq = 1u32;
    while path.exists() && seq < 1000 {
        path = dir.join(shot_file_name(ts, seq));
        seq += 1;
    }
    crate::fsutil::atomic_write(&path, &bytes)
        .map_err(|e| AppError::io(format!("截图写盘失败 / write: {e}")))?;
    Ok(path.to_string_lossy().into_owned())
}

/// Variable 相册目录（只读；前端「打开文件夹」与空态提示用）。
#[tauri::command]
pub fn shot_dir(st: State<'_, AppState>) -> String {
    shots_dir(&st).to_string_lossy().into_owned()
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

#[tauri::command(async)]
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

#[tauri::command(async)]
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
#[tauri::command(async)]
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

#[cfg(test)]
mod tests {
    use super::{decode_image_data_url, shot_file_name, shots_dir_with};

    /// 相册目录决策：程序目录可写 → 用之（截图共享给整个环境文件夹可见）；
    /// 程序目录不可写 → 回退数据目录。
    #[test]
    fn shots_dir_prefers_exe_dir_then_falls_back() {
        let base = std::env::temp_dir().join(format!("var-shots-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let exe = base.join("app");
        let data = base.join("data");

        // 程序目录可建 → 命中 <程序目录>/Screenshots
        std::fs::create_dir_all(&exe).unwrap();
        assert_eq!(shots_dir_with(Some(&exe), &data), exe.join("Screenshots"));

        // 无程序目录（None）→ 回退数据目录
        assert_eq!(shots_dir_with(None, &data), data.join("Screenshots"));

        // 程序目录存在但是「文件」→ create_dir_all 必失败 → 回退数据目录
        let not_a_dir = base.join("blocked");
        std::fs::write(&not_a_dir, b"x").unwrap();
        assert_eq!(shots_dir_with(Some(&not_a_dir), &data), data.join("Screenshots"));

        let _ = std::fs::remove_dir_all(&base);
    }

    /// data URL 解码：正常 PNG 载荷 → 字节；非图 / 非 base64 / 缺逗号 → 如实报错。
    #[test]
    fn decode_image_data_url_accepts_png_only_and_reports_bad_input() {
        // "iVBORw0=" 恰为 PNG 签名前 5 字节的标准 base64
        let ok = decode_image_data_url("data:image/png;base64,iVBORw0=").unwrap();
        assert_eq!(ok, b"\x89PNG\r\n\x1a\n"[..5].to_vec());

        assert!(decode_image_data_url("data:image/png;base64").is_err(), "缺逗号必须报错");
        assert!(decode_image_data_url("data:text/plain;base64,QQ==").is_err(), "非图片必须报错");
        assert!(decode_image_data_url("data:image/png,QQ==").is_err(), "非 base64 必须报错");
        assert!(decode_image_data_url("data:image/png;base64,@@@").is_err(), "非法 base64 必须报错");
    }

    /// 文件名：首张无序号后缀，同毫秒重名才追加序号（幂等、无覆盖）。
    #[test]
    fn shot_file_name_sequence() {
        assert_eq!(shot_file_name(1000, 0), "variable-shot-1000.png");
        assert_eq!(shot_file_name(1000, 1), "variable-shot-1000-1.png");
        assert_eq!(shot_file_name(1000, 42), "variable-shot-1000-42.png");
    }
}
