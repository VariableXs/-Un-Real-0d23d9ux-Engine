/**
 * SINGULARITY-100 · 域10 安全与隐私（Q-64…Q-70）行为层。
 * （Q-66 密码侦探的输入框观察在 filesDomain 同层实现，此处不重复。）
 *
 * 边界（全景 §10）：U-31/U-35/Z-48 管面板与实时指示；本域是目录级审计
 * 日志、剪贴板 TTL、下载检疫、权限对账、设备活动史与数据冰山。
 * 零残留纪律：日志仅本地 JSONL；可一键焚毁。
 */

import {
  type DomainController,
  type DomainCtx,
  UnsubBag,
  domReady,
  emitSingu,
  makeEl,
  on,
  singuLayer,
  singuStoreRead,
  singuStoreWrite,
} from "../shared";
import { singuNum } from "../registry";
import {
  singuDataProfile,
  singuJournalClear,
  singuJournalList,
  singuJournalLog,
  singuZoneCheck,
} from "../singuIpc";

// ---------------------------------------------------------------------------
// 纯逻辑（可测）
// ---------------------------------------------------------------------------

/** Q-65：密码特征判定（纯本地启发式：长度 + 字符类 + 常见 key 名）。 */
export function looksSensitive(text: string, keyName = ""): boolean {
  if (text.length < 8) return false;
  const lower = keyName.toLowerCase();
  if (/pass(word)?|pwd|secret|token|api[-_]?key|credential/.test(lower)) return true;
  let classes = 0;
  if (/[a-z]/.test(text)) classes += 1;
  if (/[A-Z]/.test(text)) classes += 1;
  if (/\d/.test(text)) classes += 1;
  if (/[^a-zA-Z0-9]/.test(text)) classes += 1;
  // 长且三类以上 → 疑似 token/密码
  return text.length >= 16 && classes >= 3;
}

/** Q-68：权限对账：声明 × 实际调用 → 越权判定。 */
export interface PermRow {
  perm: string;
  declared: boolean;
  calls: number;
}
export function permVerdict(rows: readonly PermRow[]): "clean" | "over" {
  return rows.some((r) => !r.declared && r.calls > 0) ? "over" : "clean";
}

/** Q-69：设备活动史滚动清理（30 天）。 */
export function pruneHistory<T extends { ts: number }>(items: readonly T[], now: number, days = 30): T[] {
  const cutoff = now - days * 86_400_000;
  return items.filter((i) => i.ts >= cutoff);
}

// ---------------------------------------------------------------------------
// 行为层
// ---------------------------------------------------------------------------

let bag = new UnsubBag();
let ctxRef: DomainCtx | null = null;

// ---- Q-64 访问日志（敏感目录环境内访问审计；journal overlay 数据源）----
function mountAccessJournal(): void {
  const watched = singuStoreRead<string[]>("journal-dirs", ["vault"]);
  const seen = new Set<string>();
  const logAccess = (path: string, action: string): void => {
    if (!ctxRef?.on("Q-64")) return;
    if (seen.has(`${path}:${action}`)) return; // 同路径同动作一次会话只记一次
    seen.add(`${path}:${action}`);
    void singuJournalLog(path, "Variable", action);
  };
  // 环境内文件管理器导航事件（只读观察，不改 explorer 逻辑）
  bag.add(
    on(window, "explorer:navigate", (e: Event) => {
      const path = (e as CustomEvent).detail?.path as string | undefined;
      if (path && watched.some((w) => path.toLowerCase().includes(w.toLowerCase()))) {
        logAccess(path, "read");
      }
    }),
  );
  bag.add(
    on(window, "singu:journal-add-dir", (e: Event) => {
      const dir = (e as CustomEvent).detail?.dir as string | undefined;
      if (!dir) return;
      if (!watched.includes(dir)) {
        watched.push(dir);
        singuStoreWrite("journal-dirs", watched);
      }
    }),
  );
  bag.add(
    on(window, "singu:journal-clear", () => {
      void singuJournalClear();
      ctxRef?.toast("success", "访问日志已焚毁", "零残留");
    }),
  );
  bag.add(
    on(window, "singu:journal-export", () => {
      void singuJournalList().then((rows) => {
        const blob = new Blob([JSON.stringify(rows ?? [], null, 2)], { type: "application/json" });
        const url = URL.createObjectURL(blob);
        const a = document.createElement("a");
        a.href = url;
        a.download = "variable-access-journal.json";
        a.click();
        URL.revokeObjectURL(url);
      });
    }),
  );
}

