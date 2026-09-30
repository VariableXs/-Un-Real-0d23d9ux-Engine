import { beforeEach, describe, expect, it } from "vitest";
import {
  CANDIDATE_CAP,
  RUN_ALIASES,
  RUN_HISTORY_CAP,
  RUN_HISTORY_KEY,
  addRunHistory,
  classifyRunInput,
  loadRunHistory,
  matchRunCandidates,
  parseRunHistory,
  saveRunHistory,
} from "../runParse";

/**
 * Z-28 运行对话框纯逻辑回归：
 * - classifyRunInput ≥20 条典型输入全分类正确（引号空格路径 / 环境变量 / UNC /
 *   盘符 / scheme URI / 可执行名 / 纯数字 unknown）
 * - RUN_ALIASES 别名表完整
 * - matchRunCandidates 排序（精确 → 别名字母序 → 历史新→旧）、去重、≤8 条
 * - 运行历史 ≤20 上限淘汰与持久化
 */

beforeEach(() => {
  globalThis.localStorage?.clear?.();
});

describe("classifyRunInput 输入分类", () => {
  const cases: Array<[string, "uri" | "path" | "exec" | "unknown", string]> = [
    // 空输入 / 纯空白
    ["", "unknown", ""],
    ["   ", "unknown", ""],
    // 引号包裹的含空格路径（normalized 剥离引号）
    ['"C:\\Program Files\\app.exe"', "path", "C:\\Program Files\\app.exe"],
    ['"C:\\My Docs\\report.docx"', "path", "C:\\My Docs\\report.docx"],
    // 常规绝对路径 / 盘符根 / 裸盘符 / 正斜杠
    ["C:\\Windows\\System32\\notepad.exe", "path", "C:\\Windows\\System32\\notepad.exe"],
    ["C:\\Program Files\\app.exe", "path", "C:\\Program Files\\app.exe"],
    ["X:\\", "path", "X:\\"],
    ["X:", "path", "X:"],
    ["C:/Users/x/notes.md", "path", "C:/Users/x/notes.md"],
    // 环境变量形态
    ["%TEMP%\\x.txt", "path", "%TEMP%\\x.txt"],
    ["%TEMP%", "path", "%TEMP%"],
    ["%ProgramData%\\Vendor\\app.cfg", "path", "%ProgramData%\\Vendor\\app.cfg"],
    // UNC
    ["\\\\server\\share\\doc", "path", "\\\\server\\share\\doc"],
    ["\\\\server\\share", "path", "\\\\server\\share"],
    // URI（scheme:// 与无斜杠 scheme）
    ["https://example.com/a?b=1", "uri", "https://example.com/a?b=1"],
    ["http://a.b", "uri", "http://a.b"],
    ["file:///C:/Users/x/readme.md", "uri", "file:///C:/Users/x/readme.md"],
    ["mailto:a@b.c", "uri", "mailto:a@b.c"],
    ["ms-settings:display", "uri", "ms-settings:display"],
    // 裸可执行名
    ["notepad", "exec", "notepad"],
    ["calc", "exec", "calc"],
    ["calc.exe", "exec", "calc.exe"],
    ["abc", "exec", "abc"],
    ["my tool", "exec", "my tool"],
    // 含分隔符的相对路径
    ["foo\\bar", "path", "foo\\bar"],
    ["subdir/file.txt", "path", "subdir/file.txt"],
    // 纯数字 / 不配对引号 → unknown（如实失败）
    ["123", "unknown", "123"],
    ['"abc', "unknown", '"abc'],
    ['""', "unknown", ""],
  ];

  it("≥20 条典型输入全部分类正确", () => {
    expect(cases.length).toBeGreaterThanOrEqual(20);
    for (const [raw, kind, normalized] of cases) {
      expect(classifyRunInput(raw)).toEqual({ kind, normalized });
    }
  });

  it("首尾空白被裁剪", () => {
    expect(classifyRunInput("  notepad  ")).toEqual({ kind: "exec", normalized: "notepad" });
    expect(classifyRunInput("\thttps://a.b\n")).toEqual({ kind: "uri", normalized: "https://a.b" });
  });

  it("单字母盘符不被误判为 URI（scheme 长度 ≥2）", () => {
    expect(classifyRunInput("D:\\work").kind).toBe("path");
    expect(classifyRunInput("D:").kind).toBe("path");
  });
});

describe("RUN_ALIASES 别名表", () => {
  it("18 个内部别名齐全且指向同名 VWM 工具/动作 id", () => {
    expect(Object.keys(RUN_ALIASES)).toHaveLength(18);
    const expectAliases: Record<string, string> = {
      calc: "calc",
      notes: "notes",
      calendar: "calendar",
      clipboard: "clipboard",
      taskman: "taskman",
      explorer: "explorer",
      settings: "settings",
      snapshot: "snapshot",
      rename: "rename",
      dupe: "dupe",
      space: "space",
      checksum: "checksum",
      clockhub: "clockhub",
      emoji: "emoji",
      magnifier: "magnifier",
      convert: "convert",
      sysinfo: "sysinfo",
      printqueue: "printqueue",
    };
    expect(RUN_ALIASES).toEqual(expectAliases);
  });

  it("键全部小写非空（大小写不敏感路由的前提）", () => {
    for (const k of Object.keys(RUN_ALIASES)) {
      expect(k).toBe(k.toLowerCase());
      expect(k.length).toBeGreaterThan(0);
      expect(typeof RUN_ALIASES[k]).toBe("string");
    }
  });
});

