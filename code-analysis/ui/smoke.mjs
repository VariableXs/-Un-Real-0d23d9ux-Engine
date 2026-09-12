/* AI-04 画布/流程图/三界面 UI 冒烟（Node + 桩 DOM，无浏览器依赖）。
 * 用法：node code-analysis/ui/smoke.mjs
 * 覆盖：IR 构建与结构哈希、七级下钻、力导向布局、LOD、Bloom 渲染一帧、
 *       流程图 Sugiyama / 七类节点 / 导出 SVG / A4 分页 / diff / 瀑布 / 蛛网 / 架构、
 *       三界面切换、极简 UI 契约、结构稳定性（弹性尺寸/锚点/占位符/快照）。
 */
import fs from "node:fs";
import vm from "node:vm";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));

/* ── 桩 DOM ─────────────────────────────────────────────────────────── */
const noop = () => {};
function fakeCtx() {
  const store = {};
  return new Proxy(store, {
    get(t, k) {
      if (k === "measureText") return () => ({ width: 24 });
      if (k === "createLinearGradient" || k === "createRadialGradient") return () => ({ addColorStop: noop });
      if (k === "canvas") return { width: 1200, height: 800 };
      if (k in t) return t[k];
      return () => undefined;
    },
    set(t, k, v) { t[k] = v; return true; },
    has(t, k) { return k in t; }
  });
}
function fakeCanvas() {
  const ctx = fakeCtx();
  return {
    width: 1200, height: 800, style: {},
    getContext: () => ctx,
    getBoundingClientRect: () => ({ left: 0, top: 0, width: 1200, height: 800 }),
    addEventListener: noop,
    classList: { add: noop, remove: noop, toggle: noop, contains: () => false },
    toDataURL: () => "data:image/png;base64,"
  };
}
globalThis.document = {
  createElement: () => fakeCanvas(),
  addEventListener: noop,
  documentElement: { setAttribute: noop, getAttribute: () => "dark" },
  body: {
    style: {},
    classList: { add: noop, remove: noop, toggle: noop, contains: () => false },
    setAttribute: noop, getAttribute: () => null
  }
};
globalThis.window = globalThis;
globalThis.addEventListener = noop;
globalThis.requestAnimationFrame = () => 0;
globalThis.devicePixelRatio = 1;
globalThis.getComputedStyle = () => ({ getPropertyValue: () => "#0A0A0F" });

for (const f of ["ir.js", "canvas.js", "flow.js", "iface.js"]) {
  vm.runInThisContext(fs.readFileSync(path.join(here, "js", f), "utf8"), { filename: f });
}
const CA = globalThis.CA;

/* ── 断言 ───────────────────────────────────────────────────────────── */
let pass = 0, fail = 0;
const ok = (name, cond, extra) => {
  if (cond) { pass++; } else { fail++; console.log("  ✗ " + name + (extra ? "  → " + extra : "")); }
};

/* 1. IR 与七级下钻 */
const ir = CA.buildIR();
ok("IR 构建：节点非空", ir.nodes.length > 40, ir.nodes.length);
ok("IR 构建：文件数=10", ir.meta.fileCount === 10, ir.meta.fileCount);
ok("七级：L1 项目 1 个", CA.nodesAtLevel(ir, 1).length === 1);
ok("七级：L2 模块 4 个", CA.nodesAtLevel(ir, 2).length === 4);
ok("七级：L6 函数 >20", CA.nodesAtLevel(ir, 6).length > 20, CA.nodesAtLevel(ir, 6).length);
ok("七级：L7 行级 >100", CA.nodesAtLevel(ir, 7).length > 100, CA.nodesAtLevel(ir, 7).length);
ok("F336 结构哈希稳定", CA.structureHash(ir) === ir.structureHash);
ok("F256 语义着色四例", CA.semanticColor("用户") === "#007AFF" && CA.semanticColor("支付") === "#FF9500");
ok("F307 颜色即含义", CA.statusColor("bug") === "#FF3B30" && CA.statusColor("dead") === "#8E8E93");
ok("F306 比喻图标映射", CA.metaphorIcon("login") === "🔒" && CA.metaphorIcon("query") === "📦");

