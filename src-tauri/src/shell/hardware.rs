//! L3 shell — hardware.rs（M5）
//! 宿主机硬件状态：摄像头/麦克风占用检测（隐私核心）、音频音量（读/写/静音）、
//! Wi-Fi 当前连接（只读）、蓝牙开关状态（只读）。
//! 全部走本机 Windows API / 注册表，零网络、零遥测。

use serde::Serialize;

#[derive(Serialize)]
pub struct DeviceUsage {
    /// "microphone" | "webcam"
    pub kind: String,
    /// 占用方（packaged = 包族名；NonPackaged = 可执行文件名）
    pub app: String,
}

#[derive(Serialize)]
pub struct AudioState {
    /// 主音量 0..1
    pub volume: f32,
    pub muted: bool,
}

#[derive(Serialize)]
pub struct WifiState {
    pub connected: bool,
    pub ssid: Option<String>,
    /// 0..100
    pub signal: Option<u32>,
    /// 无线电开关状态（批次E 规格 6.2.1；None = 读取失败/无无线电）
    pub radio_on: Option<bool>,
}

/// Wi-Fi 无线电开关状态（WinRT Radio；零网络）。
#[cfg(windows)]
fn wifi_radio() -> Option<bool> {
    use windows::Devices::Radios::{Radio, RadioKind, RadioState};
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
    let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    let need_uninit = hr.is_ok();
    let result = (|| -> Option<bool> {
        let radios = Radio::GetRadiosAsync().ok()?.get().ok()?;
        for i in 0..radios.Size().ok()? {
            let Ok(r) = radios.GetAt(i) else { continue };
            if r.Kind().ok()? == RadioKind::WiFi {
                return Some(r.State().ok()? == RadioState::On);
            }
        }
        None
    })();
    if need_uninit {
        unsafe { windows::Win32::System::Com::CoUninitialize() };
    }
    result
}

#[derive(Serialize)]
pub struct BluetoothState {
    /// 机器是否具备蓝牙无线电
    pub available: bool,
    pub enabled: bool,
}

// ---------------------------------------------------------------------------
// 摄像头/麦克风占用 — ConsentStore 注册表（Windows 隐私仪表盘同源数据）
// ---------------------------------------------------------------------------

#[cfg(windows)]
fn read_consent_usage(kind_key: &str, kind: &str, out: &mut Vec<DeviceUsage>) {
    use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ};
    use winreg::RegKey;

    const ROOT: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore";
    let hk = RegKey::predef(HKEY_LOCAL_MACHINE);
    let Ok(root) = hk.open_subkey_with_flags(format!(r"{ROOT}\{kind_key}"), KEY_READ) else {
        return;
    };
    let mut scan = |key: &RegKey| {
        for name in key.enum_keys().flatten() {
            let Ok(sub) = key.open_subkey(&name) else { continue };
            let Some(start) = filetime_value(&sub, "LastUsedTimeStart") else { continue };
            let stop = filetime_value(&sub, "LastUsedTimeStop");
            // 在使用中：有开始时间，且尚未结束（或结束早于开始）
            if stop.map_or(true, |e| e < start) {
                let app = name
                    .rsplit(['#', '\\'])
                    .find(|seg| !seg.is_empty())
                    .unwrap_or(&name)
                    .to_string();
                out.push(DeviceUsage {
                    kind: kind.to_string(),
                    app,
                });
            }
        }
    };
    scan(&root);
    if let Ok(np) = root.open_subkey("NonPackaged") {
        scan(&np);
    }
}

/// ConsentStore 的时间值在不同 Windows 版本上是 REG_QWORD 或 REG_BINARY(8B FILETIME)，统一读出。
#[cfg(windows)]
fn filetime_value(key: &winreg::RegKey, name: &str) -> Option<u64> {
    let rv = key.get_raw_value(name).ok()?;
    if rv.bytes.len() == 8 {
        return Some(u64::from_le_bytes(rv.bytes.try_into().ok()?));
    }
    None
}

#[cfg(windows)]
#[tauri::command]
pub fn privacy_usage() -> Vec<DeviceUsage> {
    let mut v = Vec::new();
    read_consent_usage("microphone", "microphone", &mut v);
    read_consent_usage("webcam", "webcam", &mut v);
    v
}

// ---------------------------------------------------------------------------
// 音频 — Core Audio (IAudioEndpointVolume)
// ---------------------------------------------------------------------------

#[cfg(windows)]
fn with_endpoint_volume<T>(
    f: impl FnOnce(&windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume) -> windows::core::Result<T>,
) -> Result<T, String> {
    use windows::Win32::Media::Audio::{
        eMultimedia, eRender, IMMDeviceEnumerator, MMDeviceEnumerator,
    };
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED,
    };

    unsafe {
        // 线程池线程：MTA 初始化（已初始化时返回 S_FALSE / RPC_E_CHANGED_MODE 均可继续）
        let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
        let need_uninit = hr.is_ok();
        let result = (|| {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(e2s)?;
            let device = enumerator
                .GetDefaultAudioEndpoint(eRender, eMultimedia)
                .map_err(e2s)?;
            let volume: IAudioEndpointVolume = device
                .Activate(CLSCTX_ALL, None)
                .map_err(e2s)?;
            f(&volume).map_err(e2s)
        })();
        if need_uninit {
            CoUninitialize();
        }
        result
    }
}

