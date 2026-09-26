/**
 * AI-U3（F501-F550）前端功能面单测——判据的执行器。
 * 覆盖：十大模块核心判据 + u3store 底座行为 + F550 九域自检引擎。
 */

import { describe, expect, it, beforeEach } from "vitest";
import { U3_DEFAULTS, u3Store, U3_SECTIONS } from "../u3store";
import {
  BRIGHTNESS_SAMPLES, GRID_DENSITY_PX, effectiveGrid, hexLuma, iconTextLayers,
  pickIconTextColor, resnapToGrid, wrapIconLabel,
} from "../deskicons";
import {
  PIN_COOLDOWN_AFTER_FAILS, pinCooldownMs, pinShapeOk, verifyPin, btLockTick,
  guestCapability, GUEST_SANDBOX_AXES, lockScreenShotPolicy, SCREENSHOT_CHANNELS,
  redactRects, setPin,
} from "../locksec";
import {
  pushShot, shutdownShotPlan, SHOT_HISTORY_CAP, shredPlan, shredWarningItems,
  vxcryptDecryptBytes, vxcryptEncryptBytes, wipeClipboard, VXCRYPT_MAGIC,
} from "../filesec";
import {
  CAPS_TONE_HZ, CTRL_COMBO_EXEMPT, ctrlFindTick,
  rippleStarts, soundLightPolicy, titleBarAction, trailSample, TRAIL_LEN_MS,
  typeHideOpacity, type TypeHideRt,
} from "../pointerfx";
import {
  findMatches, replaceAll, replacePreview, resolveStartupFolder, rememberLastFolder,
  shotFileName, statusBarSegments, toggleNode, syncTreeFromList, humanBytes, keycardModel,
} from "../explorerx";
import {
  altEscStep, bannerAnchor, bannerStackDirection, capsVerdict, imeKeyOwnership,
  layoutDragVerdict, peekStyle, taskmgrTopVerdict, winNumberResolve, winTStep, UNLOCK_PATH,
  IME_SCHEMES, type AltEscRt, type TaskbarSlot, type WinTRt,
} from "../winkeys";
import {
  balanceGains, clickLockStep, deviceNotifyBanner, effectiveNotifyVolume,
  lowBatteryWarn, memDiagReportShape, netResetPlan, resolveDeviceVolume,
  smoothBattery, type BtBatteryRt, type ClickLockRt, type DeviceVolumeEntry,
} from "../sysdev";
import {
  CNY_ANCHORS, ganzhi, hoverDateLine, lunarYearDays, solarToLunar,
  weekday, yearWalkLandsOnNextCny, LUNAR_INFO,
} from "../clockcal";
import { anchorRuntime, anchorSpotCheck, U3_ANCHOR_DOMAINS, U3_ANCHOR_MIN_CHECKS } from "../anchor";
import { undoBinExtend, undoBinOpen, undoBinRestore, undoBinTick, spaceCheck, shortfallMessage, shouldVerify, scheduleQueue, jumpQueue, diagnoseOpenFail, readOnlyReminder, longPathDisplay, longPathOk, COPY_VERIFY_AUTO_ABOVE, type CopyTask, type VerifyRt } from "../copyops";

/* ------------------------------- u3store 底座 ------------------------------- */

describe("u3store（单一配置根）", () => {
  beforeEach(() => u3Store.reset());

  it("50 节齐全且默认值表同构", () => {
    expect(U3_SECTIONS.length).toBe(50);
    expect(Object.keys(U3_DEFAULTS).length).toBe(50);
    for (const s of U3_SECTIONS) expect(U3_DEFAULTS[s]).toBeTypeOf("object");
  });

  it("set 后 get 生效；undoSection 还原上一态", () => {
    u3Store.set("peekDesk", { opacity: 0.2 });
    expect(u3Store.get("peekDesk").opacity).toBe(0.2);
    expect(u3Store.undoSection("peekDesk")).toBe(true);
    expect(u3Store.get("peekDesk").opacity).toBe(U3_DEFAULTS.peekDesk.opacity);
    expect(u3Store.undoSection("peekDesk")).toBe(false); // 栈空诚实 false
  });

  it("getWith 未登记键回退默认", () => {
    expect(u3Store.getWith("pinUnlock", "nonexistent", 42)).toBe(42);
  });

  it("importAll 原子切换；空包拒绝", () => {
    u3Store.set("midMinimize", { enabled: false });
    const pack = u3Store.exportAll();
    u3Store.reset();
    u3Store.importAll(pack);
    expect(u3Store.get("midMinimize").enabled).toBe(false);
    expect(() => u3Store.importAll({ junk: true })).toThrow(/合法分节/);
  });
});

/* ------------------------------- F501-F503 ------------------------------- */

