//! L3 shell — capture.rs（批次C-4 L3 画面捕获 + 输入转发）：
//! - 捕获：Windows.Graphics.Capture（WinRT，Win10 1903+）→ D3D11 纹理 →
//!   Staging Map → BGRA 帧经 Tauri 事件送前端 WebView 合成（计划允许的 CPU 回退通路）。
//! - 帧率自适应：内容变化检测（抽样像素哈希），静止降至 5fps，活动恢复 30fps。
//! - 窗口隐藏：真实窗口 SetWindowPos(-32000,-32000) + WS_EX_TOOLWINDOW，
//!   **不最小化**（避免部分应用暂停渲染）。
//! - 输入转发：PostMessage 直注（WM_MOUSEMOVE/LBUTTONDOWN/…/WM_KEYDOWN…），
//!   归一化坐标(0..1) → 客户区物理坐标。屏外窗口天然不泄漏光标、不抢焦点。
//! - 失败降级：WinRT/D3D 不可用 → start_capture 返回 Err，embed_launch 降级独立窗口。

#[cfg(windows)]
pub mod win {
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    use base64::Engine as _;
    use windows::core::{factory, Interface};
    use windows::Graphics::Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem};
    use windows::Graphics::DirectX::DirectXPixelFormat;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
    use windows::Win32::Graphics::Direct3D11::{
        ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D, D3D11CreateDevice,
        D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ,
        D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_USAGE_STAGING,
        D3D11_CPU_ACCESS_READ, D3D11_TEXTURE2D_DESC,
    };
    use windows::Win32::System::WinRT::Direct3D11::IDirect3DDxgiInterfaceAccess;
    use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;

    /// COM 指针非 Send：采集线程专用包装（进程内单线程使用，安全）。
    struct SendObjs {
        d3d: D3dCtx,
        pool: Direct3D11CaptureFramePool,
        session: windows::Graphics::Capture::GraphicsCaptureSession,
    }
    unsafe impl Send for SendObjs {}

    /// 采集会话句柄（停止旗标 + 线程）。
    struct CaptureHandle {
        stop: Arc<AtomicBool>,
        #[allow(dead_code)]
        thread: std::thread::JoinHandle<()>,
    }

    static CAPTURES: Mutex<Option<HashMap<String, CaptureHandle>>> = Mutex::new(None);

    fn with_captures<R>(f: impl FnOnce(&mut HashMap<String, CaptureHandle>) -> R) -> R {
        let mut guard = CAPTURES.lock().unwrap_or_else(|e| e.into_inner());
        let map = guard.get_or_insert_with(HashMap::new);
        f(map)
    }

    /// 批次C-4 窗口隐藏：移出屏外 + TOOLWINDOW（不最小化，保渲染）。
    pub fn hide_offscreen(hwnd: isize) -> bool {
        use windows::Win32::UI::WindowsAndMessaging::{
            GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE, HWND_BOTTOM,
            SWP_NOACTIVATE, SWP_NOZORDER, SWP_NOSIZE, WS_EX_TOOLWINDOW,
        };
        let h = HWND(hwnd as *mut core::ffi::c_void);
        unsafe {
            let ex = GetWindowLongPtrW(h, GWL_EXSTYLE) as u32;
            SetWindowLongPtrW(h, GWL_EXSTYLE, (ex | WS_EX_TOOLWINDOW.0) as isize);
            SetWindowPos(
                h,
                HWND_BOTTOM,
                -32000,
                -32000,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            )
            .is_ok()
        }
    }

    /// 批次C-4 恢复：还原屏外窗口到可见区（L3 会话关闭时；不杀进程）。
    pub fn restore_window(hwnd: isize) {
        use windows::Win32::UI::WindowsAndMessaging::{
            GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE, HWND_TOP,
            SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, WS_EX_TOOLWINDOW,
        };
        let h = HWND(hwnd as *mut core::ffi::c_void);
        unsafe {
            let ex = GetWindowLongPtrW(h, GWL_EXSTYLE) as u32;
            SetWindowLongPtrW(h, GWL_EXSTYLE, (ex & !WS_EX_TOOLWINDOW.0) as isize);
            let _ = SetWindowPos(h, HWND_TOP, 100, 100, 0, 0, SWP_NOSIZE | SWP_NOMOVE | SWP_NOACTIVATE);
        }
    }

    /// 启动 L3 采集（独立线程轮询 FreeThreaded 帧池，自适应帧率）。
    /// 失败（WinRT 不可用/D3D 创建失败）返回 Err → 调用方降级独立窗口。
    pub fn start_capture(app: tauri::AppHandle, embed_id: String, hwnd: isize) -> Result<(), String> {
        stop_capture(&embed_id);
        let d3d = create_d3d_device()?;
        let item = create_item_for_window(hwnd)?;

        let size = windows::Graphics::SizeInt32 {
            Width: item.Size().map_err(|e| e.to_string())?.Width,
            Height: item.Size().map_err(|e| e.to_string())?.Height,
        };
        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &d3d.winrt_device,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            2,
            size,
        )
        .map_err(|e| e.to_string())?;
        let session = pool.CreateCaptureSession(&item).map_err(|e| e.to_string())?;
        session.StartCapture().map_err(|e| e.to_string())?;

        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = stop.clone();
        let eid = embed_id.clone();
        let objs = SendObjs { d3d, pool: pool.clone(), session: session.clone() };
        let init_size = (size.Width, size.Height);
        let thread = std::thread::spawn(move || {
            let objs = objs; // 整体捕获（规避分域字段捕获绕过 unsafe impl Send）
            let (d3d, pool, session) = (objs.d3d, objs.pool, objs.session);
            frame_loop(&app, &eid, &pool, &d3d, init_size, stop2);
            let _ = session.Close();
            let _ = pool.Close();
        });
        with_captures(|m| {
            m.insert(embed_id, CaptureHandle { stop, thread });
        });
        Ok(())
    }

    /// 停止采集（会话关闭；真实窗口由 embed_close/close_all 负责恢复）。
    pub fn stop_capture(embed_id: &str) {
        if let Some(h) = with_captures(|m| m.remove(embed_id)) {
            h.stop.store(true, Ordering::SeqCst);
            let _ = h.thread.join();
        }
    }

    /// 停止全部采集（embed_close_all 全局清场用）。
    pub fn stop_all() {
        let handles = with_captures(|m| std::mem::take(m));
        for (_, h) in handles {
            h.stop.store(true, Ordering::SeqCst);
            let _ = h.thread.join();
        }
    }

    /// 采集线程：TryGetNextFrame 轮询 → staging Map → 变化检测 → 事件发帧。
    /// 自适应：内容静止 5fps（200ms），活动 30fps（33ms）。
    fn frame_loop(
        app: &tauri::AppHandle,
        embed_id: &str,
        pool: &Direct3D11CaptureFramePool,
        d3d: &D3dCtx,
        init_size: (i32, i32),
        stop: Arc<AtomicBool>,
    ) {
        let mut last_hash: u64 = 0;
        let mut staging: Option<(ID3D11Texture2D, i32, i32)> = None;
        let mut buf: Vec<u8> = Vec::new();
        let mut pool_size = init_size;
        while !stop.load(Ordering::SeqCst) {
            let frame = match pool.TryGetNextFrame() {
                Ok(f) => f,
                Err(_) => {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                    continue;
                }
            };
            let content_size = frame.ContentSize().unwrap_or_default();
            // 第十二轮大检查：窗口尺寸变化 → 重建帧池。此前池保持初始尺寸，
            // 捕获画面与真实窗口错位（staging 会按 desc 自适应，池不会）。
            if content_size.Width > 0
                && content_size.Height > 0
                && (content_size.Width != pool_size.0 || content_size.Height != pool_size.1)
            {
                let _ = pool.Recreate(
                    &d3d.winrt_device,
                    DirectXPixelFormat::B8G8R8A8UIntNormalized,
                    2,
                    content_size,
                );
                pool_size = (content_size.Width, content_size.Height);
            }
            // 第十二轮大检查：单次抓取（哈希 + 像素一次完成）——此前变更检测
            // 和取数对同一帧各调一次 grab_frame，等于每帧做两遍完整的
            // GPU→CPU 拷贝（1080p ≈ 8.3MB×2），纯浪费一半带宽。
            let hash = grab_frame(&frame, d3d, &mut staging, &mut buf);
            // 释放帧池缓冲再睡眠（2 缓冲池不能长期占住 1 个）
            drop(frame);
            let Some(h) = hash else {
                std::thread::sleep(std::time::Duration::from_millis(200));
                continue;
            };
            if h != last_hash || last_hash == 0 {
                last_hash = h;
                // 上报实际抓取到的表面尺寸（与 buf 逐字节对应；重建帧池的
                // 过渡帧上 ContentSize 可能与表面不一致）
                let (w, hgt) = staging
                    .as_ref()
                    .map(|(_, w, h)| (*w, *h))
                    .unwrap_or((content_size.Width, content_size.Height));
                let _ = tauri::Emitter::emit(
                    app,
                    "embed-frame",
                    serde_json::json!({
                        "embedId": embed_id,
                        "width": w,
                        "height": hgt,
                        "bgra": base64::engine::general_purpose::STANDARD.encode(&buf),
                    }),
                );
                std::thread::sleep(std::time::Duration::from_millis(33));
            } else {
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
        }
    }

    /// 帧纹理 → staging Map → BGRA 缓冲；返回内容抽样哈希（变化检测）。
    fn grab_frame(
        frame: &windows::Graphics::Capture::Direct3D11CaptureFrame,
        d3d: &D3dCtx,
        staging: &mut Option<(ID3D11Texture2D, i32, i32)>,
        buf: &mut Vec<u8>,
    ) -> Option<u64> {
        unsafe {
            let surface = frame.Surface().ok()?;
            let access: IDirect3DDxgiInterfaceAccess = surface.cast().ok()?;
            let tex: ID3D11Texture2D = access.GetInterface().ok()?;
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            tex.GetDesc(&mut desc);
            let (w, h) = (desc.Width as i32, desc.Height as i32);
            // staging 复用（尺寸变化才重建）
            let need_recreate = match staging {
                Some((_, sw, sh)) => *sw != w || *sh != h,
                None => true,
            };
            if need_recreate {
                let sd = D3D11_TEXTURE2D_DESC {
                    Width: desc.Width,
                    Height: desc.Height,
                    MipLevels: 1,
                    ArraySize: 1,
                    Format: desc.Format,
                    SampleDesc: desc.SampleDesc,
                    Usage: D3D11_USAGE_STAGING,
                    BindFlags: 0,
                    CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                    MiscFlags: 0,
                };
                let mut st_opt: Option<ID3D11Texture2D> = None;
                d3d.device
                    .CreateTexture2D(&sd, None, Some(&mut st_opt))
                    .ok()?;
                let st = st_opt.ok_or("no staging texture").ok()?;
                *staging = Some((st, w, h));
            }
            let (st, _, _) = staging.as_ref()?;
            d3d.ctx.CopyResource(st, &tex);
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            d3d.ctx
                .Map(st, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .ok()?;
            let row = desc.Width as usize * 4;
            buf.clear();
            buf.reserve(desc.Height as usize * row);
            let src = mapped.pData as *const u8;
            for y in 0..desc.Height as usize {
                let line = std::slice::from_raw_parts(src.add(y * mapped.RowPitch as usize), row);
                buf.extend_from_slice(line);
            }
            d3d.ctx.Unmap(st, 0);
            // 抽样哈希：均匀取 64×64 网格像素（变化检测，不做精确比较）
            let mut hash: u64 = 0xcbf29ce484222325;
            let (gw, gh) = (desc.Width as usize, desc.Height as usize);
            for j in 0..64 {
                for i in 0..64 {
                    let x = (i + 1) * gw / 65;
                    let y = (j + 1) * gh / 65;
                    let off = y * mapped.RowPitch as usize + x * 4;
                    if off + 3 < buf.len() {
                        hash = (hash ^ (buf[off] as u64)).wrapping_mul(0x100000001b3);
                        hash = (hash ^ (buf[off + 2] as u64)).wrapping_mul(0x100000001b3);
                    }
                }
            }
            Some(hash)
        }
    }

    struct D3dCtx {
        device: ID3D11Device,
        ctx: ID3D11DeviceContext,
        winrt_device: windows::Graphics::DirectX::Direct3D11::IDirect3DDevice,
    }

    fn create_d3d_device() -> Result<D3dCtx, String> {
        unsafe {
            let mut device: Option<ID3D11Device> = None;
            let mut ctx: Option<ID3D11DeviceContext> = None;
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                None,
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut ctx),
            )
            .map_err(|e| format!("D3D11CreateDevice: {e}"))?;
            let device = device.ok_or("no D3D device")?;
            let ctx = ctx.ok_or("no D3D context")?;
            let dxgi = device.cast::<windows::Win32::Graphics::Dxgi::IDXGIDevice>()
                .map_err(|e| e.to_string())?;
            let insp = windows::Win32::System::WinRT::Direct3D11::CreateDirect3D11DeviceFromDXGIDevice(&dxgi)
                .map_err(|e| e.to_string())?;
            let winrt_device = insp
                .cast::<windows::Graphics::DirectX::Direct3D11::IDirect3DDevice>()
                .map_err(|e| e.to_string())?;
            Ok(D3dCtx { device, ctx, winrt_device })
        }
    }

    /// hwnd → GraphicsCaptureItem（interop CreateForWindow）。
    fn create_item_for_window(hwnd: isize) -> Result<GraphicsCaptureItem, String> {
        let interop: IGraphicsCaptureItemInterop =
            factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()
                .map_err(|e| format!("capture factory: {e}"))?;
        let h = HWND(hwnd as *mut core::ffi::c_void);
        unsafe { interop.CreateForWindow(h).map_err(|e| e.to_string()) }
    }

    // ---------------- 输入转发（PostMessage 直注） ----------------

    /// 归一化坐标(0..1) + 输入事件 → 客户区物理坐标 PostMessage。
    /// kind: move | down | up | dbl | wheel | key | char
    /// 屏外真实窗口坐标钳制在客户区内（不泄漏到屏幕其它区域）。
    pub fn forward_input(
        hwnd: isize,
        kind: &str,
        nx: f64,
        ny: f64,
        button: &str,
        key: u32,
        delta: f64,
    ) -> bool {
        use windows::Win32::Foundation::{LPARAM, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{
            GetClientRect, PostMessageW, WM_CHAR, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDBLCLK,
            WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEMOVE,
            WM_MOUSEWHEEL, WM_RBUTTONDBLCLK, WM_RBUTTONDOWN, WM_RBUTTONUP,
        };
        let h = HWND(hwnd as *mut core::ffi::c_void);
        unsafe {
            let mut rc = windows::Win32::Foundation::RECT::default();
            let _ = GetClientRect(h, &mut rc);
            let cw = (rc.right - rc.left).max(1);
            let ch = (rc.bottom - rc.top).max(1);
            let clamp = |v: f64, max: i32| -> i32 { (v.clamp(0.0, 1.0) * max as f64) as i32 };
            let x = clamp(nx, cw);
            let y = clamp(ny, ch);
            let lp = |y_hi: i32| LPARAM((y_hi as isize) << 16 | (x as isize & 0xFFFF));
            let mk = {
                // 简化按键状态标志：仅按钮按下时置位对应 MK_*
                match (kind, button) {
                    ("move", "left") => 0x0001,   // MK_LBUTTON
                    ("move", "right") => 0x0002,  // MK_RBUTTON
                    ("move", "middle") => 0x0010, // MK_MBUTTON
                    _ => 0,
                }
            };
            let wp_move = WPARAM(mk as usize);
            let sent = match kind {
                "move" => PostMessageW(h, WM_MOUSEMOVE, wp_move, lp(y)),
                "down" | "up" | "dbl" => {
                    let (md, mu, mdb) = match button {
                        "right" => (WM_RBUTTONDOWN, WM_RBUTTONUP, WM_RBUTTONDBLCLK),
                        "middle" => (WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEMOVE), // dbl 用不到
                        _ => (WM_LBUTTONDOWN, WM_LBUTTONUP, WM_LBUTTONDBLCLK),
                    };
                    let msg = match kind {
                        "down" => md,
                        "up" => mu,
                        _ => mdb,
                    };
                    PostMessageW(h, msg, wp_move, lp(y))
                }
                "wheel" => {
                    let wp = WPARAM((((delta as i16 as i32) as isize) << 16) as usize);
                    PostMessageW(h, WM_MOUSEWHEEL, wp, LPARAM(y as isize))
                }
                "key" => {
                    let _ = PostMessageW(h, WM_KEYDOWN, WPARAM(key as usize), LPARAM(0));
                    let _ = PostMessageW(h, WM_KEYUP, WPARAM(key as usize), LPARAM(0));
                    Ok(())
                }
                "char" => PostMessageW(h, WM_CHAR, WPARAM(key as usize), LPARAM(0)),
                _ => Ok(()),
            };
            sent.is_ok()
        }
    }

    #[cfg(test)]
    mod tests {
        /// 归一化坐标钳制语义：0..1 之外钳入，映射到客户区物理坐标。
        #[test]
        fn input_clamp_semantics() {
            let clamp = |v: f64, max: i32| -> i32 { (v.clamp(0.0, 1.0) * max as f64) as i32 };
            assert_eq!(clamp(-0.5, 800), 0);
            assert_eq!(clamp(1.5, 800), 800);
            assert_eq!(clamp(0.5, 800), 400);
        }
    }
}
