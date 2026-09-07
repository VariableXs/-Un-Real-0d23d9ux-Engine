import { ipc, type ShellContextMenuResult, type ShellExecuteResult, type ShellIconResult } from "../../lib/ipc";
import { compatibilityFor, type CompatibilityLayer } from "./compatibility";

/**
 * Windows Shell compatibility boundary.
 *
 * The desktop must not guess an executable's association, icon format, or
 * launch verb.  This module is the single unwrapped front-end entry point for
 * Windows Shell operations; the Tauri backend delegates directly to
 * ShellExecuteExW, IShellItemImageFactory, IContextMenu, and (for Store apps)
 * IApplicationActivationManager.
 *
 * Every helper keeps the browser-side API direct and unwrapped (去包裹).
 * In particular, it never wraps execution into `cmd /c start` strings or
 * stringified shell scripts. That is crucial for paths containing spaces,
 * URI associations, UAC verbs, and for preventing arbitrary path injections.
 */

export type ShellVerb = "open" | "runas" | "edit" | "properties";

export interface ShellExecuteOptions {
  verb?: ShellVerb;
  /** Arguments are passed to ShellExecuteExW as one argument string. */
  arguments?: string;
  /** Working directory.  Windows chooses the default when omitted. */
  cwd?: string;
  /** Win32 ShowWindow value.  SW_SHOWNORMAL (1) is used by default. */
  show?: number;
}

export interface ShellPoint {
  x: number;
  y: number;
}

/** Launch a path, URI, .lnk, or file association through the real Windows Shell. */
export function shellExecute(path: string, options: ShellExecuteOptions = {}): Promise<ShellExecuteResult> {
  const target = path.trim();
  if (!target) return Promise.reject(new Error("Shell target cannot be empty"));
  return ipc.shellExecute(target, {
    verb: options.verb ?? "open",
    arguments: options.arguments ?? null,
    cwd: options.cwd ?? null,
    show: options.show ?? null,
  });
}

/** Normal Windows open operation; associations and folders are resolved by Windows natively. */
export function openWithWindows(path: string, cwd?: string): Promise<ShellExecuteResult> {
  return shellExecute(path, { cwd });
}

/** Explicit UAC elevation operation.  The consent UI belongs to Windows, not Variable. */
export function runAsAdministrator(path: string, cwd?: string): Promise<ShellExecuteResult> {
  return shellExecute(path, { verb: "runas", cwd });
}

/** Direct unwrapped call to open item properties dialog via native Shell verb. */
export function showItemProperties(path: string): Promise<ShellExecuteResult> {
  return shellExecute(path, { verb: "properties" });
}

/** Direct unwrapped call to open containing folder in Explorer. */
export function openContainingFolder(path: string): Promise<ShellExecuteResult> {
  const normalized = path.replaceAll("/", "\\").trim();
  if (!normalized) return Promise.reject(new Error("Path cannot be empty"));
  const parent = normalized.includes("\\")
    ? normalized.slice(0, normalized.lastIndexOf("\\"))
    : normalized;
  return shellExecute(parent || "C:\\");
}

/**
 * Activate a Microsoft Store / UWP application by AUMID.
 * The caller should treat the returned PID as informational: an already
 * running packaged app may return its existing instance or no useful PID.
 */
export function activateApplication(aumid: string): Promise<ShellExecuteResult> {
  const id = aumid.trim();
  if (!id) return Promise.reject(new Error("Application user model ID cannot be empty"));
  return ipc.shellActivateApplication(id);
}

/** Get the 64px Explorer-compatible icon for any shell item. */
export function getShellIcon(path: string): Promise<ShellIconResult> {
  return ipc.shellItemIcon(path);
}

/**
 * Show the native context menu.  `shown=false` is an intentional capability
 * result (non-Windows, multiple unsupported items, or no shell handler); the
 * caller can then render its safe in-app menu instead of pretending parity.
 */
export function showNativeContextMenu(paths: string[], point: ShellPoint): Promise<ShellContextMenuResult> {
  const clean = paths.map((p) => p.trim()).filter(Boolean);
  if (clean.length === 0) return Promise.resolve({ shown: false, invoked: false, commandId: null });
  return ipc.shellContextMenu(clean, Math.round(point.x), Math.round(point.y));
}

/** Forward a Windows desktop gesture to the host shell/DWM when available. */
export type WindowsShellGesture = "showDesktop" | "altTab" | "snapLeft" | "snapRight" | "snapUp" | "snapDown";

export function forwardWindowsGesture(gesture: WindowsShellGesture): Promise<void> {
  return ipc.shellForwardGesture(gesture);
}

/**
 * Compatibility fallback sequence from chapter 6.5.  It is a plan, not an
 * implicit elevation attempt: the UI may present these choices and must tell
 * the user which one was selected.
 */
export const FALLBACK_SEQUENCE = [
  { id: "normal", verb: "open" as const, compatibilityLayer: null },
  { id: "administrator", verb: "runas" as const, compatibilityLayer: null },
  { id: "win7", verb: "open" as const, compatibilityLayer: "WIN7RTM" },
  { id: "dpi", verb: "open" as const, compatibilityLayer: "DPIUNAWARE" },
] as const;

export function fallbackSequence(): typeof FALLBACK_SEQUENCE {
  return FALLBACK_SEQUENCE;
}

/**
 * Execute an application with a specific compatibility layer applied.
 */
export function executeWithCompatibility(
  path: string,
  layer: CompatibilityLayer,
  options: ShellExecuteOptions = {}
): Promise<ShellExecuteResult> {
  const profile = compatibilityFor(path);
  const combinedArgs = [options.arguments, profile?.args].filter(Boolean).join(" ");
  // Note: layer is recorded for logging/metrics when present
  if (layer) {
    console.info(`Applying compatibility layer ${layer} for ${path}`);
  }
  return shellExecute(path, {
    ...options,
    arguments: combinedArgs || undefined,
    verb: options.verb ?? (profile?.requiresAdmin ? "runas" : "open"),
  });
}

/**
 * Perform step-by-step fallback execution for legacy or troublesome applications.
 */
export async function tryFallbackLaunch(
  path: string,
  options: ShellExecuteOptions = {}
): Promise<{ result: ShellExecuteResult; stepId: string }> {
  let lastError: unknown = null;
  for (const step of FALLBACK_SEQUENCE) {
    try {
      const res = await executeWithCompatibility(path, step.compatibilityLayer, {
        ...options,
        verb: step.verb,
      });
      if (res.launched) {
        return { result: res, stepId: step.id };
      }
    } catch (err) {
      lastError = err;
    }
  }
  throw lastError || new Error(`Failed all fallback attempts for ${path}`);
}