#[cfg(windows)]
fn e2s(e: windows::core::Error) -> String {
    e.to_string()
}

/// MTA apartment helper（线程池线程进入 COM 前初始化；已初始化过则不重复 Uninit）。
#[cfg(windows)]
fn with_mta<T>(f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};
    let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    let need_uninit = hr.is_ok();
    let out = f();
    if need_uninit {
        unsafe { CoUninitialize() };
    }
    out
}

#[cfg(windows)]
#[tauri::command]
pub fn audio_get() -> Result<AudioState, String> {
    with_endpoint_volume(|v| unsafe {
        let volume = v.GetMasterVolumeLevelScalar()?;
        let muted = v.GetMute()?.as_bool();
        Ok(AudioState { volume, muted })
    })
}

#[cfg(windows)]
#[tauri::command]
pub fn audio_set(volume: f32, muted: Option<bool>) -> Result<AudioState, String> {
    let volume = volume.clamp(0.0, 1.0);
    with_endpoint_volume(move |v| unsafe {
        use windows::Win32::Foundation::BOOL;
        v.SetMasterVolumeLevelScalar(volume, std::ptr::null())?;
        if let Some(m) = muted {
            v.SetMute(BOOL::from(m), std::ptr::null())?;
        }
        let volume = v.GetMasterVolumeLevelScalar()?;
        let muted = v.GetMute()?.as_bool();
        Ok(AudioState { volume, muted })
    })
}

// ---------------------------------------------------------------------------
// Wi-Fi — Native WiFi (wlanapi)，只读当前连接
// ---------------------------------------------------------------------------

#[cfg(windows)]
#[tauri::command]
pub fn wifi_get() -> Result<WifiState, String> {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::NetworkManagement::WiFi::{
        wlan_intf_opcode_current_connection, wlan_interface_state_connected, WlanCloseHandle,
        WlanEnumInterfaces, WlanFreeMemory, WlanOpenHandle, WlanQueryInterface,
        WLAN_CONNECTION_ATTRIBUTES, WLAN_INTERFACE_INFO_LIST,
    };

    fn wlan_err(op: &str, code: u32) -> String {
        format!("{op} failed: {code}")
    }

    unsafe {
        let mut negotiated = 0u32;
        let mut handle = HANDLE::default();
        let err = WlanOpenHandle(2u32, None, &mut negotiated, &mut handle);
        if err != 0 {
            return Err(wlan_err("WlanOpenHandle", err));
        }
        let result = (|| {
            let mut list_ptr: *mut WLAN_INTERFACE_INFO_LIST = std::ptr::null_mut();
            let err = WlanEnumInterfaces(handle, None, &mut list_ptr);
            if err != 0 {
                return Err(wlan_err("WlanEnumInterfaces", err));
            }
            if list_ptr.is_null() {
                return Ok(WifiState { connected: false, ssid: None, signal: None, radio_on: wifi_radio() });
            }
            let list = &*list_ptr;
            let mut state = WifiState { connected: false, ssid: None, signal: None, radio_on: wifi_radio() };
            for i in 0..list.dwNumberOfItems as usize {
                let iface = &(*list.InterfaceInfo.as_ptr().add(i));
                let mut data_ptr: *mut WLAN_CONNECTION_ATTRIBUTES = std::ptr::null_mut();
                let mut data_size = 0u32;
                let err = WlanQueryInterface(
                    handle,
                    &iface.InterfaceGuid,
                    wlan_intf_opcode_current_connection,
                    None,
                    &mut data_size,
                    &mut data_ptr as *mut _ as *mut *mut core::ffi::c_void,
                    None,
                );
                if err != 0 || data_ptr.is_null() {
                    continue;
                }
                let attrs = &*data_ptr;
                if attrs.isState == wlan_interface_state_connected {
                    let ssid = &attrs.wlanAssociationAttributes.dot11Ssid;
                    let len = (ssid.uSSIDLength as usize).min(32);
                    if len > 0 {
                        state.connected = true;
                        state.ssid = Some(String::from_utf8_lossy(&ssid.ucSSID[..len]).to_string());
                        state.signal = Some(attrs.wlanAssociationAttributes.wlanSignalQuality);
                    }
                }
                WlanFreeMemory(data_ptr as _);
            }
            WlanFreeMemory(list_ptr as _);
            Ok(state)
        })();
        let _ = WlanCloseHandle(handle, None);
        result
    }
}

