//! L3 shell — compat_probe.rs（批次C-6 分级探测器，四层兼容引擎的裁判）：
//! - 对目标窗口采样：GWL_STYLE/EX_STYLE、类名前缀、DWM CLOAK/
//!   EXTENDED_FRAME、IsCompositionActive、窗口区域 vs 客户区差异、
//!   是否 UWP（GetPackageFullName 成功）。
//! - 决策树 → CompatTier = L1 | L2 | L3 | L4 | Native：
//!   · UWP（AppX 包窗口）/ DWM CLOAKED 隐身窗口 → L3（画面捕获 + 输入转发）
//!   · WS_CAPTION 完整 + 标准非客户区 → L1（增强重父化）
//!   · 无标题栏 / 自绘非客户区（窗口区域与客户区差异异常）→ L2（容器包裹）
//!   · 类名命中独占全屏/反作弊特征 → L4（智能让位）
//!   · 其余 → Native（独立窗口如实降级）
//! - 结果持久化 apps.json 每登记项 compat: { tier, probedAt, evidence }；
//!   exe mtime 变化（版本更新）→ 下次嵌入自动重探；用户覆盖最高优先。

use serde::{Deserialize, Serialize};

/// 兼容层级（C-6 决策树输出；C-3/4/5 按层路由）。
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "UPPERCASE")]
pub enum CompatTier {
    L1,
    L2,
    L3,
    L4,
    /// 独立窗口如实降级
    Native,
}

impl CompatTier {
    pub fn as_str(self) -> &'static str {
        match self {
            CompatTier::L1 => "L1",
            CompatTier::L2 => "L2",
            CompatTier::L3 => "L3",
            CompatTier::L4 => "L4",
            CompatTier::Native => "Native",
        }
    }

    pub fn from_str(s: &str) -> Option<CompatTier> {
        match s {
            "L1" => Some(CompatTier::L1),
            "L2" => Some(CompatTier::L2),
            "L3" => Some(CompatTier::L3),
            "L4" => Some(CompatTier::L4),
            "Native" => Some(CompatTier::Native),
            _ => None,
        }
    }
}

/// 每登记项兼容探测结果（apps.json 持久化；serde default 平滑升级旧文件）。
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct CompatInfo {
    /// 最近一次自动探测层级（None = 未探测）
    #[serde(default)]
    pub tier: Option<CompatTier>,
    /// 用户强制层级（最高优先，覆盖自动探测）
    #[serde(default)]
    pub override_tier: Option<CompatTier>,
    /// 探测时间（ms since epoch）
    #[serde(default)]
    pub probed_at: Option<u64>,
    /// 探测证据（决策树输入快照；C-8 归因回填用）
    #[serde(default)]
    pub evidence: serde_json::Value,
    /// 批次C-5：L4 让位归因提示（"fullscreen" 独占全屏 | "anticheat" 反作弊；
    /// None = 非 L4）。C-5 看护与看门狗用它区分让位语义：anticheat 期间
    /// kbdhook 主动停用并横幅声明，fullscreen 仅桌面层收起。
    #[serde(default)]
    pub hint: Option<String>,
    /// 探测时 exe mtime（版本更新检测）
    #[serde(default)]
    pub exe_mtime: Option<u64>,
}

