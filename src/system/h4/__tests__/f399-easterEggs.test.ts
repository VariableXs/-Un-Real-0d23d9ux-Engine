import { describe, expect, it } from "vitest";
import { EGG_TRIGGERS, auditNoFeatureGating, auditPerfRegistry, bootEgg, countBoot, lineageDataForEgg, loadState, tapVersion, terminalCommand } from "../f399-easterEggs";
import { __clearMem, memStore } from "../internal/store";

describe("F399 彩蛋总谱", () => {
  it("三枚触发条件口径齐备（判据）", () => {
    expect(Object.keys(EGG_TRIGGERS)).toEqual(["boot100", "about7taps", "terminalStar"]);
    expect(EGG_TRIGGERS.boot100.threshold).toBe(100);
    expect(EGG_TRIGGERS.boot100.once).toBe(true);
    expect(EGG_TRIGGERS.about7taps.threshold).toBe(7);
    expect(EGG_TRIGGERS.terminalStar.value).toBe("star");
  });

  it("彩蛋①：恰在第 100 次开机触发，且一生一次（一次性判据）", () => {
    __clearMem();
    const s = memStore();
    for (let i = 0; i < 99; i++) countBoot(s); // 计到 99
    expect(bootEgg(s).play).toBe(false);
    countBoot(s); // 第 100 次
    const hit = bootEgg(s);
    expect(hit.play).toBe(true);
    expect(hit.variant).toBe("star-emblem-particles");
    countBoot(s); // 第 101 次
    expect(bootEgg(s).play).toBe(false); // 不再重复
    expect(loadState(s).boot100Played).toBe(true);
  });

  it("彩蛋②：版本号 7 连点触发、触发后重置循环", () => {
    __clearMem();
    const s = memStore();
    let play = false;
    for (let i = 0; i < 6; i++) play = tapVersion(s).play;
    expect(play).toBe(false);
    play = tapVersion(s).play; // 第 7 次
    expect(play).toBe(true);
    expect(loadState(s).aboutTaps).toBe(0); // 重置
    expect(tapVersion(s).play).toBe(false); // 下一轮重新计
  });

  it("彩蛋③：star 命令进星野、Esc 退（成对判据）", () => {
    expect(terminalCommand("star")).toEqual({ play: true, exit: false, scene: "starfield-particles" });
    expect(terminalCommand("STAR ").play).toBe(true); // 大小写宽容
    expect(terminalCommand("esc").exit).toBe(true);
    expect(terminalCommand("ls").play).toBe(false);
  });

  it("性能零影响：彩蛋场景必须在 F124 总谱表登记（判据）", () => {
    const f124 = new Set(["starfield-particles", "star-emblem-particles"]);
    expect(auditPerfRegistry("starfield-particles", f124).pass).toBe(true);
    expect(auditPerfRegistry("rogue-uncapped-animation", f124).pass).toBe(false);
  });

  it("不藏功能审计：彩蛋解锁功能 = 缺陷（判据）", () => {
    expect(auditNoFeatureGating([]).pass).toBe(true);
    const bad = auditNoFeatureGating(["pro-mode"]);
    expect(bad.pass).toBe(false);
    expect(bad.gated).toEqual(["pro-mode"]);
  });

  it("谱系动画与 F199 数据同源（判据）：同一加载器、零复制", () => {
    const f199Data = [
      { version: "1.0", codename: "Aurora" },
      { version: "1.1", codename: "Nebula" },
    ];
    expect(lineageDataForEgg(() => f199Data)).toBe(f199Data); // 引用相同——同源
  });
});
