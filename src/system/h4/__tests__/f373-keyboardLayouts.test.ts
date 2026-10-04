import { describe, expect, it } from "vitest";
import { DEFAULT_STATE, activeLayout, addLayout, loadState, persistState, removeLayout, reorder, rotate, setOption, simulateMixedTyping, threePlaceSync, type KeyboardLayout, type LayoutState, type KeystrokeEvent, type SwitchMarker } from "../f373-keyboardLayouts";
import { __clearMem, memStore } from "../internal/store";

function sp(): KeyboardLayout {
  return { id: "shuangpin", name: "中文（双拼）", options: {} };
}

describe("F373 键盘布局管理", () => {
  it("增删即时反映：添加入表尾、删除收拢索引、至少保留一个", () => {
    let s: LayoutState = DEFAULT_STATE;
    const added = addLayout(s, sp());
    expect(added.ok).toBe(true);
    s = added.state;
    expect(s.layouts.map((l) => l.id)).toEqual(["pinyin", "english", "shuangpin"]);
    const dup = addLayout(s, sp());
    expect(dup.ok).toBe(false);
    const rm = removeLayout(s, "english");
    expect(rm.state.layouts.map((l) => l.id)).toEqual(["pinyin", "shuangpin"]);
    expect(removeLayout({ ...DEFAULT_STATE, layouts: [DEFAULT_STATE.layouts[0]!] }, "pinyin").ok).toBe(false);
    expect(removeLayout(s, "ghost").ok).toBe(false);
  });

  it("轮切顺序=设置顺序：reorder 后 rotate 沿新序走", () => {
    let s: LayoutState = { ...addLayout(DEFAULT_STATE, sp()).state };
    expect(rotate(s).layouts[1]!.id).toBe("english");
    s = reorder(s, 2, 0); // 双拼提到最前
    expect(s.layouts[0]!.id).toBe("shuangpin");
    const after = rotate({ ...s, activeIndex: 0 });
    expect(after.layouts[after.activeIndex]!.id).toBe("pinyin");
    expect(rotate({ ...s, activeIndex: 0 }, -1).layouts[2]!.id).toBe("english"); // 反向回绕
    expect(reorder(s, 5, 9)).toEqual(s); // 非法 reorder 无害（原样返回）
  });

  it("布局级选项隔离：pinyin 开双拼相关选项不影响 english", () => {
    let s: LayoutState = DEFAULT_STATE;
    s = setOption(s, "fuzzy", "on"); // 当前 pinyin
    s = rotate(s); // 切到 english
    expect(activeLayout(s).id).toBe("english");
    expect(activeLayout(s).options.fuzzy).toBeUndefined();
    s = rotate(s, -1); // 切回 pinyin
    expect(activeLayout(s).options.fuzzy).toBe("on");
  });

  it("切换不吞键：100 键快速混切零丢失（热键本身除外）", () => {
    const keys: KeystrokeEvent[] = Array.from({ length: 100 }, (_, i) => ({ seq: i, key: `k${i}`, atMs: i * 30 }));
    const switches: SwitchMarker[] = [
      { atSeq: 20, toLayout: "english" },
      { atSeq: 50, toLayout: "pinyin" },
      { atSeq: 77, toLayout: "shuangpin" },
    ];
    const r = simulateMixedTyping(keys, switches);
    expect(r.delivered).toHaveLength(97);
    expect(r.dropped.map((k) => k.seq)).toEqual([20, 50, 77]); // 只有热键本身被消耗
  });

  it("三处同步：读同一状态（一致性判据）", () => {
    const s = rotate({ ...addLayout(DEFAULT_STATE, sp()).state });
    const sync = threePlaceSync(s);
    expect(sync.consistent).toBe(true);
    expect(sync.settings).toBe(sync.taskbar);
    expect(sync.taskbar).toBe(sync.ime);
  });

  it("持久化 round-trip；损坏回退默认", () => {
    __clearMem();
    const s = memStore();
    const st = addLayout(DEFAULT_STATE, sp()).state;
    expect(persistState(st, s)).toBe(true);
    expect(loadState(s).layouts).toHaveLength(3);
    s.setItem("variable:h4:f373:layouts", "not-json");
    expect(loadState(s)).toEqual(DEFAULT_STATE);
  });
});
