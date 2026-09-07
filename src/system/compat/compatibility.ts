/**
 * Chapter 6.5 compatibility database.  This is intentionally data, not a
 * second launcher: ShellProxy remains the source of truth for opening an
 * item and these entries only describe an application's known preference.
 */

export type CompatibilityLayer = "WIN7RTM" | "DPIUNAWARE" | null;

export interface CompatibilityProfile {
  args: string;
  compatibility: "WIN10" | "WIN7" | "native";
  dpi: "aware" | "unaware";
  note?: string;
  /** The UI can show this before asking for a UAC retry. */
  requiresAdmin?: boolean;
}

export const COMPATIBILITY_DATABASE: Record<string, CompatibilityProfile> = {
  "blender.exe": { args: "", compatibility: "WIN10", dpi: "aware" },
  "photoshop.exe": { args: "", compatibility: "native", dpi: "aware" },
  "wallpaper32.exe": {
    args: "",
    compatibility: "native",
    dpi: "aware",
    note: "CEF/GPU conflict: use the static wallpaper fallback when required.",
  },
  "wallpaper64.exe": {
    args: "",
    compatibility: "native",
    dpi: "aware",
    note: "CEF/GPU conflict: use the static wallpaper fallback when required.",
  },
  "steam.exe": {
    args: "",
    compatibility: "native",
    dpi: "aware",
    note: "Anti-cheat games may require native B-mode and an independent window.",
  },
};

function basename(path: string): string {
  const normalized = path.replaceAll("/", "\\");
  return normalized.slice(normalized.lastIndexOf("\\") + 1).toLowerCase();
}

export function compatibilityFor(path: string): CompatibilityProfile | null {
  return COMPATIBILITY_DATABASE[basename(path)] ?? null;
}

/** Environment values for a user-approved compatibility retry. */
export function compatibilityEnvironment(layer: CompatibilityLayer): Record<string, string> {
  return layer ? { __COMPAT_LAYER: layer } : {};
}
