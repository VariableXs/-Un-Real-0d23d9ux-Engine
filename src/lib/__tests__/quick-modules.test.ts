/** AI-07 · V-41/V-42(模型)/V-43/V-44/V-46/V-47/V-48/V-49 + N-16/N-18/N-14 模块单测。 */
import { describe, expect, it } from "vitest";
import { inlineCalc, looksLikeNonMath } from "../quick/inlineCalc";
import { dayOfYear, isTimestampQuery, timestampCard } from "../quick/timestamps";
import { makeQr, canMakeQr, utf8Bytes } from "../quick/qrcode";
import {
  normalizeHiddenChars,
  NATIVE_PURE_PASTE_APPS,
  scrubToPlainText,
  shouldScrub,
  shouldYieldToApp,
} from "../quick/scrub";
import { policyLabelKey, SearchHistory } from "../quick/privacy";
import { complete, defaultSources, SHELL_DIRS } from "../quick/autocomplete";
import { appendQuickNote, quickNoteFilename } from "../quick/quicknote";
import {
  EmergencyStop,
  isSensitiveMacro,
  isValidCron,
  macroToYaml,
  relativizeSteps,
  runMacro,
  yamlToMacro,
  type Macro,
} from "../macros/engine";
import {
  assignStepNumbers,
  isDestructive,
  mosaicBlocks,
  needsMosaicConfirm,
  ocrStatus,
} from "../snip/annotations";
import {
  aggregate,
  applyFileFilters,
  isExcludedPath,
  lockCategory,
  parseQuery,
} from "../search/unified";

describe("V-41 内联计算", () => {
  it("基础算式与函数", () => {
    expect(inlineCalc("128*7")?.formatted).toBe("896");
    expect(inlineCalc("(3+4)^2")?.formatted).toBe("49");
    expect(inlineCalc("sqrt(2)")?.value).toBeCloseTo(1.41421356, 6);
  });

  it("路径/命令/URL 让位（零误判）", () => {
    for (const s of ["C:\\Program Files\\x", "\\\\server\\share", "https://a.b/c", "shell:startup", "explorer", "%TEMP%\\f"]) {
      expect(looksLikeNonMath(s)).toBe(true);
      expect(inlineCalc(s)).toBeNull();
    }
  });

  it("非法算式静默让位", () => {
    expect(inlineCalc("2+")).toBeNull();
    expect(inlineCalc("abc")).toBeNull();
  });
});

describe("V-49 时间戳速插", () => {
  it("五格式齐全且相互一致", () => {
    const now = new Date("2026-09-08T10:30:05.123");
    const card = timestampCard(now);
    expect(card.iso).toBe("2026-09-08T10:30:05");
    expect(card.unixSeconds).toBe(String(Math.floor(now.getTime() / 1000)));
    expect(card.unixMillis).toBe(String(now.getTime()));
    expect(card.human).toContain("2026-09-08");
    expect(card.dayOfYear).toBe(dayOfYear(now));
    expect(card.relative).toContain("第");
  });

  it("触发词", () => {
    expect(isTimestampQuery("now")).toBe(true);
    expect(isTimestampQuery("TS")).toBe(true);
    expect(isTimestampQuery("时间戳")).toBe(true);
    expect(isTimestampQuery("nown")).toBe(false);
  });
});

describe("V-43 二维码速递（本地生成）", () => {
  it("URL/文本/路径三类可生成（模块矩阵合理）", () => {
    for (const text of ["https://example.com/a?b=1", "Hello 世界", "D:\\docs\\报告.md"]) {
      const qr = makeQr(text);
      expect(qr.moduleCount).toBeGreaterThanOrEqual(21);
      expect(qr.moduleCount % 4).toBe(1); // version*4+17
      expect(qr.svg).toContain("<svg");
      expect(qr.dataUrl).toMatch(/^data:image\/(png|gif);base64,/);
    }
  });

  it("超容量如实拒绝（绝不截断）", () => {
    const tooLong = "x".repeat(3000);
    expect(canMakeQr(tooLong)).toBe(false);
    expect(() => makeQr(tooLong)).toThrow(/超出二维码容量/);
    expect(utf8Bytes("中")).toBe(3);
  });
});