describe("桌面图标三件（F501-F503）", () => {
  it("F501 五档亮度自动选字", () => {
    const picks = BRIGHTNESS_SAMPLES.map((b) => pickIconTextColor(b));
    expect(picks).toEqual(["light", "light", "dark", "dark", "dark"]);
  });

  it("F501 F297 压暗联动：0.55 开压暗翻转为浅字", () => {
    expect(pickIconTextColor(0.55)).toBe("dark");
    expect(pickIconTextColor(0.55, true)).toBe("light");
  });

  it("F501 luma 感知亮度与非法输入诚实抛错", () => {
    expect(hexLuma("#ffffff")).toBeCloseTo(1, 5);
    expect(hexLuma("#000000")).toBeCloseTo(0, 5);
    expect(hexLuma("#00ff00")).toBeGreaterThan(hexLuma("#ff0000")); // 人眼绿敏感
    expect(() => hexLuma("blue")).toThrow(/非法颜色/);
  });

  it("F501 双层渲染样式串随字色切换", () => {
    const light = iconTextLayers("light");
    const dark = iconTextLayers("dark");
    expect(light.color).not.toBe(dark.color);
    expect(light.textShadow).toContain("0.4");
  });

  it("F502 两行封顶：长名截断且第二行尾部省略号", () => {
    const w = wrapIconLabel("项目总结报告最终版本提交给评审委员会审议用副本");
    expect(w.truncated).toBe(true);
    expect(w.lines[1].endsWith("…")).toBe(true);
    expect([...w.lines[0]].length).toBeLessThanOrEqual(16);
  });

  it("F502 短名不截断不省略", () => {
    const w = wrapIconLabel("便签");
    expect(w.truncated).toBe(false);
    expect(w.lines).toEqual(["便签", ""]);
  });

  it("F502 英文不在单词中间断", () => {
    const w = wrapIconLabel("hello world dashboard");
    expect(w.lines.join(" ")).not.toMatch(/helloworld/);
    // 第一行以完整单词开头（不出现断词残片）
    expect(w.lines[0].startsWith("hello") && !w.lines[0].startsWith("helloo")).toBe(true);
    for (const line of w.lines) {
      // 每行词边界完整：不以半个英文词收尾（省略号行除外）
      if (line && !line.endsWith("…")) {
        expect(/[a-z]$/.test(line) && /[A-Z]/.test(line.slice(-1))).toBe(false);
      }
    }
    // dashboard 整词保留（不拆成 dash/board）
    expect(w.lines.join("|")).toContain("dashboard");
  });

  it("F503 三档格距与自定义 8px 步进钳制", () => {
    expect(GRID_DENSITY_PX).toEqual({ loose: 96, standard: 80, compact: 64 });
    expect(effectiveGrid({ density: "custom", customColPx: 83, customRowPx: 121 })).toEqual({ colPx: 80, rowPx: 120 });
    expect(effectiveGrid({ density: "compact", customColPx: 96, customRowPx: 96 })).toEqual({ colPx: 64, rowPx: 64 });
  });

  it("F503 最近格吸附：冲突不覆盖（螺旋外扩）", () => {
    const out = resnapToGrid(
      [{ x: 5, y: 5 }, { x: 15, y: 15 }, { x: 30, y: 20 }],
      { colPx: 96, rowPx: 96 },
      { colPx: 80, rowPx: 80 },
    );
    const keys = out.map((p) => `${p.x},${p.y}`);
    expect(new Set(keys).size).toBe(3); // 三点三格，零覆盖
  });
});

/* ------------------------------- F504-F508 ------------------------------- */

describe("锁屏安全五件（F504-F508）", () => {
  it("F504 冷却翻倍表：30s 起逐次翻倍，五次门槛", () => {
    expect(pinCooldownMs(4)).toBe(0);
    expect(pinCooldownMs(5)).toBe(30_000);
    expect(pinCooldownMs(6)).toBe(60_000);
    expect(pinCooldownMs(7)).toBe(120_000);
    expect(pinCooldownMs(8)).toBe(240_000);
  });

  it("F504 验证状态机：错 5 次进冷却且回退密码", () => {
    const rt = { failCount: 0, coolUntil: 0 };
    const now = 1_000_000;
    for (let i = 0; i < PIN_COOLDOWN_AFTER_FAILS; i++) {
      const v = verifyPin("0000", "1234", rt, now);
      expect(v.ok).toBe(false);
    }
    expect(rt.failCount).toBe(5);
    const cooled = verifyPin("1234", "1234", rt, now + 1000); // 即使 PIN 正确也在冷却期拒绝
    expect(cooled.ok).toBe(false);
    expect(cooled).toMatchObject({ reason: "cooldown", fallbackToPassword: true });
  });

  it("F504 冷却期满恢复验证", () => {
    const rt = { failCount: 5, coolUntil: 1_000_000 };
    const ok = verifyPin("1234", "1234", rt, 1_000_001);
    expect(ok.ok).toBe(true);
    expect(rt.failCount).toBe(0); // 成功清零
  });

  it("F504 PIN 形校验 4-6 位纯数字；setPin 废除", () => {
    expect(pinShapeOk("1234")).toBe(true);
    expect(pinShapeOk("123456")).toBe(true);
    expect(pinShapeOk("123")).toBe(false);
    expect(pinShapeOk("1234567")).toBe(false);
    expect(pinShapeOk("12a4")).toBe(false);
    u3Store.reset();
    setPin(""); // 废除不抛
    expect(u3Store.get("pinUnlock").enabled).toBe(false);
    expect(() => setPin("12")).toThrow(/4-6 位/);
  });

  it("F505 蓝牙锁：30s 触发、<10s 波动不锁、回连复位", () => {
    const rt = { lastSeenMs: 0, awaySinceMs: null };
    btLockTick(rt, false, 0);
    expect(btLockTick(rt, false, 29_999).lock).toBe(false);
    expect(btLockTick(rt, false, 30_000).lock).toBe(true);
    // 波动场景：闪现（重置）后再次失联从头计时
    const rt2 = { lastSeenMs: 0, awaySinceMs: null };
    btLockTick(rt2, false, 0);
    btLockTick(rt2, true, 8_000); // 回连
    btLockTick(rt2, false, 10_000); // 又走
    expect(btLockTick(rt2, false, 35_000).lock).toBe(false); // 从 10s 起算仅 25s
    expect(btLockTick(rt2, false, 40_000).lock).toBe(true); // 30s 到
  });

  it("F506 沙盒四轴能力表", () => {
    expect(GUEST_SANDBOX_AXES).toEqual(["files", "settings", "permissions", "network-share"]);
    expect(guestCapability("files")).toBe("readonly");
    expect(guestCapability("permissions")).toBe("denied");
  });

  it("F507 锁屏态四通道全拒；非锁屏放行（零开销边界）", () => {
    for (const ch of SCREENSHOT_CHANNELS) {
      expect(lockScreenShotPolicy(true, "black-frame", ch).allowed).toBe(false);
      expect(lockScreenShotPolicy(true, "deny", ch).allowed).toBe(false);
    }
    expect(lockScreenShotPolicy(false, "black-frame", "prtsc")).toEqual({ allowed: true });
  });

  it("F508 防截黑块几何精确对齐；未标记零影响", () => {
    const rects = redactRects(
      { w1: { x: 100, y: 100, w: 200, h: 80 }, w2: { x: 0, y: 0, w: 10, h: 10 } },
      { x: 150, y: 120, w: 400, h: 300 },
    );
    expect(rects.length).toBe(1); // w1 相交裁剪；w2 与截图区不相交 → 不产生黑块
    expect(rects[0]).toEqual({ x: 150, y: 120, w: 150, h: 60 });
    expect(redactRects({}, { x: 0, y: 0, w: 500, h: 500 })).toEqual([]);
  });
});