// ---------------------------------------------------------------------------
// 蓝牙 — WinRT Radios API（只读开关状态；Bluetooth 适配器经 Radio 枚举）
// ---------------------------------------------------------------------------

#[cfg(windows)]
#[tauri::command]
pub fn bluetooth_get() -> Result<BluetoothState, String> {
    use windows::Devices::Radios::{Radio, RadioKind, RadioState};

    // CoInitializeEx(MTA) 已随本命令线程初始化（WinRT static 调用需要 apartment）
    let hr = unsafe { windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED) };
    let need_uninit = hr.is_ok();

    // 无蓝牙设备 / 系统服务不可用 → available=false（真实状态，不是错误）
    let result = (|| -> Result<BluetoothState, String> {
        let radios = Radio::GetRadiosAsync()
            .map_err(e2s)?
            .get()
            .map_err(e2s)?;
        let mut found = BluetoothState { available: false, enabled: false };
        for i in 0..radios.Size().map_err(e2s)? {
            let r = radios.GetAt(i).map_err(e2s)?;
            if r.Kind().map_err(e2s)? == RadioKind::Bluetooth {
                found.available = true;
                found.enabled = r.State().map_err(e2s)? == RadioState::On;
                break;
            }
        }
        Ok(found)
    })();

    if need_uninit {
        unsafe { windows::Win32::System::Com::CoUninitialize() };
    }
    result
}

// ---------------------------------------------------------------------------
// 批次C — 蓝牙开关 + 已配对设备（WinRT Radio / BluetoothDevice，规格 6.1）
// ---------------------------------------------------------------------------

/// 蓝牙无线电开关（规格 6.1：开关切换；WinRT Radio.SetStateAsync）。
#[cfg(windows)]
#[tauri::command]
pub fn bluetooth_set(enabled: bool) -> Result<BluetoothState, String> {
    use windows::Devices::Radios::{Radio, RadioKind, RadioState};
    with_mta(|| {
        let radios = Radio::GetRadiosAsync().map_err(e2s)?.get().map_err(e2s)?;
        let mut state = BluetoothState { available: false, enabled: false };
        for i in 0..radios.Size().map_err(e2s)? {
            let r = radios.GetAt(i).map_err(e2s)?;
            if r.Kind().map_err(e2s)? == RadioKind::Bluetooth {
                state.available = true;
                r.SetStateAsync(if enabled { RadioState::On } else { RadioState::Off })
                    .map_err(e2s)?
                    .get()
                    .map_err(e2s)?;
                state.enabled = enabled;
                break;
            }
        }
        Ok(state)
    })
}

#[derive(Serialize)]
pub struct BtDevice {
    pub name: String,
    /// 设备实例 ID（设备管理器口径，连接/断开操作用；批次E 规格 6.1.3）
    pub id: String,
    /// 已配对且系统认为处于连接状态
    pub connected: bool,
}

/// WinRT 接口 ID（\\?\BTHENUM#...）→ 设备实例 ID（BTHENUM\...）。
#[cfg(windows)]
fn bt_instance_id(interface_id: &windows::core::HSTRING) -> Option<String> {
    use windows::Win32::Devices::DeviceAndDriverInstallation::{CM_Get_Device_Interface_PropertyW, CONFIGRET};
    use windows::Win32::Devices::Properties::DEVPKEY_Device_InstanceId;

    let mut prop_type = windows::Win32::Devices::Properties::DEVPROPTYPE(0);
    let mut size = 0u32;
    let _rc = unsafe {
        CM_Get_Device_Interface_PropertyW(
            interface_id,
            &DEVPKEY_Device_InstanceId,
            &mut prop_type,
            None,
            &mut size,
            0,
        )
    };
    // 首次调用以取得缓冲区大小（成功或失败都要求 size > 0）
    if size == 0 {
        return None;
    }
    let mut buf = vec![0u16; (size / 2).max(1) as usize];
    let rc = unsafe {
        CM_Get_Device_Interface_PropertyW(
            interface_id,
            &DEVPKEY_Device_InstanceId,
            &mut prop_type,
            Some(buf.as_mut_ptr() as *mut u8),
            &mut size,
            0,
        )
    };
    if rc != CONFIGRET(0) {
        return None;
    }
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    let s = String::from_utf16_lossy(&buf[..end]);
    (!s.is_empty()).then_some(s)
}

/// 设备实例 ID → devinst 句柄（连接/断开共用）。
#[cfg(windows)]
fn locate_devnode(instance_id: &str) -> Result<u32, String> {
    use windows::Win32::Devices::DeviceAndDriverInstallation::{CM_Locate_DevNodeW, CM_LOCATE_DEVNODE_NORMAL, CONFIGRET};
    use windows::core::PWSTR;
    let mut id_wide: Vec<u16> = instance_id.encode_utf16().collect();
    id_wide.push(0);
    let mut devinst = 0u32;
    let rc = unsafe { CM_Locate_DevNodeW(&mut devinst, PWSTR(id_wide.as_mut_ptr()), CM_LOCATE_DEVNODE_NORMAL) };
    if rc != CONFIGRET(0) {
        return Err(format!("CM_Locate_DevNode failed: {:?}", rc));
    }
    Ok(devinst)
}