describe("V-44 纯文本净化粘贴", () => {
  it("HTML 剥离彻底（标签/实体/块级换行）", () => {
    const html = '<p style="color:red">Hello <b>World</b></p><script>evil()</script><div>第二段&nbsp;&amp;&nbsp;more</div>';
    const plain = scrubToPlainText(html);
    expect(plain).toBe("Hello World\n第二段 & more");
  });

  it("隐藏字符剥离（零宽/BOM/NBSP/双向控制/CR）", () => {
    expect(normalizeHiddenChars("a\u200Bb\uFEFFc")).toBe("abc");
    expect(normalizeHiddenChars("a\u00A0b")).toBe("a b");
    expect(normalizeHiddenChars("a\u202Eb")).toBe("ab");
    expect(normalizeHiddenChars("a\r\nb\rc")).toBe("a\nb\nc");
  });

  it("让位检测：自带 Ctrl+Shift+V 的应用透传", () => {
    expect(shouldYieldToApp("Code.exe")).toBe(true);
    expect(NATIVE_PURE_PASTE_APPS.has("typora")).toBe(true);
    expect(shouldYieldToApp("notepad")).toBe(false);
  });

  it("图片/文件源不介入", () => {
    expect(shouldScrub("image")).toBe(false);
    expect(shouldScrub("file")).toBe(false);
    expect(shouldScrub("html")).toBe(true);
    expect(shouldScrub("text")).toBe(true);
  });
});

describe("V-47 搜索历史隐私三态", () => {
  it("local 模式记录与去重", () => {
    const h = new SearchHistory("local");
    expect(h.record("alpha")).toBe(true);
    h.record("beta");
    h.record("alpha");
    expect(h.list()).toEqual(["alpha", "beta"]);
    expect(h.count()).toBe(2);
  });

  it("off 模式零残留（内存也不留）", () => {
    const h = new SearchHistory("local");
    h.record("secret");
    h.setPolicy("off");
    expect(h.record("secret2")).toBe(false);
    expect(h.list()).toEqual([]);
    expect(h.count()).toBe(0);
    expect(policyLabelKey("off")).toBe("privacyHistoryOff");
  });

  it("一键清空不可逆", () => {
    const h = new SearchHistory("local");
    h.record("a");
    h.clear();
    expect(h.list()).toEqual([]);
  });
});

describe("V-48 运行框自动补全", () => {
  it("历史 > 程序 > shell 前缀补全", () => {
    const src = { history: ["ms-settings:bluetooth"], programs: ["mspaint", "msedge"], shellDirs: SHELL_DIRS };
    const hits = complete("ms", src);
    expect(hits.map((h) => h.kind)).toEqual(["history", "program", "program"]);
    const shell = complete("shell:st", src);
    expect(shell.map((h) => h.value)).toEqual(["shell:startup"]);
    expect(complete("shell:", src).length).toBeGreaterThan(5);
  });

  it("空输入不出补全；整体关闭由调用方不调用保证", () => {
    expect(complete("", defaultSources([]))).toEqual([]);
  });
});

