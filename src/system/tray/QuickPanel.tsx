import { useEffect, useRef, useState } from "react";
import {
  Battery, BatteryCharging, Bluetooth, Lock, Mic, Moon, RefreshCw, Speaker,
  Volume2, VolumeX, Wifi, WifiOff, X,
} from "lucide-react";
import { useI18n } from "../../i18n";
import { CloseLight } from "../../components/CloseLight";
import type { QuickSection } from "../../state/uiStore";
import { pushToast } from "../../state/uiStore";
import { errMessage, ipc } from "../../lib/ipc";
import {
  clearNotifications,
  markAllRead,
  toggleDnd,
  useDnd,
  useNotifications,
  useUnreadCount,
} from "../../state/notifyStore";
import {
  disconnectWifi, loadAudioDevices, loadBtDevices, loadBrightness, refreshHardware,
  scanWifi, setAudio, setBrightness, switchAudioDevice, switchBtDevice, toggleBluetooth, toggleWifi, useHw,
} from "./hardware";

/**
 * 快捷面板 + 通知中心（M5 → 批次C，规格 6.1-6.6）：
 * - 快捷设置：蓝牙开关、勿扰（Ctrl+Shift+M）、电池、亮度（supported 才显示）；
 * - 网络（规格 6.2）：当前连接 + 断开；周边网络仅在用户点击"扫描"后列出（默认不扫描）；
 * - 蓝牙（规格 6.1）：无线电开关 + 已配对设备（连接中的置顶）；配对/连接管理交给 Windows；
 * - 音频（规格 6.3）：音量/静音 + 输出/输入设备点选切换；
 * - 通知中心：本会话本地通知历史，勿扰只静默横幅、历史照常记录。
 * 状态获取失败如实显示"状态未知"，浏览器 dev 无后端同样如实降级。
 */