impl CompatInfo {
    /// 生效层级：用户覆盖 > 自动探测 > 缺省 L1。
    pub fn effective(&self) -> CompatTier {
        self.override_tier.or(self.tier).unwrap_or(CompatTier::L1)
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 采样目标窗口并输出层级 + 证据（纯探测，不改窗口状态）。
#[cfg(windows)]
pub fn probe_hwnd(hwnd: isize) -> CompatInfo {
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::Graphics::Dwm::{
        DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS,
    };
    use windows::Win32::Storage::Packaging::Appx::GetPackageFullName;
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    use windows::Win32::UI::Controls::IsCompositionActive;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, GetClientRect, GetWindowLongPtrW, GetWindowRect, GetWindowThreadProcessId,
        GWL_EXSTYLE, GWL_STYLE, WS_CAPTION,
    };

    let h = HWND(hwnd as *mut core::ffi::c_void);
    let style = unsafe { GetWindowLongPtrW(h, GWL_STYLE) } as u32;
    let ex_style = unsafe { GetWindowLongPtrW(h, GWL_EXSTYLE) } as u32;
    let has_caption = style & WS_CAPTION.0 != 0;

    let mut cls = [0u16; 128];
    let cls_len = unsafe { GetClassNameW(h, &mut cls) };
    let class_name = String::from_utf16_lossy(&cls[..cls_len as usize]);

    // DWM：CLOAKED（UWP 隐身/挂起特征）与 EXTENDED_FRAME_BOUNDS（自绘边框检测）
    let mut cloaked: u32 = 0;
    let cloaked_ok = unsafe {
        DwmGetWindowAttribute(
            h,
            DWMWA_CLOAKED,
            &mut cloaked as *mut u32 as *mut core::ffi::c_void,
            std::mem::size_of::<u32>() as u32,
        )
    }
    .is_ok();
    let mut frame = RECT::default();
    let frame_ok = unsafe {
        DwmGetWindowAttribute(
            h,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut frame as *mut RECT as *mut core::ffi::c_void,
            std::mem::size_of::<RECT>() as u32,
        )
    }
    .is_ok();

    let mut wrect = RECT::default();
    let mut crect = RECT::default();
    let _ = unsafe { GetWindowRect(h, &mut wrect) };
    let _ = unsafe { GetClientRect(h, &mut crect) };
    // 非客户区厚度（自绘窗口常为 0：窗口矩形 ≈ 客户区矩形）
    let nonclient_w = (wrect.right - wrect.left) - (crect.right - crect.left);
    let nonclient_h = (wrect.bottom - wrect.top) - (crect.bottom - crect.top);

    // UWP 判定：窗口进程有 AppX 包全名
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
    let is_uwp = pid != 0 && unsafe {
        OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
            .map(|proc| {
                let mut buf = [0u16; 512];
                let mut len = buf.len() as u32;
                GetPackageFullName(proc, &mut len, windows::core::PWSTR(buf.as_mut_ptr()))
                    .is_ok()
            })
            .unwrap_or(false)
    };

    let composition = unsafe { IsCompositionActive() }.as_bool();

    // 决策树（顺序即优先级）
    let l4_hint = anti_cheat_or_exclusive(&class_name);
    let (tier, reason) = if is_uwp {
        (crate::shell::compat_probe::CompatTier::L3, "appx package window")
    } else if cloaked_ok && cloaked != 0 {
        (crate::shell::compat_probe::CompatTier::L3, "dwm cloaked")
    } else if l4_hint.is_some() {
        (crate::shell::compat_probe::CompatTier::L4, "exclusive/anticheat class")
    } else if has_caption && nonclient_w >= 8 && nonclient_h >= 8 {
        (crate::shell::compat_probe::CompatTier::L1, "standard caption + nonclient frame")
    } else if has_caption && frame_ok && (nonclient_w < 8 || nonclient_h < 8) {
        // 有标题栏但非客户区极薄 → 自绘边框（Qt/自绘壳）
        (crate::shell::compat_probe::CompatTier::L2, "custom-drawn frame")
    } else {
        (crate::shell::compat_probe::CompatTier::L2, "borderless/self-drawn window")
    };

    CompatInfo {
        tier: Some(tier),
        override_tier: None,
        probed_at: Some(now_ms()),
        evidence: serde_json::json!({
            "reason": reason,
            "className": class_name,
            "style": style,
            "exStyle": ex_style,
            "hasCaption": has_caption,
            "cloaked": if cloaked_ok { Some(cloaked) } else { None },
            "nonClientW": nonclient_w,
            "nonClientH": nonclient_h,
            "extFrame": frame_ok,
            "uwp": is_uwp,
            "composition": composition,
            "pid": pid,
        }),
        // 批次C-5：L4 让位归因（fullscreen / anticheat），看护语义分流用
        hint: l4_hint.map(|s| s.to_string()),
        exe_mtime: None,
    }
}

#[cfg(not(windows))]
pub fn probe_hwnd(_hwnd: isize) -> CompatInfo {
    CompatInfo::default()
}

/// 探测 + 持久化：结果写入 apps.json 该登记项 compat 字段（保留用户覆盖；
/// exe mtime 记入证据，版本更新后下次嵌入自动重探）。改窗口样式**之前**调用。
pub fn probe_and_persist(
    st: &crate::state::AppState,
    tp_id: &str,
    hwnd: isize,
    exe_path: Option<&str>,
) -> CompatInfo {
    let mut info = probe_hwnd(hwnd);
    info.exe_mtime = exe_path.and_then(|p| std::fs::metadata(p).ok()).and_then(|m| {
        m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
    }).map(|d| d.as_millis() as u64);
    // 用户覆盖最高优先：保留既有 override_tier
    let previous_override = crate::shell::launcher::registry_snapshot(st)
        .iter()
        .find(|a| a.id == tp_id)
        .map(|a| a.compat.override_tier)
        .unwrap_or(None);
    info.override_tier = previous_override;
    // 持久化
    let mut apps = crate::shell::launcher::registry_snapshot(st);
    if let Some(app) = apps.iter_mut().find(|a| a.id == tp_id) {
        app.compat = info.clone();
        let _ = crate::shell::launcher::save_registry(st, &apps);
    }
    info
}

/// 批次C-6：用户强制层级（设置 → 第三方 → 兼容层级；优先级高于自动探测）。
#[tauri::command]
pub fn compat_set_override(
    st: tauri::State<'_, crate::state::AppState>,
    id: String,
    tier: Option<String>,
) -> CmdResult<()> {
    let parsed = tier.as_deref().map(|t| {
        CompatTier::from_str(t)
            .ok_or_else(|| crate::error::AppError::validation(format!("未知层级 / unknown tier: {t}")))
    });
    let mut apps = crate::shell::launcher::registry_snapshot(&st);
    let Some(app) = apps.iter_mut().find(|a| a.id == id) else {
        return Err(crate::error::AppError::not_found(format!("未找到登记项 / Not found: {id}")));
    };
    match parsed {
        Some(Ok(t)) => app.compat.override_tier = Some(t),
        Some(Err(e)) => return Err(e),
        None => app.compat.override_tier = None,
    }
    crate::shell::launcher::save_registry(&st, &apps)
}

use crate::error::CmdResult;

/// L4 特征：独占全屏/反作弊窗口类名启发（C-5 联动）。
/// 返回让位归因 hint：anticheat（反作弊服务/启动器）| fullscreen（独占全屏引擎窗口）；
/// None = 非 L4。C-5 看护按 hint 分流语义（anticheat 停用 kbdhook + 横幅声明）。
#[cfg(windows)]
fn anti_cheat_or_exclusive(class_name: &str) -> Option<&'static str> {
    const ANTICHEAT: [&str; 5] = [
        "EasyAntiCheat",
        "BEService",      // BattlEye
        "EACLaunchService",
        "STARTUP",        // 部分反作弊启动窗口
        "BuriedScene",    // NetEase guard
    ];
    const FULLSCREEN: [&str; 1] = [
        "UnrealWindow",   // UE 独占全屏常见类名
    ];
    let lower = class_name.to_lowercase();
    if ANTICHEAT.iter().any(|m| lower.contains(&m.to_lowercase())) {
        return Some("anticheat");
    }
    if FULLSCREEN.iter().any(|m| lower.contains(&m.to_lowercase())) {
        return Some("fullscreen");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{CompatInfo, CompatTier};

    /// 生效层级：用户覆盖 > 自动探测 > 缺省 L1。
    #[test]
    fn effective_tier_priority() {
        assert_eq!(CompatInfo::default().effective(), CompatTier::L1);
        let probed = CompatInfo { tier: Some(CompatTier::L2), ..Default::default() };
        assert_eq!(probed.effective(), CompatTier::L2);
        let overridden = CompatInfo { override_tier: Some(CompatTier::Native), ..probed };
        assert_eq!(overridden.effective(), CompatTier::Native);
    }

    /// serde 序列化往返（apps.json 持久化格式稳定）。
    #[test]
    fn compat_info_roundtrip() {
        let info = CompatInfo {
            tier: Some(CompatTier::L3),
            override_tier: Some(CompatTier::L4),
            probed_at: Some(123),
            evidence: serde_json::json!({"reason": "appx package window"}),
            hint: Some("anticheat".into()),
            exe_mtime: Some(456),
        };
        let json = serde_json::to_string(&info).unwrap();
        let back: CompatInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(back.effective(), CompatTier::L4);
        assert_eq!(back.tier, Some(CompatTier::L3));
        assert_eq!(back.hint.as_deref(), Some("anticheat"));
        // 旧文件（无 compat 字段）→ default 平滑升级
        let legacy: CompatInfo = serde_json::from_str("{}").unwrap();
        assert_eq!(legacy.effective(), CompatTier::L1);
        assert!(legacy.hint.is_none(), "旧登记项无 hint，serde default 平滑升级");
    }

    /// 批次C-5：L4 让位归因分流——反作弊特征 → anticheat，
    /// 独占全屏引擎窗口 → fullscreen，普通窗口 → None。
    #[cfg(windows)]
    #[test]
    fn l4_hint_classification() {
        assert_eq!(super::anti_cheat_or_exclusive("UnrealWindow"), Some("fullscreen"));
        assert_eq!(super::anti_cheat_or_exclusive("UnrealWindow 42"), Some("fullscreen"));
        assert_eq!(super::anti_cheat_or_exclusive("EasyAntiCheat_RawInput"), Some("anticheat"));
        assert_eq!(super::anti_cheat_or_exclusive("BEService"), Some("anticheat"));
        assert_eq!(super::anti_cheat_or_exclusive("BuriedScene_wnd"), Some("anticheat"));
        assert_eq!(super::anti_cheat_or_exclusive("Chrome_WidgetWin_1"), None);
        assert_eq!(super::anti_cheat_or_exclusive("Notepad"), None);
    }

    /// 层级字符串编解码（前端展示/回传用）。
    #[test]
    fn tier_str_roundtrip() {
        for t in [CompatTier::L1, CompatTier::L2, CompatTier::L3, CompatTier::L4, CompatTier::Native] {
            assert_eq!(CompatTier::from_str(t.as_str()), Some(t));
        }
        assert_eq!(CompatTier::from_str("nope"), None);
    }
}
