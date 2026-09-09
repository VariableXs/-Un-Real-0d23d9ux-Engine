import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  currentItem,
  entryPatch,
  fromEngineItem,
  fromImageFile,
  loadPlaylist,
  loadPlaylistState,
  mergeLibrary,
  nextIndex,
  savePlaylist,
  savePlaylistState,
  WP_CENTER_PLAYLIST_CHANGED,
} from "../center/centerCore";

// 测试环境 window stub 只带定时器（src/test/setup.ts）——补事件三件套供广播断言
const w = window as unknown as Record<string, unknown>;
if (typeof w.dispatchEvent !== "function") {
  const bus = new EventTarget();
  w.dispatchEvent = (e: Event) => bus.dispatchEvent(e);
  w.addEventListener = (t: string, l: EventListener) => bus.addEventListener(t, l);
  w.removeEventListener = (t: string, l: EventListener) => bus.removeEventListener(t, l);
}

/** 壁纸中心核心数据层：合并去重 / 播放列表持久化 / 轮换索引 / 应用 patch。 */
describe("centerCore", () => {
  beforeEach(() => {
    localStorage.clear();
  });
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("mergeLibrary：按 path 去重（cur 固定最前不重复计数）", () => {
    const cur = currentItem({ imagePath: "C:/w/a.jpg", videoPath: "", htmlPath: "", shaderPath: "" });
    expect(cur?.kind).toBe("image");
    const lib = mergeLibrary([
      cur!,
      fromImageFile("C:/w/a.jpg"),      // 与当前重复 → 丢弃
      fromImageFile("C:/w/b.jpg"),
      fromImageFile("C:/w/b.jpg"),      // 自身重复 → 丢弃
    ]);
    expect(lib.map((x) => x.path)).toEqual(["C:/w/a.jpg", "C:/w/b.jpg"]);
  });

  it("fromEngineItem：kind 映射 + unsupported 保留展示", () => {
    const img = fromEngineItem({ id: "1", title: "T", kind: "image", file: "C:/x", preview: null, supported: true, source: "we" });
    const scene = fromEngineItem({ id: "2", title: "S", kind: "scene", file: "C:/y", preview: null, supported: false, source: "we" });
    expect(img.kind).toBe("image");
    expect(scene.kind).toBe("shader");
    expect(scene.supported).toBe(false);
  });

  it("currentItem：video 优先于 image（shader > web > video > image）", () => {
    expect(
      currentItem({ imagePath: "a.jpg", videoPath: "v.mp4", htmlPath: "", shaderPath: "" })?.kind,
    ).toBe("video");
    expect(
      currentItem({ imagePath: "a.jpg", videoPath: "v.mp4", htmlPath: "h.html", shaderPath: "" })?.kind,
    ).toBe("web");
    expect(
      currentItem({ imagePath: "", videoPath: "", htmlPath: "", shaderPath: "" }),
    ).toBeNull();
  });

  it("playlist：localStorage 持久化 + 变更事件广播", () => {
    const spy = vi.fn();
    window.addEventListener(WP_CENTER_PLAYLIST_CHANGED, spy);
    savePlaylist([{ title: "t", kind: "image", path: "C:/a.jpg" }]);
    expect(loadPlaylist()).toEqual([{ title: "t", kind: "image", path: "C:/a.jpg" }]);
    expect(spy).toHaveBeenCalledTimes(1);
    window.removeEventListener(WP_CENTER_PLAYLIST_CHANGED, spy);
  });

  it("playlistState：坏 JSON 回默认；保存回读", () => {
    localStorage.setItem("variable:wpcenter:playlist-state:v1", "{oops");
    expect(loadPlaylistState().intervalMin).toBe(15);
    savePlaylistState({ enabled: true, intervalMin: 30, shuffle: false, cursor: 2 });
    const st = loadPlaylistState();
    expect(st).toEqual({ enabled: true, intervalMin: 30, shuffle: false, cursor: 2 });
  });

  it("nextIndex：顺序前进 / 随机不等于当前 / 边界", () => {
    expect(nextIndex(5, 2, false)).toBe(3);
    expect(nextIndex(5, 4, false)).toBe(0);
    expect(nextIndex(1, 0, true)).toBe(0);
    expect(nextIndex(0, 0, false)).toBe(0);
    const i = nextIndex(9, 4, true);
    expect(i).toBeGreaterThanOrEqual(0);
    expect(i).toBeLessThan(9);
    expect(i).not.toBe(4);
  });

  it("entryPatch：kind → 壁纸模式映射", () => {
    expect(entryPatch({ title: "t", kind: "image", path: "a.jpg" })).toEqual({
      wallpaperMode: "living",
      customBg: { imagePath: "a.jpg" },
    });
    expect(entryPatch({ title: "t", kind: "video", path: "v.mp4" })).toEqual({
      wallpaperMode: "video",
      customBg: { videoPath: "v.mp4", playVideo: true },
    });
    expect(entryPatch({ title: "t", kind: "web", path: "h.html" }).wallpaperMode).toBe("web");
    expect(entryPatch({ title: "t", kind: "shader", path: "s.frag" }).wallpaperMode).toBe("shader");
  });
});
