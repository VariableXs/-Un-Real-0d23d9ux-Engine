/**
 * SINGULARITY-100 · 域6 文件与数据能力（Q-37…Q-43）行为层。
 *
 * 边界（全景 §6）：存量覆盖预览/标签/回收站/校验/重命名；本域补临时治理、
 * 批量属性、体量直觉、命名智能与时间筛选。重活（篝火/批量/快检）在 overlay
 * 工具窗中执行（Rust 命令支撑），行为层只装拖拽配重与全局入口。
 */

import {
  type DomainController,
  type DomainCtx,
  UnsubBag,
  clamp,
  domReady,
  makeEl,
  on,
  singuLayer,
} from "../shared";
import { singuMotionOK } from "../registry";

// ---------------------------------------------------------------------------
// 纯逻辑（可测）
// ---------------------------------------------------------------------------

/** Q-39：体量分档（0-3 = <1MB / <100MB / <1GB / ≥1GB）。 */
export function weightTier(bytes: number): 0 | 1 | 2 | 3 {
  if (bytes < 1024 * 1024) return 0;
  if (bytes < 100 * 1024 * 1024) return 1;
  if (bytes < 1024 * 1024 * 1024) return 2;
  return 3;
}

/** Q-43：时间透镜区间（返回 [起, 止) unix ms）。 */
export function timeLensRange(
  lens: "today" | "yesterday" | "week" | "month" | "quarter",
  now: Date,
): [number, number] {
  const day0 = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime();
  const DAY = 86_400_000;
  switch (lens) {
    case "today":
      return [day0, day0 + DAY];
    case "yesterday":
      return [day0 - DAY, day0];
    case "week":
      return [day0 - 6 * DAY, day0 + DAY];
    case "month":
      return [new Date(now.getFullYear(), now.getMonth(), 1).getTime(), day0 + DAY];
    case "quarter":
      return [new Date(now.getFullYear(), Math.floor(now.getMonth() / 3) * 3, 1).getTime(), day0 + DAY];
  }
}

/** Q-40：命名规律检测（日期型 / 序号型 / 项目型 → 3 条建议）。 */
export function nameSuggestions(existing: string[], baseName: string, now: Date): string[] {
  if (existing.length < 2) return []; // 无规律时如实不出建议
  const dateRe = /^(\d{4})[-_.]?(\d{1,2})[-_.]?(\d{1,2})/;
  const dateHits = existing.filter((n) => dateRe.test(n)).length;
  const numRe = /\((\d+)\)(?:\.[^.]+)?$/; // 允许 (n) 后跟扩展名：report(1).pdf
  const numHits = existing.filter((n) => numRe.test(n)).length;
  const out: string[] = [];
  const ext = baseName.includes(".") ? baseName.slice(baseName.lastIndexOf(".")) : "";
  const stem = baseName.replace(ext, "") || "untitled";
  if (dateHits / existing.length >= 0.5) {
    const p = (n: number) => String(n).padStart(2, "0");
    out.push(`${now.getFullYear()}-${p(now.getMonth() + 1)}-${p(now.getDate())}${ext}`);
  }
  if (numHits / existing.length >= 0.5) {
    const nums = existing.map((n) => Number(numRe.exec(n)?.[1] ?? 0)).filter((n) => n > 0);
    const next = (nums.length ? Math.max(...nums) : 0) + 1;
    out.push(`${stem}(${next})${ext}`);
  }
  if (out.length < 3) {
    out.push(`${stem}-new${ext}`);
  }
  return out.slice(0, 3);
}

// ---------------------------------------------------------------------------
// 行为层
// ---------------------------------------------------------------------------

let bag = new UnsubBag();
let ctxRef: DomainCtx | null = null;

// ---- Q-39 配重感（拖拽体量映射拖影重量）----
function mountWeightFeel(): void {
  let ghost: HTMLElement | null = null;
  bag.add(
    on(window, "dragstart", (e) => {
      if (!ctxRef?.on("Q-39")) return;
      const de = e as DragEvent;
      const files = Array.from(de.dataTransfer?.files ?? []);
      const total = files.reduce((s, f) => s + f.size, 0);
      const tier = files.length > 0 ? weightTier(total) : 1;
      if (ghost) ghost.remove();
      ghost = makeEl("div", `singu-dragweight t${tier}`);
      ghost.innerHTML = `<span>${files.length ? formatBytes(total) : "DRAG"}</span>`;
      singuLayer().appendChild(ghost);
      const move = (ev: PointerEvent): void => {
        if (!ghost) return;
        // 垂坠感：体量越大跟随滞后越明显（低帧跟随）
        const lag = 1 - tier * 0.22;
        const gx = ghost.offsetLeft + (ev.clientX - ghost.offsetLeft - ghost.offsetWidth / 2) * (0.35 + lag * 0.4);
        const gy = ghost.offsetTop + (ev.clientY - ghost.offsetTop - 20) * (0.35 + lag * 0.4);
        ghost.style.left = `${clamp(gx + 16, 0, window.innerWidth - 80)}px`;
        ghost.style.top = `${clamp(gy + 12, 0, window.innerHeight - 40)}px`;
        ghost.classList.toggle("tremble", tier === 3 && singuMotionOK());
      };
      window.addEventListener("pointermove", move);
      const drop = (): void => {
        window.removeEventListener("pointermove", move);
        ghost?.remove();
        ghost = null;
      };
      window.addEventListener("dragend", drop, { once: true });
      window.addEventListener("drop", drop, { once: true });
    }),
  );
  bag.add(() => ghost?.remove());
}

function formatBytes(b: number): string {
  if (b < 1024) return `${b} B`;
  if (b < 1024 * 1024) return `${(b / 1024).toFixed(0)} KB`;
  if (b < 1024 * 1024 * 1024) return `${(b / 1024 / 1024).toFixed(1)} MB`;
  return `${(b / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

// ---- Q-66 密码侦探（非密码框输入密码特征 → 橙色提醒；隐私域，但同为输入框观察）----
function mountPasswordDetective(): void {
  bag.add(
    on(window, "input", (e) => {
      if (!ctxRef?.on("Q-66")) return;
      const t = e.target as HTMLInputElement;
      if (!(t instanceof HTMLInputElement) || t.type === "password" || t.type === "search") return;
      const suspicious = t.value.length >= 8 && (/(sk-|ghp_|gho_|AKIA|eyJ)/.test(t.value) || /^[^\s]{12,64}$/.test(t.value));
      t.classList.toggle("singu-secret-warn", suspicious);
    }),
  );
  bag.add(() => document.querySelectorAll(".singu-secret-warn").forEach((el) => el.classList.remove("singu-secret-warn")));
}

export function filesDomain(): DomainController {
  return {
    domain: "files",
    mount(ctx: DomainCtx) {
      ctxRef = ctx;
      bag = new UnsubBag();
      domReady(() => {
        mountWeightFeel();
        mountPasswordDetective();
        // Q-37 篝火 / Q-38 批量属性 / Q-40 命名建议 / Q-42 快检 / Q-43 时间透镜
        // 由 overlay 工具窗承载（Rust 命令实时执行），此处不重复装观察器。
      });
    },
    unmount() {
      bag.run();
      document.querySelectorAll(".singu-dragweight").forEach((e) => e.remove());
    },
  };
}
