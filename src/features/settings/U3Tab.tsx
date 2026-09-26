/**
 * I 通用域 · AI-U3 — 设置中心「通用体验」标签页（F501-F550 全量面板）。
 *
 * 面板纪律（与 J1 同源）：
 * - 名称 + 一句话说明 + 调节控件三件套齐全（F474 同源）；
 * - 默认档 = 主册判据默认；全部改动即时生效（u3Store 订阅制广播）；
 * - 高密度纵深排布：九个分组区 + v4 引擎实验室区、五十项各就各位（F302 乙基线）；
 * - F550 组内嵌九域自检引擎的实时执行结果（锚点可执行性证据）。
 */

import { useEffect, useMemo, useState } from "react";
import { u3Store, U3_DEFAULTS, type U3Section } from "../u3/u3store";
import { U3LabSection, U3WalkCheckSection, U3ExpLogSection } from "../u3/U3Lab";
import {
  GRID_DENSITY_PX, BRIGHTNESS_SAMPLES, pickIconTextColor, wrapIconLabel, iconTextLayers,
  effectiveGrid, resnapToGrid, type GridDensity,
} from "../u3/deskicons";
import {
  pinShapeOk, pinCooldownMs, setPin, GUEST_SESSION_CAP_MIN,
  GUEST_SANDBOX_AXES, guestCapability, SCREENSHOT_CHANNELS, lockScreenShotPolicy, redactRects, REDACT_NOTE,
} from "../u3/locksec";
import { shredPlan, shredWarningItems, ONE_CRYPT_KEEP_NOTE, CLIP_WIPE_HOTKEY, CLIP_SECRET_PROMPT_MS, SHOT_HISTORY_CAP } from "../u3/filesec";
import {
  CTRL_FIND_HOLD_MS, CTRL_COMBO_EXEMPT, SV_EDGE_PX, CAPS_TONE_HZ, titleBarAction,
  TRAIL_LEN_MS, TYPE_HIDE_OPACITY, TYPE_HIDE_RESUME_MS, type TrailLen,
} from "../u3/pointerfx";
import {
  EXPLORER_HOME_NOTES, type ExplorerHome, shotFileName, keycardModel,
  statusBarSegments, TREE_COMMAND_KEYS, TREE_SYNC_FOCUS_NOTE, SHOT_TARGET_LABELS, type ShotTarget,
} from "../u3/explorerx";
import {
  UNDO_BIN_WINDOW_MS, UNDO_BIN_EXTEND_MS, SPACE_CHECK_BUFFER_PCT, COPY_VERIFY_AUTO_ABOVE,
  shortfallMessage,
} from "../u3/copyops";
import {
  DEVICE_VOLUME_CAP, NEW_DEVICE_DEFAULT, NOTIFY_VOLUME_DEFAULT,
  BT_LOW_PCT, BT_LOW_THROTTLE_MS, BALANCE_SW_DELAY_MS,
} from "../u3/sysdev";
import {
  IME_SCHEMES, CAPS_LONG_PRESS_MS, capsVerdict, bannerStackDirection, OSD_SEPARATE_NOTE,
  PEEK_OPACITY, PEEK_FADE_MS, LOCK_SHAKE_MS, UNLOCK_PATH,
  type BannerPos, type ImeScheme, type TaskbarSlot, winNumberResolve,
} from "../u3/winkeys";
import {
  MEM_DIAG_EST_MINUTES, NET_RESET_CLEAR_LIST, NET_RESET_COUNTDOWN_SEC, CLICK_LOCK_THRESHOLD_MS,
} from "../u3/sysdev";
void DEVICE_VOLUME_CAP;
import { humanBytes } from "../u3/explorerx";
import { hoverDateLine } from "../u3/clockcal";
import { anchorRuntime, U3_ANCHOR_MIN_CHECKS } from "../u3/anchor";
import { SectionCard } from "./MouseJ1Panels";
import "../../styles/u3.css";

type Cfg = Record<string, unknown>;

function useSection<T extends Cfg>(section: U3Section): [T, (patch: Partial<T>) => void] {
  const [, tick] = useState(0);
  useEffect(() => u3Store.subscribe(() => tick((v) => v + 1)), []);
  const value = { ...(U3_DEFAULTS[section] as T), ...(u3Store.get(section) as unknown as T) };
  return [value, (patch) => u3Store.set(section, patch as Record<string, unknown>)];
}

/* ------------------------------- 通用行与控件 ------------------------------- */

function Row(props: { fno: string; name: string; desc: string; children: React.ReactNode }) {
  return (
    <div className="u3-row">
      <div>
        <div className="u3-name"><span className="fno">{props.fno}</span>{props.name}</div>
        <div className="u3-desc">{props.desc}</div>
      </div>
      <div className="u3-ctl" style={{ gridColumn: "2 / span 2" }}>{props.children}</div>
    </div>
  );
}

function Toggle(props: { checked: boolean; onChange: (v: boolean) => void; label?: string }) {
  return (
    <label style={{ display: "inline-flex", gap: 6, alignItems: "center", cursor: "pointer", fontSize: 12 }}>
      <input type="checkbox" checked={props.checked} onChange={(e) => props.onChange(e.target.checked)} />
      {props.label && <span>{props.label}</span>}
    </label>
  );
}

function Pick<T extends string>(props: { value: T; options: Array<{ v: T; label: string }>; onChange: (v: T) => void }) {
  return (
    <select value={props.value} onChange={(e) => props.onChange(e.target.value as T)} style={{ fontSize: 12, padding: "4px 8px" }}>
      {props.options.map((o) => <option key={o.v} value={o.v}>{o.label}</option>)}
    </select>
  );
}

