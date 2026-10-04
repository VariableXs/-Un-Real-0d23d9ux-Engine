import { describe, expect, it } from "vitest";
import { defaultVariant } from "../tokens";
import { buildThumbnail, parseVtheme, serializeVtheme, validateVtheme } from "../vtheme";

function goodFile(): Record<string, unknown> {
  return {
    format: "vtheme",
    version: 1,
    name: "测试主题",
    variants: { dark: defaultVariant("dark") },
  };
}

describe("validateVtheme", () => {
  it("合法文件通过", () => {
    const r = validateVtheme(goodFile());
    expect(r.ok).toBe(true);
    expect(r.data?.name).toBe("测试主题");
    expect(r.issues).toHaveLength(0);
  });

  it("format/version/name 错误", () => {
    const bad = { ...goodFile(), format: "theme", version: 2, name: "" };
    const r = validateVtheme(bad);
    expect(r.ok).toBe(false);
    const fields = r.issues.filter((i) => i.level === "error").map((i) => i.field);
    expect(fields).toContain("format");
    expect(fields).toContain("version");
    expect(fields).toContain("name");
  });

  it("未知 token 键 → conflict（不阻断）", () => {
    const f = goodFile();
    const dark = (f.variants as Record<string, { colors: Record<string, string> }>).dark as { colors: Record<string, string> };
    dark.colors["--brand-new-token"] = "#123456";
    const r = validateVtheme(f);
    expect(r.ok).toBe(true);
    const conflict = r.issues.find((i) => i.level === "conflict");
    expect(conflict?.field).toContain("--brand-new-token");
  });

  it("未知变体键 → conflict", () => {
    const f = goodFile();
    (f.variants as Record<string, unknown>)["sepia"] = defaultVariant("dark");
    const r = validateVtheme(f);
    expect(r.ok).toBe(true);
    expect(r.issues.some((i) => i.level === "conflict" && i.field === "variants.sepia")).toBe(true);
  });

  it("坏颜色值 → error", () => {
    const f = goodFile();
    const dark = (f.variants as Record<string, { colors: Record<string, string> }>).dark as { colors: Record<string, string> };
    dark.colors["--accent"] = "rgb(1,2,3)";
    expect(validateVtheme(f).ok).toBe(false);
  });

  it("坏形档 → error；至少一个变体", () => {
    const f = goodFile();
    const dark = (f.variants as Record<string, { shape: Record<string, string> }>).dark as { shape: Record<string, string> };
    dark.shape.radius = "wobbly";
    expect(validateVtheme(f).ok).toBe(false);
    const g = goodFile();
    g.variants = {};
    expect(validateVtheme(g).ok).toBe(false);
  });

  it("thumbnail 非 data:image → warning 不阻断", () => {
    const f = { ...goodFile(), thumbnail: "http://evil.example/x.png" };
    const r = validateVtheme(f);
    expect(r.ok).toBe(true);
    expect(r.issues.some((i) => i.level === "warning" && i.field === "thumbnail")).toBe(true);
  });

  it("非对象输入", () => {
    expect(validateVtheme(null).ok).toBe(false);
    expect(validateVtheme([1, 2]).ok).toBe(false);
    expect(validateVtheme("vtheme").ok).toBe(false);
  });
});

describe("parseVtheme 导入导出往返", () => {
  it("serialize → parse 一致（含三变体与 thumbnail）", () => {
    const file = {
      format: "vtheme" as const,
      version: 1 as const,
      name: "roundtrip",
      variants: { light: defaultVariant("light"), dark: defaultVariant("dark"), hc: defaultVariant("hc") },
      thumbnail: buildThumbnail(defaultVariant("dark"), "roundtrip"),
    };
    const r = parseVtheme(serializeVtheme(file));
    expect(r.ok).toBe(true);
    expect(r.data?.variants.dark?.colors["--bg-canvas"]).toBe(file.variants.dark.colors["--bg-canvas"]);
    expect(r.data?.variants.light?.shape.density).toBe("regular");
    expect(r.data?.thumbnail).toBe(file.thumbnail);
  });

  it("坏 JSON 报错不抛异常", () => {
    const r = parseVtheme("{oops");
    expect(r.ok).toBe(false);
    expect(r.issues[0]?.level).toBe("error");
  });
});