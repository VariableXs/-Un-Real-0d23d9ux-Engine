import { describe, expect, it, vi, beforeEach } from "vitest";
import {
  shellExecute,
  openWithWindows,
  runAsAdministrator,
  showItemProperties,
  openContainingFolder,
  activateApplication,
  getShellIcon,
  showNativeContextMenu,
  forwardWindowsGesture,
  FALLBACK_SEQUENCE,
  fallbackSequence,
  executeWithCompatibility,
  tryFallbackLaunch,
} from "../ShellProxy";
import {
  COMPATIBILITY_DATABASE,
  compatibilityFor,
  compatibilityEnvironment,
} from "../compatibility";
import { ipc } from "../../../lib/ipc";

vi.mock("../../../lib/ipc", () => ({
  ipc: {
    shellExecute: vi.fn(),
    shellActivateApplication: vi.fn(),
    shellItemIcon: vi.fn(),
    shellContextMenu: vi.fn(),
    shellForwardGesture: vi.fn(),
  },
}));

describe("AI-3 ShellProxy Unwrapped Shell Proxy", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(ipc.shellExecute).mockResolvedValue({
      launched: true,
      processId: 1234,
      backend: "shellExecuteEx",
      errorCode: null,
    });
    vi.mocked(ipc.shellActivateApplication).mockResolvedValue({
      launched: true,
      processId: 5678,
      backend: "applicationActivationManager",
      errorCode: null,
    });
    vi.mocked(ipc.shellItemIcon).mockResolvedValue({
      dataUrl: "data:image/png;base64,test",
      size: 64,
      source: "shellItemImageFactory",
    });
    vi.mocked(ipc.shellContextMenu).mockResolvedValue({
      shown: true,
      invoked: true,
      commandId: 0,
    });
    vi.mocked(ipc.shellForwardGesture).mockResolvedValue(undefined);
  });

  it("rejects empty shell targets", async () => {
    await expect(shellExecute("  ")).rejects.toThrow("Shell target cannot be empty");
  });

  it("calls ipc.shellExecute with normalized options", async () => {
    const res = await shellExecute("C:\\Program Files\\Blender\\blender.exe", {
      arguments: "--background",
      cwd: "C:\\Program Files\\Blender",
    });
    expect(ipc.shellExecute).toHaveBeenCalledWith("C:\\Program Files\\Blender\\blender.exe", {
      verb: "open",
      arguments: "--background",
      cwd: "C:\\Program Files\\Blender",
      show: null,
    });
    expect(res.launched).toBe(true);
    expect(res.processId).toBe(1234);
  });

  it("openWithWindows invokes shellExecute with default open verb", async () => {
    await openWithWindows("C:\\Docs\\report.pdf");
    expect(ipc.shellExecute).toHaveBeenCalledWith("C:\\Docs\\report.pdf", {
      verb: "open",
      arguments: null,
      cwd: null,
      show: null,
    });
  });

  it("runAsAdministrator invokes shellExecute with runas verb", async () => {
    await runAsAdministrator("C:\\Tools\\setup.exe");
    expect(ipc.shellExecute).toHaveBeenCalledWith("C:\\Tools\\setup.exe", {
      verb: "runas",
      arguments: null,
      cwd: null,
      show: null,
    });
  });

  it("showItemProperties calls properties verb", async () => {
    await showItemProperties("C:\\Windows\\explorer.exe");
    expect(ipc.shellExecute).toHaveBeenCalledWith("C:\\Windows\\explorer.exe", {
      verb: "properties",
      arguments: null,
      cwd: null,
      show: null,
    });
  });

  it("openContainingFolder extracts parent folder correctly", async () => {
    await openContainingFolder("C:\\Program Files\\Blender\\blender.exe");
    expect(ipc.shellExecute).toHaveBeenCalledWith("C:\\Program Files\\Blender", {
      verb: "open",
      arguments: null,
      cwd: null,
      show: null,
    });
  });

  it("activateApplication validates AUMID and invokes ipc", async () => {
    await expect(activateApplication("   ")).rejects.toThrow("Application user model ID cannot be empty");
    const res = await activateApplication("Microsoft.WindowsCalculator_8wekyb3d8bbwe!App");
    expect(ipc.shellActivateApplication).toHaveBeenCalledWith(
      "Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"
    );
    expect(res.processId).toBe(5678);
  });

  it("getShellIcon retrieves 64px icon from ipc", async () => {
    const icon = await getShellIcon("C:\\Windows\\notepad.exe");
    expect(ipc.shellItemIcon).toHaveBeenCalledWith("C:\\Windows\\notepad.exe");
    expect(icon.size).toBe(64);
    expect(icon.source).toBe("shellItemImageFactory");
  });

  it("showNativeContextMenu filters paths and calls ipc with rounded coordinates", async () => {
    const res = await showNativeContextMenu(["C:\\file.txt"], { x: 100.4, y: 200.7 });
    expect(ipc.shellContextMenu).toHaveBeenCalledWith(["C:\\file.txt"], 100, 201);
    expect(res.shown).toBe(true);

    const empty = await showNativeContextMenu(["   "], { x: 0, y: 0 });
    expect(empty.shown).toBe(false);
  });

  it("forwardWindowsGesture forwards DWM gestures to ipc", async () => {
    await forwardWindowsGesture("showDesktop");
    expect(ipc.shellForwardGesture).toHaveBeenCalledWith("showDesktop");
  });

  it("provides complete fallback sequence definitions", () => {
    const seq = fallbackSequence();
    expect(seq).toEqual(FALLBACK_SEQUENCE);
    expect(seq.length).toBe(4);
    expect(seq[0].id).toBe("normal");
    expect(seq[1].id).toBe("administrator");
    expect(seq[2].compatibilityLayer).toBe("WIN7RTM");
    expect(seq[3].compatibilityLayer).toBe("DPIUNAWARE");
  });

  it("executeWithCompatibility combines profile args and layer", async () => {
    await executeWithCompatibility("blender.exe", "WIN7RTM", { arguments: "--factory-startup" });
    expect(ipc.shellExecute).toHaveBeenCalledWith("blender.exe", {
      verb: "open",
      arguments: "--factory-startup",
      cwd: null,
      show: null,
    });
  });

  it("tryFallbackLaunch loops through sequence until success", async () => {
    const { result, stepId } = await tryFallbackLaunch("C:\\app.exe");
    expect(result.launched).toBe(true);
    expect(stepId).toBe("normal");
  });
});

describe("AI-3 Compatibility Database", () => {
  it("resolves compatibility profile for known executables", () => {
    const blender = compatibilityFor("C:\\Program Files\\Blender\\blender.exe");
    expect(blender).not.toBeNull();
    expect(blender?.compatibility).toBe("WIN10");

    const photoshop = compatibilityFor("d:\\apps\\photoshop.exe");
    expect(photoshop?.compatibility).toBe("native");

    const unknown = compatibilityFor("unknown_app.exe");
    expect(unknown).toBeNull();
  });

  it("generates correct environment mapping for compatibility layers", () => {
    expect(compatibilityEnvironment("WIN7RTM")).toEqual({ __COMPAT_LAYER: "WIN7RTM" });
    expect(compatibilityEnvironment("DPIUNAWARE")).toEqual({ __COMPAT_LAYER: "DPIUNAWARE" });
    expect(compatibilityEnvironment(null)).toEqual({});
  });

  it("includes expected apps in database", () => {
    expect(COMPATIBILITY_DATABASE["wallpaper32.exe"]).toBeDefined();
    expect(COMPATIBILITY_DATABASE["wallpaper64.exe"]).toBeDefined();
    expect(COMPATIBILITY_DATABASE["steam.exe"]).toBeDefined();
  });
});