function Num(props: { value: number; min: number; max: number; step?: number; onChange: (v: number) => void; suffix?: string }) {
  return (
    <span style={{ display: "inline-flex", gap: 4, alignItems: "center", fontSize: 12 }}>
      <input type="number" value={props.value} min={props.min} max={props.max} step={props.step ?? 1}
        onChange={(e) => props.onChange(Number(e.target.value))} style={{ width: 72, fontSize: 12, padding: "4px 6px" }} />
      {props.suffix && <span className="u3-badge">{props.suffix}</span>}
    </span>
  );
}

/* ------------------------------- 面板 ------------------------------- */

export function U3Tab(): React.ReactElement {
  return (
    <div className="u3-panel">
      <DesktopGroup />
      <LockSecGroup />
      <FileSecGroup />
      <PointerFxGroup />
      <ExplorerGroup />
      <CopyOpsGroup />
      <WinKeysGroup />
      <SysDevGroup />
      <ClockAnchorGroup />
      <U3LabSection />
      <U3WalkCheckSection />
      <U3ExpLogSection />
    </div>
  );
}

/* ------- 组一：桌面图标（F501-F503） ------- */

function DesktopGroup(): React.ReactElement {
  const [iconRead, setIconRead] = useSection<Cfg>("iconRead");
  const [iconWrap, setIconWrap] = useSection<Cfg>("iconWrap");
  const [grid, setGrid] = useSection<Cfg>("gridDensity");

  const mode = iconRead.mode as "auto" | "light" | "dark";
  const demoLuma = 0.62; // 样张壁纸亮度（花壁纸亮区）
  const picked = mode === "auto" ? pickIconTextColor(demoLuma) : mode === "light" ? "light" : "dark";
  const layers = iconTextLayers(picked);
  const wrapDemo = wrapIconLabel("项目总结报告最终版本-Submit v3");
  const gridNow = effectiveGrid(grid as never);

  const snapDemo = useMemo(() => {
    const pts = [{ x: 10, y: 6 }, { x: 30, y: 12 }, { x: 88, y: 20 }, { x: 140, y: 84 }];
    return resnapToGrid(pts, { colPx: 96, rowPx: 96 }, gridNow);
  }, [gridNow.colPx, gridNow.rowPx]);

  return (
    <SectionCard title="桌面图标" f="F501-F503" defaultOpen>
      <Row fno="F501" name="图标文字可读性" desc="深浅壁纸自动选字色（亮度分析）+ 柔投影 40%/模糊 2px + 选中胶囊底衬">
        <Pick value={mode} options={[{ v: "auto", label: "自动（按壁纸亮度）" }, { v: "light", label: "浅字（深壁纸）" }, { v: "dark", label: "深字（亮壁纸）" }]} onChange={(v) => setIconRead({ mode: v })} />
        <span className="u3-icon-label-demo is-selected" style={layers}>{picked === "light" ? "白字样张" : "黑字样张"}</span>
        <span className="u3-stat">五档样张 <b>{BRIGHTNESS_SAMPLES.map((b) => pickIconTextColor(b) === "light" ? "白" : "黑").join("·")}</b></span>
      </Row>
      <Row fno="F502" name="图标文字两行封顶" desc="每行约 8 全角字符；省略号全文三路可达（Tooltip/重命名/属性）；中英不断词">
        <Toggle checked={!!iconWrap.centerAlign} onChange={(v) => setIconWrap({ centerAlign: v })} label="水平居中" />
        <span className="u3-stat">样张 <b>{wrapDemo.lines[0]} / {wrapDemo.lines[1] || "␣"}{wrapDemo.truncated ? "（截断）" : ""}</b></span>
      </Row>
      <Row fno="F503" name="图标网格密度" desc={`三档格距 96/80/64px；自定义按 8px 步进；切换按最近格吸附重排（当前 ${gridNow.colPx}×${gridNow.rowPx}px）`}>
        <Pick
          value={grid.density as GridDensity}
          options={[
            { v: "loose", label: `宽松 ${GRID_DENSITY_PX.loose}px` },
            { v: "standard", label: `标准 ${GRID_DENSITY_PX.standard}px` },
            { v: "compact", label: `紧凑 ${GRID_DENSITY_PX.compact}px` },
            { v: "custom" as GridDensity, label: "自定义…" },
          ]}
          onChange={(v) => setGrid({ density: v })}
        />
        {(grid.density as string) === "custom" && (<>
          <Num value={grid.customColPx as number} min={48} max={200} step={8} suffix="列距" onChange={(v) => setGrid({ customColPx: v })} />
          <Num value={grid.customRowPx as number} min={48} max={200} step={8} suffix="行距" onChange={(v) => setGrid({ customRowPx: v })} />
        </>)}
        <span className="u3-stat">吸附样张 <b>{snapDemo.map((p) => `${Math.round(p.x / gridNow.colPx)},${Math.round(p.y / gridNow.rowPx)}`).join(" → ")}</b></span>
      </Row>
    </SectionCard>
  );
}

/* ------- 组二：锁屏与安全（F504-F508） ------- */

