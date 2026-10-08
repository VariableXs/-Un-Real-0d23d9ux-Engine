import { beforeEach, describe, expect, it } from "vitest";
import {
  embedFocusLinkEnabled,
  embedVisualActive,
  embedFocusLinkStore,
  noteEmbedInteraction,
  noteVwmFocusChange,
  resetEmbedFocusLink,
  setEmbedFocusLinkEnabled,
} from "../embedFocusLink";

describe("V-27 嵌入窗口焦点联动", () => {
  beforeEach(() => {
    resetEmbedFocusLink();
    setEmbedFocusLinkEnabled(true);
  });

  it("默认开启", () => {
    expect(embedFocusLinkEnabled()).toBe(true);
  });

  it("嵌入交互 → 宿主标题栏进入激活态视觉", () => {
    noteEmbedInteraction("w1");
    expect(embedVisualActive("w1")).toBe(true);
    expect(embedVisualActive("w2")).toBe(false);
  });

  it("关闭联动后交互不再产生视觉态（默认开，可关）", () => {
    setEmbedFocusLinkEnabled(false);
    noteEmbedInteraction("w1");
    expect(embedVisualActive("w1")).toBe(false);
  });

  it("仅视觉联动：不产生任何真实焦点副作用（store 只有 visualActiveId）", () => {
    noteEmbedInteraction("w1");
    expect(Object.keys(embedFocusLinkStore.getState())).toEqual(["visualActiveId"]);
  });

  it("真实焦点移动到其它窗口 → 视觉态让位", () => {
    noteEmbedInteraction("w1");
    noteVwmFocusChange("w2");
    expect(embedVisualActive("w1")).toBe(false);
  });

  it("真实焦点移动到同一窗口 → 视觉态保持（等价于真实激活）", () => {
    noteEmbedInteraction("w1");
    noteVwmFocusChange("w1");
    expect(embedVisualActive("w1")).toBe(true);
  });

  it("真实焦点清空（null）→ 视觉态让位", () => {
    noteEmbedInteraction("w1");
    noteVwmFocusChange(null);
    expect(embedVisualActive("w1")).toBe(false);
  });

  it("无视觉态时焦点变化是空操作（不触发订阅）", () => {
    let fired = 0;
    const un = embedFocusLinkStore.subscribe(() => { fired += 1; });
    noteVwmFocusChange("w9");
    un();
    expect(fired).toBe(0);
  });
});
