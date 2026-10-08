import { describe, expect, it } from "vitest";
import { imTotal, parseUnreadFromTitle } from "../imbadge";

/** M-14：IM 未读聚合（标题计数与 Rust imwatch 同口径；Σ 聚合）。 */
describe("imbadge (M-14)", () => {
  it("parseUnreadFromTitle：半角/全角/方括号三种标记", () => {
    expect(parseUnreadFromTitle("微信 (3)")).toBe(3);
    expect(parseUnreadFromTitle("Telegram（12）")).toBe(12);
    expect(parseUnreadFromTitle("Discord【5】")).toBe(5);
    expect(parseUnreadFromTitle("无未读")).toBeNull();
  });

  it("imTotal 聚合各 IM 之和（99+ 封顶由 UI 层处理）", () => {
    expect(imTotal({ wx: 3, tg: 12 })).toBe(15);
    expect(imTotal({})).toBe(0);
  });
});