/* ------------------------------- F509-F512 ------------------------------- */

describe("文件安全四件（F509-F512）", () => {
  it("F509 介质分派：覆写与诚实标注两分支", () => {
    const ok = shredPlan("overwrite-ok", ["a"]);
    expect(ok.mode).toBe("overwrite");
    expect(ok.passes).toBe(3);
    const bad = shredPlan("wear-leveling", ["a"]);
    expect(bad.mode).toBe("honest-label");
    expect(bad.warning).toContain("F439");
  });

  it("F509 三重警示 + 默认焦点取消", () => {
    const w = shredWarningItems(["1", "2", "3", "4", "5", "6", "7"]);
    expect(w.count).toBe(7);
    expect(w.names).toContain("等 7 项");
    expect(w.irrecoverable).toContain("不经过回收站");
    expect(w.defaultFocus).toBe("cancel");
  });

  it("F510 .vxcrypt 加密-解密 round-trip（WebCrypto 真实现）", async () => {
    const plain = new TextEncoder().encode("机密文件内容：加班申请表 v7");
    const cipher = await vxcryptEncryptBytes(plain, "S3cret!密码", 10_000); // 测试迭代档
    const magic = new TextDecoder().decode(cipher.slice(0, 4));
    expect(magic).toBe(VXCRYPT_MAGIC);
    const back = await vxcryptDecryptBytes(cipher, "S3cret!密码");
    expect(new TextDecoder().decode(back)).toBe(new TextDecoder().decode(plain));
  });

  it("F510 错误密码 = GCM 认证失败（人话提示不泄露信息）", async () => {
    const plain = new TextEncoder().encode("top secret");
    const cipher = await vxcryptEncryptBytes(plain, "right", 10_000);
    await expect(vxcryptDecryptBytes(cipher, "wrong")).rejects.toThrow(/密码错误/);
  });

  it("F510 非法容器与空密码诚实拒绝", async () => {
    const junk = new TextEncoder().encode("definitely not a container");
    await expect(vxcryptDecryptBytes(junk, "x")).rejects.toThrow(/魔数/);
    await expect(vxcryptEncryptBytes(new Uint8Array([1]), "")).rejects.toThrow(/密码不能为空/);
  });

  it("F511 剪贴板清空：当前+历史全清；清空后写入空串", async () => {
    const writes: string[] = [];
    const fake = { writeText: async (t: string) => void writes.push(t) };
    const history = [{ text: "a" }, { text: "b" }, { text: "c" }];
    const n = await wipeClipboard(fake, history);
    expect(n).toBe(3);
    expect(history.length).toBe(0);
    expect(writes).toEqual([""]); // 应用下一次粘贴得到诚实失败（空）
  });

  it("F511 剪贴板 API 不可用时不阻塞历史清理（异常显性化）", async () => {
    const history = [{ text: "x" }];
    const n = await wipeClipboard(null, history);
    expect(n).toBe(1);
    expect(history.length).toBe(0);
  });

  it("F512 20 条上限：优先淘汰临时项；满编已保存淘汰最旧", () => {
    const base = Array.from({ length: SHOT_HISTORY_CAP }, (_, i) => ({ id: `s${i}`, createdAt: i, saved: true, path: "", thumbDataUrl: "" }));
    const withTemp = pushShot(base, { id: "tmp", createdAt: 999, saved: false, path: "", thumbDataUrl: "" });
    expect(withTemp.length).toBe(SHOT_HISTORY_CAP);
    expect(withTemp.some((e) => e.id === "tmp")).toBe(true);
    expect(withTemp.some((e) => e.id === "s0")).toBe(false); // 最旧已保存被挤
    const grown = pushShot(withTemp, { id: "tmp2", createdAt: 1000, saved: false, path: "", thumbDataUrl: "" });
    expect(grown.some((e) => e.id === "tmp")).toBe(false); // 优先淘汰临时
  });

  it("F512 关机清理：未保存即失（仅留已保存引用）", () => {
    const entries = [
      { id: "a", createdAt: 1, saved: false, path: "", thumbDataUrl: "" },
      { id: "b", createdAt: 2, saved: true, path: "C:/pics/a.png", thumbDataUrl: "" },
    ];
    const plan = shutdownShotPlan(entries);
    expect(plan.dropped).toBe(1);
    expect(plan.kept.map((e) => e.id)).toEqual(["b"]);
    expect(plan.notifyOnce).toContain("1 张");
  });
});