describe("matchRunCandidates 自动补全候选", () => {
  it("空查询返回空表", () => {
    expect(matchRunCandidates("", ["calc"])).toEqual([]);
    expect(matchRunCandidates("   ", ["calc"])).toEqual([]);
  });

  it("别名前缀匹配按字母序稳定输出", () => {
    const c = matchRunCandidates("c", []);
    expect(c.map((x) => x.text)).toEqual(["calc", "calendar", "checksum", "clipboard", "clockhub", "convert"]);
    expect(c.every((x) => x.kind === "alias")).toBe(true);
  });

  it("精确别名排最前；大小写不敏感", () => {
    expect(matchRunCandidates("calc", [])[0]).toEqual({ text: "calc", kind: "alias" });
    expect(matchRunCandidates("CALC", [])[0]).toEqual({ text: "calc", kind: "alias" });
    expect(matchRunCandidates("calc", []).map((x) => x.text)).toEqual(["calc"]);
  });

  it("历史前缀跟在别名后（新→旧），与别名去重", () => {
    const c = matchRunCandidates("c", ["calc", "C:\\Stuff", "dnote", "clock"]);
    expect(c.map((x) => x.text)).toEqual([
      "calc", // 别名（历史里的 calc 被去重）
      "calendar",
      "checksum",
      "clipboard",
      "clockhub",
      "convert",
      "C:\\Stuff", // 历史前缀
      "clock", // 历史前缀
    ]);
    expect(c.filter((x) => x.kind === "history").map((x) => x.text)).toEqual(["C:\\Stuff", "clock"]);
  });

  it("历史顺序保持新→旧（无别名干扰时）", () => {
    const c = matchRunCandidates("h", ["h2", "h1"]);
    expect(c.map((x) => x.text)).toEqual(["h2", "h1"]);
    expect(c.every((x) => x.kind === "history")).toBe(true);
  });

  it("候选 ≤8 条（上限截断）", () => {
    const hist: string[] = [];
    for (let i = 0; i < 12; i++) hist.push(`s${i}`);
    const c = matchRunCandidates("s", hist);
    expect(c).toHaveLength(CANDIDATE_CAP);
    // 别名（settings/snapshot/space/sysinfo 字母序）在前 4，历史随后截断
    expect(c.slice(0, 4).map((x) => x.text)).toEqual(["settings", "snapshot", "space", "sysinfo"]);
    expect(c[4]).toEqual({ text: "s0", kind: "history" });
  });
});

describe("运行历史（≤20）", () => {
  it("parseRunHistory：坏 JSON / 非数组 / 空串过滤", () => {
    expect(parseRunHistory(null)).toEqual([]);
    expect(parseRunHistory("[oops")).toEqual([]);
    expect(parseRunHistory('["a", 42, ""]')).toEqual(["a"]);
  });

  it("addRunHistory：置顶 / 大小写不敏感去重 / 20 条上限淘汰最旧", () => {
    let list: string[] = [];
    for (let i = 1; i <= 25; i++) list = addRunHistory(list, `item${i}`);
    expect(list).toHaveLength(RUN_HISTORY_CAP);
    expect(list[0]).toBe("item25");
    expect(list).not.toContain("item1");
    expect(list).not.toContain("item5");
    expect(list).toContain("item6");
    // 大小写不敏感去重并上移
    list = addRunHistory(list, "ITEM20");
    expect(list[0]).toBe("ITEM20");
    expect(list.filter((x) => x.toLowerCase() === "item20")).toHaveLength(1);
    expect(list).toHaveLength(RUN_HISTORY_CAP);
    // 空串不入历史
    expect(addRunHistory(list, "   ")).toBe(list);
  });

  it("loadRunHistory / saveRunHistory 走指定键并往返一致", () => {
    expect(loadRunHistory()).toEqual([]);
    saveRunHistory(addRunHistory(addRunHistory([], "notepad"), "https://a.b"));
    expect(globalThis.localStorage?.getItem(RUN_HISTORY_KEY)).toBe('["https://a.b","notepad"]');
    expect(loadRunHistory()).toEqual(["https://a.b", "notepad"]);
    // 超上限持久化时截断
    const many: string[] = [];
    for (let i = 0; i < 30; i++) many.push(`cmd${i}`);
    saveRunHistory(many);
    expect(loadRunHistory()).toHaveLength(RUN_HISTORY_CAP);
  });
});
