import { describe, expect, it } from "vitest";
import { coerceSettings } from "../settings";

describe("阶段6/7 · 双域系统设置组 coerce（写坏值回落默认，绝不带病持久化）", () => {
  it("默认值 = 引导页 5s 进 VARIX、引擎关闭、均衡档（施工总案基线）", () => {
    const s = coerceSettings({});
    expect(s.bootDefaultOs).toBe("varix");
    expect(s.bootTimeoutSec).toBe(5);
    expect(s.bootMenuVisible).toBe(true);
    expect(s.channelPolicy).toBe("auto");
    expect(s.resProfile).toBe("balanced");
    expect(s.engineEnabled).toBe(false);
    expect(s.engineHibernateIdleMin).toBe(15);
    expect(s.engineQuality).toBe("balanced");
    expect(s.engineQuotaCores).toBe(0);
    expect(s.engineQualityCustom).toBeNull();
  });

  it("引导行为三项：合法值生效；非法值回落", () => {
    const ok = coerceSettings({ bootDefaultOs: "windows", bootTimeoutSec: "30", bootMenuVisible: "0" });
    expect(ok.bootDefaultOs).toBe("windows");
    expect(ok.bootTimeoutSec).toBe(30);
    expect(ok.bootMenuVisible).toBe(false);
    const bad = coerceSettings({ bootDefaultOs: "macos", bootTimeoutSec: "7", bootMenuVisible: "maybe" });
    expect(bad.bootDefaultOs).toBe("varix");
    expect(bad.bootTimeoutSec).toBe(5);
    expect(bad.bootMenuVisible).toBe(true); // 非 "0" 一律视为开（与既有布尔口径一致）
  });

  it("通道与资源档位：白名单外回落 auto/balanced", () => {
    expect(coerceSettings({ channelPolicy: "engine-first" }).channelPolicy).toBe("engine-first");
    expect(coerceSettings({ channelPolicy: "everything" }).channelPolicy).toBe("auto");
    expect(coerceSettings({ resProfile: "gaming" }).resProfile).toBe("gaming");
    expect(coerceSettings({ resProfile: "turbo" }).resProfile).toBe("balanced");
  });

  it("引擎组：休眠阈值/配额钳制，画质自定义非法 JSON 回落 null", () => {
    expect(coerceSettings({ engineHibernateIdleMin: "999" }).engineHibernateIdleMin).toBe(240);
    expect(coerceSettings({ engineQuotaCores: "-3" }).engineQuotaCores).toBe(0);
    expect(coerceSettings({ engineQuality: "gaming" }).engineQuality).toBe("gaming");
    expect(coerceSettings({ engineQuality: "ultra" }).engineQuality).toBe("balanced");
    const custom = coerceSettings({
      engineQualityCustom: JSON.stringify({ fps: 45, bitrateKbps: 12000, codecPreset: "speed" }),
    });
    expect(custom.engineQualityCustom).toMatchObject({ fps: 45, bitrateKbps: 12000 });
    expect(coerceSettings({ engineQualityCustom: "{bad json" }).engineQualityCustom).toBeNull();
    expect(coerceSettings({ engineQualityCustom: "[]" }).engineQualityCustom).toBeNull();
    // 白名单外 codecPreset 丢弃，其余字段保留
    const partial = coerceSettings({ engineQualityCustom: JSON.stringify({ fps: 60, codecPreset: "h266" }) });
    expect(partial.engineQualityCustom).toEqual({ fps: 60 });
  });
});