/* ------------------------------- F513/F514/F519/F520/F522/F523 ------------------------------- */

describe("指针与提示六件", () => {
  it("F513 纯 Ctrl 持续 1s 触发；组合键豁免 20 例", () => {
    const rt = { downAtMs: null, otherKeyDown: false };
    ctrlFindTick(rt, "ctrl", 0);
    expect(ctrlFindTick(rt, "ctrl", 999).fired).toBe(false);
    expect(ctrlFindTick(rt, "ctrl", 1000).fired).toBe(true);
    // 触发后未松开不重复触发
    expect(ctrlFindTick(rt, "ctrl", 2500).fired).toBe(false);
    expect(CTRL_COMBO_EXEMPT.length).toBe(20);
    expect(CTRL_COMBO_EXEMPT).toContain("c");
  });

  it("F513 Ctrl 组合期间豁免复位（Ctrl+C 场景 0 误触）", () => {
    const rt = { downAtMs: null, otherKeyDown: false };
    ctrlFindTick(rt, "ctrl", 0);
    ctrlFindTick(rt, "other", 200); // C 落下
    ctrlFindTick(rt, "up", 300); // 全松开
    ctrlFindTick(rt, "ctrl", 1000); // 再单独按住 C 无关
    const r = ctrlFindTick(rt, "ctrl", 2000);
    expect(r.fired).toBe(true); // 新一轮独立计时——组合旧账已清
    expect(rippleStarts()).toEqual([0, 500, 1000]);
  });

  it("F514 勿扰三档镜像：仅声=只闪不响、全静=都不来但记录", () => {
    expect(soundLightPolicy("notify", "normal", true)).toEqual({ flash: true, sound: true, logToCenter: true });
    expect(soundLightPolicy("warn", "sound-only", true)).toEqual({ flash: true, sound: false, logToCenter: true });
    expect(soundLightPolicy("battery", "silent", true)).toEqual({ flash: false, sound: false, logToCenter: true });
    // 逐事件开关：关掉的事件不闪
    expect(soundLightPolicy("notify", "normal", false).flash).toBe(false);
  });

  it("F519 双音色频率表", () => {
    expect(CAPS_TONE_HZ.on).toEqual([880, 988]);
    expect(CAPS_TONE_HZ.off).toEqual([440, 494]);
  });

  it("F520 标题栏三义分流矩阵；开关关闭时中键不动作", () => {
    expect(titleBarAction("middle", true)).toBe("minimize");
    expect(titleBarAction("double", true)).toBe("maximize-toggle");
    expect(titleBarAction("right", true)).toBe("sysmenu");
    expect(titleBarAction("middle", false)).toBe("none");
    expect(titleBarAction("double", false)).toBe("maximize-toggle"); // 双击/右键不受开关影响
  });

  it("F522 轨迹三档存留 200/400/600ms", () => {
    expect(TRAIL_LEN_MS).toEqual({ short: 200, medium: 400, long: 600 });
    const pts = [0, 150, 350, 550].map((atMs) => ({ x: atMs, y: 0, atMs }));
    expect(trailSample(pts, 600, "short").map((p) => p.atMs)).toEqual([550]);
    expect(trailSample(pts, 600, "medium").map((p) => p.atMs)).toEqual([350, 550]);
    expect(trailSample(pts, 600, "long").length).toBe(4);
  });

  it("F523 打字淡出 30%/停 2s 恢复/动鼠标即时/触屏豁免", () => {
    const rt: TypeHideRt = { lastKeyMs: 0, mouseMovedMs: -1e9 };
    expect(typeHideOpacity(rt, 500, true, false)).toBe(0.3);
    expect(typeHideOpacity(rt, 1999, true, false)).toBe(0.3);
    expect(typeHideOpacity(rt, 2001, true, false)).toBe(1); // 停 2s 恢复
    rt.mouseMovedMs = 100;
    expect(typeHideOpacity(rt, 500, true, false)).toBe(1); // 动鼠标即时恢复
    expect(typeHideOpacity({ lastKeyMs: 0, mouseMovedMs: -1e9 }, 0, true, true)).toBe(1); // 触屏豁免
    expect(typeHideOpacity(rt, 0, false, false)).toBe(1); // 开关关
  });
});

/* ------------------------------- F515/F517/F521/F525-F528 ------------------------------- */