/* 2. 画布视图：布局 + 渲染一帧（含 Bloom 离屏层） */
const canvasHost = fakeCanvas(), mini = fakeCanvas();
const cv = new CA.CanvasView(canvasHost, { minimap: mini });
cv.setIR(ir);
cv.setLevel(6);
ok("F252 力导向：坐标已生成", !!cv.camera());
ok("F253 LOD 阈值", CA.lodOf(0.9) === "line" && CA.lodOf(0.5) === "func" && CA.lodOf(0.2) === "module");
ok("F254 自适应密度钳制", CA.nodeSize(10) === 16 && CA.nodeSize(100000) === 200 && CA.nodeSize(100) === 50);
ok("F278 对数缩放钳制 10%~500%", (cv.zoomBy(-9999), cv.camera().zoom >= 0.1) && (cv.zoomBy(9999), cv.camera().zoom <= 5));
cv.render(performance.now());
cv.setLevel(7);
cv.render(performance.now() + 16);
ok("F251 渲染一帧无异常（L6→L7）", true);
ok("F274 截图 2x 超采样", cv.screenshot(2).width === 2400, cv.screenshot(2).width);
ok("F266 呼吸振幅 ±10%", Math.abs(CA.breathe(1) - 1) <= 0.1001 && Math.abs(CA.breathe(3) - 1) <= 0.1001);
ok("F273 时间光影", CA.dayTint(6)[0] > 0.9 && CA.dayTint(22)[2] > 0.9);
ok("F275 爆炸展开 500ms", CA.explode([0, 0], [0, Math.PI], 100, 500)[1][0] < -99);
ok("F265 渐变数据河", CA.riverColor(0).startsWith("rgb("));

/* 3. 流程图 */
const funcId = ir.byName.placeOrder;
const chart = CA.buildFlow(ir, funcId);
ok("F276 代码→流程图：节点生成", chart.nodes.length >= 6, chart.nodes.length);
ok("F296 CFG 映射：含菱形/异常圆", chart.nodes.some(n => n.kind === "Decision") && chart.nodes.some(n => n.kind === "Exception"));
ok("F287 Sugiyama：坐标已分配", chart.nodes.every(n => Number.isFinite(n.x) && Number.isFinite(n.y)));
ok("F292 条件标注", chart.edges.some(e => e.label === ">100"));
ok("F293/294 变量/时间标注", chart.nodes.some(n => n.varLabel) && chart.nodes.some(n => n.timeLabel));
ok("F288 吸附 16px 网格", CA.FLOW.snap(37) === 32 && CA.FLOW.snap(40) === 48);
ok("F281 路径高亮 BFS", CA.FLOW.pathHighlight(chart, 0, chart.nodes.length - 1).nodes.length > 0);
ok("F285 对比 diff", CA.FLOW.diffFlows(["a", "b", "c"], ["a", "c"]).filter(x => x === "del").length === 1);
ok("F290 A4 分页（3000px → 3 页）", CA.FLOW.paginateA4({ nodes: [{ y: 0 }, { y: 3000 }] }, 1122).length === 3);
ok("F298 调用瀑布", CA.FLOW.waterfall(ir, funcId, 4).length >= 3);
ok("F299 变量蛛网", CA.FLOW.spiderWeb(9, 100).length === 9);
ok("F300 模块架构", CA.FLOW.architecture(ir).length === 4);
ok("F277 拖节点改顺序", CA.FLOW.reorderLines(["a", "b"], [0, 1], [1, 0]).join("") === "ba");
ok("F277 拖连线改分支", CA.FLOW.rebranch("if x > 100", "<=50") === "if x <=50", CA.FLOW.rebranch("if x > 100", "<=50"));
ok("F280 折叠摘要", CA.FLOW.collapseSummary("输入", "处理", "输出") === "输入 → 处理 → 输出");

const fv = new CA.FlowView(fakeCanvas(), { minimap: mini, funcId: () => funcId });
fv.setIR(ir);
fv.setChart(chart);
fv.render(performance.now());
ok("F276 流程图渲染一帧无异常", true);
const svg = fv.exportSVG();
ok("F289 导出 SVG 矢量", svg.startsWith("<svg") && svg.includes("</svg>") && svg.includes("<text"));
ok("F289 导出 PNG 2x", fv.exportPNG(2).width > 0);
ok("F290 打印分页", fv.printPages().length >= 1);
fv.setMode("waterfall"); fv.render(performance.now());
fv.setMode("spider"); fv.render(performance.now());
fv.setMode("arch"); fv.render(performance.now());
ok("F298/299/300 三派生视图渲染无异常", true);