function LockSecGroup(): React.ReactElement {
  const [pin, setPinCfg] = useSection<Cfg>("pinUnlock");
  const [bt, setBt] = useSection<Cfg>("btLock");
  const [guest, setGuest] = useSection<Cfg>("guestMode");
  const [shield, setShield] = useSection<Cfg>("lockShield");
  const [appShield, setAppShield] = useSection<Cfg>("appShield");
  const [pinDraft, setPinDraft] = useState("");
  const [pinMsg, setPinMsg] = useState("");

  const shotDemo = redactRects(
    { demo: { x: 12, y: 12, w: 45, h: 48 } },
    { x: 0, y: 0, w: 100, h: 72 },
  );

  return (
    <SectionCard title="锁屏与安全" f="F504-F508">
      <Row fno="F504" name="PIN 快速解锁" desc={`4-6 位数字本机验证；错 5 次冷却 30s 起翻倍；冷却期回退密码；解锁全链 <1.5s`}>
        <Toggle checked={!!pin.enabled} onChange={(v) => setPinCfg({ enabled: v })} label="启用" />
        <input value={pinDraft} placeholder="输入 4-6 位数字" inputMode="numeric" maxLength={6}
          onChange={(e) => setPinDraft(e.target.value.replace(/\D/g, ""))}
          style={{ width: 110, fontSize: 12, padding: "4px 6px" }} />
        <button type="button" className="j1x-btn" onClick={() => {
          if (!pinShapeOk(pinDraft)) { setPinMsg("PIN 必须为 4-6 位数字"); return; }
          setPin(pinDraft); setPinCfg({ length: pinDraft.length }); setPinMsg("PIN 已更新（即时生效）"); setPinDraft("");
        }}>保存 PIN</button>
        <button type="button" className="j1x-btn j1x-btn--danger" onClick={() => { setPin(""); setPinMsg("PIN 已废除——回退密码登录"); }}>废除</button>
        {pinMsg && <span className="u3-stat">{pinMsg}</span>}
        <span className="u3-stat">冷却表 <b>{[5, 6, 7, 8].map((n) => `${(pinCooldownMs(n) / 1000) | 0}s`).join("→")}</b></span>
      </Row>
      <Row fno="F505" name="蓝牙动态锁" desc="绑定蓝牙钥匙设备；信号消失 30s±5s 自动锁屏；短暂波动 <10s 不锁；与闲置锁屏双保险">
        <Toggle checked={!!bt.enabled} onChange={(v) => setBt({ enabled: v })} label="启用" />
        <span className="u3-badge">🔒 钥匙设备标记：{String(bt.keyDeviceId || "未绑定")}</span>
        <Num value={bt.awaySeconds as number} min={25} max={35} suffix="秒触发" onChange={(v) => setBt({ awaySeconds: v })} />
      </Row>
      <Row fno="F506" name="访客模式" desc={`沙盒四隔离（文件只读/设置拒/权限拒/共享拒）；退出全清；会话上限 ${GUEST_SESSION_CAP_MIN} 分钟提示`}>
        <Toggle checked={!!guest.entryOpen} onChange={(v) => setGuest({ entryOpen: v })} label="锁屏显示访客入口" />
        <span className="u3-stat">四轴 <b>{GUEST_SANDBOX_AXES.map((a) => `${a}=${guestCapability(a)}`).join(" · ")}</b></span>
      </Row>
      <Row fno="F507" name="锁屏防截图" desc="锁屏态四通道（PrtSc/截图工具/录屏/第三方 API）全拒；纯黑帧或直接拒绝（文档化）；非锁屏零开销">
        <Pick value={shield.mode as string} options={[{ v: "black-frame", label: "纯黑帧（防时间侧信道）" }, { v: "deny", label: "直接拒绝" }]} onChange={(v) => setShield({ mode: v })} />
        <span className="u3-stat">通道 <b>{SCREENSHOT_CHANNELS.length}</b> 锁屏全拒 <b>{SCREENSHOT_CHANNELS.every((c) => !lockScreenShotPolicy(true, shield.mode as never, c).allowed) ? "✓" : "✗"}</b></span>
      </Row>
      <Row fno="F508" name="应用防截标记" desc="vxapp 清单声明防截窗 → 截图/录屏该区域输出黑块（几何精确对齐）；未标记应用零影响">
        <span className="u3-redact-demo">
          <span className="redact">防截区</span>
        </span>
        <span className="u3-stat">样张黑块 <b>{shotDemo.length}</b> 块 · {REDACT_NOTE}</span>
        <Toggle checked={Object.keys(appShield.markers ?? {}).length > 0} onChange={(v) => setAppShield({ markers: v ? { "demo-window": { x: 0, y: 0, w: 400, h: 300 } } : {} })} label="样例标记" />
      </Row>
    </SectionCard>
  );
}

/* ------- 组三：文件安全（F509-F512） ------- */