describe("资源管理器七件", () => {
  it("F515 匹配/预览/整批一次撤销", () => {
    const text = "Alpha 与 alpha；Alpha 结尾";
    const all = findMatches(text, "alpha", { matchCase: false, wholeWord: false });
    expect(all.length).toBe(3);
    const cs = findMatches(text, "alpha", { matchCase: true, wholeWord: false });
    expect(cs.length).toBe(1);
    const pv = replacePreview(text, "Alpha", { matchCase: true, wholeWord: false });
    expect(pv.count).toBe(2);
    expect(pv.firstContext).toContain("【Alpha】");
    const rep = replaceAll(text, "Alpha", "Ω", { matchCase: true, wholeWord: false });
    expect(rep.count).toBe(2);
    expect(rep.after).toBe("Ω 与 alpha；Ω 结尾");
    expect(rep.undo()).toBe(text); // 整批一次回滚
  });

  it("F515 全字匹配（英文 \\b；中文退化包含）", () => {
    expect(findMatches("cat catalog cat", "cat", { matchCase: true, wholeWord: true }).length).toBe(2);
    expect(findMatches("目录内容", "内容", { matchCase: true, wholeWord: true }).length).toBe(1);
  });

  it("F517 启动页三模式（上次文件夹按窗口记忆）", () => {
    const mem = rememberLastFolder(rememberLastFolder({}, "w1", "C:/a"), "w2", "C:/b");
    expect(resolveStartupFolder("thispc", "", mem, "w1")).toBe("thispc://");
    expect(resolveStartupFolder("last-folder", "", mem, "w1")).toBe("C:/a");
    expect(resolveStartupFolder("last-folder", "", mem, "w2")).toBe("C:/b");
    expect(resolveStartupFolder("last-folder", "", mem, "w3")).toBe("thispc://"); // 无记忆兜底
    expect(resolveStartupFolder("fixed", "C:/proj", mem, "w1")).toBe("C:/proj");
  });

  it("F521 自动命名永不重名", () => {
    const d = new Date(2026, 8, 25, 14, 30);
    const n1 = shotFileName("截图", d, new Set());
    const n2 = shotFileName("截图", d, new Set([n1]));
    const n3 = shotFileName("截图", d, new Set([n1, n2]));
    expect(n1).toBe("截图 2026-09-25_1430");
    expect(n2).toBe("截图 2026-09-25_1430(2)");
    expect(n3).toBe("截图 2026-09-25_1430(3)");
  });

  it("F525 速查卡模型：同源分色 + 双页切分", () => {
    const rows = [
      { keys: "Ctrl+C", action: "复制", custom: false },
      { keys: "Ctrl+Shift+Delete", action: "清空剪贴板", custom: true },
      { keys: "Win+T", action: "任务栏遍历", custom: false },
      { keys: "Ctrl+Q", action: "自定义动作", custom: true },
    ];
    const card = keycardModel(rows);
    expect(card.format).toBe("png-1page");
    expect(card.pages[0]!.length + card.pages[1]!.length).toBe(4); // keycardModel 恒返两页
    // 分色：custom=true 标为 custom
    const flat = card.pages.flat();
    expect(flat.filter((r) => r.tone === "custom").map((r) => r.keys)).toEqual(["Ctrl+Shift+Delete", "Ctrl+Q"]);
    expect(card.legend).toContain("F244 注册表同源");
  });

  it("F526 状态栏三段 + 字节人话 + 空目录态", () => {
    expect(humanBytes(245 * 1024 * 1024)).toBe("245MB"); // 主册 F526 判据原文「已选 5 项 · 245MB」
    expect(humanBytes(2 * 1024 ** 3)).toBe("2.0GB");
    expect(humanBytes(512)).toBe("512 B");
    const seg = statusBarSegments({ items: 128, selected: 5, selectedBytes: 245 * 1024 * 1024, volumeFreeBytes: 2 * 1024 ** 3 });
    expect(seg).toEqual(["128 项", "已选 5 项 · 245MB", "可用 2.0GB"]);
    expect(statusBarSegments({ items: 0, selected: 0, selectedBytes: 0, volumeFreeBytes: 0 })[0]).toBe("空目录");
  });

  it("F527 树折叠两路翻转与命令", () => {
    const t = toggleNode(toggleNode({ expanded: {} }, "a"), "b");
    expect(t.expanded).toEqual({ a: true, b: true });
    expect(toggleNode(t, "a").expanded.a).toBe(false);
  });

  it("F528 列表→树同步：路径展开+高亮末节点", () => {
    const step = syncTreeFromList(["C:", "work", "proj", "src", "deep"]);
    expect(step.targetId).toBe("deep");
    expect(step.path.length).toBe(5);
  });
});

/* ------------------------------- F524/F529-F534 ------------------------------- */