export function QuickPanel(props: {
  open: boolean;
  section: QuickSection | null;
  onClose: () => void;
}): React.ReactElement | null {
  const { t } = useI18n();
  const wifi = useHw((s) => s.wifi);
  const bluetooth = useHw((s) => s.bluetooth);
  const audio = useHw((s) => s.audio);
  const btDevices = useHw((s) => s.btDevices);
  const audioDevices = useHw((s) => s.audioDevices);
  const battery = useHw((s) => s.battery);
  const brightness = useHw((s) => s.brightness);
  const wifiNetworks = useHw((s) => s.wifiNetworks);
  const scanning = useHw((s) => s.scanning);
  const dnd = useDnd();
  const items = useNotifications();

  const [bright, setBrightLocal] = useState<number | null>(null);
  const [flash, setFlash] = useState<string | null>(null);
  // 批次E（规格 6.1）：Wi-Fi 详情 — 本机 IPv4（面板展开时读取一次）
  const [ip, setIp] = useState<string | null>(null);
  const wifiRef = useRef<HTMLDivElement>(null);
  const btRef = useRef<HTMLDivElement>(null);
  const audioRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!props.open) return;
    markAllRead();
    // 面板展开时按需加载设备/亮度明细（15s 轮询之外的即时刷新）
    void refreshHardware();
    void loadBtDevices();
    void loadAudioDevices();
    void loadBrightness();
    void ipc.netIp().then(setIp).catch(() => setIp(null));
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === "Escape") {
        e.stopPropagation();
        props.onClose();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.open]);

  // 快捷键/托盘指定分区 → 滚动定位 + 高亮提示
  useEffect(() => {
    if (!props.open || !props.section) return;
    const el = props.section === "wifi" ? wifiRef.current
      : props.section === "bluetooth" ? btRef.current
      : audioRef.current;
    if (!el) return;
    el.scrollIntoView({ behavior: "smooth", block: "start" });
    setFlash(props.section);
    const id = window.setTimeout(() => setFlash(null), 1300);
    return () => window.clearTimeout(id);
  }, [props.open, props.section]);

  // 亮度滑杆本地值随后端刷新（拖动中不被覆盖）
  useEffect(() => {
    if (bright !== null) return;
    if (brightness.kind === "ok" && brightness.value.supported) {
      setBrightLocal(brightness.value.level);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [brightness]);

  if (!props.open) return null;

  const commitBrightness = (): void => {
    if (bright === null) return;
    void setBrightness(bright).catch((e) =>
      pushToast("error", t("brightness"), errMessage(e).message),
    );
  };

  const wifiConnected = wifi.kind === "ok" && wifi.value.connected;

  return (
    <div
      className="qp-overlay"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) props.onClose();
      }}
    >
      <div className="quick-panel" role="dialog" aria-label={t("trayQuick")}>
        <div className="qp-head">
          <CloseLight onClose={props.onClose} />
          <span>{t("trayQuick")}</span>
          <button type="button" className="qp-close" aria-label={t("close")} onClick={props.onClose}>
            <X size={14} />
          </button>
        </div>

        {/* 快捷设置（规格 6.5.2）：蓝牙开关 / 勿扰 / 电池 + 亮度 */}
        <div className="qp-tiles">
          <button
            type="button"
            className={`qp-tile${bluetooth.kind === "ok" && bluetooth.value.enabled ? " on" : ""}`}
            disabled={bluetooth.kind !== "ok" || !bluetooth.value.available}
            aria-pressed={bluetooth.kind === "ok" && bluetooth.value.enabled}
            title={t("trayBluetooth")}
            onClick={() => {
              if (bluetooth.kind !== "ok") return;
              void toggleBluetooth(!bluetooth.value.enabled)
                .then(() => { void loadBtDevices(); })
                .catch((e) => pushToast("error", t("trayBluetooth"), errMessage(e).message));
            }}
          >
            <Bluetooth size={17} strokeWidth={1.8} />
            <span>{t("trayBluetooth")}</span>
          </button>
          <button
            type="button"
            className={`qp-tile${dnd ? " on" : ""}`}
            aria-pressed={dnd}
            title={t("dndTitle")}
            onClick={() => toggleDnd()}
          >
            <Moon size={17} strokeWidth={1.8} />
            <span>{t("dndTitle")}</span>
          </button>
          {battery.kind === "ok" && battery.value.hasBattery && (
            <div
              className={`qp-tile qp-static${battery.value.acOnline ? " on" : ""}`}
              title={t("battery")}
            >
              {battery.value.acOnline ? (
                <BatteryCharging size={17} strokeWidth={1.8} />
              ) : (
                <Battery size={17} strokeWidth={1.8} />
              )}
              <span>
                {battery.value.percent != null
                  ? `${battery.value.percent}%${battery.value.acOnline ? ` · ${t("batteryCharging")}` : ""}`
                  : t("battery")}
              </span>
            </div>
          )}
        </div>

        {brightness.kind === "ok" && brightness.value.supported && bright !== null && (
          <div className="qp-bright">
            <span className="qp-row-name">{t("brightness")}</span>
            <input
              type="range"
              min={5}
              max={100}
              value={bright}
              aria-label={t("brightness")}
              onChange={(e) => setBrightLocal(Number(e.target.value))}
              onPointerUp={commitBrightness}
              onKeyUp={commitBrightness}
            />
            <span className="qp-row-value">{bright}%</span>
          </div>
        )}

        {/* 网络（规格 6.2）：当前连接 + 断开；扫描仅用户显式点击（6.2.2） */}
        <div ref={wifiRef} className={`qp-sec${flash === "wifi" ? " flash" : ""}`}>
          <div className="qp-sec-head">
            {wifiConnected ? <Wifi size={15} strokeWidth={1.8} /> : <WifiOff size={15} strokeWidth={1.8} className={wifi.kind === "ok" ? "dim" : "qp-unknown"} />}
            <span>{t("trayNetwork")}</span>
            <span className="qp-sec-val dim">
              {wifi.kind === "ok"
                ? wifi.value.connected
                  ? `${wifi.value.ssid ?? t("wifiConnected")}${wifi.value.signal != null ? ` · ${wifi.value.signal}%` : ""}`
                  : t("wifiNotConnected")
                : t("hwUnknown")}
            </span>
            {/* 批次E（规格 6.2.1）：Wi-Fi 无线电开关 */}
            {wifi.kind === "ok" && wifi.value.radio_on !== null && (
              <button
                type="button"
                className="qp-sec-act"
                title={t("wifiRadio")}
                onClick={() => {
                  void toggleWifi(!wifi.value.radio_on).catch((e) =>
                    pushToast("error", t("trayNetwork"), errMessage(e).message),
                  );
                }}
              >
                {wifi.value.radio_on ? t("btOff") : t("btOn")}
              </button>
            )}
            {wifiConnected && (
              <button
                type="button"
                className="qp-sec-act"
                onClick={() => {
                  void disconnectWifi().catch((e) =>
                    pushToast("error", t("trayNetwork"), errMessage(e).message),
                  );
                }}
              >
                {t("wifiDisconnectAction")}
              </button>
            )}
          </div>
          <div className="qp-sec-body">
            {/* 批次E（规格 6.1）：Wi-Fi 详情 — 本机 IPv4 */}
            {wifiConnected && ip && (
              <p className="dim small qp-hint" title={t("wifiIp")}>
                {t("wifiIp")}: {ip}
              </p>
            )}
            <button
              type="button"
              className="qp-sec-act"
              disabled={scanning}
              onClick={() => void scanWifi()}
            >
              <RefreshCw size={12} className={scanning ? "spin" : undefined} />
              {scanning ? t("wifiScanning") : t("wifiScanAction")}
            </button>
            {wifiNetworks === null ? (
              <p className="dim small qp-hint">{t("wifiScanHint")}</p>
            ) : wifiNetworks.length === 0 ? (
              <p className="dim small qp-hint">{t("wifiNoneFound")}</p>
            ) : (
              <div className="qp-net-list">
                {wifiNetworks.map((n) => (
                  <div key={n.ssid} className="qp-net-row">
                    <Wifi size={14} strokeWidth={1.7} className="dim" />
                    <span className="qp-net-name" title={n.ssid}>{n.ssid}</span>
                    <span className="qp-row-value">{n.signal}%</span>
                    {n.secured && <Lock size={12} strokeWidth={1.8} className="dim" />}
                  </div>
                ))}
              </div>
            )}
          </div>
        </div>

        {/* 蓝牙（规格 6.1）：开关在快捷设置；此处为已配对设备（连接中置顶） */}
        <div ref={btRef} className={`qp-sec${flash === "bluetooth" ? " flash" : ""}`}>
          <div className="qp-sec-head">
            <Bluetooth size={15} strokeWidth={1.8} className={bluetooth.kind === "ok" && bluetooth.value.enabled ? "" : "dim"} />
            <span>{t("trayBluetooth")}</span>
            <span className="qp-sec-val dim">
              {bluetooth.kind === "ok"
                ? bluetooth.value.available
                  ? bluetooth.value.enabled ? t("btOn") : t("btOff")
                  : t("btNone")
                : t("hwUnknown")}
            </span>
          </div>
          <div className="qp-sec-body">
            {btDevices.kind === "ok" && btDevices.value.length > 0 ? (
              <div className="qp-net-list">
                {btDevices.value.map((d) => (
                  <button
                    key={d.id || d.name}
                    type="button"
                    className={`qp-net-row as-btn${d.connected ? " connected" : ""}`}
                    title={
                      d.connected
                        ? `${d.name} · ${t("btDisconnectAction")}`
                        : `${d.name} · ${t("btConnectAction")}`
                    }
                    onClick={() => {
                      if (!d.id) return;
                      void switchBtDevice(d.id, !d.connected).catch((err) =>
                        pushToast("error", t("trayBluetooth"), errMessage(err).message),
                      );
                    }}
                  >
                    <span className={`qp-dot${d.connected ? " on" : ""}`} aria-hidden />
                    <Bluetooth size={14} strokeWidth={1.7} className="dim" />
                    <span className="qp-net-name" title={d.name}>{d.name}</span>
                    <span className="qp-row-value">{d.connected ? t("btConnected") : t("btConnectAction")}</span>
                  </button>
                ))}
              </div>
            ) : (
              <p className="dim small qp-hint">
                {btDevices.kind === "ok" ? t("btNoDevices") : t("hwUnknown")}
              </p>
            )}
          </div>
        </div>

        {/* 音频（规格 6.3）：音量/静音 + 输出/输入设备点选切换 */}
        <div ref={audioRef} className={`qp-sec${flash === "audio" ? " flash" : ""}`}>
          <div className="qp-sec-head">
            <button
              type="button"
              className="qp-mute"
              aria-label={audio.kind === "ok" && audio.value.muted ? t("unmuteAction") : t("muteAction")}
              disabled={audio.kind !== "ok"}
              onClick={() => {
                if (audio.kind !== "ok") return;
                void setAudio(audio.value.volume, !audio.value.muted).catch((e) =>
                  pushToast("error", t("trayAudio"), errMessage(e).message),
                );
              }}
            >
              {audio.kind === "ok" && audio.value.muted ? (
                <VolumeX size={15} strokeWidth={1.8} />
              ) : (
                <Volume2 size={15} strokeWidth={1.8} />
              )}
            </button>
            <span>{t("volume")}</span>
            {audio.kind === "ok" && audio.value.muted && <span className="qp-sec-val dim">{t("muted")}</span>}
          </div>
          <div className="qp-sec-body">
            {audio.kind === "ok" ? (
              <input
                type="range"
                min={0}
                max={100}
                value={Math.round(audio.value.volume * 100)}
                aria-label={t("volume")}
                onChange={(e) => {
                  const v = Number(e.target.value) / 100;
                  void setAudio(v).catch((err) =>
                    pushToast("error", t("trayAudio"), errMessage(err).message),
                  );
                }}
              />
            ) : (
              <span className="qp-row-value qp-unknown">{t("hwUnknown")}</span>
            )}
            {(["render", "capture"] as const).map((kind) => {
              const list = audioDevices.kind === "ok" ? audioDevices.value.filter((d) => d.kind === kind) : [];
              return (
                <div key={kind} className="qp-dev-group">
                  <span className="qp-row-name dim small">
                    {kind === "render" ? <Speaker size={12} /> : <Mic size={12} />}
                    {kind === "render" ? t("audioOutput") : t("audioInput")}
                  </span>
                  {audioDevices.kind !== "ok" ? (
                    <p className="dim small qp-hint">{t("hwUnknown")}</p>
                  ) : list.length === 0 ? (
                    <p className="dim small qp-hint">{t("audioNoDevice")}</p>
                  ) : (
                    list.map((d) => (
                      <button
                        key={d.id}
                        type="button"
                        className={`qp-dev-row${d.default ? " sel" : ""}`}
                        title={d.name}
                        onClick={() => {
                          if (d.default) return;
                          void switchAudioDevice(d.id).catch((err) =>
                            pushToast("error", t("audioSwitchFail"), errMessage(err).message),
                          );
                        }}
                      >
                        <span className={`qp-dot${d.default ? " on" : ""}`} aria-hidden />
                        <span className="qp-net-name">{d.name}</span>
                      </button>
                    ))
                  )}
                </div>
              );
            })}
          </div>
        </div>

        <div className="qp-notif-head">
          <span>{t("notifyCenter")}</span>
          {items.length > 0 && (
            <button type="button" className="qp-clear" onClick={() => clearNotifications()}>
              {t("clearAll")}
            </button>
          )}
        </div>
        <div className="qp-notif-list">
          {items.length === 0 ? (
            <p className="dim small qp-empty">{t("notifyEmpty")}</p>
          ) : (
            // 批次E（规格 6.4）：按类别分组（隐私/硬件/系统），新到旧
            ["privacy", "hardware", "system"].map((kind) => {
              const group = [...items].reverse().filter((n) => n.kind === kind);
              if (group.length === 0) return null;
              const label = kind === "privacy" ? t("notifyGroupPrivacy") : kind === "hardware" ? t("notifyGroupHardware") : t("notifyGroupSystem");
              return (
                <div key={kind} className="qp-notif-group">
                  <p className="dim small qp-group-label">{label}</p>
                  {group.map((n) => (
                    <div key={n.id} className={`qp-notif kind-${n.kind}`}>
                      <span className="qp-notif-title">{n.title}</span>
                      {n.body && <span className="qp-notif-body dim small">{n.body}</span>}
                      <span className="qp-notif-time dim small">
                        {new Date(n.time).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" })}
                      </span>
                    </div>
                  ))}
                </div>
              );
            })
          )}
        </div>
      </div>
    </div>
  );
}

/** 通知中心未读数（Taskbar 铃铛角标用）。 */
export function useNotifyBadge(): number {
  return useUnreadCount();
}