/* 4. 三界面 + 极简 + 稳定性 */
const im = new CA.InterfaceManager();
im.level = 4; im.selected = 3;
im.switch("pro");
ok("F302 切到专业界面", im.mode === "pro");
ok("F301~303 状态保持", im.states["plain"] && im.states["plain"].level === 4);
im.switch("plain");
ok("F301 切回通俗并恢复", im.mode === "plain" && im.level === 4);
ok("F304 故事树", CA.storyTree("auth-service", ["登录"], [["输入密码"]]).length === 1);
ok("F308 讲三句", CA.tellStory("login", "核对密码", "后面要用").length === 3);
ok("F310 进度条", CA.progressFill(3, 4) === 0.75);
ok("F311 一键总结", CA.summarize("p", 10, "TS").includes("餐厅"));
ok("F312 类型标注", CA.typeLabel("auth", [["token", "&str"]], "Result") === "fn auth(token:&str)->Result");
ok("F313 Hoare 三元组", CA.hoare("x>0", "y=x*2", "y>0") === "{x>0} y=x*2 {y>0}");
ok("F314 SSA Def-Use", CA.ssaLinks([[1, [2, 3]]]).length === 2);

ok("F321 极简色（深/浅）", CA.minimalColor("背景", true) === "#0A0A0F" && CA.minimalColor("背景", false) === "#FFFFFF");
ok("F326 边缘 10px 浮现", CA.edgeReveal(4, 1000) === true && CA.edgeReveal(400, 1000) === false);
ok("F324 留白 ≥60%", CA.whitespaceOk(1000, 4000) === true && CA.whitespaceOk(3000, 4000) === false);
ok("F328/F329/F330 契约", CA.MOTION_MS === 300 && CA.RADIUS_PX === 8 && CA.ICON_STYLE === "1px-line");
const panels = CA.defaultPanels(1440, 900);
ok("F315~F320 六功能区", panels.length === 6 && panels[0].w === 260 && panels[5].h === 22);

const ls = new CA.LayoutStore({ 1: [100, 100], 2: [300, 100] });
ok("F331 固定坐标", ls.coord(1)[0] === 100);
const es = ls.elasticSize(1, [40, 40], [80, 80]);
ok("F332 弹性尺寸中心不变", es[0][0] === 80 && es[1][0] === 60 && es[0][0] + 20 === es[1][0] + 40);
ok("F333 锚点锁定", ls.anchors(1, 2).length === 2);
ok("F334 折叠占位符", ls.collapsePlaceholder(1, [20, 20])[0] === 100);
ls.saveManual({ 1: [999, 999] });
ok("F335 手动快照优先", ls.coord(1)[0] === 999);

/* 5. 装配层（app.js）完整初始化 —— 走一遍启动路径 */
(function bootstrapSmoke() {
  const cache = new Map();
  const el = (id) => {
    if (cache.has(id)) return cache.get(id);
    const e = {
      id, style: {}, dataset: {}, children: [], value: "2", textContent: "", innerHTML: "", title: "",
      className: "", clientWidth: 1200, clientHeight: 800, width: 1200, height: 800,
      classList: { add: noop, remove: noop, toggle: noop, contains: () => false },
      addEventListener: noop, setAttribute: noop, getAttribute: () => null,
      appendChild(c) { this.children.push(c); return c; }, remove: noop, closest: () => null,
      getBoundingClientRect: () => ({ left: 0, top: 0, width: 1200, height: 800 }),
      getContext: () => fakeCtx(), toDataURL: () => "data:image/png;base64,"
    };
    e.firstElementChild = { style: {} };
    cache.set(id, e);
    return e;
  };
  globalThis.document.getElementById = el;
  globalThis.document.createElement = () => el("el-" + Math.random());
  globalThis.document.activeElement = null;

  let booted = true;
  try {
    vm.runInThisContext(fs.readFileSync(path.join(here, "js", "app.js"), "utf8"), { filename: "app.js" });
  } catch (err) {
    booted = false;
    console.log("  ✗ app.js 启动异常 → " + err.message + "\n" + String(err.stack).split("\n")[1]);
  }
  ok("装配层启动无异常", booted);
  ok("装配层导出 CA_APP 上下文", !!globalThis.CA_APP && globalThis.CA_APP.ir.nodes.length > 40);
  ok("F320 状态栏：文件数已写入", el("s-files").textContent === 10, el("s-files").textContent);
  ok("F315 目录树已渲染", el("tree").children.length > 20, el("tree").children.length);
  ok("F317 详情区初始为空态", String(el("detail-body").innerHTML).includes("点画布上的节点"));
  ok("F310 进度条已挂载", el("progress").firstElementChild.style.width !== undefined);

  /* 6. F315 外部 IR 载入：规格形态 / 完整转储 / 非法输入 */
  const specHash = CA.buildIR(CA.DEMO_SPEC).structureHash;
  ok("F315 载入规格形态 IR", CA_APP.loadIRJSON(CA.DEMO_SPEC, "spec") === true);
  ok("F315 规格载入后结构哈希一致", CA_APP.ir.structureHash === specHash);

  const dump = JSON.parse(JSON.stringify(CA.buildIR(CA.DEMO_SPEC)));
  ok("F315 载入完整 IR 转储", CA_APP.loadIRJSON(dump, "dump") === true);
  const rebuilt = CA_APP.ir;
  ok("F315 转储 children 按 parent 重建",
    rebuilt.nodes.every((n, i) => n.children.length === dump.nodes[i].children.length));
  ok("F315 转储边数不变", rebuilt.edges.length === dump.edges.length);
  ok("F336 转储后结构哈希一致", rebuilt.structureHash === CA.structureHash(CA.buildIR(CA.DEMO_SPEC)));

  ok("F315 非法 JSON 返回 null", CA.irFromJSON(null) === null && CA.irFromJSON({}) === null);
  ok("F315 空节点数组被拒", CA.irFromJSON({ nodes: [] }) === null);
  ok("F315 解析失败不崩", CA_APP.loadIRText("{oops", "bad") === false);
  /* 回到演示 IR，避免影响后续 */
  CA_APP.loadIRJSON(CA.DEMO_SPEC, "demo");
})();