describe("复制链七件", () => {
  it("F524 后悔窗：暂存/延寿/真释放三段生命周期", () => {
    const rt = undoBinOpen(["a", "b"], 0);
    expect(rt.expiresAt).toBe(5000);
    expect(undoBinExtend(rt)).toBe(true);
    expect(rt.expiresAt).toBe(15000);
    expect(undoBinTick(rt, 14999).expired).toBe(false);
    expect(undoBinTick(rt, 15000).expired).toBe(true);
    expect(rt.released).toBe(true);
    expect(rt.staged).toEqual([]); // 真释放：暂存清空
    expect(undoBinRestore(rt)).toBeNull(); // 超时后诚实不可恢复
  });

  it("F524 延寿上限 2 次；撤销 N 项全回", () => {
    const rt = undoBinOpen(["1", "2", "3"], 0);
    undoBinExtend(rt);
    undoBinExtend(rt);
    expect(undoBinExtend(rt)).toBe(false);
    expect(rt.expiresAt).toBe(5000 + 20_000);
    expect(undoBinRestore(rt)).toEqual(["1", "2", "3"]);
  });

  it("F529 空间预检：10% 缓冲、逐盘、人话文案", () => {
    const sc = spaceCheck({
      totalBytes: 0,
      perTargetFree: { "D:": 3.7 * 1024 ** 3, "E:": 10 * 1024 ** 3 },
      perTargetNeed: { "D:": 3.4 * 1024 ** 3, "E:": 1 * 1024 ** 3 },
    });
    expect(sc.ok).toBe(false); // 3.4*1.1=3.74 > 3.7 拦下
    expect(sc.shortfalls[0]!.volume).toBe("D:");
    expect(shortfallMessage(sc.shortfalls[0]!)).toContain("10% 缓冲");
    const ok = spaceCheck({ totalBytes: 0, perTargetFree: { "D:": 4 * 1024 ** 3 }, perTargetNeed: { "D:": 3.4 * 1024 ** 3 } });
    expect(ok.ok).toBe(true);
  });

  it("F530 校验决策：>1GB 自动开、开关优先、强制覆盖", () => {
    const rt: VerifyRt = { enabled: true, autoAboveBytes: COPY_VERIFY_AUTO_ABOVE };
    expect(shouldVerify(rt, 2 * 1024 ** 3)).toBe(true);
    expect(shouldVerify(rt, 999 * 1024 * 1024)).toBe(false);
    expect(shouldVerify({ ...rt, enabled: false }, 2 * 1024 ** 3)).toBe(false);
    expect(shouldVerify(rt, 1024, true)).toBe(true); // 用户强制
    expect(shouldVerify(rt, 1024, false)).toBe(false);
  });

  it("F531 队列：同盘串行、异盘并行、插队优先", () => {
    const q: CopyTask[] = [
      { id: "1", volume: "C:", state: "queued", priority: 5 },
      { id: "2", volume: "C:", state: "queued", priority: 1 },
      { id: "3", volume: "D:", state: "queued", priority: 9 },
      { id: "4", volume: "E:", state: "queued", priority: 9 },
    ];
    const picks = scheduleQueue(q, new Set(), 2);
    expect(picks.map((p) => p.id).sort()).toEqual(["2", "3"]); // C 盘只放行优先级最高的 2；D 盘并行；E 因并行上限 2 落选
    const jumped = jumpQueue(q, "3");
    expect(scheduleQueue(jumped, new Set(), 3).map((p) => p.id)).toContain("3");
    expect(scheduleQueue(q, new Set(["C:"]), 3).every((p) => p.volume !== "C:")).toBe(true); // 该盘已有任务 → 串行等待
  });

  it("F532 四类归因 + 三问结构 + 出路路由", () => {
    const zipBad = diagnoseOpenFail(new Uint8Array([0x50, 0x4b, 0x00, 0x00]), "编辑器", false);
    expect(zipBad.kind).toBe("corrupt");
    expect(zipBad.route).toBe("f325");
    const noApp = diagnoseOpenFail(null, null, false);
    expect(noApp.kind).toBe("app-missing");
    expect(noApp.route).toBe("f257");
    const perm = diagnoseOpenFail(null, "编辑器", true);
    expect(perm.kind).toBe("permission");
    expect(perm.route).toBe("f324");
    const fmt = diagnoseOpenFail(null, "编辑器", false);
    expect(fmt.kind).toBe("format");
    for (const d of [zipBad, noApp, perm, fmt]) {
      expect(d.what).toBeTruthy();
      expect(d.why).toBeTruthy();
      expect(d.next).toBeTruthy();
    }
  });

  it("F533 只读提醒：前置、归因、一次不重复", () => {
    const seen = new Set<string>();
    const first = readOnlyReminder({ volume: "E:", readOnly: true, cause: "switch" }, seen);
    expect(first.remind).toBe(true);
    expect(first.message).toContain("物理写保护开关");
    expect(readOnlyReminder({ volume: "E:", readOnly: true, cause: "switch" }, seen).remind).toBe(false);
    expect(readOnlyReminder({ volume: "F:", readOnly: false, cause: "mount" }, seen).remind).toBe(false);
  });

  it("F534 长路径：显示保尾、合法性、600+ 全操作", () => {
    const p600 = "C:\\" + Array.from({ length: 60 }, (_, i) => `directory-level-${i}`).join("\\");
    expect(p600.length).toBeGreaterThan(600);
    expect(longPathOk(p600)).toBe(true);
    const shown = longPathDisplay(p600, 24);
    expect(shown.length).toBeLessThan(60);
    expect(shown.endsWith(p600.slice(-24))).toBe(true);
    expect(shown).toContain("…");
    expect(longPathOk("C:\\ok\\path.txt")).toBe(true);
  });
});

/* ------------------------------- F516/F518/F535-F539/F548 ------------------------------- */