describe("V-46 全局速记", () => {
  it("文件名与分节格式", () => {
    const d = new Date("2026-09-08T08:09:10");
    expect(quickNoteFilename(d)).toBe("速记/2026-09-08.md");
    const note = appendQuickNote("", "第一条", d);
    expect(note).toContain("# 速记 2026-09-08.md");
    expect(note).toContain("## 08:09:10");
    const note2 = appendQuickNote(note, "第二条", new Date(d.getTime() + 60000));
    expect(note2.match(/## /g)?.length).toBe(2);
  });
});

describe("N-18 宏引擎", () => {
  const mkMacro = (actions: Macro["actions"], enabled = true): Macro => ({
    id: "m1", name: "测试宏", enabled, trigger: { type: "hotkey", value: "ctrl+alt+f9" }, actions, createdAt: 0,
  });

  it("执行链路 + 失败即停报步骤号", async () => {
    const calls: string[] = [];
    const es = new EmergencyStop();
    const ctx = {
      uacForeground: false, passwordFocus: false, emergency: es,
      runCommand: async (id: string) => { calls.push(`cmd:${id}`); },
      notify: (t: string) => void calls.push(`notify:${t}`),
      writeClipboard: async (t: string) => void calls.push(`clip:${t}`),
      sendText: async (t: string) => void calls.push(`send:${t}`),
    };
    const ok = await runMacro(mkMacro([
      { type: "command", value: "snap.left" },
      { type: "notify", value: "done" },
    ]), ctx);
    expect(ok.ok).toBe(true);
    expect(calls).toEqual(["cmd:snap.left", "notify:done"]);

    const fail = await runMacro(mkMacro([
      { type: "command", value: "ok" },
      { type: "command", value: "boom" },
      { type: "command", value: "never" },
    ]), {
      ...ctx,
      runCommand: async (id: string) => {
        if (id === "boom") throw new Error("x");
        calls.push(id);
      },
    });
    expect(fail).toEqual({ ok: false, failedAtStep: 2, reason: "command-error" });
  });

  it("护栏：UAC/密码框判停；急停 100% 生效", async () => {
    const es = new EmergencyStop();
    const ctx = {
      uacForeground: true, passwordFocus: false, emergency: es,
      runCommand: async () => {}, notify: () => {}, writeClipboard: async () => {}, sendText: async () => {},
    };
    expect((await runMacro(mkMacro([{ type: "command", value: "x" }]), ctx)).reason).toBe("guard-pause");
    ctx.uacForeground = false;
    ctx.passwordFocus = true;
    expect((await runMacro(mkMacro([{ type: "command", value: "x" }]), ctx)).reason).toBe("guard-pause");
    ctx.passwordFocus = false;
    es.stop();
    expect((await runMacro(mkMacro([{ type: "command", value: "x" }]), ctx)).reason).toBe("emergency-stop");
    es.clear();
    expect((await runMacro(mkMacro([{ type: "command", value: "x" }]), ctx)).ok).toBe(true);
    expect((await runMacro(mkMacro([{ type: "command", value: "x" }], false), ctx)).reason).toBe("disabled");
  });

  it("敏感宏判定（sendText = 键鼠模拟）", () => {
    expect(isSensitiveMacro(mkMacro([{ type: "command", value: "x" }]))).toBe(false);
    expect(isSensitiveMacro(mkMacro([{ type: "sendText", value: "hi" }]))).toBe(true);
  });

  it("cron 子集校验", () => {
    expect(isValidCron("* * * * *")).toBe(true);
    expect(isValidCron("30 9 * * 1-5")).toBe(true);
    expect(isValidCron("0 8 1,15 * *")).toBe(true);
    expect(isValidCron("60 * * * *")).toBe(false);
    expect(isValidCron("* * * *")).toBe(false);
    expect(isValidCron("a * * * *")).toBe(false);
  });

  it("YAML 双形态同源往返", () => {
    const m: Macro = {
      id: "daily", name: "早间例行", enabled: true,
      trigger: { type: "time", value: "30 9 * * 1-5" },
      actions: [
        { type: "command", value: "app.notes" },
        { type: "notify", value: "开工: 打开便签" },
      ],
      createdAt: 0,
    };
    const yaml = macroToYaml(m);
    const back = yamlToMacro(yaml);
    expect(back.id).toBe(m.id);
    expect(back.trigger).toEqual(m.trigger);
    expect(back.actions).toEqual(m.actions);
    expect(macroToYaml(back)).toBe(yaml);
  });

  it("非法 .vmacro 行级报错", () => {
    expect(() => yamlToMacro("id: x\nname: y\ntrigger:\n  type: time\n  value: 99 * * * *\nactions:\n")).toThrow(/cron/);
    expect(() => yamlToMacro("id: x\nname: y\nbogus: 1\n")).toThrow(/未知字段/);
  });

  it("录制参数化：绝对 → 窗口相对坐标", () => {
    const steps = relativizeSteps([
      { x: 110, y: 220, originX: 100, originY: 200 },
      { x: 150, y: 260, originX: 100, originY: 200, text: "a" },
    ]);
    expect(steps).toEqual([
      { dx: 10, dy: 20, text: undefined },
      { dx: 50, dy: 60, text: "a" },
    ]);
  });
});

describe("N-16 截图标注", () => {
  it("六工具 + 序号步进分配", () => {
    const anns = assignStepNumbers([
      { id: "1", tool: "number" as const, points: [{ x: 1, y: 1 }], color: "#f00", size: 2 },
      { id: "2", tool: "number" as const, points: [{ x: 2, y: 2 }], color: "#f00", size: 2 },
      { id: "3", tool: "arrow" as const, points: [{ x: 0, y: 0 }, { x: 9, y: 9 }], color: "#000", size: 1 },
    ]);
    expect(anns[0]!.text).toBe("1");
    expect(anns[1]!.text).toBe("2");
    expect(anns[2]!.text).toBeUndefined();
  });

  it("马赛克 = 破坏性 + 确认门 + 块计算", () => {
    expect(isDestructive({ id: "m", tool: "mosaic", points: [], color: "#000", size: 4 })).toBe(true);
    expect(needsMosaicConfirm([{ id: "m", tool: "mosaic", points: [], color: "#000", size: 4 }])).toBe(true);
    const blocks = mosaicBlocks({ id: "m", tool: "mosaic", points: [{ x: 0, y: 0 }, { x: 20, y: 10 }], color: "#000", size: 4 }, 8);
    expect(blocks).toHaveLength(3 * 2); // 20 宽 = 3 列（8+8+4），10 高 = 2 行
    expect(blocks[0]).toEqual({ x: 0, y: 0, w: 8, h: 8 });
    expect(blocks[2]).toEqual({ x: 16, y: 0, w: 4, h: 8 });
  });

  it("OCR 语言包缺失诚实提示", () => {
    expect(ocrStatus(["en-US"], true).available).toBe(false);
    expect(ocrStatus(["en-US"], true).missingHintKey).toBe("ocrMissingLangpack");
    expect(ocrStatus(["zh-CN", "en-US"], true).available).toBe(true);
  });
});

describe("N-14 统一搜索", () => {
  const now = Date.now();
  const files = [
    { path: "D:/docs/report.md", title: "report.md", sizeBytes: 20_000_000, modifiedAt: now - 1000 },
    { path: "D:/data/safe/secret.txt", title: "secret.txt", sizeBytes: 10, modifiedAt: now },
    { path: "D:/tmp/old.log", title: "old.log", sizeBytes: 100, modifiedAt: now - 40 * 86400000 },
    { path: "D:/notes/report.txt", title: "report.txt", sizeBytes: 500, modifiedAt: now - 1000 },
  ];

  it("高级语法解析", () => {
    expect(parseQuery("报告 ext:md size:>10m modified:week")).toEqual({
      text: "报告", ext: "md", sizeMin: 10 * 1024 * 1024, modifiedWithin: "week",
    });
    expect(parseQuery("x size:<5k")).toEqual({ text: "x", sizeMax: 5 * 1024 });
  });

  it("保险箱路径硬排除（零泄漏）", () => {
    expect(isExcludedPath("D:/data/safe/secret.txt")).toBe(true);
    expect(applyFileFilters(files, { text: "" })).not.toContain(files[1]);
  });

  it("过滤组合（ext + size + modified）", () => {
    const f = parseQuery("report ext:md size:>10m modified:week");
    const out = applyFileFilters(files, f, now);
    expect(out.map((x) => x.title)).toEqual(["report.md"]);
  });

  it("五类聚合 + 权重 + Tab 锁定", () => {
    const hits = aggregate({
      apps: [{ title: "Reportly" }],
      files,
      contents: [{ title: "report content", folderPath: "docs" }],
      commands: [{ id: "cmd.report", title: "导出报告" }],
      settings: [{ title: "Report Settings" }],
    }, { text: "report" }, now);
    const cats = new Set(hits.map((h) => h.category));
    expect(cats).toEqual(new Set(["app", "file", "content", "command", "settings"]));
    expect(lockCategory(hits, "file").every((h) => h.category === "file")).toBe(true);
    expect(lockCategory(hits, null).length).toBe(hits.length);
    // 行内三键（文件类）
    expect(hits.find((h) => h.category === "file")!.quickActions).toEqual(["open", "reveal", "copyPath"]);
  });
});