function FileSecGroup(): React.ReactElement {
  const [shred, setShred] = useSection<Cfg>("shred");
  const [crypt, setCrypt] = useSection<Cfg>("oneCrypt");
  const [clip, setClip] = useSection<Cfg>("clipWipe");
  const [shots, setShots] = useSection<Cfg>("shotHistory");
  const [target, setTarget] = useSection<Cfg>("shotTarget");

  const plan = shredPlan("wear-leveling", ["a.txt"]);
  const demoNames = ["合同扫描件.pdf", "密码清单.txt"];
  const warn = shredWarningItems(demoNames);

  return (
    <SectionCard title="文件安全" f="F509-F512">
      <Row fno="F509" name="文件粉碎" desc="右键「显示更多选项」折叠入口；三重警示默认焦点取消；介质不支持覆写时诚实标注">
        <Num value={shred.passes as number} min={1} max={7} suffix="覆写遍数" onChange={(v) => setShred({ passes: v })} />
        <span className="u3-badge warn">{plan.warning || "介质支持覆写"}</span>
        <span className="u3-stat">警示样张 <b>{warn.count}</b> 项 · {warn.irrecoverable} · 焦点={warn.defaultFocus}</span>
      </Row>
      <Row fno="F510" name="单文件加密 .vxcrypt" desc="PBKDF2(600k)+AES-256-GCM；浏览不解盘（临时视图关闭即无痕）；密码丢了就是真丢了">
        <Toggle checked={!!crypt.keepOriginal} onChange={(v) => setCrypt({ keepOriginal: v })} label="加密后保留原文件" />
        <span className="u3-badge">{ONE_CRYPT_KEEP_NOTE}</span>
      </Row>
      <Row fno="F511" name="剪贴板一键清空" desc={`热键 ${CLIP_WIPE_HOTKEY}；当前+历史全清；确认框列条数；敏感复制后提示条 5s`}>
        <span className="u3-badge">{CLIP_WIPE_HOTKEY}</span>
        <Num value={(clip.afterSecretPromptMs as number) / 1000} min={3} max={10} suffix="秒提示驻留" onChange={(v) => setClip({ afterSecretPromptMs: v * 1000 })} />
        <span className="u3-stat">提示时序 <b>{(Number(clip.afterSecretPromptMs) / 1000).toFixed(0)}s</b>（判据 {CLIP_SECRET_PROMPT_MS / 1000}s）</span>
      </Row>
      <Row fno="F512" name="截图历史" desc={`最近 ${SHOT_HISTORY_CAP} 张缩略条；临时/已保存两态；未保存关机即失（通知一次）；重编辑/另存链路`}>
        <Toggle checked={!!shots.purgeOnShutdown} onChange={(v) => setShots({ purgeOnShutdown: v })} label="关机清理临时截图" />
      </Row>
      <Row fno="F521" name="截图保存位置" desc="图片/截图目录（默认）/桌面/每次询问；自动命名「截图 2026-09-25_1430」可加前缀；历史另存同落点">
        <Pick value={target.target as ShotTarget}
          options={(Object.keys(SHOT_TARGET_LABELS) as ShotTarget[]).map((t) => ({ v: t, label: SHOT_TARGET_LABELS[t] }))}
          onChange={(v) => setTarget({ target: v })} />
        <input value={target.prefix as string} onChange={(e) => setTarget({ prefix: e.target.value })} style={{ width: 90, fontSize: 12, padding: "4px 6px" }} />
        <span className="u3-stat">命名 <b>{shotFileName(String(target.prefix || "截图"), new Date(2026, 8, 25, 14, 30), new Set())}</b></span>
      </Row>
    </SectionCard>
  );
}

/* ------- 组四：指针与提示（F513/F514/F519/F520/F522/F523） ------- */

function PointerFxGroup(): React.ReactElement {
  const [ctrl, setCtrl] = useSection<Cfg>("ctrlFind");
  const [sv, setSv] = useSection<Cfg>("soundLight");
  const [caps, setCaps] = useSection<Cfg>("capsSound");
  const [mid, setMid] = useSection<Cfg>("midMinimize");
  const [trail, setTrail] = useSection<Cfg>("pointerTrail");
  const [hide, setHide] = useSection<Cfg>("typeHide");

  return (
    <SectionCard title="指针与提示" f="F513·F514·F519·F520·F522·F523">
      <Row fno="F513" name="Ctrl 定位指针" desc={`按住 Ctrl ${CTRL_FIND_HOLD_MS / 1000}s：三轮同心涟漪 1.5s；组合键豁免（Ctrl+C 等 ${CTRL_COMBO_EXEMPT.length} 例 0 误触）；多屏定位准确`}>
        <Toggle checked={!!ctrl.enabled} onChange={(v) => setCtrl({ enabled: v })} label="启用（默认开）" />
      </Row>
      <Row fno="F514" name="声音视觉提示" desc={`通知蓝/警告黄/电量红——边缘 ${SV_EDGE_PX}px 光带 2 次脉冲；逐事件开关；勿扰档镜像（仅声=只闪不响）`}>
        <Toggle checked={!!sv.notify} onChange={(v) => setSv({ notify: v })} label="通知" />
        <Toggle checked={!!sv.warn} onChange={(v) => setSv({ warn: v })} label="警告" />
        <Toggle checked={!!sv.battery} onChange={(v) => setSv({ battery: v })} label="电量" />
        <button type="button" className="j1x-btn" onClick={() => window.dispatchEvent(new CustomEvent("vx-sound-event", { detail: { kind: "notify", disturb: "normal" } }))}>试闪</button>
      </Row>
      <Row fno="F519" name="大写锁定提示音" desc={`开=高双音 ${CAPS_TONE_HZ.on.join("/")}Hz、关=低双音 ${CAPS_TONE_HZ.off.join("/")}Hz；默认关；与视觉角标互补`}>
        <Toggle checked={!!caps.enabled} onChange={(v) => setCaps({ enabled: v })} label="启用（默认关）" />
      </Row>
      <Row fno="F520" name="标题栏中键最小化" desc="三义分流：中键收起/双击最大化/右键菜单——按按键类型天然分流；不喜可关">
        <Toggle checked={!!mid.enabled} onChange={(v) => setMid({ enabled: v })} label="启用" />
        <span className="u3-stat">分流矩阵 <b>中={titleBarAction("middle", true)} 双={titleBarAction("double", true)} 右={titleBarAction("right", true)}</b></span>
      </Row>
      <Row fno="F522" name="指针轨迹显示" desc={`渐隐轨迹三档 ${Object.entries(TRAIL_LEN_MS).map(([k, v]) => `${k}=${v}ms`).join(" / ")}；F335 优先平面渲染；默认关`}>
        <Toggle checked={!!trail.enabled} onChange={(v) => setTrail({ enabled: v })} label="启用（默认关）" />
        <Pick value={trail.length as TrailLen} options={[{ v: "short", label: "短 200ms" }, { v: "medium", label: "中 400ms" }, { v: "long", label: "长 600ms" }]} onChange={(v) => setTrail({ length: v })} />
      </Row>
      <Row fno="F523" name="打字时隐藏指针" desc={`键盘输入开始 <100ms 淡出至 ${TYPE_HIDE_OPACITY * 100}%；停止 ${TYPE_HIDE_RESUME_MS / 1000}s 或动鼠标即回；触屏豁免；默认关`}>
        <Toggle checked={!!hide.enabled} onChange={(v) => setHide({ enabled: v })} label="启用（默认关）" />
      </Row>
    </SectionCard>
  );
}

