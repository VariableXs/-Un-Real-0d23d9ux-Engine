import { describe, expect, it } from "vitest";
import { exportSkeleton, iconResource, PREVIEW_KEYS, scrubSvg, validateVicon } from "../vicon";

const okPack = JSON.stringify({
  format: "vicon",
  version: 1,
  name: "测试包",
  icons: {
    "desktop:sys-recycle": "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect/></svg>",
    "start:app-write": "data:image/png;base64,iVBORw0KGgo=",
    "file:.md": "<svg><circle/></svg>",
  },
});

describe("validateVicon", () => {
  it("合法包通过", () => {
    const r = validateVicon(okPack);
    expect(r.ok).toBe(true);
    expect(r.data?.name).toBe("测试包");
    expect(Object.keys(r.data?.icons ?? {})).toHaveLength(3);
  });

  it("坏 JSON / 非对象", () => {
    expect(validateVicon("{nope").ok).toBe(false);
    expect(validateVicon("[1]").ok).toBe(false);
  });

  it("format/version/name 校验", () => {
    const bad = JSON.stringify({ format: "icons", version: 2, name: "", icons: { "a:b": "<svg/>" } });
    expect(validateVicon(bad).errors.join()).toContain("format");
  });

  it("键名规范：须带区域段", () => {
    const bad = JSON.stringify({ format: "vicon", version: 1, name: "x", icons: { recycle: "<svg/>" } });
    const r = validateVicon(bad);
    expect(r.ok).toBe(false);
    expect(r.errors.join()).toContain("键名不规范");
  });

  it("资源形态：仅内联 SVG 或 data:image", () => {
    const bad = JSON.stringify({ format: "vicon", version: 1, name: "x", icons: { "a:b": "http://evil/x.png" } });
    expect(validateVicon(bad).ok).toBe(false);
    const bad2 = JSON.stringify({ format: "vicon", version: 1, name: "x", icons: { "a:b": "data:text/html,<b>" } });
    expect(validateVicon(bad2).ok).toBe(false);
  });

  it("空 icons 拒绝", () => {
    const bad = JSON.stringify({ format: "vicon", version: 1, name: "x", icons: {} });
    expect(validateVicon(bad).ok).toBe(false);
  });

  it("5MB 上限", () => {
    const big = " ".repeat(5 * 1024 * 1024 + 1);
    expect(validateVicon(big).ok).toBe(false);
  });
});

describe("scrubSvg 清洗", () => {
  it("去 script / on* 事件 / javascript: 链接", () => {
    const dirty = `<svg onload="alert(1)"><script>alert(2)</script><a href="javascript:alert(3)">x</a></svg>`;
    const clean = scrubSvg(dirty);
    expect(clean).not.toContain("script");
    expect(clean).not.toContain("onload");
    expect(clean).not.toContain("javascript:");
    expect(clean).toContain("<svg");
  });
});

describe("iconResource / 骨架导出", () => {
  it("区分 svg 与 img", () => {
    expect(iconResource("<svg><g/></svg>").kind).toBe("svg");
    expect(iconResource("data:image/png;base64,x").kind).toBe("img");
  });
  it("骨架：无包时用 12 预览键模板", () => {
    const sk = exportSkeleton(null, "var");
    expect(sk.format).toBe("vicon");
    expect(sk.name).toContain("skeleton");
    expect(Object.keys(sk.icons)).toEqual(expect.arrayContaining(PREVIEW_KEYS.slice(0, 4)));
    expect(Object.values(sk.icons).every((v) => v === "")).toBe(true);
  });
  it("骨架：有包时保留键清空值", () => {
    const sk = exportSkeleton({ "desktop:sys-recycle": "<svg/>" }, "p");
    expect(sk.icons["desktop:sys-recycle"]).toBe("");
  });
});