// ---- Q-65 剪贴板过期（敏感内容 TTL 清除 + 清除前倒计时角标）----
function mountClipExpiry(): void {
  let deadline = 0;
  let badge: HTMLElement | null = null;
  let tick: ReturnType<typeof setInterval> | null = null;
  const clearClip = (): void => {
    void navigator.clipboard.writeText("").then(() => {
      ctxRef?.toast("success", "剪贴板已自动清除", "敏感内容 TTL 到期");
      badge?.remove();
      badge = null;
    });
  };
  bag.add(
    on(window, "paste", () => {
      // 粘贴成功后开始监控本轮剪贴板内容是否敏感
      void navigator.clipboard.readText().then((text) => {
        if (!ctxRef?.on("Q-65") || !looksSensitive(text)) return;
        const ttl = singuNum("Q-65", "ttlSec") || 90;
        deadline = Date.now() + ttl * 1000;
        if (!badge) {
          badge = makeEl("div", "singu-clip-ttl");
          singuLayer().appendChild(badge);
        }
        if (!tick) {
          tick = setInterval(() => {
            const remain = Math.ceil((deadline - Date.now()) / 1000);
            if (remain <= 0) {
              if (tick) clearInterval(tick);
              tick = null;
              clearClip();
              return;
            }
            if (badge) {
              badge.textContent = `CLIP ${remain}s`;
              badge.classList.toggle("warn", remain <= 10);
            }
          }, 1000);
        }
      });
    }),
  );
  // 主动复制敏感内容同样进入 TTL（观察 copy 事件，只读）
  bag.add(
    on(window, "copy", () => {
      const sel = window.getSelection()?.toString() ?? "";
      if (!ctxRef?.on("Q-65") || !looksSensitive(sel)) return;
      const ttl = singuNum("Q-65", "ttlSec") || 90;
      deadline = Date.now() + ttl * 1000;
      if (!badge) {
        badge = makeEl("div", "singu-clip-ttl");
        singuLayer().appendChild(badge);
      }
    }),
  );
  bag.add(() => {
    if (tick) clearInterval(tick);
    badge?.remove();
  });
}

// ---- Q-67 下载检疫（Zone.Identifier 判定网络来源可执行文件首跑）----
const EXEC_EXT = /\.(exe|msi|bat|ps1|cmd|com|scr)$/i;
function mountDownloadQuarantine(): void {
  const approved = singuStoreRead<Record<string, string>>("zone-approved", {});
  const gate = (path: string): void => {
    if (!ctxRef?.on("Q-67") || !EXEC_EXT.test(path)) return;
    if (approved[path]) return; // 已放行（zone 记录一致即复用）
    void singuZoneCheck(path).then((zone) => {
      if (!zone?.from_internet) return; // 本地文件不误报
      const card = makeEl("div", "singu-quarantine");
      card.innerHTML = `
        <div class="title">DOWNLOAD QUARANTINE</div>
        <div class="file">${escapeHtml(path.split(/[\\/]/).pop() ?? path)}</div>
        <div class="meta">zone ${zone.zone_id ?? "?"} · 来自互联网的可执行文件</div>
        <div class="row"><button class="singu-btn danger" data-a="block">Block</button><button class="singu-btn" data-a="allow">Allow once</button></div>`;
      card.querySelector('[data-a="block"]')?.addEventListener("click", () => {
        card.remove();
        ctxRef?.toast("success", "已阻止启动", path);
      });
      card.querySelector('[data-a="allow"]')?.addEventListener("click", () => {
        approved[path] = String(zone.zone_id ?? "net");
        singuStoreWrite("zone-approved", approved);
        card.remove();
        window.dispatchEvent(new CustomEvent("singu:zone-approved", { detail: { path } }));
        ctxRef?.toast("success", "已放行并记录", "可在访问日志中审计");
      });
      singuLayer().appendChild(card);
    });
  };
  // 启动器/文件管理器的启动事件（只读观察）
  bag.add(
    on(window, "tp:launch", (e: Event) => {
      const path = (e as CustomEvent).detail?.path as string | undefined;
      if (path) gate(path);
    }),
  );
  bag.add(
    on(window, "explorer:open", (e: Event) => {
      const path = (e as CustomEvent).detail?.path as string | undefined;
      if (path) gate(path);
    }),
  );
}