/* ------- 组五：资源管理器（F515/F517/F525/F526/F527/F528） ------- */

function ExplorerGroup(): React.ReactElement {
  const [fr, setFr] = useSection<Cfg>("findReplace");
  const [home, setHome] = useSection<Cfg>("explorerHome");
  const [keycard, setKeycard] = useSection<Cfg>("keycardExport");
  const [bar, setBar] = useSection<Cfg>("statusBar");
  const [tree, setTree] = useSection<Cfg>("treeCollapse");
  const [sync, setSync] = useSection<Cfg>("treeSync");

  const card = keycardModel([
    { keys: "Ctrl+C", action: "复制", custom: false },
    { keys: "Ctrl+Shift+Delete", action: "清空剪贴板", custom: true },
    { keys: "Win+T", action: "任务栏遍历", custom: false },
  ]);

  return (
    <SectionCard title="资源管理器" f="F515·F517·F525-F528">
      <Row fno="F515" name="查找与替换" desc="Ctrl+H 与查找条同形制；全部替换先看账（影响 N 处+首处预览）；整批一次 Ctrl+Z 回滚">
        <Toggle checked={!!fr.matchCase} onChange={(v) => setFr({ matchCase: v })} label="区分大小写" />
        <Toggle checked={!!fr.wholeWord} onChange={(v) => setFr({ wholeWord: v })} label="全字匹配" />
      </Row>
      <Row fno="F517" name="启动页设置" desc={Object.values(EXPLORER_HOME_NOTES).join("；")}>
        <Pick value={home.mode as ExplorerHome}
          options={[{ v: "thispc", label: "此机（默认）" }, { v: "last-folder", label: "上次关闭的文件夹" }, { v: "fixed", label: "固定文件夹" }]}
          onChange={(v) => setHome({ mode: v })} />
        {(home.mode as string) === "fixed" && (
          <input value={home.fixedFolder as string} placeholder="C:\work" onChange={(e) => setHome({ fixedFolder: e.target.value })} style={{ width: 140, fontSize: 12, padding: "4px 6px" }} />
        )}
      </Row>
      <Row fno="F525" name="快捷键速查卡导出" desc="PNG 一页/PDF 双页；与 F244 注册表同源；默认灰/自定义蓝分色">
        <Pick value={keycard.format as string} options={[{ v: "png", label: "PNG 一页版" }, { v: "pdf", label: "PDF 双页版" }]} onChange={(v) => setKeycard({ format: v })} />
        <span className="u3-stat">样张 <b>{(card.pages[0]?.length ?? 0)}+{(card.pages[1]?.length ?? 0)}</b> 行 · {card.legend.slice(0, 18)}…</span>
      </Row>
      <Row fno="F526" name="状态栏" desc={statusBarSegments({ items: 128, selected: 5, selectedBytes: 245 * 1024 * 1024, volumeFreeBytes: 2 * 1024 ** 3 }).join(" | ")}>
        <Toggle checked={!!bar.visible} onChange={(v) => setBar({ visible: v })} label="显示状态栏" />
        <Num value={bar.heightPx as number} min={20} max={32} suffix="px 高度" onChange={(v) => setBar({ heightPx: v })} />
      </Row>
      <Row fno="F527" name="导航树折叠展开" desc={`双击/箭头两路；全部展开 ${TREE_COMMAND_KEYS.expandAll} / 折叠 ${TREE_COMMAND_KEYS.collapseAll}；展开态持久（重启保持）`}>
        <Toggle checked={!!tree.remember} onChange={(v) => setTree({ remember: v })} label="记住展开状态" />
      </Row>
      <Row fno="F528" name="树与列表双向同步" desc="列表进子目录 → 树自动展开路径并高亮（滚动到可见）；高亮不抢焦点">
        <Toggle checked={!!sync.enabled} onChange={(v) => setSync({ enabled: v })} label="启用" />
        <span className="u3-badge">{TREE_SYNC_FOCUS_NOTE}</span>
      </Row>
    </SectionCard>
  );
}

/* ------- 组六：复制与回收站（F524/F529-F534） ------- */

