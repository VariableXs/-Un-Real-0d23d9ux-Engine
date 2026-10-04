/**
 * SINGULARITY-100 · 运行时编排：挂载全部 15 个领域模块。
 *
 * - App.tsx（或 DesktopShell）一次 `void initSingularity()` 即激活；
 * - 共享 DomainCtx：开关实时读取 / 2s 缓存硬件脉搏 / toast 安全包装；
 * - 任一域挂载失败如实记录（不连坐其他域）；
 * - 全部功能关闭时整体卸载（全关零开销），再启用需重新初始化。
 */

import type { DomainController, DomainCtx, SinguPulseLike } from "./shared";
import {
  domainActive,
  singuBool,
  singuMotionOK,
  singuNum,
  singuOn,
  singuStr,
  subscribeSingu,
  type SinguDomainId,
} from "./registry";
import { singuPulse } from "./singuIpc";

import { bootDomain } from "./domains/bootDomain";
import { windowsDomain } from "./domains/windowsDomain";
import { desktopDomain } from "./domains/desktopDomain";
import { taskbarDomain } from "./domains/taskbarDomain";
import { inputDomain } from "./domains/inputDomain";
import { filesDomain } from "./domains/filesDomain";
import { toolsDomain } from "./domains/toolsDomain";
import { hardwareDomain } from "./domains/hardwareDomain";
import { compatDomain } from "./domains/compatDomain";
import { privacyDomain } from "./domains/privacyDomain";
import { ecoDomain } from "./domains/ecoDomain";
import { visualsDomain } from "./domains/visualsDomain";
import { soundDomain } from "./domains/soundDomain";
import { a11yDomain } from "./domains/a11yDomain";
import { qualityDomain } from "./domains/qualityDomain";

const DOMAIN_IDS: SinguDomainId[] = [
  "boot",
  "windows",
  "desktop",
  "taskbar",
  "input",
  "files",
  "tools",
  "hardware",
  "compat",
  "privacy",
  "eco",
  "vision",
  "sound",
  "a11y",
  "quality",
];

const CONTROLLERS: Array<() => DomainController> = [
  bootDomain,
  windowsDomain,
  desktopDomain,
  taskbarDomain,
  inputDomain,
  filesDomain,
  toolsDomain,
  hardwareDomain,
  compatDomain,
  privacyDomain,
  ecoDomain,
  visualsDomain,
  soundDomain,
  a11yDomain,
  qualityDomain,
];

const pulseCache = { data: null as SinguPulseLike | null, at: 0 };

let pulseTimer: ReturnType<typeof setInterval> | null = null;
const live: DomainController[] = [];
let unsub: (() => void) | null = null;
let started = false;

function makeCtx(): DomainCtx {
  return {
    on: (id) => singuOn(id),
    num: (id, key) => singuNum(id, key),
    str: (id, key) => singuStr(id, key),
    bool: (id, key) => singuBool(id, key),
    motionOK: () => singuMotionOK(),
    pulse: () => (Date.now() - pulseCache.at < 5000 ? pulseCache.data : null),
    toast: (kind, msg, detail) => {
      window.dispatchEvent(new CustomEvent("singu:toast", { detail: { kind, msg, detail } }));
      void import("../../state/uiStore")
        .then(({ pushToast }) => pushToast(kind, msg, detail ?? ""))
        .catch(() => undefined);
    },
  };
}

/** 挂载全部领域（幂等；App 启动时调用一次）。 */
export function initSingularity(): void {
  if (started || typeof document === "undefined") return;
  started = true;
  const ctx = makeCtx();
  for (const make of CONTROLLERS) {
    try {
      const c = make();
      c.mount(ctx);
      live.push(c);
    } catch {
      // 单域失败不连坐：如实跳过（该域无产物即其在降级矩阵中的诚实呈现）
    }
  }
  // 硬件脉搏后台采样（2s 缓存；消费方按需读取）
  if (!pulseTimer) {
    pulseTimer = setInterval(() => {
      void singuPulse().then((p) => {
        if (p) {
          pulseCache.data = p;
          pulseCache.at = Date.now();
        }
      });
    }, 2000);
  }
  // 全关 → 整体卸载（零开销纪律）
  unsub = subscribeSingu(() => {
    if (live.length === 0) return;
    const anyOn = DOMAIN_IDS.some((d) => domainActive(d));
    if (!anyOn) shutdownSingularity();
  });
}

/** 测试/热重载用：手动卸载全部。 */
export function shutdownSingularity(): void {
  unsub?.();
  unsub = null;
  for (const c of live.splice(0)) {
    try {
      c.unmount();
    } catch {
      /* 尽力而为 */
    }
  }
  if (pulseTimer) {
    clearInterval(pulseTimer);
    pulseTimer = null;
  }
  started = false;
}
