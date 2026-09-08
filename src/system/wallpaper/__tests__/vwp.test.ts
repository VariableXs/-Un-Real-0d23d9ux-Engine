import { describe, expect, it } from "vitest";
import {
  buildVwp,
  dataUrlToText,
  downloadVwp,
  parseVwpText,
  textToDataUrl,
  validateVwp,
  VWP_MAX_RESOURCE_BYTES,
  type VwpPack,
} from "../vwp";

const base = (): VwpPack =>
  buildVwp({
    kind: "shader",
    manifest: { name: "demo", netAccess: false },
    uniforms: { uSpeed: 1.5 },
    resources: { "shader.frag": textToDataUrl("void main(){}", "text/plain") },
  });

describe("N-07 .vwp 校验（validateVwp）", () => {
  it("合法包通过；导出→JSON 往返→导入一致（验收②）", () => {
    const pack = base();
    expect(validateVwp(pack)).toMatchObject({ ok: true });
    const round = JSON.parse(JSON.stringify(pack)) as unknown;
    const res = validateVwp(round);
    expect(res.ok).toBe(true);
    if (res.ok) expect(res.pack).toEqual(pack);
  });

  it("红线：resources 含远程 URL（http/https/file/协议相对）→ resource-url 拒绝", () => {
    for (const url of [
      "https://cdn.example.com/a.png",
      "http://evil.example/x.html",
      "file:///C:/windows/a.png",
      "//cdn.example.com/a.png",
    ]) {
      const res = validateVwp({ ...base(), resources: { "x.png": url } });
      expect(res.ok).toBe(false);
      if (!res.ok) expect(res.errors).toContain("resource-url");
    }
  });

  it("红线：非 data: 前缀与未允许的 data 类型拒绝", () => {
    const res1 = validateVwp({ ...base(), resources: { a: "C:\\local\\a.png" } });
    expect(res1.ok).toBe(false);
    const res2 = validateVwp({ ...base(), resources: { a: "data:application/octet-stream;base64,AAAA" } });
    expect(res2.ok).toBe(false);
    if (!res2.ok) expect(res2.errors).toContain("resource-type");
  });

  it("资源超 2MB → resource-size", () => {
    const big = "data:image/png;base64," + "A".repeat(Math.ceil((VWP_MAX_RESOURCE_BYTES * 4) / 3) + 8); // base64 长度≈3/4 字节，超限
    const res = validateVwp({ ...base(), resources: { "big.png": big } });
    expect(res.ok).toBe(false);
    if (!res.ok) expect(res.errors).toContain("resource-size");
  });

  it("format/version/kind/manifest/uniforms 各自拒绝", () => {
    expect(validateVwp({ ...base(), format: "zip" }).ok).toBe(false);
    expect(validateVwp({ ...base(), version: 2 }).ok).toBe(false);
    const badKind = validateVwp({ ...base(), kind: "video" });
    expect(badKind.ok).toBe(false);
    if (!badKind.ok) expect(badKind.errors).toContain("kind");
    expect(validateVwp({ ...base(), manifest: {} }).ok).toBe(false);
    expect(validateVwp({ ...base(), uniforms: { uX: "fast" } }).ok).toBe(false);
    expect(validateVwp("not even an object").ok).toBe(false);
  });

  it("parseVwpText：坏 JSON → format；合法文本 → ok", () => {
    expect(parseVwpText("{oops").ok).toBe(false);
    const pack = base();
    expect(parseVwpText(JSON.stringify(pack)).ok).toBe(true);
  });

  it("textToDataUrl / dataUrlToText 往返一致；非文本 data 返回 null", () => {
    const url = textToDataUrl("uniform float u; // 0..1", "text/plain");
    expect(url.startsWith("data:text/plain;base64,")).toBe(true);
    expect(dataUrlToText(url)).toBe("uniform float u; // 0..1");
    expect(dataUrlToText("data:image/png;base64,AAAA")).toBeNull();
  });

  it("downloadVwp 在无 DOM 环境安全 no-op（返回 false）", () => {
    expect(downloadVwp(base(), "demo.vwp")).toBe(false);
  });
});