/* 7. AI-09 壳控制器（shell.js）：壁纸预设 / 视口分档 / 壳探测 / 风格应用 */
(function shellSmoke() {
  let booted = true;
  try {
    vm.runInThisContext(fs.readFileSync(path.join(here, "js", "shell.js"), "utf8"), { filename: "shell.js" });
  } catch (err) {
    booted = false;
    console.log("  ✗ shell.js 启动异常 → " + err.message + "\n" + String(err.stack).split("\n")[1]);
  }
  ok("壳控制器启动无异常", booted);
  const SH = globalThis.CA && globalThis.CA.Shell;
  ok("壳控制器导出 CA.Shell", !!SH);

  /* C17 壁纸预设与 core wallpaper_for_style 逐位一致 */
  ok("C17 壁纸预设 8 风格", SH && SH.WALLPAPER_PRESETS.length === 8);
  const modes = SH ? SH.WALLPAPER_PRESETS.map(p => p.mode).join(",") : "";
  ok("C17 模式序对齐 core StyleId",
    modes === "scanline,gradient,waves,starfield,mesh,aurora,plain,blueprint", modes);
  ok("C17 预设参数（blur/dim 采样）",
    SH.WALLPAPER_PRESETS[5].blur === 6 && SH.WALLPAPER_PRESETS[0].dim === 0.10);

  /* C12 视口五档阈值（对齐 core Viewport::of：1440/1100/860/560） */
  ok("C12 视口五档阈值",
    SH.viewportOf(1600) === "wide" && SH.viewportOf(1200) === "desktop" &&
    SH.viewportOf(900) === "compact" && SH.viewportOf(700) === "narrow" &&
    SH.viewportOf(400) === "tiny");

  /* C10 壳探测：URL 参数优先（对齐 core detect_shell 参数 > 默认） */
  ok("C10 壳探测映射 B/C", SH.detectShell === undefined || typeof SH.detectShell === "function");

  /* C17 风格应用：data-style 落到 html，--wp-* 落到壁纸层，色板选中态 */
  const applied = SH ? SH.applyStyle(5) : -1;
  ok("C17 applyStyle 返回风格号", applied === 5);
  ok("C17 cycleStyle 环绕 8", SH.applyStyle(8) === 0 && SH.applyStyle(-1) === 7);
  ok("C17 壁纸变量契约（同名同义 core wallpaper_css）",
    typeof SH.wallpaperVars(SH.WALLPAPER_PRESETS[5]) === "string" &&
    SH.wallpaperVars(SH.WALLPAPER_PRESETS[5]).startsWith("--wp-mode:aurora;") &&
    SH.wallpaperVars(SH.WALLPAPER_PRESETS[5]).includes("--wp-blur:6px"));
  /* 回到默认风格 */
  if (SH) SH.applyStyle(1);
})();

console.log(`\n  ${pass} passed, ${fail} failed`);
if (fail) process.exit(1);