// ---- Q-68 权限说明书（声明 × 调用计数对账；overlay 数据源）----
function mountPermissionSheet(): void {
  interface CallCount {
    perm: string;
    calls: number;
  }
  const counters = singuStoreRead<Record<string, CallCount[]>>("perm-counters", {});
  const bump = (appId: string, perm: string): void => {
    const rows = counters[appId] ?? [];
    const row = rows.find((r) => r.perm === perm);
    if (row) row.calls += 1;
    else rows.push({ perm, calls: 1 });
    counters[appId] = rows;
    singuStoreWrite("perm-counters", counters);
    emitSingu("perm-calls", { appId, perm });
  };
  // 环境内权限敏感操作事件（只读计数，不拦截）
  bag.add(on(window, "singu:perm-mic", (e: Event) => bump(String((e as CustomEvent).detail?.app ?? "unknown"), "microphone")));
  bag.add(on(window, "singu:perm-cam", (e: Event) => bump(String((e as CustomEvent).detail?.app ?? "unknown"), "camera")));
  bag.add(on(window, "singu:perm-net", (e: Event) => bump(String((e as CustomEvent).detail?.app ?? "unknown"), "network")));
  bag.add(on(window, "singu:perm-file", (e: Event) => bump(String((e as CustomEvent).detail?.app ?? "unknown"), "files")));
}

// ---- Q-69 设备活动史（麦克风/摄像头启停 30 天账本）----
function mountDeviceHistory(): void {
  interface DevEvent {
    ts: number;
    device: "mic" | "cam";
    on: boolean;
    by: string;
  }
  let history = singuStoreRead<DevEvent[]>("device-history", []);
  const append = (device: "mic" | "cam", isOn: boolean, by: string): void => {
    history = pruneHistory([{ ts: Date.now(), device, on: isOn, by }, ...history], Date.now());
    singuStoreWrite("device-history", history.slice(0, 2000));
    emitSingu("device-history", { device, on: isOn });
  };
  bag.add(on(window, "mic:active", (e: Event) => append("mic", Boolean((e as CustomEvent).detail?.on), String((e as CustomEvent).detail?.by ?? "system"))));
  bag.add(on(window, "cam:active", (e: Event) => append("cam", Boolean((e as CustomEvent).detail?.on), String((e as CustomEvent).detail?.by ?? "system"))));
  bag.add(
    on(window, "singu:device-history-export", () => {
      const blob = new Blob([JSON.stringify(history, null, 2)], { type: "application/json" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = "variable-device-history.json";
      a.click();
      URL.revokeObjectURL(url);
    }),
  );
}

// ---- Q-70 数据冰山（各区存储构成 → overlay 渲染；下钻事件）----
function mountDataIceberg(): void {
  bag.add(
    on(window, "singu:iceberg-load", () => {
      void singuDataProfile().then((zones) => {
        if (!zones || zones.length === 0) {
          emitSingu("iceberg", { zones: [], error: "unavailable" });
          return;
        }
        emitSingu("iceberg", { zones, error: null });
      });
    }),
  );
  // 下钻：冰山各层 → 对应治理工具
  bag.add(
    on(window, "singu:iceberg-drill", (e: Event) => {
      const zone = String((e as CustomEvent).detail?.zone ?? "");
      const map: Record<string, string> = {
        docs: "library",
        cache: "singu-campfire",
        logs: "singu-journal",
        temp: "singu-campfire",
      };
      const target = map[zone];
      if (!target) return;
      window.dispatchEvent(
        new CustomEvent("ai04:open-feature", { detail: { feature: target } }),
      );
    }),
  );
}

function escapeHtml(s: string): string {
  return s.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c] ?? c);
}

export function privacyDomain(): DomainController {
  return {
    domain: "privacy",
    mount(ctx: DomainCtx) {
      ctxRef = ctx;
      bag = new UnsubBag();
      domReady(() => {
        mountAccessJournal();
        mountClipExpiry();
        mountDownloadQuarantine();
        mountPermissionSheet();
        mountDeviceHistory();
        mountDataIceberg();
      });
    },
    unmount() {
      bag.run();
      document.querySelectorAll(".singu-clip-ttl,.singu-quarantine").forEach((e) => e.remove());
    },
  };
}