/// 已配对蓝牙设备列表 + 连接状态（规格 6.1.2；批次E 增加 id 供连接/断开操作）。
/// 电量读数需要 GATT 连接，当前不显示。
#[cfg(windows)]
#[tauri::command]
pub fn bt_devices() -> Result<Vec<BtDevice>, String> {
    use windows::Devices::Bluetooth::{BluetoothConnectionStatus, BluetoothDevice};
    use windows::Devices::Enumeration::DeviceInformation;

    with_mta(|| {
        let selector = BluetoothDevice::GetDeviceSelectorFromPairingState(true).map_err(e2s)?;
        let coll = DeviceInformation::FindAllAsyncAqsFilter(&selector)
            .map_err(e2s)?
            .get()
            .map_err(e2s)?;
        let mut out = Vec::new();
        for i in 0..coll.Size().map_err(e2s)? {
            let d = coll.GetAt(i).map_err(e2s)?;
            let name = d.Name().map_err(e2s)?.to_string();
            let dev_id = d.Id().map_err(e2s)?;
            let connected = match BluetoothDevice::FromIdAsync(&dev_id).map_err(e2s)?.get().map_err(e2s) {
                Ok(dev) => {
                    dev.ConnectionStatus().map_err(e2s)? == BluetoothConnectionStatus::Connected
                }
                Err(_) => false,
            };
            let id = bt_instance_id(&dev_id).unwrap_or_default();
            out.push(BtDevice { name, id, connected });
        }
        // 已连接的排前（规格 6.1.4 设备记忆：连接中的设备置顶）
        out.sort_by(|a, b| b.connected.cmp(&a.connected).then(a.name.cmp(&b.name)));
        Ok(out)
    })
}

/// 蓝牙设备连接（批次E 规格 6.1.3：重新启用设备节点触发回连；失败如实报错）。
#[cfg(windows)]
#[tauri::command]
pub fn bt_connect(id: String) -> Result<(), String> {
    use windows::Win32::Devices::DeviceAndDriverInstallation::{CM_Enable_DevNode, CM_Reenumerate_DevNode, CM_REENUMERATE_NORMAL, CONFIGRET};
    if id.is_empty() {
        return Err("device has no instance id".into());
    }
    let devinst = locate_devnode(&id)?;
    let rc = unsafe { CM_Enable_DevNode(devinst, 0) };
    if rc != CONFIGRET(0) {
        return Err(format!("CM_Enable_DevNode failed: {:?}（部分设备需要管理员权限）", rc));
    }
    // 触发重枚举让协议栈尝试回连
    unsafe { CM_Reenumerate_DevNode(devinst, CM_REENUMERATE_NORMAL) };
    Ok(())
}

