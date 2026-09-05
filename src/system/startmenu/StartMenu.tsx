import { useEffect, useRef, useState } from "react";
import {
  AppWindow, FolderOpen, Lock, LogOut, PackagePlus, Power, RotateCcw,
  Settings as SettingsIcon, Search, Trash2, X,
} from "lucide-react";
import { useI18n } from "../../i18n";
import { errMessage, ipc } from "../../lib/ipc";
import { matchPinyin } from "../../lib/pinyin";
import type { AppMode } from "../../state/uiStore";
import { pushToast } from "../../state/uiStore";
import { askConfirm } from "../../components/Modal";
import { desktopIconDefs, desktopAppLabel } from "../desktop-icons/DesktopIcons";
import { launchThirdApp, useThirdApps } from "../launcher/thirdApps";
import { useUninstalledOfficial } from "../launcher/official";
import { openVwmSystem } from "../windows/vwm";
import { pushRecent, useRecent } from "./recent";

/**
 * 开始菜单（M3 → 批次E，桌面环境 L1，Windows 11 习惯 + V 品牌入口）：
 * - 顶部搜索框 → 打开全局搜索（复用 SearchOverlay，零重复实现）
 * - 最近使用（批次E，规格 4.6.2）：纯本地 localStorage 记录，最近 8 项
 * - 已固定网格：四款独立软件 + 系统入口 + 第三方，支持拖拽重排（localStorage 持久化）
 *   （批次C：已卸载的预装软件不再显示；卸载入口在「软件管理 → 已安装软件」）
 * - 底栏：本机用户名（批次E）+ 品牌标识 + 电源完整菜单（锁定/注销/重启/关机/退出）
 * - Esc / 点击菜单外关闭
 */

const ORDER_KEY = "variable:start:order:v1";

function loadOrder(): string[] {
  try {
    const raw = localStorage.getItem(ORDER_KEY);
    return raw ? (JSON.parse(raw) as string[]) : [];
  } catch {
    return [];
  }
}

function saveOrder(ids: string[]): void {
  try {
    localStorage.setItem(ORDER_KEY, JSON.stringify(ids));
  } catch {
    /* storage full — 忽略 */
  }
}

