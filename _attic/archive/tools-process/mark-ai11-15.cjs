const fs = require("fs");
const p = "AURORA-10000-AI分工完成图.md";
let t = fs.readFileSync(p, "utf8");
t = t.replace(
  "| W2 | AI-11~AI-25 | F01251~F03125 | 🔶 625/1875 |",
  "| W2 | AI-11~AI-25 | F01251~F03125 | 🔶 1250/1875（AI-11~AI-15、AI-21~AI-25 区块均 ✅，待波次出口统验） |"
);
const luodian =
  "src/system/desktop-design/{types,state,catalog,logic,runtime,runners,particles,ambience,labels,ritualBus,mounts}.ts + {DesignCenter,TheaterOverlay,RitualOverlay}.tsx + design-center.css + activate.ts + __tests__/{catalog,logic}.test.ts；src/design/tokens.css（族0066 令牌段「AURORA-10000：AI-11~AI-15 批次，勿删」）；src/system/desktop/DesktopShell.tsx（activate 一行接入）；src/system/desktop/taskbarMenu.ts + src/system/taskbar/Taskbar.tsx（任务栏空区菜单「设计中心」入口，默认隐藏）；src/i18n/dictionaries.ts（aurW2DesignCenter 键，三语）。";
const lines = t.split("\n");
let cur = null;
let filled = 0;
for (let i = 0; i < lines.length; i++) {
  const m = lines[i].match(/^### (AI-1[1-5]) /);
  if (m) cur = m[1];
  if (cur && lines[i].startsWith("- 落点记录：（实施会话完成后填写实际改动文件与提交号）")) {
    lines[i] = "- 落点记录：" + luodian;
    filled++;
    cur = null;
  }
}
fs.writeFileSync(p, lines.join("\n"));
console.log("filled:", filled);