/// 蓝牙设备断开（批次E 规格 6.1.3：停用设备节点即断开链路；失败如实报错）。
#[cfg(windows)]
#[tauri::command]
pub fn bt_disconnect(id: String) -> Result<(), String> {
    use windows::Win32::Devices::DeviceAndDriverInstallation::{CM_Disable_DevNode, CONFIGRET};
    if id.is_empty() {
        return Err("device has no instance id".into());
    }
    let devinst = locate_devnode(&id)?;
    let rc = unsafe { CM_Disable_DevNode(devinst, 0) };
    if rc != CONFIGRET(0) {
        return Err(format!("CM_Disable_DevNode failed: {:?}（部分设备需要管理员权限）", rc));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 批次E — Wi-Fi 无线电开关（规格 6.2.1；与蓝牙同走 WinRT Radio）
// ---------------------------------------------------------------------------

/// Wi-Fi 无线电开关（规格 6.2.1；WinRT Radio.SetStateAsync）。
#[cfg(windows)]
#[tauri::command]
pub fn wifi_set(enabled: bool) -> Result<WifiState, String> {
    use windows::Devices::Radios::{Radio, RadioKind, RadioState};
    with_mta(|| {
        let radios = Radio::GetRadiosAsync().map_err(e2s)?.get().map_err(e2s)?;
        let mut found = false;
        for i in 0..radios.Size().map_err(e2s)? {
            let r = radios.GetAt(i).map_err(e2s)?;
            if r.Kind().map_err(e2s)? == RadioKind::WiFi {
                found = true;
                r.SetStateAsync(if enabled { RadioState::On } else { RadioState::Off })
                    .map_err(e2s)?
                    .get()
                    .map_err(e2s)?;
                break;
            }
        }
        if !found {
            return Err("no wifi radio".into());
        }
        let mut st = wifi_get()?;
        st.radio_on = Some(enabled);
        Ok(st)
    })
}

// ---------------------------------------------------------------------------
// 批次C — Wi-Fi 扫描（仅用户点击"扫描"时）/ 断开（规格 6.2.2：默认不主动扫描）
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct WifiNetwork {
    pub ssid: String,
    /// 0..100
    pub signal: u32,
    pub secured: bool,
}

#[cfg(windows)]
#[tauri::command]
pub fn wifi_scan() -> Result<Vec<WifiNetwork>, String> {
    use windows::Win32::NetworkManagement::WiFi::{
        WlanCloseHandle, WlanEnumInterfaces, WlanFreeMemory, WlanGetAvailableNetworkList,
        WlanOpenHandle, WLAN_AVAILABLE_NETWORK_LIST, WLAN_INTERFACE_INFO_LIST,
    };

    unsafe {
        let mut negotiated = 0u32;
        let mut handle = windows::Win32::Foundation::HANDLE::default();
        let err = WlanOpenHandle(2u32, None, &mut negotiated, &mut handle);
        if err != 0 {
            return Err(format!("WlanOpenHandle failed: {err}"));
        }
        let result = (|| {
            let mut list_ptr: *mut WLAN_INTERFACE_INFO_LIST = std::ptr::null_mut();
            if WlanEnumInterfaces(handle, None, &mut list_ptr) != 0 {
                return Err("WlanEnumInterfaces failed".into());
            }
            let mut out: Vec<WifiNetwork> = Vec::new();
            if !list_ptr.is_null() {
                let ifaces = &*list_ptr;
                for i in 0..ifaces.dwNumberOfItems as usize {
                    let iface = &(*ifaces.InterfaceInfo.as_ptr().add(i));
                    let mut nets_ptr: *mut WLAN_AVAILABLE_NETWORK_LIST = std::ptr::null_mut();
                    if WlanGetAvailableNetworkList(handle, &iface.InterfaceGuid, 0, None, &mut nets_ptr) != 0 {
                        continue;
                    }
                    if !nets_ptr.is_null() {
                        let nets = &*nets_ptr;
                        for j in 0..nets.dwNumberOfItems as usize {
                            let net = &(*nets.Network.as_ptr().add(j));
                            let ssid = &net.dot11Ssid;
                            let len = (ssid.uSSIDLength as usize).min(32);
                            if len == 0 {
                                continue;
                            }
                            let name = String::from_utf8_lossy(&ssid.ucSSID[..len]).to_string();
                            if out.iter().any(|n| n.ssid == name) {
                                continue;
                            }
                            out.push(WifiNetwork {
                                ssid: name,
                                signal: net.wlanSignalQuality,
                                secured: net.bSecurityEnabled.as_bool(),
                            });
                        }
                        WlanFreeMemory(nets_ptr as _);
                    }
                }
                WlanFreeMemory(list_ptr as _);
            }
            out.sort_by(|a, b| b.signal.cmp(&a.signal));
            Ok(out)
        })();
        let _ = WlanCloseHandle(handle, None);
        result
    }
}

/// 断开当前 Wi-Fi 连接（用户在面板中显式点击）。
#[cfg(windows)]
#[tauri::command]
pub fn wifi_disconnect() -> Result<(), String> {
    use windows::Win32::NetworkManagement::WiFi::{
        WlanCloseHandle, WlanDisconnect, WlanEnumInterfaces, WlanFreeMemory, WlanOpenHandle,
        WLAN_INTERFACE_INFO_LIST,
    };

    unsafe {
        let mut negotiated = 0u32;
        let mut handle = windows::Win32::Foundation::HANDLE::default();
        let err = WlanOpenHandle(2u32, None, &mut negotiated, &mut handle);
        if err != 0 {
            return Err(format!("WlanOpenHandle failed: {err}"));
        }
        let result = (|| {
            let mut list_ptr: *mut WLAN_INTERFACE_INFO_LIST = std::ptr::null_mut();
            if WlanEnumInterfaces(handle, None, &mut list_ptr) != 0 {
                return Err("WlanEnumInterfaces failed".into());
            }
            let mut disconnected = false;
            if !list_ptr.is_null() {
                let ifaces = &*list_ptr;
                for i in 0..ifaces.dwNumberOfItems as usize {
                    let iface = &(*ifaces.InterfaceInfo.as_ptr().add(i));
                    if WlanDisconnect(handle, &iface.InterfaceGuid, None) == 0 {
                        disconnected = true;
                    }
                }
                WlanFreeMemory(list_ptr as _);
            }
            if disconnected {
                Ok(())
            } else {
                Err("未能断开 Wi-Fi（可能并未连接）/ Failed to disconnect (not connected?)".into())
            }
        })();
        let _ = WlanCloseHandle(handle, None);
        result
    }
}

// ---------------------------------------------------------------------------
// 批次C — 音频设备枚举 / 默认输出切换（规格 6.3）
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct AudioDeviceInfo {
    pub id: String,
    pub name: String,
    /// render = 输出；capture = 输入
    pub kind: String,
    pub default: bool,
}

/// 枚举激活的输出/输入端点 + 友好名 + 是否默认（规格 6.3.1 面板）。
#[cfg(windows)]
#[tauri::command]
pub fn audio_devices() -> Result<Vec<AudioDeviceInfo>, String> {
    use windows::core::BSTR;
    use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
    use windows::Win32::Media::Audio::{
        eCapture, eMultimedia, eRender, EDataFlow, IMMDeviceEnumerator, MMDeviceEnumerator,
    };
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL, STGM_READ};
    use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;

    fn enum_flow(
        enumerator: &IMMDeviceEnumerator,
        flow: EDataFlow,
        kind: &str,
        default_id: &str,
        out: &mut Vec<AudioDeviceInfo>,
    ) -> Result<(), String> {
        unsafe {
            let coll = enumerator
                .EnumAudioEndpoints(flow, windows::Win32::Media::Audio::DEVICE_STATE_ACTIVE)
                .map_err(e2s)?;
            let count = coll.GetCount().map_err(e2s)?;
            for i in 0..count {
                let dev = coll.Item(i).map_err(e2s)?;
                let id = dev.GetId().map_err(e2s)?.to_string().unwrap_or_default();
                let name = {
                    let store: IPropertyStore = dev.OpenPropertyStore(STGM_READ).map_err(e2s)?;
                    let v = store.GetValue(&PKEY_Device_FriendlyName).map_err(e2s)?;
                    // VT_LPWSTR → BSTR → String
                    BSTR::try_from(&v)
                        .map(|b| {
                            let ptr = b.as_ptr();
                            let mut n = 0usize;
                            while *ptr.add(n) != 0 {
                                n += 1;
                            }
                            String::from_utf16_lossy(std::slice::from_raw_parts(ptr, n))
                        })
                        .unwrap_or_default()
                };
                out.push(AudioDeviceInfo {
                    default: id == default_id,
                    id,
                    name,
                    kind: kind.to_string(),
                });
            }
        }
        Ok(())
    }

    with_mta(|| {
        unsafe {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(e2s)?;
            let default_render = enumerator
                .GetDefaultAudioEndpoint(eRender, eMultimedia)
                .and_then(|d| d.GetId())
                .map(|s| s.to_string().unwrap_or_default())
                .unwrap_or_default();
            let default_capture = enumerator
                .GetDefaultAudioEndpoint(eCapture, eMultimedia)
                .and_then(|d| d.GetId())
                .map(|s| s.to_string().unwrap_or_default())
                .unwrap_or_default();
            let mut out = Vec::new();
            enum_flow(&enumerator, eRender, "render", &default_render, &mut out)?;
            enum_flow(&enumerator, eCapture, "capture", &default_capture, &mut out)?;
            Ok(out)
        }
    })
}

/// 切换默认音频端点（规格 6.3.2：设备行点击即切换）。
/// 经 PolicyConfig COM（IPolicyConfig::SetDefaultEndpoint，vtable 槽位 13），
/// Windows 10/11 通用；失败时如实报错，不伪造成功。
#[cfg(windows)]
#[tauri::command]
pub fn audio_set_default(device_id: String) -> Result<(), String> {
    use windows::core::{GUID, HSTRING, HRESULT, Interface, PCWSTR};
    use windows::Win32::Media::Audio::eConsole;
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};

    // CLSID_PolicyConfigClient（Windows 10/11 稳定）；vtable 槽位 13 = SetDefaultEndpoint
    const CLSID_POLICY_CONFIG: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);
    type SetDefaultEndpointFn =
        unsafe extern "system" fn(this: *mut core::ffi::c_void, device_id: PCWSTR, role: i32) -> HRESULT;

    with_mta(|| unsafe {
        // 未公开 COM 接口（IID 不在 windows-rs 元数据中）：以 IUnknown 创建后按槽位调用
        let unk: windows::core::IUnknown =
            CoCreateInstance(&CLSID_POLICY_CONFIG, None, CLSCTX_ALL).map_err(e2s)?;
        let vtable =
            *(unk.as_raw() as *mut *mut core::ffi::c_void) as *const *mut core::ffi::c_void;
        let set_default: SetDefaultEndpointFn = std::mem::transmute(*vtable.add(13));
        let wid = HSTRING::from(device_id.as_str());
        let hr = set_default(unk.as_raw(), PCWSTR(wid.as_ptr()), eConsole.0);
        // 引用计数由 unk Drop 释放
        if hr.is_ok() {
            Ok(())
        } else {
            Err(format!("SetDefaultEndpoint failed: {hr:?}"))
        }
    })
}

