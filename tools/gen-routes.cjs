#!/usr/bin/env node
/**
 * M-55 路由注册公开表生成器：docs/routes.src.json → docs/ROUTES.md
 * 用法：
 *   node tools/gen-routes.cjs           # 生成（写入 docs/ROUTES.md）
 *   node tools/gen-routes.cjs --check   # 校验（已生成文档与源 100% 同步则退出 0，否则 1 → CI 红）
 * 红线：生成器产物必须可再生成，过期即 CI 红。
 */
const fs = require("fs");
const path = require("path");

const ROOT = path.join(__dirname, "..");
const SRC = path.join(ROOT, "docs", "routes.src.json");
const OUT = path.join(ROOT, "docs", "ROUTES.md");

function render(reg) {
  const lines = [];
  lines.push("# ROUTES — variable:// 深链路由注册公开表");
  lines.push("");
  lines.push("> 自动生成：`node tools/gen-routes.cjs`（源：`docs/routes.src.json`）。手工修改会被下次生成覆盖。");
  lines.push("> 深链解析协议见 U-37 协议中枢；协议注册仅便携部署态且退出退订。");
  lines.push("");
  lines.push(`共 ${reg.routes.length} 条路由。`);
  lines.push("");
  lines.push("| Verb | 参数 | 必填 | 参数说明 | 所有者 | Since | 说明 | 示例链接 |");
  lines.push("|---|---|---|---|---|---|---|---|");
  for (const r of reg.routes) {
    const params = r.params.map((p) => p.name).join(", ") || "—";
    const required = r.params.map((p) => (p.required ? "✓" : "—")).join(", ") || "—";
    const desc = r.params.map((p) => `${p.name}: ${p.desc}`).join("<br>") || "—";
    lines.push(`| \`${r.verb}\` | ${params} | ${required} | ${desc} | ${r.owner} | ${r.since} | ${r.desc} | \`${r.example}\` |`);
  }
  lines.push("");
  lines.push("## 可复制示例链接");
  lines.push("");
  for (const r of reg.routes) {
    lines.push(`- ${r.desc}：\`${r.example}\``);
  }
  lines.push("");
  return lines.join("\n");
}

function main() {
  const reg = JSON.parse(fs.readFileSync(SRC, "utf8"));
  if (!reg.version || reg.version !== 1) {
    console.error("gen-routes: routes.src.json version 必须为 1");
    process.exit(2);
  }
  const text = render(reg);
  if (process.argv.includes("--check")) {
    const cur = fs.existsSync(OUT) ? fs.readFileSync(OUT, "utf8") : "";
    if (cur !== text) {
      console.error("gen-routes --check: docs/ROUTES.md 与 routes.src.json 不同步（请运行 node tools/gen-routes.cjs 后提交）");
      process.exit(1);
    }
    console.log(`gen-routes --check: OK（${reg.routes.length} 条路由，文档与源同步）`);
    return;
  }
  fs.writeFileSync(OUT, text, "utf8");
  console.log(`gen-routes: 已生成 docs/ROUTES.md（${reg.routes.length} 条路由）`);
}

main();