export function StartMenu(props: {
  open: boolean;
  onClose: () => void;
  onOpenApp: (app: AppMode) => void;
  onOpenSettings: () => void;
  onOpenLauncher: () => void;
  onOpenSearch: () => void;
  onExit: () => void;
}): React.ReactElement | null {
  const { t } = useI18n();
  const uninstalled = useUninstalledOfficial();
  const thirds = useThirdApps();
  const recent = useRecent();
  const defs = desktopIconDefs().filter((d) => uninstalled[d.app] === undefined);
  const [userName, setUserName] = useState<string>("");
  // 批次E-8：开始菜单搜索框（拼音/首字母过滤，Enter 转全局搜索）
  const [q, setQ] = useState<string>("");
  const [powerOpen, setPowerOpen] = useState(false);
  const [order, setOrder] = useState<string[]>(() => loadOrder());
  const [dragOverId, setDragOverId] = useState<string | null>(null);
  const dragId = useRef<string | null>(null);

  useEffect(() => {
    if (!props.open) return;
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === "Escape") {
        e.stopPropagation();
        props.onClose();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [props.open, props]);

  // 批次E：本机用户名（打开时读一次即可）
  useEffect(() => {
    if (!props.open || userName) return;
    void ipc
      .sysUser()
      .then(setUserName)
      .catch(() => {});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.open]);

  if (!props.open) return null;

  // ---- 批次E：固定网格拖拽重排 ----
  interface GridItem {
    id: string;
    label: string;
    hue: string;
    icon: React.ReactNode;
    onClick: () => void;
    title?: string;
    custom?: React.ReactNode;
  }
  const items: GridItem[] = [
    ...defs.map((d) => {
      const Icon = d.icon;
      return {
        id: `app-${d.app}`,
        label: desktopAppLabel(d.app),
        hue: String(d.hue),
        icon: <Icon size={22} strokeWidth={1.6} />,
        onClick: () => {
          pushRecent("app", d.app, desktopAppLabel(d.app));
          props.onOpenApp(d.app);
        },
      };
    }),
    {
      id: "sys-explorer",
      label: t("explorerWin"),
      hue: "210",
      icon: <FolderOpen size={22} strokeWidth={1.6} />,
      onClick: () => {
        pushRecent("sys", "explorer", t("explorerWin"));
        props.onClose();
        openVwmSystem("explorer");
      },
      title: t("explorerWin"),
    },
    ...thirds.map((a) => ({
      id: `tp-${a.id}`,
      label: a.name,
      hue: "158",
      icon: a.icon ? (
        <img src={a.icon} alt="" className="tb-custom-icon" />
      ) : (
        <AppWindow size={22} strokeWidth={1.6} />
      ),
      onClick: () => {
        pushRecent("tp", a.id, a.name);
        props.onClose();
        void launchThirdApp(a.id, a.name);
      },
      title: a.path,
    })),
    {
      id: "sys-recycle",
      label: t("recycleBin"),
      hue: "0",
      icon: <Trash2 size={22} strokeWidth={1.6} />,
      onClick: () => {
        pushRecent("sys", "recycle", t("recycleBin"));
        props.onClose();
        openVwmSystem("recycle");
      },
      title: t("recycleBin"),
    },
    {
      id: "sys-launcher",
      label: t("launcherTitle"),
      hue: "158",
      icon: <PackagePlus size={22} strokeWidth={1.6} />,
      onClick: props.onOpenLauncher,
      title: t("launcherTitle"),
    },
    {
      id: "sys-settings",
      label: t("settings"),
      hue: "210",
      icon: <SettingsIcon size={22} strokeWidth={1.6} />,
      onClick: props.onOpenSettings,
      title: t("settings"),
    },
  ];
  const orderIdx = new Map(order.map((id, i) => [id, i]));
  items.sort((a, b) => (orderIdx.get(a.id) ?? 1e9) - (orderIdx.get(b.id) ?? 1e9));

  // 批次E-8（规格 N5）：拼音/首字母即时过滤 —— "sz" 命中"设置"、"swdt" 命中"思维导图"
  const filtered = q.trim() ? items.filter((it) => matchPinyin(it.label, q) || matchPinyin(it.title ?? it.label, q)) : items;
  const filteredRecent = q.trim() ? recent.filter((r) => matchPinyin(r.name, q)) : recent;

  const onDropTo = (targetId: string): void => {
    const from = dragId.current;
    dragId.current = null;
    if (!from || from === targetId) return;
    const ids = items.map((i) => i.id);
    const fi = ids.indexOf(from);
    const ti = ids.indexOf(targetId);
    if (fi < 0 || ti < 0) return;
    ids.splice(ti, 0, ids.splice(fi, 1)[0] as string);
    setOrder(ids);
    saveOrder(ids);
  };

  // ---- 批次E：最近使用行（点击直达） ----
  const recentLabel = (kind: string, name: string): string =>
    kind === "tp" ? name : name;

  // ---- 批次E：电源完整菜单（规格 4.6.3） ----
  const power = async (action: "lock" | "logoff" | "reboot" | "shutdown"): Promise<void> => {
    setPowerOpen(false);
    if (action === "reboot" || action === "shutdown") {
      const label = action === "reboot" ? t("powerRestart") : t("powerShutdown");
      const ok = await askConfirm({
        title: label,
        body: t("powerConfirmBody", { action: label }),
        danger: true,
        okLabel: label,
      });
      if (!ok) return;
    }
    await ipc
      .powerAction(action)
      .catch((e) => pushToast("error", t("powerMenu"), errMessage(e).message));
  };

  return (
    <div
      className="start-overlay"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) props.onClose();
      }}
    >
      <div className="start-menu" role="dialog" aria-label={t("startMenu")}>
        <div className="start-search">
          <Search size={16} className="dim" />
          <input
            value={q}
            placeholder={t("startSearchHint")}
            onChange={(e) => setQ(e.target.value)}
            onKeyDown={(e) => {
              e.stopPropagation();
              if (e.key === "Escape") {
                if (q) setQ("");
                else props.onClose();
              }
              if (e.key === "Enter" && q.trim()) {
                setQ("");
                props.onOpenSearch();
              }
            }}
          />
          {q && (
            <button type="button" className="icon-btn tiny" aria-label={t("close")} onClick={() => setQ("")}>
              <X size={13} />
            </button>
          )}
        </div>

        {recent.length > 0 && (
          <>
            <p className="start-section dim small">{t("recent")}</p>
            <div className="start-recent">
              {filteredRecent.map((r) => (
                <button
                  key={`${r.kind}-${r.id}`}
                  type="button"
                  className="start-recent-chip"
                  title={recentLabel(r.kind, r.name)}
                  onClick={() => {
                    if (r.kind === "app") {
                      props.onOpenApp(r.id as AppMode);
                    } else if (r.kind === "tp") {
                      props.onClose();
                      void launchThirdApp(r.id, r.name);
                    } else if (r.kind === "sys") {
                      props.onClose();
                      openVwmSystem(r.id as "explorer" | "recycle");
                    } else {
                      props.onClose();
                    }
                  }}
                >
                  {r.name}
                </button>
              ))}
            </div>
          </>
        )}

        <p className="start-section dim small">{t("pinned")}</p>
        <div className="start-grid">
          {filtered.map((it) => (
            <button
              key={it.id}
              type="button"
              onClick={it.onClick}
              title={it.title ?? it.label}
              draggable
              onDragStart={() => {
                dragId.current = it.id;
              }}
              onDragOver={(e) => {
                e.preventDefault();
                setDragOverId(it.id);
              }}
              onDrop={() => onDropTo(it.id)}
              onDragEnd={() => {
                dragId.current = null;
                setDragOverId(null);
              }}
              className={`start-app${dragOverId === it.id ? " drag-over" : ""}`}
            >
              <span className="desktop-icon-tile" style={{ ["--hue" as string]: it.hue }}>
                {it.icon}
              </span>
              <span className="start-app-name">{it.label}</span>
            </button>
          ))}
        </div>

        <div className="start-foot">
          <span className="start-user" title={userName || undefined}>
            <span className="start-avatar" aria-hidden>
              {(userName || "U").slice(0, 1).toUpperCase()}
            </span>
            <span className="start-user-name">{userName || "…"}</span>
          </span>
          <span className="start-foot-spacer" />
          <span className="start-brand">
            <span className="start-brand-v">V</span> Variable
          </span>
          <div className="start-power-wrap">
            {powerOpen && (
              <div className="start-power-menu card-pop" role="menu" aria-label={t("powerMenu")}>
                <button type="button" role="menuitem" onClick={() => void power("lock")}>
                  <Lock size={14} /> {t("powerLock")}
                </button>
                <button type="button" role="menuitem" onClick={() => void power("logoff")}>
                  <LogOut size={14} /> {t("powerLogoff")}
                </button>
                <button type="button" role="menuitem" onClick={() => void power("reboot")}>
                  <RotateCcw size={14} /> {t("powerRestart")}
                </button>
                <button type="button" role="menuitem" onClick={() => void power("shutdown")}>
                  <Power size={14} /> {t("powerShutdown")}
                </button>
                <button type="button" role="menuitem" className="danger" onClick={props.onExit}>
                  <Power size={14} /> {t("exitVariable")}
                </button>
              </div>
            )}
            <button
              type="button"
              className={`tb-btn start-power${powerOpen ? " active" : ""}`}
              aria-label={t("powerMenu")}
              title={t("powerMenu")}
              onClick={() => setPowerOpen(!powerOpen)}
            >
              <Power size={18} strokeWidth={1.7} />
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