// ---------------------------------------------------------------------------
// 批次C — 电池状态（GetSystemPowerStatus，规格 6.6.2）
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct BatteryState {
    pub has_battery: bool,
    pub ac_online: bool,
    /// 0..100（未知 = None）
    pub percent: Option<u8>,
    /// 剩余秒数（未知/充电中 = None）
    pub lifetime_secs: Option<u32>,
}

#[cfg(windows)]
#[tauri::command]
pub fn battery_get() -> Result<BatteryState, String> {
    use windows::Win32::System::Power::GetSystemPowerStatus;
    unsafe {
        let mut sps = windows::Win32::System::Power::SYSTEM_POWER_STATUS::default();
        GetSystemPowerStatus(&mut sps).map_err(e2s)?;
        let has_battery = sps.BatteryFlag & 128 == 0;
        let percent = if sps.BatteryLifePercent <= 100 && has_battery {
            Some(sps.BatteryLifePercent)
        } else {
            None
        };
        let lifetime_secs = if sps.BatteryLifeTime != u32::MAX && has_battery && sps.ACLineStatus == 0 {
            Some(sps.BatteryLifeTime)
        } else {
            None
        };
        Ok(BatteryState {
            has_battery,
            ac_online: sps.ACLineStatus == 1,
            percent,
            lifetime_secs,
        })
    }
}