function CopyOpsGroup(): React.ReactElement {
  const [undoBin, setUndoBin] = useSection<Cfg>("undoEmptyBin");
  const [space, setSpace] = useSection<Cfg>("spaceCheck");
  const [verify, setVerify] = useSection<Cfg>("copyVerify");
  const [queue, setQueue] = useSection<Cfg>("copyQueue");
  const [ro, setRo] = useSection<Cfg>("roRemind");
  const [lp, setLp] = useSection<Cfg>("longPath");

  const shortfallDemo = { volume: "D:", freeGB: 2.1, needGB: 3.4 * (1 + SPACE_CHECK_BUFFER_PCT / 100) };

  return (
    <SectionCard title="复制与回收站" f="F524·F529-F534">
      <Row fno="F524" name="撤销清空回收站" desc={`清空后通知条驻留 ${UNDO_BIN_WINDOW_MS / 1000}s；长按延寿 +${UNDO_BIN_EXTEND_MS / 1000}s（上限 2 次）；暂存到窗末才真释放`}>
        <Num value={(undoBin.windowMs as number) / 1000} min={3} max={10} suffix="秒后悔窗" onChange={(v) => setUndoBin({ windowMs: v * 1000 })} />
        <Num value={(undoBin.extendMs as number) / 1000} min={5} max={20} suffix="秒延寿" onChange={(v) => setUndoBin({ extendMs: v * 1000 })} />
        <Num value={undoBin.maxExtends as number} min={1} max={5} suffix="延寿上限" onChange={(v) => setUndoBin({ maxExtends: v })} />
      </Row>
      <Row fno="F529" name="复制前空间预检" desc="进度框出现前预检；不足（含 10% 缓冲）开跑前拦下；三选出路（仍要复制/换目标/取消）；跨盘批逐盘检">
        <Num value={space.bufferPct as number} min={0} max={30} suffix="% 缓冲" onChange={(v) => setSpace({ bufferPct: v })} />
        <span className="u3-badge warn">{shortfallMessage(shortfallDemo)}</span>
      </Row>
      <Row fno="F530" name="复制后校验" desc={`>1GB 自动开（当前阈值 ${humanBytes(COPY_VERIFY_AUTO_ABOVE)}）；后台空闲 IO；失败报告源/目标路径；关闭开关`}>
        <Toggle checked={!!verify.enabled} onChange={(v) => setVerify({ enabled: v })} label="启用" />
        <Num value={(verify.autoAboveBytes as number) / 1024 ** 3} min={0.1} max={10} step={0.1} suffix="GB 自动开" onChange={(v) => setVerify({ autoAboveBytes: v * 1024 ** 3 })} />
      </Row>
      <Row fno="F531" name="复制任务队列化" desc="同盘串行（顺序写性能最好）；异盘并行各走各；「先传这批」右键插队；每批独立暂停/取消">
        <Toggle checked={!!queue.sameDiskSerial} onChange={(v) => setQueue({ sameDiskSerial: v })} label="同盘串行" />
        <Num value={queue.crossDiskParallel as number} min={1} max={4} suffix="异盘并行数" onChange={(v) => setQueue({ crossDiskParallel: v })} />
      </Row>
      <Row fno="F532" name="打开失败人话诊断" desc="四类归因（格式/损坏/应用缺失/权限）+ 为什么 + 出路链接（F257/F294/Edge/F324）；三问结构">
        <Toggle checked={true} onChange={() => undefined} label="始终启用（三要素铁律）" />
      </Row>
      <Row fno="F533" name="只读介质提醒" desc="写前置提醒（写保护开关/只读挂载点破原因）；卷角标+此机页可见；提示一次不重复">
        <Toggle checked={!!ro.remindOncePerMount} onChange={(v) => setRo({ remindOncePerMount: v })} label="每次挂载提醒一次" />
      </Row>
      <Row fno="F534" name="长路径全程支持" desc=">260 字符（600+ 实测）全操作；显示保尾；索引覆盖；复制路径完整">
        <Toggle checked={!!lp.enabled} onChange={(v) => setLp({ enabled: v })} label="启用" />
        <Num value={lp.tailKeep as number} min={8} max={48} suffix="保尾字符" onChange={(v) => setLp({ tailKeep: v })} />
      </Row>
    </SectionCard>
  );
}

/* ------- 组七：窗口与快捷键（F516/F518/F535-F539/F548） ------- */