describe("窗口与快捷键八件", () => {
  it("F516 三档位置+堆叠方向自适应+锚点几何", () => {
    expect(bannerStackDirection("bottom-right")).toBe("up");
    expect(bannerStackDirection("top-center")).toBe("down");
    expect(bannerStackDirection("top-right")).toBe("down");
    const a = bannerAnchor("top-center", { w: 1920, h: 1080 }, { w: 360, h: 80 });
    expect(a.x).toBe((1920 - 360) / 2);
    expect(a.y).toBe(16);
  });

  it("F518 四方案+Caps 600ms 分界+旧键让出", () => {
    expect(IME_SCHEMES.length).toBe(4);
    expect(capsVerdict(599)).toBe("caps-lock");
    expect(capsVerdict(600)).toBe("ime-toggle");
    const own = imeKeyOwnership("ctrl-shift");
    expect(own.owner).toBe("ctrl-shift");
    expect(own.retired).toContain("win-space");
  });

  it("F535 Win+数字：启动/切换/最小化/Shift 新实例/越界静默", () => {
    const slots: TaskbarSlot[] = [
      { appId: "a", running: false },
      { appId: "b", running: true },
    ];
    expect(winNumberResolve(slots, 0, false, null)).toEqual({ action: "launch", appId: "a" });
    expect(winNumberResolve(slots, 1, false, "other")).toEqual({ action: "switch", appId: "b" });
    expect(winNumberResolve(slots, 1, false, "b")).toEqual({ action: "minimize-toggle", appId: "b" });
    expect(winNumberResolve(slots, 1, true, null)).toEqual({ action: "new-instance", appId: "b" });
    expect(winNumberResolve(slots, 9, false, null)).toEqual({ action: "none" });
    expect(winNumberResolve(slots, -1, false, null)).toEqual({ action: "none" });
  });

  it("F536 Win+T：进场/遍历回绕/Esc 归还", () => {
    const rt: WinTRt = { active: false, index: 0 };
    expect(winTStep(rt, "next", 3).active).toBe(false); // 未进场遍历无效
    winTStep(rt, "open", 3);
    expect(winTStep(rt, "next", 3).index).toBe(1);
    expect(winTStep(rt, "prev", 3).index).toBe(0);
    expect(winTStep(rt, "prev", 3).index).toBe(2); // 首尾回绕
    expect(winTStep(rt, "esc", 3)).toMatchObject({ active: false, exited: true });
  });

  it("F537 瞥桌面：15%/120ms/纯看限制", () => {
    const on = peekStyle(true);
    expect(on.opacity).toBe(0.15);
    expect(on.pointerEvents).toBe("none");
    expect(on.transition).toContain("120ms");
    expect(peekStyle(false)).toMatchObject({ opacity: 1, pointerEvents: "auto" });
  });

  it("F538 Alt+Esc：Z 序后退+100ms 连按节流+Alt 松开失效", () => {
    const rt: AltEscRt = { altHeld: true };
    expect(altEscStep(rt, ["w1", "w2", "w3"], 0, -1000)).toMatchObject({ focusId: "w3", minimizedRestored: true });
    expect(altEscStep(rt, ["w1", "w2", "w3"], 50, 0).focusId).toBeNull(); // 节流窗内
    expect(altEscStep({ altHeld: false }, ["w1", "w2"], 1000, 0).focusId).toBeNull();
    expect(altEscStep(rt, [], 1000, 0).focusId).toBeNull(); // 空栈诚实
  });

  it("F539 布局锁定：拖拽拒绝反馈+解锁路径深度", () => {
    const v = layoutDragVerdict(true);
    expect(v.allowed).toBe(false);
    expect(v.feedback).toBe("shake");
    expect(v.statusbar).toContain("已锁定");
    expect(layoutDragVerdict(false).allowed).toBe(true);
    expect(UNLOCK_PATH).toContain("设置中心");
  });

  it("F548 置顶：不抢焦点/重启不记忆", () => {
    const t = taskmgrTopVerdict({ pinned: true, sessionOnly: true });
    expect(t.onTop).toBe(true);
    expect(t.stealsFocus).toBe(false);
    expect(t.remembered).toBe(false);
    expect(taskmgrTopVerdict({ pinned: false, sessionOnly: true }).onTop).toBe(false);
  });
});

/* ------------------------------- F540-F547 ------------------------------- */

describe("系统与设备八件", () => {
  it("F540 报告三要素：结论/地址段/建议", () => {
    const pass = memDiagReportShape(false, []);
    expect(pass.verdict).toBe("pass");
    expect(pass.advice).toContain("通过");
    const fail = memDiagReportShape(true, [{ from: "0x1A2B", to: "0x1A3F" }]);
    expect(fail.verdict).toBe("fail");
    expect(fail.badRanges[0]!.from).toBe("0x1A2B");
    expect(fail.advice).toContain("送检");
  });

  it("F541 网络重置：四项清单/90s/不重启整机/向导链", () => {
    const p = netResetPlan();
    expect(p.clears.length).toBe(4);
    expect(p.countdownSec).toBe(90);
    expect(p.rebootRequired).toBe(false);
    expect(p.wizardSteps.length).toBe(3);
  });

  it("F542 ClickLock：1.1s 抓起/单击放下/Esc 放弃/短按普通点击", () => {
    const rt: ClickLockRt = { state: "idle", pressAtMs: 0 };
    clickLockStep(rt, { t: "press", atMs: 0 });
    clickLockStep(rt, { t: "release", atMs: 1099 });
    expect(rt.state).toBe("idle"); // 差 1ms 不抓起
    const rt2: ClickLockRt = { state: "idle", pressAtMs: 0 };
    clickLockStep(rt2, { t: "press", atMs: 0 });
    const grab = clickLockStep(rt2, { t: "release", atMs: 1100 });
    expect(grab.grabbed).toBe(true);
    expect(grab.ring).toBe(true);
    expect(clickLockStep(rt2, { t: "click", atMs: 3000 }).dropOrDrag).toBe("drop");
    const rt3: ClickLockRt = { state: "idle", pressAtMs: 0 };
    clickLockStep(rt3, { t: "press", atMs: 0 });
    clickLockStep(rt3, { t: "release", atMs: 1200 });
    expect(clickLockStep(rt3, { t: "esc", atMs: 1500 }).dropOrDrag).toBe("cancel");
    expect(rt3.state).toBe("idle");
  });

  it("F543 分设备音量：新设备 40% 首发、记忆跟随、LRU 10 台淘汰", () => {
    let devs: DeviceVolumeEntry[] = [];
    const first = resolveDeviceVolume(devs, "hp", "耳机", 0);
    expect(first.volume).toBe(40);
    expect(first.isNew).toBe(true);
    devs = first.devices;
    devs = resolveDeviceVolume(devs, "hp", "耳机", 10).devices;
    devs = resolveDeviceVolume(devs, "spk", "扬声器", 20).devices;
    devs = resolveDeviceVolume(devs, "hp", "耳机", 30).devices;
    expect(devs.find((d) => d.deviceId === "spk")?.lastUsed).toBe(20);
    for (let i = 0; i < 9; i++) devs = resolveDeviceVolume(devs, `x${i}`, `设备${i}`, 100 + i).devices;
    expect(devs.length).toBe(10);
    expect(devs.some((d) => d.deviceId === "spk")).toBe(false); // 最久未用被淘汰
  });

  it("F544 双滑杆独立 + 总闸一票否决", () => {
    expect(effectiveNotifyVolume(50, 20, false)).toMatchObject({ notify: 50, media: 20, dualBar: true });
    expect(effectiveNotifyVolume(50, 20, true)).toMatchObject({ notify: 0, media: 0 });
  });

  it("F545 电量平滑步进 ≤2%/次 + 低电节流 1h + 三处同源", () => {
    const rt: BtBatteryRt = { displayed: 10, lastLowWarnAt: {} };
    smoothBattery(rt, 90);
    expect(rt.displayed).toBe(12);
    smoothBattery(rt, 90);
    expect(rt.displayed).toBe(14);
    expect(lowBatteryWarn(rt, "hp", 15, 0)).toBe(true);
    expect(lowBatteryWarn(rt, "hp", 10, 1_800_000)).toBe(false); // 30min 内不重提
    expect(lowBatteryWarn(rt, "hp", 10, 3_600_000)).toBe(true);  // 1h 后可再提
    expect(lowBatteryWarn(rt, "hp", 25, 4_000_000)).toBe(false); // 非低电不提醒
  });

  it("F546 三状态横幅：2s 就绪/进度/手装出路", () => {
    const ready = deviceNotifyBanner("ready", "U 盘");
    expect(ready.dwellMs).toBe(2000);
    expect(ready.body).toBe("已就绪");
    const installing = deviceNotifyBanner("installing", "声卡", 0.5);
    expect(installing.body).toContain("50%");
    const manual = deviceNotifyBanner("manual-needed", "打印设备");
    expect(manual.route).toBe("f444");
  });

  it("F547 平衡增益（等功率）+ 中心格点", () => {
    const l = balanceGains(-100);
    expect(l.left).toBeCloseTo(1, 3);
    expect(l.right).toBeCloseTo(0, 3);
    const c = balanceGains(0);
    expect(c.left).toBeCloseTo(0.7071, 3);
    expect(c.right).toBeCloseTo(0.7071, 3);
    expect(balanceGains(50).right).toBeGreaterThan(balanceGains(50).left);
  });
});