#[cfg(not(windows))]
#[tauri::command]
pub fn battery_get() -> Result<BatteryState, String> {
    Err("unsupported platform".into())
}

// ---------------------------------------------------------------------------
// 批次C — 屏幕亮度（WMI WmiMonitorBrightness，仅内置面板；规格 6.6.1）
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct BrightnessState {
    /// 当前亮度 0..100（外接显示器/台式机 = false，不伪造可调）
    pub supported: bool,
    pub level: u8,
}

#[cfg(windows)]
mod brightness_wmi {
    use windows::core::{BSTR, VARIANT};
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
    use windows::Win32::System::Wmi::{
        IWbemClassObject, IWbemLocator, IWbemServices, WBEM_FLAG_FORWARD_ONLY,
        WBEM_GENERIC_FLAG_TYPE, WBEM_INFINITE, WbemLocator,
    };

    pub(super) fn connect() -> Result<IWbemServices, String> {
        unsafe {
            let locator: IWbemLocator =
                CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER).map_err(super::e2s)?;
            let server = locator
                .ConnectServer(
                    &BSTR::from("ROOT\\WMI"),
                    &BSTR::new(),
                    &BSTR::new(),
                    &BSTR::new(),
                    0,
                    &BSTR::new(),
                    None,
                )
                .map_err(super::e2s)?;
            Ok(server)
        }
    }

    /// 执行 WQL 并返回首个对象（无实例 → None）。
    fn query_first(server: &IWbemServices, query: &str) -> Result<Option<IWbemClassObject>, String> {
        let enumr = unsafe {
            server
                .ExecQuery(&BSTR::from("WQL"), &BSTR::from(query), WBEM_FLAG_FORWARD_ONLY, None)
                .map_err(super::e2s)?
        };
        let mut objs: [Option<IWbemClassObject>; 1] = [None];
        let mut returned = 0u32;
        unsafe {
            enumr
                .Next(WBEM_INFINITE, &mut objs, &mut returned)
                .ok()
                .map_err(super::e2s)?;
        }
        if returned == 0 {
            return Ok(None);
        }
        Ok(objs.into_iter().flatten().next())
    }

    /// 查询首个亮度实例（WmiMonitorBrightness / …Methods 的实例路径）。
    pub(super) fn first_path(server: &IWbemServices, class: &str) -> Result<Option<BSTR>, String> {
        let Some(obj) = query_first(server, &format!("SELECT * FROM {class}"))? else {
            return Ok(None);
        };
        unsafe {
            let mut path = VARIANT::default();
            obj.Get(&BSTR::from("__PATH"), 0, &mut path, None, None)
                .map_err(super::e2s)?;
            // VARIANT Drop 自动 VariantClear，无泄漏
            Ok(Some(BSTR::try_from(&path).map_err(super::e2s)?))
        }
    }

    pub(super) fn current_level(server: &IWbemServices) -> Result<Option<u8>, String> {
        let Some(obj) = query_first(server, "SELECT CurrentBrightness FROM WmiMonitorBrightness")?
        else {
            return Ok(None);
        };
        unsafe {
            let mut v = VARIANT::default();
            obj.Get(&BSTR::from("CurrentBrightness"), 0, &mut v, None, None)
                .map_err(super::e2s)?;
            // WMI 可能报 VT_UI1 / VT_I4，VariantToDouble 统一做数值归一
            let level = f64::try_from(&v).map_err(super::e2s)? as u8;
            Ok(Some(level))
        }
    }

    pub(super) fn set_level(server: &IWbemServices, level: u8) -> Result<(), String> {
        let Some(path) = first_path(server, "WmiMonitorBrightnessMethods")? else {
            return Err("未找到亮度控制实例（台式机或外接显示器不支持）/ No brightness instance found".into());
        };
        unsafe {
            let mut obj_out: Option<IWbemClassObject> = None;
            server
                .GetObject(&path, WBEM_GENERIC_FLAG_TYPE(0), None, Some(&mut obj_out), None)
                .map_err(super::e2s)?;
            let obj = obj_out.ok_or("GetObject returned no object")?;
            let mut in_sig: Option<IWbemClassObject> = None;
            obj.GetMethod(
                &BSTR::from("WmiSetBrightness"),
                0,
                &mut in_sig,
                std::ptr::null_mut(),
            )
            .map_err(super::e2s)?;
            let Some(in_obj) = in_sig else {
                return Err("WmiSetBrightness 签名获取失败".into());
            };
            // VT_UI1 Brightness + VT_I4 Timeout；CIM_EMPTY(0) 让 WMI 按 VARIANT 类型判定
            in_obj
                .Put(&BSTR::from("Brightness"), 0, &VARIANT::from(level), 0)
                .map_err(super::e2s)?;
            in_obj
                .Put(&BSTR::from("Timeout"), 0, &VARIANT::from(0i32), 0)
                .map_err(super::e2s)?;
            server
                .ExecMethod(
                    &path,
                    &BSTR::from("WmiSetBrightness"),
                    WBEM_GENERIC_FLAG_TYPE(0),
                    None,
                    Some(&in_obj),
                    None,
                    None,
                )
                .map_err(super::e2s)?;
            Ok(())
        }
    }
}