function WinKeysGroup(): React.ReactElement {
  const [banner, setBanner] = useSection<Cfg>("bannerPos");
  const [ime, setIme] = useSection<Cfg>("imeToggle");
  const [wn, setWn] = useSection<Cfg>("winNumber");
  const [lock, setLock] = useSection<Cfg>("layoutLock");
  const [top, setTop] = useSection<Cfg>("taskmgrTop");

  const slots: TaskbarSlot[] = [{ appId: "files", running: true }, { appId: "edge", running: false }, { appId: "notes", running: true }];
  const demo3 = winNumberResolve(slots, 1, false, null);

  return (
    <SectionCard title="窗口与快捷键" f="F516·F518·F535-F539·F548">
      <Row fno="F516" name="通知横幅位置" desc={`右下（默认）/顶部中央/顶部右侧；堆叠方向自适应（当前 ${bannerStackDirection(banner.position as BannerPos) === "up" ? "向上堆" : "向下堆"}）；OSD 独立；出主屏`}>
        <Pick value={banner.position as BannerPos}
          options={[{ v: "bottom-right", label: "右下（默认）" }, { v: "top-center", label: "顶部中央" }, { v: "top-right", label: "顶部右侧" }]}
          onChange={(v) => setBanner({ position: v })} />
        <span className="u3-badge">{OSD_SEPARATE_NOTE}</span>
      </Row>
      <Row fno="F518" name="输入法切换键" desc={`四方案；与点击循环并存；改后旧键让出（一键一职）；Caps 双用：短按锁大写 / 长按 ${CAPS_LONG_PRESS_MS}ms 切输入法`}>
        <Pick value={ime.scheme as ImeScheme} options={IME_SCHEMES.map((s) => ({ v: s.id, label: s.label }))} onChange={(v) => setIme({ scheme: v })} />
        <span className="u3-stat">Caps 分界 <b>599ms={capsVerdict(599)} / 600ms={capsVerdict(600)}</b></span>
      </Row>
      <Row fno="F535" name="Win+数字快捷启动" desc="Win+1..0 = 任务栏第 N 个图标：没运行启动 / 运行中切换 / 聚焦中最小化；Shift+Win+数字 = 新开实例；溢出项可达">
        <Toggle checked={!!wn.enabled} onChange={(v) => setWn({ enabled: v })} label="启用" />
        <span className="u3-stat">样例 Win+2 <b>{demo3.action}</b>（{demo3.action !== "none" ? demo3.appId : "—"}）</span>
      </Row>
      <Row fno="F537" name="Win+逗号 瞥桌面" desc={`按住：窗口 120ms 淡出至 ${PEEK_OPACITY * 100}% 透明；松开 120ms 恢复（几何不动 <1px）；纯看限制`}>
        <Toggle checked={true} onChange={() => undefined} label="始终启用（组合键即时）" />
        <span className="u3-stat">进出 <b>{PEEK_FADE_MS}ms</b> · 透明度 <b>{PEEK_OPACITY}</b></span>
      </Row>
      <Row fno="F539" name="桌面布局锁定" desc={`锁定后拖拽温和拒绝（抖动 ${LOCK_SHAKE_MS}ms + 状态栏一句话）；右键排列照常（锁乱序不锁秩序）；解锁路径：${UNLOCK_PATH}`}>
        <Toggle checked={!!lock.locked} onChange={(v) => setLock({ locked: v })} label="锁定布局（默认关）" />
        <Toggle checked={!!lock.cornerBadge} onChange={(v) => setLock({ cornerBadge: v })} label="锁角标" />
      </Row>
      <Row fno="F548" name="任务管理器置顶" desc="置顶切换即时；不抢焦点（F248 语义）；边框亮标记；重启不记忆">
        <Toggle checked={!!top.remember} onChange={(v) => setTop({ remember: v })} label="（判据：重启不记忆——此处仅显式声明语义）" />
      </Row>
    </SectionCard>
  );
}

/* ------- 组八：系统与设备（F540-F547） ------- */

function SysDevGroup(): React.ReactElement {
  const [mem, setMem] = useSection<Cfg>("memDiag");
  const [, setNet] = useSection<Cfg>("netReset");
  const [cl, setCl] = useSection<Cfg>("clickLock");
  const [dv] = useSection<Cfg>("devVolume");
  const [nv, setNv] = useSection<Cfg>("notifyVolume");
  const [btb, setBtb] = useSection<Cfg>("btBattery");
  const [bal, setBal] = useSection<Cfg>("balance");

  const devices = (dv.devices as Array<never>) ?? [];

  return (
    <SectionCard title="系统与设备" f="F540-F547">
      <Row fno="F540" name="内存诊断" desc={`预约（下次重启时跑）/立即两模式；两遍标准读写约 ${MEM_DIAG_EST_MINUTES} 分钟；报告三要素（结论/地址段/建议）入时间线`}>
        <Pick value={mem.mode as string} options={[{ v: "standard", label: "预约下次重启" }, { v: "immediate", label: "立即重启并检测" }]} onChange={(v) => setMem({ mode: v })} />
        <Num value={mem.passes as number} min={1} max={4} suffix="遍" onChange={(v) => setMem({ passes: v })} />
      </Row>
      <Row fno="F541" name="网络重置" desc={`${NET_RESET_COUNTDOWN_SEC}s 倒计时重启网络栈（不重启整机）；清单逐项列出；重配向导链路；入口在诊断链末端`}>
        <span className="u3-stat">清单 <b>{NET_RESET_CLEAR_LIST.length}</b> 项 · {NET_RESET_CLEAR_LIST.join(" / ")}</span>
        <button type="button" className="j1x-btn j1x-btn--danger" onClick={() => setNet({ countdownSec: NET_RESET_COUNTDOWN_SEC })}>模拟准备（不执行）</button>
      </Row>
      <Row fno="F542" name="ClickLock 拖拽锁定" desc={`按住主键 ${(CLICK_LOCK_THRESHOLD_MS / 1000).toFixed(1)}s 松开=抓起（微光环）；单击放下；Esc 放弃；默认关`}>
        <Toggle checked={!!cl.enabled} onChange={(v) => setCl({ enabled: v })} label="启用（默认关）" />
        <span className={`u3-clicklock-demo${cl.enabled ? " grabbed" : ""}`}>🖱</span>
      </Row>
      <Row fno="F543" name="分设备音量记忆" desc={`耳机 30%/扬声器 70% 各记各的；新设备首发 ${NEW_DEVICE_DEFAULT}%；清单 ${DEVICE_VOLUME_CAP} 台 LRU 淘汰；当前 ${devices.length} 台`}>
        <span className="u3-stat">记忆 <b>{devices.length}/{DEVICE_VOLUME_CAP}</b> 台</span>
      </Row>
      <Row fno="F544" name="通知音量独立分级" desc={`通知音量独立滑杆（默认 ${NOTIFY_VOLUME_DEFAULT}）；OSD 双条形制；完全静音档仍总闸一票否决`}>
        <input type="range" min={0} max={100} value={nv.notify as number} onChange={(e) => setNv({ notify: Number(e.target.value) })} style={{ width: 140 }} />
        <span className="u3-stat">通知 <b>{String(nv.notify)}</b>/100</span>
      </Row>
      <Row fno="F545" name="蓝牙耳机电量显示" desc={`三处同源（设备页/电池浮层/连接通知条）；低电 <${BT_LOW_PCT}% 提示（同设备 ${(BT_LOW_THROTTLE_MS / 60000) | 0} 分钟一次）；无电量诚实显示；跳变平滑`}>
        <Num value={btb.lowPct as number} min={10} max={40} suffix="% 低电阈值" onChange={(v) => setBtb({ lowPct: v })} />
        <span className="u3-stat">节流 <b>{((btb.throttleMs as number) / 60000) | 0}min</b> · 平滑步进 <b>{String(btb.smoothStep)}%</b></span>
      </Row>
      <Row fno="F546" name="新设备接入通知" desc="三状态横幅（已就绪 2s 即隐 / 安装驱动中 / 需手动安装→F444）；失败诚实归因">
        <Toggle checked={true} onChange={() => undefined} label="统一形制（系统级纪律）" />
      </Row>
      <Row fno="F547" name="音量左右平衡" desc={`平衡滑杆中心格点；偏离即时试听；按输出设备记忆（F543 族）；软件平衡延迟 <${BALANCE_SW_DELAY_MS}ms`}>
        <input type="range" min={-100} max={100} value={bal.pan as number} onChange={(e) => setBal({ pan: Number(e.target.value) })} style={{ width: 160 }} />
        <span className="u3-stat">平衡 <b>{String(bal.pan)}</b>{Math.abs(Number(bal.pan)) < 3 ? "（中心）" : Number(bal.pan) < 0 ? "（偏左）" : "（偏右）"}</span>
      </Row>
    </SectionCard>
  );
}