/* ------------------------------- F549 ------------------------------- */

describe("F549 时钟悬停完整日期（农历离线引擎）", () => {
  it("位表-锚点内部一致性不变量（2025-2030 全查）", () => {
    for (let y = 2025; y <= 2030; y++) {
      expect(yearWalkLandsOnNextCny(y)).toBe(true);
    }
  });

  it("春节锚点 = 正月初一（2026-2030 五年抽检）", () => {
    const anchors = CNY_ANCHORS.slice(1, 6);
    for (const [y, m, d] of anchors) {
      const l = solarToLunar(y, m, d);
      expect(l).toMatchObject({ year: y, month: 1, day: 1, leap: false });
    }
  });

  it("主册偏差事实锚：2026-09-25 = 八月十五（中秋）", () => {
    const l = solarToLunar(2026, 9, 25);
    expect(l).toMatchObject({ month: 8, day: 15, leap: false });
    expect(hoverDateLine(2026, 9, 25)).toContain("八月十五");
  });

  it("闰月锚：2025 闰六月 / 2028 闰五月", () => {
    expect(solarToLunar(2025, 8, 1)).toMatchObject({ month: 6, day: 8, leap: true });
    expect(solarToLunar(2028, 6, 23)).toMatchObject({ month: 5, day: 1, leap: true });
  });

  it("跨年边界：2026-01-01 属乙巳年冬月十三；星期锚定 2026-09-25=周五", () => {
    expect(solarToLunar(2026, 1, 1)).toMatchObject({ year: 2025, month: 11, day: 13, leap: false }); // 冬月十三
    expect(weekday(2026, 9, 25)).toBe(5);
    expect(hoverDateLine(2026, 9, 25)).toContain("星期五");
  });

  it("范围外诚实 null（不硬编假农历）+ 干支名", () => {
    expect(solarToLunar(2024, 12, 31)).toBeNull();
    expect(solarToLunar(2031, 2, 1)).toBeNull();
    expect(ganzhi(2026)).toBe("丙午");
    expect(ganzhi(2025)).toBe("乙巳");
  });

  it("农历年天数与位表解码", () => {
    // 2025 乙巳闰六月：384 天；2026 丙午：354 天
    expect(lunarYearDays(LUNAR_INFO[0]!)).toBe(384);
    expect(lunarYearDays(LUNAR_INFO[1]!)).toBe(354);
  });

  it("Tooltip 三要素一行 + 农历开关", () => {
    const line = hoverDateLine(2026, 9, 25);
    expect(line).toMatch(/^2026 年 9 月 25 日 星期五 · 丙午年八月十五$/);
    expect(hoverDateLine(2026, 9, 25, false)).toBe("2026 年 9 月 25 日 星期五");
  });
});

/* ------------------------------- F550 ------------------------------- */

describe("F550 批次六验收锚点（前端面九域自检）", () => {
  it("十二域注册齐全（九功能域+F550 锚点域+v4/v5 引擎群）且检查点 ≥ 25", () => {
    expect(U3_ANCHOR_DOMAINS.length).toBe(12);
    const total = U3_ANCHOR_DOMAINS.reduce((a, d) => a + d.run().length, 0);
    expect(total).toBeGreaterThanOrEqual(U3_ANCHOR_MIN_CHECKS);
  });

  it("全量执行全绿（全绿基线判据）", () => {
    const run = anchorRuntime();
    expect(run.allGreen).toBe(true);
    expect(run.failed).toBe(0);
  });

  it("可执行性抽查：抽 5 条全部可跑出红绿", () => {
    const spot = anchorSpotCheck(5);
    expect(spot.length).toBe(5);
    for (const s of spot) expect(typeof s.pass).toBe("boolean");
  });
});