#[cfg(windows)]
#[tauri::command]
pub fn brightness_get() -> Result<BrightnessState, String> {
    with_mta(|| {
        let server = brightness_wmi::connect()?;
        match brightness_wmi::current_level(&server)? {
            Some(level) => Ok(BrightnessState { supported: true, level }),
            None => Ok(BrightnessState { supported: false, level: 0 }),
        }
    })
}

#[cfg(windows)]
#[tauri::command]
pub fn brightness_set(level: u8) -> Result<BrightnessState, String> {
    let level = level.clamp(5, 100);
    with_mta(|| {
        let server = brightness_wmi::connect()?;
        brightness_wmi::set_level(&server, level)?;
        Ok(BrightnessState { supported: true, level })
    })
}

#[cfg(not(windows))]
#[tauri::command]
pub fn brightness_get() -> Result<BrightnessState, String> {
    Err("unsupported platform".into())
}

#[cfg(not(windows))]
#[tauri::command]
pub fn brightness_set(_level: u8) -> Result<BrightnessState, String> {
    Err("unsupported platform".into())
}

// ---------------------------------------------------------------------------
// 非 Windows 平台占位（仅编译通过；发布目标为 Windows）
// ---------------------------------------------------------------------------

#[cfg(not(windows))]
#[tauri::command]
pub fn privacy_usage() -> Vec<DeviceUsage> {
    Vec::new()
}

#[cfg(not(windows))]
#[tauri::command]
pub fn audio_get() -> Result<AudioState, String> {
    Err("unsupported platform".into())
}

#[cfg(not(windows))]
#[tauri::command]
pub fn audio_set(_volume: f32, _muted: Option<bool>) -> Result<AudioState, String> {
    Err("unsupported platform".into())
}

#[cfg(not(windows))]
#[tauri::command]
pub fn wifi_get() -> Result<WifiState, String> {
    Err("unsupported platform".into())
}

#[cfg(not(windows))]
#[tauri::command]
pub fn bluetooth_get() -> Result<BluetoothState, String> {
    Err("unsupported platform".into())
}

#[cfg(not(windows))]
#[tauri::command]
pub fn bluetooth_set(_enabled: bool) -> Result<BluetoothState, String> {
    Err("unsupported platform".into())
}

#[cfg(not(windows))]
#[tauri::command]
pub fn bt_devices() -> Result<Vec<BtDevice>, String> {
    Err("unsupported platform".into())
}

#[cfg(not(windows))]
#[tauri::command]
pub fn wifi_scan() -> Result<Vec<WifiNetwork>, String> {
    Err("unsupported platform".into())
}

#[cfg(not(windows))]
#[tauri::command]
pub fn wifi_disconnect() -> Result<(), String> {
    Err("unsupported platform".into())
}

#[cfg(not(windows))]
#[tauri::command]
pub fn wifi_set(_enabled: bool) -> Result<WifiState, String> {
    Err("unsupported platform".into())
}

#[cfg(not(windows))]
#[tauri::command]
pub fn bt_connect(_id: String) -> Result<(), String> {
    Err("unsupported platform".into())
}

#[cfg(not(windows))]
#[tauri::command]
pub fn bt_disconnect(_id: String) -> Result<(), String> {
    Err("unsupported platform".into())
}

#[cfg(not(windows))]
#[tauri::command]
pub fn audio_devices() -> Result<Vec<AudioDeviceInfo>, String> {
    Err("unsupported platform".into())
}

#[cfg(not(windows))]
#[tauri::command]
pub fn audio_set_default(_device_id: String) -> Result<(), String> {
    Err("unsupported platform".into())
}
