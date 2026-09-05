import { ipc, type AudioDeviceInfo, type BatteryState, type BluetoothState, type BtDevice, type BrightnessState, type WifiNetwork } from "../../lib/ipc";
import { createStore, useStore } from "../../lib/store";

/**
 * 任务栏托盘区的硬件状态（M5 → 批次C）：
 * - 单一轮询源（Taskbar 挂载时启动，15s 周期，仅本机 Windows API，零网络）；
 * - 获取失败 = { kind:"error" }，UI 如实显示"状态未知"，不伪造状态；
 * - 音量写入成功后立即回写 store（不等待下一轮询）；
 * - 批次C：蓝牙设备 / 音频设备 / Wi-Fi 扫描结果为按需加载（面板展开或用户显式操作时），
 *   Wi-Fi 周边网络遵循规格 6.2.2 —— 默认不主动扫描，仅用户点击"扫描"时获取。
 */
export interface AudioState {
  volume: number;
  muted: boolean;
}
export interface WifiState {
  connected: boolean;
  ssid: string | null;
  signal: number | null;
  /** 批次E（规格 6.2.1）：无线电开关状态（null = 读取失败/无无线电）。 */
  radio_on: boolean | null;
}
export type { BluetoothState, BtDevice, WifiNetwork, AudioDeviceInfo, BatteryState, BrightnessState };
export type HwResult<T> = { kind: "ok"; value: T } | { kind: "error" };

interface HwStoreState {
  audio: HwResult<AudioState>;
  wifi: HwResult<WifiState>;
  bluetooth: HwResult<BluetoothState>;
  /** 按需：已配对蓝牙设备（面板展开时刷新）。 */
  btDevices: HwResult<BtDevice[]>;
  /** 按需：音频端点列表（面板展开时刷新）。 */
  audioDevices: HwResult<AudioDeviceInfo[]>;
  battery: HwResult<BatteryState>;
  /** 按需：屏幕亮度（supported=false = 不可调，如实显示）。 */
  brightness: HwResult<BrightnessState>;
  /** 用户最近一次"扫描"到的周边 Wi-Fi（null = 从未扫描，不伪造）。 */
  wifiNetworks: WifiNetwork[] | null;
  scanning: boolean;
}

export const hwStore = createStore<HwStoreState>({
  audio: { kind: "error" },
  wifi: { kind: "error" },
  bluetooth: { kind: "error" },
  btDevices: { kind: "error" },
  audioDevices: { kind: "error" },
  battery: { kind: "error" },
  brightness: { kind: "error" },
  wifiNetworks: null,
  scanning: false,
});

export function useHw<S>(sel: (s: HwStoreState) => S): S {
  return useStore(hwStore, sel);
}

export async function refreshHardware(): Promise<void> {
  const [a, w, b, bat] = await Promise.allSettled([
    ipc.audioGet(),
    ipc.wifiGet(),
    ipc.bluetoothGet(),
    ipc.batteryGet(),
  ]);
  hwStore.setState({
    audio: a.status === "fulfilled" ? { kind: "ok", value: a.value } : { kind: "error" },
    wifi: w.status === "fulfilled" ? { kind: "ok", value: w.value } : { kind: "error" },
    bluetooth: b.status === "fulfilled" ? { kind: "ok", value: b.value } : { kind: "error" },
    battery: bat.status === "fulfilled" ? { kind: "ok", value: bat.value } : { kind: "error" },
  });
}

/** 启动轮询（Taskbar mount 时调用），返回清理函数。 */
export function startHardwarePolling(): () => void {
  void refreshHardware();
  const id = window.setInterval(() => void refreshHardware(), 15000);
  return () => window.clearInterval(id);
}

export async function setAudio(volume: number, muted?: boolean): Promise<void> {
  const st = await ipc.audioSet(volume, muted);
  hwStore.setState({ audio: { kind: "ok", value: st } });
}

// ---------------------------------------------------------------------------
// 批次C 操作（规格 6.1-6.6）：全部由用户显式触发，失败如实抛出
// ---------------------------------------------------------------------------

/** 蓝牙无线电开关（规格 6.1）。 */
export async function toggleBluetooth(enabled: boolean): Promise<void> {
  const st = await ipc.bluetoothSet(enabled);
  hwStore.setState({ bluetooth: { kind: "ok", value: st } });
}

/** 刷新已配对蓝牙设备（面板展开时；规格 6.1.2）。 */
export async function loadBtDevices(): Promise<void> {
  try {
    const list = await ipc.btDevices();
    hwStore.setState({ btDevices: { kind: "ok", value: list } });
  } catch {
    hwStore.setState({ btDevices: { kind: "error" } });
  }
}

/** 用户点击"扫描" → 枚举周边 Wi-Fi（规格 6.2.2：默认不扫描）。 */
export async function scanWifi(): Promise<void> {
  if (hwStore.getState().scanning) return;
  hwStore.setState({ scanning: true });
  try {
    const nets = await ipc.wifiScan();
    hwStore.setState({ wifiNetworks: nets });
  } catch {
    hwStore.setState({ wifiNetworks: [] });
  } finally {
    hwStore.setState({ scanning: false });
  }
}

/** 断开当前 Wi-Fi（面板中显式点击）。 */
export async function disconnectWifi(): Promise<void> {
  await ipc.wifiDisconnect();
  await refreshHardware();
}

/** Wi-Fi 无线电开关（批次E 规格 6.2.1）。 */
export async function toggleWifi(enabled: boolean): Promise<void> {
  const st = await ipc.wifiSet(enabled);
  hwStore.setState({ wifi: { kind: "ok", value: st } });
}

/** 蓝牙设备连接/断开（批次E 规格 6.1.3：点击设备行切换；完成刷新列表）。 */
export async function switchBtDevice(id: string, connect: boolean): Promise<void> {
  if (connect) await ipc.btConnect(id);
  else await ipc.btDisconnect(id);
  await loadBtDevices();
}

/** 刷新音频端点列表（面板展开时；规格 6.3.1）。 */
export async function loadAudioDevices(): Promise<void> {
  try {
    const list = await ipc.audioDevices();
    hwStore.setState({ audioDevices: { kind: "ok", value: list } });
  } catch {
    hwStore.setState({ audioDevices: { kind: "error" } });
  }
}

/** 一键切换默认音频设备（规格 6.3.2：设备行点击即切换）。 */
export async function switchAudioDevice(deviceId: string): Promise<void> {
  await ipc.audioSetDefault(deviceId);
  await loadAudioDevices();
  const st = await ipc.audioGet();
  hwStore.setState({ audio: { kind: "ok", value: st } });
}

/** 亮度调节（规格 6.6.1；仅内置面板 supported=true）。 */
export async function setBrightness(level: number): Promise<void> {
  const st = await ipc.brightnessSet(level);
  hwStore.setState({ brightness: { kind: "ok", value: st } });
}

/** 按需读取亮度（面板展开时）。 */
export async function loadBrightness(): Promise<void> {
  try {
    const st = await ipc.brightnessGet();
    hwStore.setState({ brightness: { kind: "ok", value: st } });
  } catch {
    hwStore.setState({ brightness: { kind: "error" } });
  }
}