/* ------- 组九：时钟与锚点（F549/F550） ------- */

function ClockAnchorGroup(): React.ReactElement {
  const [ch, setCh] = useSection<Cfg>("clockHover");
  const [run, setRun] = useState<ReturnType<typeof anchorRuntime> | null>(null);
  const [rec, setRec] = useState<ReturnType<typeof import("../u3/reconcile").reconcileF501F550> | null>(null);

  const runReconcile = () => {
    void import("../u3/reconcile").then((m) => setRec(m.reconcileF501F550())).catch((e: unknown) => console.error("[u3:F550] 对账执行失败", e));
    setRun(anchorRuntime());
  };

  return (
    <SectionCard title="时钟与验收锚点" f="F549·F550">
      <Row fno="F549" name="时钟悬停完整日期" desc={`三要素一行；Tooltip 延迟 ${ch.delayMs}ms（F205 全局一致）；农历 2026-2030 离线内置；与日历飞出 F078 分工（悬停轻/点击重）`}>
        <Toggle checked={!!ch.showLunar} onChange={(v) => setCh({ showLunar: v })} label="显示农历" />
        <span className="u3-stat">样张 <b>{hoverDateLine(2026, 9, 25, !!ch.showLunar)}</b></span>
      </Row>
      <Row fno="F550" name="I 域批次六验收锚点" desc={`前端面九域自检引擎（判据 ≥${U3_ANCHOR_MIN_CHECKS} 检查点全绿基线）；与内核 ustar3 50 域 CheckSet（v1）互为对账`}>
        <button type="button" className="j1x-btn" onClick={() => setRun(anchorRuntime())}>执行九域自检</button>
        {run && (
          <span className={`u3-badge ${run.allGreen ? "ok" : "warn"}`}>
            {run.allGreen ? "✓" : "✗"} {run.passed}/{run.total} 绿{run.failed > 0 ? ` · ${run.failed} 红` : ""}
          </span>
        )}
        {run && run.results.map((r) => (
          <span key={r.domain} className="u3-stat">{r.fRange} <b>{r.checks.filter((c) => c.pass).length}/{r.checks.length}</b></span>
        ))}
      </Row>
      <Row fno="F550" name="三面对账（账册检）" desc="主册 50 判据 × 内核 616 CheckSet × 前端九域自检——逐项三面在位才算对账绿；含 F400/F575 编号空间合并无冲突验证">
        <button type="button" className="j1x-btn" onClick={runReconcile}>执行三面对账</button>
        {rec && (
          <span className={`u3-badge ${rec.allGreen ? "ok" : "warn"}`}>
            {rec.allGreen ? "✓ 三面对平" : "✗"} {rec.ok}/{rec.total} 项 · 内核账册 {rec.kernelLedger.checksTotal} 项{rec.kernelLedger.ledgerOk ? "（616✓）" : "（账册不符!）"} · 前端 {rec.frontendRuntime.passed}/{rec.frontendRuntime.total}
          </span>
        )}
        {rec && rec.missing.length > 0 && (
          <span className="u3-badge warn">缺面：{rec.missing.map((m) => `${m.fno}(${m.faces.join("/")})`).join("、")}</span>
        )}
        {rec && (
          <span className="u3-stat">编号空间 <b>{String(rec.allGreen)}</b> · 抽查 <b>{rec.rows.filter((r) => r.ok).slice(0, 5).map((r) => r.fno).join("/")}</b></span>
        )}
      </Row>
    </SectionCard>
  );
}
