//! M0（R9）：WebView2 进程故障监听 —— 崩溃原因落盘（宿主反复重启排查的盲区补全）。
//!
//! 背景：forensic 三件套（bootstrap / close REQUESTED / DESTROYED）补上 pid+label
//! 之后，仍有一类事件无任何日志可查 —— WebView2 渲染/浏览器进程死亡。它既可能造成
//! 窗口白屏（渲染进程退出），也可能表现为窗口静默消失（浏览器进程退出连带销毁），
//! 日志里只会留下一行无因的 DESTROYED。wry 0.55 / tauri 2.11 均未暴露
//! CoreWebView2 的 ProcessFailed 事件，这里经 `Webview::with_webview` 拿到底层
//! COM 接口自行挂载；失败种类与退出码写入 applog，可与窗口事件按时间轴对齐。
//!
//! 覆盖范围：看护线程每 3s 扫一遍 `app.webview_windows()`，给「新出现的 webview」
//! 补挂监听（主窗、任务栏、占位窗等后建窗口都会被覆盖；同一 label 只挂一次，
//! 窗口销毁后清账）。仅 windows 平台有 WebView2；其它平台空实现。

use tauri::Manager;

/// 已挂监听的 webview label 集合（防重复挂载；随窗口销毁清理，句柄值会回收复用）。
#[cfg(windows)]
static ATTACHED: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashSet<String>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashSet::new()));

#[cfg(windows)]
pub fn spawn(app: tauri::AppHandle) {
    std::thread::Builder::new()
        .name("webview-watchdog".into())
        .spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_secs(3));
            let wins = app.webview_windows();
            {
                let mut g = ATTACHED.lock().unwrap_or_else(|e| e.into_inner());
                g.retain(|l| wins.contains_key(l));
            }
            for (label, wv) in &wins {
                let fresh = ATTACHED
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(label.clone());
                if !fresh {
                    continue;
                }
                let tag = label.clone();
                let attached = wv.with_webview(move |pw| unsafe { attach(pw, tag) });
                if let Err(e) = attached {
                    crate::shell::applog::log(
                        "webview",
                        format!("{label}: ProcessFailed 监听挂载失败（with_webview）：{e}"),
                    );
                    // 挂载失败允许下轮重试
                    ATTACHED
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .remove(label);
                }
            }
        })
        .ok();
}

#[cfg(not(windows))]
pub fn spawn(_app: tauri::AppHandle) {}

/// 在 webview 所属线程（主线程）上挂 `ICoreWebView2::ProcessFailed`。
/// 回调里只做一件事：失败种类 + 退出码落盘 applog（线程安全追加写）。
#[cfg(windows)]
unsafe fn attach(pw: tauri::webview::PlatformWebview, label: String) {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2ProcessFailedEventArgs, COREWEBVIEW2_PROCESS_FAILED_KIND,
    };
    use webview2_com::ProcessFailedEventHandler;
    use windows_core::Interface as _;

    let controller = pw.controller();
    let core = match controller.CoreWebView2() {
        Ok(c) => c,
        Err(e) => {
            crate::shell::applog::log(
                "webview",
                format!("{label}: 取 CoreWebView2 失败，跳过 ProcessFailed 监听：{e}"),
            );
            return;
        }
    };
    // webview2-com 的 event_callback 生成物收的是 Box<dyn FnMut(..)>。
    // label 要留给挂载结果日志用；闭包内用克隆
    let tag = label.clone();
    let handler = ProcessFailedEventHandler::create(Box::new(
        move |_sender, args: Option<ICoreWebView2ProcessFailedEventArgs>| {
            let Some(args) = args else {
                crate::shell::applog::log("webview", "ProcessFailed 事件缺 EventArgs（异常）");
                return Ok(());
            };
            // sys 层是 out-param 风格：ProcessFailedKind(&mut kind) -> Result<()>
            let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND(0);
            if let Err(e) = args.ProcessFailedKind(&mut kind) {
                crate::shell::applog::log(
                    "webview",
                    format!("ProcessFailed 事件取种类失败：{e}"),
                );
                return Ok(());
            }
            let kind_name = match kind.0 {
                0 => "BrowserProcessExited（浏览器进程退出——全部窗口白屏/消失）",
                1 => "RenderProcessExited（渲染进程退出——窗口白屏）",
                2 => "RenderProcessUnresponsive（渲染进程无响应）",
                3 => "FrameRenderProcessExited（子框架渲染进程退出）",
                4 => "UtilityProcessExited（工具进程退出）",
                5 => "SandboxHelperProcessExited（沙箱辅助进程退出）",
                6 => "GpuProcessExited（GPU 进程退出）",
                7 => "PpapiPluginProcessExited（PPAPI 插件进程退出）",
                8 => "PpapiBrokerProcessExited（PPAPI 代理进程退出）",
                9 => "UnknownProcessExited（未知进程退出）",
                _ => "其它种类",
            };
            // 退出码只在「进程退出」类事件上有效；无响应类取不到就忽略。
            let mut code: i32 = 0;
            let exit_code = args
                .cast::<webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2ProcessFailedEventArgs2>()
                .ok()
                .and_then(|a2| a2.ExitCode(&mut code).ok().map(|_| code));
            match exit_code {
                Some(c) => crate::shell::applog::log(
                    "webview",
                    format!("{tag}: WebView2 进程故障 kind={kind_name} exitCode={c:#x}"),
                ),
                None => crate::shell::applog::log(
                    "webview",
                    format!("{tag}: WebView2 进程故障 kind={kind_name}"),
                ),
            }
            Ok(())
        },
    ));
    let mut token: i64 = 0;
    match core.add_ProcessFailed(&handler, &mut token) {
        Ok(()) => {
            crate::shell::applog::log("webview", format!("{label}: ProcessFailed 监听已挂载"));
        }
        Err(e) => {
            crate::shell::applog::log(
                "webview",
                format!("{label}: add_ProcessFailed 失败：{e}"),
            );
        }
    }
}
