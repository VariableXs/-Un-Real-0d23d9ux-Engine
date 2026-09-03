/**
 * 思维导图节点三件套自动化测试 v2（抗卡顿版）
 *  - 屏蔽背景 worker / WebGL，保证页面响应
 *  - 原始鼠标事件，不等待元素稳定
 *  - 日志同步写文件
 */
import { chromium as pwChromium } from "playwright";
import chromium from "@sparticuz/chromium";
import fs from "node:fs";

fs.mkdirSync("shots", { recursive: true });
fs.writeFileSync("shots/test.log", "");
const log = (...a) => {
  const line = a.map((x) => (typeof x === "string" ? x : JSON.stringify(x))).join(" ");
  fs.appendFileSync("shots/test.log", line + "\n");
  console.log(line);
};
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const exePath = await chromium.executablePath();
const browser = await pwChromium.launch({
  executablePath: exePath,
  args: [...chromium.args, "--no-sandbox", "--disable-gpu", "--disable-webgl", "--disable-webgl2"],
  env: { ...process.env, LD_LIBRARY_PATH: "/tmp/chromium-libs/lib" },
  headless: true,
});
const ctx = await browser.newContext({ viewport: { width: 1400, height: 900 } });
await ctx.grantPermissions(["clipboard-read", "clipboard-write"], { origin: "http://localhost:1420" });
// 屏蔽背景 worker（aurora 极光），避免软渲染卡死
await ctx.route("**/bg.worker*", (r) => r.abort());
await ctx.route("**/*worker*", (r) => r.abort());
const page = await ctx.newPage();
page.on("pageerror", (e) => { if (!String(e.message).includes("tauri")) log("PAGEERROR:", e.message.slice(0, 120)); });
// 插桩：autogrow/repair 事件流 + 节点/内容框 style 变更（环形缓冲，供 P6 取证）
await page.addInitScript(() => {
  window.__agLog = [];
  window.__phase = "boot";
  const AG = window.__agLog;
  window.addEventListener("variable:mm-autogrow", (ev) => {
    if (AG.length > 900) AG.splice(0, 200);
    AG.push({ t: Math.round(performance.now()), ph: window.__phase, ...ev.detail });
  });
  window.addEventListener("variable:mm-repair-node", (ev) => {
    if (AG.length > 900) AG.splice(0, 200);
    AG.push({ t: Math.round(performance.now()), ph: window.__phase, repair: ev.detail });
  });
  window.__mutLog = [];
  const ML = window.__mutLog;
  const mo = new MutationObserver((muts) => {
    for (const m of muts) {
      const el = m.target;
      if (!(el instanceof HTMLElement)) continue;
      if (el.classList.contains("mm-node") || el.classList.contains("mm-content")) {
        if (ML.length > 900) ML.splice(0, 300);
        ML.push({
          t: Math.round(performance.now()), ph: window.__phase,
          cls: el.className.slice(0, 20),
          style: el.style.width + "x" + el.style.height,
          maxWH: el.style.maxWidth + "/" + el.style.maxHeight,
          client: el.clientWidth + "x" + el.clientHeight,
        });
      }
    }
  });
  const attach = () => {
    if (document.documentElement) mo.observe(document.documentElement, { subtree: true, childList: true, attributes: true, attributeFilter: ["style"] });
    else setTimeout(attach, 30);
  };
  attach();
});
await ctx.route("**/*worker*", (r) => r.abort());
page.setDefaultTimeout(8000);
const pageErrors = [];
page.on("pageerror", (e) => pageErrors.push(e.message.slice(0, 120)));
page.on("console", (m) => { if (m.type() === "error") pageErrors.push("console: " + m.text().slice(0, 120)); });

try {
  // ---------- Phase 0: 进入思维导图模式 ----------
  await page.goto("http://localhost:1420/", { waitUntil: "domcontentloaded", timeout: 30000 });
  await sleep(2000);
  // 直接用 JS 点击模式按钮（绕过遮罩/稳定等待）
  const clicked = await page.evaluate(() => {
    const btn = Array.from(document.querySelectorAll("button")).find((b) =>
      (b.getAttribute("aria-label") || "").includes("思维导图"));
    if (!btn) return false;
    btn.click();
    return true;
  });
  log("[phase0] 点击思维导图模式按钮:", clicked);
  await sleep(1500);

  // 导图选择器 → 新建
  const chooser = await page.evaluate(() => {
    const btn = document.querySelector(".chooser-card .btn.primary");
    if (btn) { btn.click(); return btn.textContent.trim(); }
    return null;
  });
  if (chooser) log("[phase0] 选择器新建导图:", chooser);
  await sleep(1200);

  const hasView = await page.evaluate(() => !!document.querySelector(".mindmap-view"));
  log("[phase0] .mindmap-view 存在:", hasView);
  if (!hasView) {
    log("!! 页面文本:", (await page.evaluate(() => document.body.innerText)).slice(0, 400));
    throw new Error("no mindmap view");
  }

  // ---------- Phase 1: 工具栏"新建文本框"创建节点 ----------
  const vp = await page.evaluate(() => {
    const el = document.querySelector(".mindmap-view");
    const r = el.getBoundingClientRect();
    return { x: r.left, y: r.top, w: r.width, h: r.height };
  });
  // 找到工具栏的"新建文本框"按钮并 JS 点击
  const created = await page.evaluate(() => {
    const btn = Array.from(document.querySelectorAll("button")).find((b) =>
      (b.getAttribute("aria-label") || "").includes("新建文本框") ||
      (b.getAttribute("data-tip") || "").includes("新建文本框"));
    if (!btn) return false;
    btn.click();
    return true;
  });
  await sleep(1000);
  log("[phase1] 点击新建文本框按钮:", created);
  const st1 = await page.evaluate(() => ({
    nodes: document.querySelectorAll(".mm-node").length,
    editing: document.querySelectorAll(".mm-content[contenteditable='true']").length,
  }));
  log("[phase1] 双击画布:", st1);

  // 若未进入编辑，双击节点
  if (st1.editing === 0 && st1.nodes > 0) {
    const nb = await page.evaluate(() => {
      const el = document.querySelector(".mm-node");
      const r = el.getBoundingClientRect();
      return { x: r.left + r.width / 2, y: r.top + r.height / 2 };
    });
    await page.mouse.dblclick(nb.x, nb.y);
    await sleep(700);
    log("[phase1] 补双击节点后编辑态:", await page.evaluate(() =>
      document.querySelectorAll(".mm-content[contenteditable='true']").length));
  }

  // ---------- Phase 2: 输入长文字 ----------
  const measure = () => page.evaluate(() => {
    const node = document.querySelector(".mm-node");
    const content = document.querySelector(".mm-content");
    if (!node || !content) return null;
    const nr = node.getBoundingClientRect();
    const cr = content.getBoundingClientRect();
    return {
      nodeBox: [Math.round(nr.width), Math.round(nr.height)],
      nodeStyle: [node.style.width, node.style.height],
      contentScroll: [content.scrollWidth, content.scrollHeight],
      contentClient: [content.clientWidth, content.clientHeight],
      innerScrollY: content.scrollHeight - content.clientHeight,
      innerScrollX: content.scrollWidth - content.clientWidth,
      visOverflowY: Math.round(Math.max(0, cr.bottom - nr.bottom)),
      visOverflowX: Math.round(Math.max(0, cr.right - nr.right)),
      textLen: (content.textContent || "").length,
      editing: content.isContentEditable,
    };
  });

  log("[phase2] 输入前:", await measure());
  const LONG = "这是一段用于测试自适应尺寸的超长文本，修复前矩形节点会被 3:1 长宽比守卫钳制在 840px 高度，超出部分全部被裁切。".repeat(45);
  await page.keyboard.type(LONG, { delay: 0 });
  await sleep(1800);
  log("[phase2] 输入后(超长):", await measure());

  await page.keyboard.type("追加的更多文字内容继续增加以测试收敛行为。".repeat(20), { delay: 0 });
  await sleep(1500);
  const m2 = await measure();
  log("[phase2] 追加后:", m2);
  // 新契约：自动增长可用满 20000px；只有超过绝对上限才框内滚动，视觉零溢出
  const fitVerdict = m2
    ? (m2.visOverflowY <= 2 && m2.visOverflowX <= 2
        ? (m2.nodeBox[1] <= 20002
            ? (m2.innerScrollY > 2 ? `△ 限高+框内滚动(高${m2.nodeBox[1]}，滚${m2.innerScrollY}px)` : "✓ 完全自适应")
            : "✗ 高度超绝对上限!")
        : "✗ 文字视觉溢出边框!")
    : "?";
  log("[phase2] 判定:", fitVerdict);

  // ---------- Phase 3: 复制粘贴 ----------
  await page.keyboard.press("Control+a");
  await sleep(250);
  const selA = await page.evaluate(() => {
    const s = window.getSelection();
    return { len: String(s).length, collapsed: s.isCollapsed };
  });
  log("[phase3] Ctrl+A:", selA);

  await page.keyboard.press("Control+c");
  await sleep(400);
  let clipLen = -1;
  try {
    clipLen = (await page.evaluate(() => navigator.clipboard.readText())).length;
  } catch (e) { clipLen = -2; }
  log(`[phase3] Ctrl+C 剪贴板长度: ${clipLen}`, clipLen > 100 ? "✓" : "✗");

  await page.keyboard.press("Control+End").catch(() => {});
  await page.keyboard.press("End");
  const lenB = (await measure()).textLen;
  await page.keyboard.press("Control+v");
  await sleep(600);
  const lenA = (await measure()).textLen;
  log(`[phase3] Ctrl+V: ${lenB} → ${lenA}`, lenA > lenB ? "✓" : "✗");

  // ---------- Phase 4: 拖拽框选 ----------
  const cb = await page.evaluate(() => {
    const el = document.querySelector(".mm-content[contenteditable='true']");
    if (!el) return null;
    const r = el.getBoundingClientRect();
    return { x: r.left, y: r.top, w: r.width, h: r.height };
  });
  if (cb) {
    // 在视口内找一个真正落在文字行上的点：用 TreeWalker 找第一个非空文本
    // 节点，取其 Range.getClientRects() 的可视行矩形中心（caretRangeFromPoint
    // 在此环境总命中空文本节点，不可用）。从该点起拖，拖拽终点向框内收边。
    const pt = await page.evaluate((bx) => {
      const el = document.querySelector(".mm-content[contenteditable='true']");
      if (!el) return null;
      const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
      let node = walker.nextNode();
      while (node && !(node.textContent || "").trim()) node = walker.nextNode();
      if (!node) return null;
      const range = document.createRange();
      range.selectNodeContents(node);
      const rects = Array.from(range.getClientRects()).filter((r) => r.width > 8 && r.height > 8);
      const vis = rects.find((r) => r.top > 170 && r.bottom < 840 && r.left > bx - 40);
      const rect = vis ?? rects.find((r) => r.top > 170 && r.bottom < 850) ?? rects[0];
      if (!rect) return null;
      return {
        x: Math.round(rect.left + Math.min(50, rect.width / 2)),
        y: Math.round((rect.top + rect.bottom) / 2),
        line: Math.round(rect.height),
      };
    }, Math.round(cb.x));
    log("[phase4] 扫描到的文字点:", pt);
    if (pt) {
      const endX = Math.min(pt.x + 90, Math.round(cb.x + cb.w) - 15);
      const endY = Math.min(pt.y + 40, Math.round(cb.y + cb.h) - 15);
      await page.mouse.move(pt.x, pt.y);
      await page.mouse.down();
      await page.mouse.move(endX, endY, { steps: 12 });
      await page.mouse.up();
    } else {
      log("[phase4] 视口内找不到可拖拽文字 ✗");
    }
    await page.mouse.up();
    await sleep(350);
    const sel = await page.evaluate(() => {
      const s = window.getSelection();
      return { len: String(s).length, collapsed: s.isCollapsed, head: String(s).slice(0, 16) };
    });
    log("[phase4] 拖拽框选:", sel, sel && !sel.collapsed && sel.len > 4 ? "✓" : "✗");

    const lb = (await measure()).textLen;
    await page.keyboard.press("Backspace");
    await sleep(350);
    const la = (await measure()).textLen;
    log(`[phase4] 选中删除: ${lb} → ${la}`, la < lb ? "✓" : "✗");
  } else log("[phase4] 找不到编辑框 ✗");

  // ---------- Phase 5: 静态（退出编辑） ----------
  await page.mouse.click(vp.x + 150, vp.y + 150);
  await sleep(1200);
  const m5 = await measure();
  log("[phase5] 静态模式:", m5);
  log("[phase5] 静态判定:", m5 && m5.visOverflowY <= 2 && m5.visOverflowX <= 2 ? "✓ 无溢出" : "✗ 溢出!");
  // 框内滚动条可用性：设置 scrollTop 必须生效（可上下滚动查看）
  const scrollOk = await page.evaluate(() => {
    const c = document.querySelector(".mm-content");
    if (!c) return null;
    c.scrollTop = 150;
    const t = c.scrollTop;
    c.scrollTop = 0;
    return { settable: t > 140, scrollH: c.scrollHeight, clientH: c.clientHeight };
  });
  log("[phase5] 框内滚动:", scrollOk,
    scrollOk && (scrollOk.scrollH > scrollOk.clientH + 2 ? (scrollOk.settable ? "✓ 可滚动" : "✗ 不可滚动!") : "✓ 无需滚动(全可见)"));

  // ---------- Phase 6: 多边形（菱形）节点长文自适应 ----------
  log("[phase6] --- 多边形节点测试 ---");
  await page.evaluate(() => { window.__phase = "p6"; });
  const nodeBox = await page.evaluate(() => {
    const el = document.querySelector(".mm-node");
    if (!el) return null;
    const r = el.getBoundingClientRect();
    return { x: r.left + r.width / 2, y: Math.min(Math.max(r.top + r.height / 2, 160), 850) };
  });
  if (nodeBox) {
    await page.mouse.click(nodeBox.x, nodeBox.y);
    await sleep(400);
    await page.mouse.click(nodeBox.x, nodeBox.y, { button: "right" });
    await sleep(400);
    const styleOpened = await page.evaluate(() => {
      const items = Array.from(document.querySelectorAll(".ctx-menu *"));
      const it = items.find((el) => (el.textContent || "").includes("风格面板"));
      if (!it) return false;
      (it.closest("button") ?? it).click();
      return true;
    });
    log("[phase6] 打开风格面板:", styleOpened);
    await sleep(600);
    const shapeChanged = await page.evaluate(() => {
      const btns = Array.from(document.querySelectorAll(".shape-btn"));
      if (btns.length < 5) return { ok: false, n: btns.length };
      btns[4]?.click(); // 顺序: rect,rounded,circle,triangle,diamond → 菱形
      return { ok: true, n: btns.length };
    });
    log("[phase6] 切换为菱形:", shapeChanged);
    await sleep(800);
    const nb2 = await page.evaluate(() => {
      const el = document.querySelector(".mm-node");
      const r = el.getBoundingClientRect();
      return { x: r.left + r.width / 2, y: Math.min(Math.max(r.top + r.height / 2, 160), 850) };
    });
    await page.mouse.dblclick(nb2.x, nb2.y);
    await sleep(600);
    await page.keyboard.press("Control+a");
    await page.keyboard.type("菱形节点长文自适应测试：多边形的内接矩形必须完整容纳全部文字，文字驱动增长不受长宽比守卫限制。".repeat(30), { delay: 0 });
    await sleep(2000);
    const m6 = await measure();
    log("[phase6] 菱形长文:", m6);
    // 多边形同契约：可用满 20000px，超出才内接框滚动，零视觉溢出
    log("[phase6] 菱形判定:", m6
      ? (m6.visOverflowY <= 2 && m6.visOverflowX <= 2
          ? (m6.nodeBox[1] <= 20002 ? "✓ 零溢出" : "✗ 高度超绝对上限!")
          : "✗ 视觉溢出!")
      : "?");
    // 提交：先关掉可能开着的属性面板（固定在左上角，会吃掉 (150,150) 的
    // 点击 —— 之前误点中面板里的五边形按钮，触发形状切换连环增长），
    // 再点击画布空白处完成失焦提交。
    await page.evaluate(() => {
      const x = document.querySelector(".inspector button[aria-label='close']");
      x?.click();
    });
    await sleep(400);
    await page.mouse.click(Math.round(vp.x * 0.3), Math.round(vp.y * 0.9));
    await sleep(1200);
    const m6s = await measure();
    log("[phase6] 菱形静态:", m6s, m6s && m6s.visOverflowY <= 2 ? "✓" : "✗");
  } else {
    log("[phase6] 找不到节点 ✗");
  }

  // ---------- Phase 7+8: 聚焦编辑变暗 / Shift+Enter 提交 ----------
  const nodeCenter = async () => page.evaluate(() => {
    const el = document.querySelector(".mm-node");
    const r = el.getBoundingClientRect();
    // y 下限 160：避开顶部工具栏（超长节点 top 在视口外时，钳在工具栏下方）
    return { x: r.left + r.width / 2, y: Math.min(Math.max(r.top + 40, 160), 850) };
  });
  {
    const nc = await nodeCenter();
    await page.mouse.dblclick(nc.x, nc.y);
    await sleep(700);
    const focusDim = await page.evaluate(() => {
      const root = document.querySelector(".mindmap-view");
      const edges = document.querySelector(".edge-layer");
      return {
        focusClass: root?.classList.contains("editing-focus") ?? false,
        edgeOpacity: edges ? getComputedStyle(edges).opacity : null,
      };
    });
    log("[phase7] 聚焦编辑变暗:", focusDim,
      focusDim.focusClass && parseFloat(focusDim.edgeOpacity ?? "1") < 1 ? "✓" : "✗");
    await page.keyboard.type("ShiftEnter提交测试");
    await sleep(300);
    await page.keyboard.press("Shift+Enter");
    await sleep(700);
    const committed = await page.evaluate(() => {
      const c = document.querySelector(".mm-content");
      const root = document.querySelector(".mindmap-view");
      return {
        editing: c?.isContentEditable ?? null,
        focusGone: !root?.classList.contains("editing-focus"),
        text: (c?.textContent || "").includes("ShiftEnter提交测试"),
      };
    });
    log("[phase8] Shift+Enter 提交:", committed,
      committed.editing === false && committed.focusGone && committed.text ? "✓" : "✗");
  }

  // ---------- Phase 9: ``` 嵌入式代码段 + 语法高亮 ----------
  {
    const nc = await nodeCenter();
    await page.mouse.dblclick(nc.x, nc.y);
    await sleep(700);
    await page.keyboard.press("Control+a");
    await page.keyboard.type("```js");
    await page.keyboard.press("Enter");
    await sleep(300);
    const opened = await page.evaluate(() => {
      const pre = document.querySelector(".mm-content pre.mm-code");
      return pre ? { lang: pre.getAttribute("data-lang"), inCode: true } : null;
    });
    log("[phase9] ```js+Enter 开启代码段:", opened, opened && opened.lang === "js" ? "✓" : "✗");
    await page.keyboard.type("const answer = 42; // 注释");
    await page.keyboard.press("Enter");
    await page.keyboard.type("```");
    await page.keyboard.press("Enter");
    await sleep(300);
    await page.keyboard.type("正文继续");
    await page.keyboard.press("Shift+Enter");
    await sleep(900);
    const hl = await page.evaluate(() => {
      const pre = document.querySelector(".mm-content pre.mm-code");
      return {
        exists: !!pre,
        lang: pre?.getAttribute("data-lang") ?? null,
        kw: pre?.querySelectorAll(".tok-kw").length ?? 0,
        num: pre?.querySelectorAll(".tok-num").length ?? 0,
        com: pre?.querySelectorAll(".tok-com").length ?? 0,
        text: (pre?.textContent || "").slice(0, 40),
        after: (document.querySelector(".mm-content")?.textContent || "").includes("正文继续"),
      };
    });
    log("[phase9] 静态高亮:", hl,
      hl.exists && hl.kw > 0 && hl.num > 0 && hl.com > 0 && hl.after ? "✓" : "✗");
  }

  // ---------- Phase 10: LaTeX 公式渲染（新节点，避免 Ctrl+A 残留 pre） ----------
  {
    const ok = await page.evaluate(() => {
      const b = Array.from(document.querySelectorAll("button")).find((x) =>
        /新建文本框/.test(x.getAttribute("aria-label") || ""));
      b?.click();
      return !!b;
    });
    await sleep(900);
    log("[phase10] 新建节点:", ok);
    await page.keyboard.type("质能方程 $E=mc^2$ 与分数 $\\frac{a}{b}$ 都渲染");
    await page.keyboard.press("Shift+Enter");
    await sleep(900);
    const math = await page.evaluate(() => {
      const els = document.querySelectorAll(".mm-content");
      const el = els[els.length - 1];
      return {
        mathSpans: el?.querySelectorAll(".mm-math").length ?? 0,
        katex: el?.querySelectorAll(".mm-math .katex").length ?? 0,
        raw: (el?.querySelectorAll(".mm-math").length ?? 0) > 0
          ? null
          : (el?.textContent || "").slice(0, 30),
      };
    });
    log("[phase10] LaTeX 渲染:", math, math.mathSpans >= 2 && math.katex >= 2 ? "✓" : "✗");
  }

  // ---------- Phase 11: Markdown 智能粘贴（新节点） ----------
  {
    const ok = await page.evaluate(() => {
      const b = Array.from(document.querySelectorAll("button")).find((x) =>
        /新建文本框/.test(x.getAttribute("aria-label") || ""));
      b?.click();
      return !!b;
    });
    await sleep(900);
    log("[phase11] 新建节点:", ok);
    const MD = "# 粘贴标题\n**加粗**与`行内码`\n- 列表甲\n- 列表乙\n\n```py\nprint('hi')\n```";
    await page.evaluate((t) => navigator.clipboard.writeText(t).catch(() => {}), MD);
    await sleep(300);
    await page.keyboard.press("Control+v");
    await sleep(800);
    const md = await page.evaluate(() => {
      const els = document.querySelectorAll(".mm-content");
      const el = els[els.length - 1];
      return {
        h1: !!el?.querySelector("h1"),
        strong: !!el?.querySelector("strong"),
        code: !!el?.querySelector("code"),
        li: el?.querySelectorAll("li").length ?? 0,
        pre: !!el?.querySelector("pre.mm-code[data-lang='py']"),
        hl: el?.querySelectorAll("pre.mm-code .tok-fn").length ?? 0,
      };
    });
    log("[phase11] Markdown 粘贴(编辑态):", md,
      md.h1 && md.strong && md.code && md.li >= 2 && md.pre ? "✓" : "✗");
    await page.keyboard.press("Shift+Enter");
    await sleep(900);
    const mdStatic = await page.evaluate(() => {
      const els = document.querySelectorAll(".mm-content");
      const el = els[els.length - 1];
      return {
        h1: !!el?.querySelector("h1"),
        pre: !!el?.querySelector("pre.mm-code[data-lang='py']"),
        hlFn: el?.querySelectorAll("pre.mm-code .tok-fn").length ?? 0,
        str: el?.querySelectorAll("pre.mm-code .tok-str").length ?? 0,
      };
    });
    log("[phase11] Markdown 粘贴(静态+高亮):", mdStatic,
      mdStatic.h1 && mdStatic.pre && mdStatic.hlFn > 0 && mdStatic.str > 0 ? "✓" : "✗");
  }

  // ---------- Phase 12: 5 万字粘贴 —— 整体可见（新验收） ----------
  {
    const ok = await page.evaluate(() => {
      const b = Array.from(document.querySelectorAll("button")).find((x) =>
        /新建文本框/.test(x.getAttribute("aria-label") || ""));
      b?.click();
      return !!b;
    });
    await sleep(900);
    log("[phase12] 新建节点:", ok);
    const UNIT = "五万字极限压测：节点必须自动加宽列宽并长高，让全部文字同时整体可见，不允许只露一截再用滚动条遮挡。";
    const BIG = UNIT.repeat(Math.ceil(50000 / UNIT.length)).slice(0, 50000);
    await page.evaluate((t) => navigator.clipboard.writeText(t).catch(() => {}), BIG);
    await sleep(300);
    await page.keyboard.press("Control+v");
    await sleep(3000);
    const m12e = await page.evaluate(() => {
      const els = document.querySelectorAll(".mm-node");
      const cs = document.querySelectorAll(".mm-content");
      const n = els[els.length - 1], c = cs[cs.length - 1];
      if (!n || !c) return null;
      const nr = n.getBoundingClientRect(), cr = c.getBoundingClientRect();
      return {
        textLen: (c.textContent || "").length,
        nodeBox: [Math.round(nr.width), Math.round(nr.height)],
        innerScrollY: c.scrollHeight - c.clientHeight,
        visOverflowY: Math.round(Math.max(0, cr.bottom - nr.bottom)),
        visOverflowX: Math.round(Math.max(0, cr.right - nr.right)),
        editing: c.isContentEditable,
      };
    });
    log("[phase12] 5万字(编辑态):", m12e, m12e
      ? (m12e.textLen >= 49000
          ? (m12e.visOverflowY <= 2 && m12e.visOverflowX <= 2
              ? (m12e.nodeBox[1] <= 20002
                  ? (m12e.innerScrollY <= 2 ? "✓ 整体可见" : `△ 高${m12e.nodeBox[1]}+内滚${m12e.innerScrollY}px`)
                  : "✗ 超绝对上限!")
              : "✗ 视觉溢出!")
          : "✗ 粘贴不完整!")
      : "?");
    await page.keyboard.press("Shift+Enter");
    await sleep(3500);
    const m12s = await page.evaluate(() => {
      const els = document.querySelectorAll(".mm-node");
      const cs = document.querySelectorAll(".mm-content");
      const n = els[els.length - 1], c = cs[cs.length - 1];
      if (!n || !c) return null;
      const nr = n.getBoundingClientRect(), cr = c.getBoundingClientRect();
      return {
        textLen: (c.textContent || "").length,
        nodeBox: [Math.round(nr.width), Math.round(nr.height)],
        innerScrollY: c.scrollHeight - c.clientHeight,
        visOverflowY: Math.round(Math.max(0, cr.bottom - nr.bottom)),
        visOverflowX: Math.round(Math.max(0, cr.right - nr.right)),
        editing: c.isContentEditable,
      };
    });
    log("[phase12] 5万字(静态):", m12s, m12s
      ? (m12s.textLen >= 49000
          ? (m12s.visOverflowY <= 2 && m12s.visOverflowX <= 2
              ? (m12s.nodeBox[1] <= 20002
                  ? (m12s.innerScrollY <= 2 ? "✓ 整体可见" : `△ 高${m12s.nodeBox[1]}+内滚${m12s.innerScrollY}px`)
                  : "✗ 超绝对上限!")
              : "✗ 视觉溢出!")
          : "✗ 内容丢失!")
      : "?");
  }

  // ---------- Phase 13: 12 万字 —— 封顶 20000 + 框内滚动兜底 ----------
  {
    const ok = await page.evaluate(() => {
      const b = Array.from(document.querySelectorAll("button")).find((x) =>
        /新建文本框/.test(x.getAttribute("aria-label") || ""));
      b?.click();
      return !!b;
    });
    await sleep(900);
    log("[phase13] 新建节点:", ok);
    const UNIT = "十二万字超级压测：超出绝对上限的长文由框内右侧滚动条兜底，节点封顶两万像素，文字不越过边框半步。";
    const HUGE = UNIT.repeat(Math.ceil(120000 / UNIT.length)).slice(0, 120000);
    await page.evaluate((t) => navigator.clipboard.writeText(t).catch(() => {}), HUGE);
    await sleep(300);
    await page.keyboard.press("Control+v");
    await sleep(3500);
    await page.keyboard.press("Shift+Enter");
    await sleep(4000);
    const m13 = await page.evaluate(() => {
      const els = document.querySelectorAll(".mm-node");
      const cs = document.querySelectorAll(".mm-content");
      const n = els[els.length - 1], c = cs[cs.length - 1];
      if (!n || !c) return null;
      const nr = n.getBoundingClientRect(), cr = c.getBoundingClientRect();
      c.scrollTop = 2000;
      const scrolled = c.scrollTop;
      c.scrollTop = 0;
      return {
        textLen: (c.textContent || "").length,
        nodeBox: [Math.round(nr.width), Math.round(nr.height)],
        innerScrollY: c.scrollHeight - c.clientHeight,
        scrollSettable: scrolled > 1900,
        visOverflowY: Math.round(Math.max(0, cr.bottom - nr.bottom)),
        visOverflowX: Math.round(Math.max(0, cr.right - nr.right)),
        editing: c.isContentEditable,
      };
    });
    log("[phase13] 12万字(静态):", m13, m13
      ? (m13.textLen >= 119000
          ? (m13.visOverflowY <= 2 && m13.visOverflowX <= 2
              ? (m13.nodeBox[1] <= 20002
                  ? (m13.innerScrollY > 100
                      ? (m13.scrollSettable ? "✓ 封顶+框内滚动兜底" : "✗ 不可滚动!")
                      : `△ 高${m13.nodeBox[1]} 全可见(未触发兜底)`)
                  : "✗ 超绝对上限!")
              : "✗ 视觉溢出!")
          : "✗ 粘贴不完整!")
      : "?");
  }

  // ---------- Phase 14: 精简工具条 —— 次要按钮收纳弹层 ----------
  {
    const slim = await page.evaluate(() => {
      const tb = document.querySelector(".mm-toolbar");
      if (!tb) return null;
      const visible = Array.from(tb.children).filter((c) => c.tagName === "BUTTON" || c.className === "seg").length;
      const primary = ["新建文本框", "更多工具"].every((label) =>
        Array.from(tb.querySelectorAll(":scope > button[aria-label]")).some((b) =>
          (b.getAttribute("aria-label") || "").includes(label)));
      const secondaryHidden = !tb.querySelector(":scope > button[aria-label='gridToggle']");
      return { visible, primary, secondaryHidden };
    });
    log("[phase14] 工具条收纳:", slim);
    const moreOpened = await page.evaluate(() => {
      const b = Array.from(document.querySelectorAll(".mm-toolbar button")).find((x) =>
        x.getAttribute("aria-label") === "更多工具");
      b?.click();
      return !!b;
    });
    await sleep(400);
    const pop = await page.evaluate(() => {
      const p = document.querySelector(".tb-more-pop");
      return p ? {
        n: p.querySelectorAll("button").length,
        labels: Array.from(p.querySelectorAll("button[aria-label]")).map((b) => b.getAttribute("aria-label")),
      } : null;
    });
    log("[phase14] ⋯弹层:", moreOpened, pop, pop && pop.n >= 8 ? "✓" : "✗");
    // 点外收起
    await page.mouse.click(700, 500);
    await sleep(350);
    const popClosed = await page.evaluate(() => !document.querySelector(".tb-more-pop"));
    log("[phase14] 点外收起:", popClosed, popClosed ? "✓" : "✗");
  }

  // ---------- Phase 15: 悬浮径向菜单 ----------
  {
    const before = await page.evaluate(() => document.querySelectorAll(".mm-node").length);
    const opened = await page.evaluate(() => {
      const f = document.querySelector(".radial-fab .fab-main");
      f?.click();
      return !!f;
    });
    await sleep(500);
    const fab = await page.evaluate(() => {
      const w = document.querySelector(".radial-fab");
      return {
        open: !!w?.classList.contains("open"),
        items: w?.querySelectorAll(".radial-item").length ?? 0,
        labels: Array.from(w?.querySelectorAll(".radial-item") ?? []).map((b) => b.getAttribute("aria-label")),
      };
    });
    log("[phase15] 径向菜单展开:", opened, fab, fab.open && fab.items >= 6 ? "✓" : "✗");
    // 点第 1 项（新建文本框）→ 收起 + 节点 +1
    await page.evaluate(() => document.querySelector(".radial-fab .radial-item")?.click());
    await sleep(900);
    const after = await page.evaluate(() => ({
      nodes: document.querySelectorAll(".mm-node").length,
      closed: !document.querySelector(".radial-fab")?.classList.contains("open"),
    }));
    log("[phase15] 径向项执行:", after, after.nodes === before + 1 && after.closed ? "✓" : "✗");
  }

  // ---------- Phase 16: 沉浸模式 ----------
  {
    const sidebarBefore = await page.evaluate(() => !!document.querySelector(".sidebar"));
    const on = await page.evaluate(() => {
      const b = Array.from(document.querySelectorAll(".mm-toolbar button")).find((x) =>
        (x.getAttribute("aria-label") || "").includes("沉浸"));
      b?.click();
      return !!b;
    });
    await sleep(700);
    const hidden = await page.evaluate(() => ({
      toolbarGone: !document.querySelector(".mm-toolbar"),
      statusGone: !document.querySelector(".mm-status"),
      mapNameGone: !document.querySelector(".mm-map-name"),
      minimapGone: !document.querySelector(".minimap"),
      sidebarGone: !document.querySelector(".sidebar") || getComputedStyle(document.querySelector(".sidebar")).display === "none",
      titlebarGone: !document.querySelector(".titlebar"),
      exitPill: !!document.querySelector(".immersive-exit"),
      canvasAlive: !!document.querySelector(".mm-canvas"),
      fabAlive: !!document.querySelector(".radial-fab"),
    }));
    const allHidden = hidden.toolbarGone && hidden.statusGone && hidden.mapNameGone && hidden.minimapGone
      && hidden.sidebarGone && hidden.titlebarGone && hidden.exitPill && hidden.canvasAlive && hidden.fabAlive;
    log("[phase16] 沉浸模式:", on, hidden, allHidden ? "✓ 全部隐藏+保留入口" : "✗");
    // Ctrl+Shift+H 退出 → UI 恢复（侧栏与进入前基线一致 —— 默认折叠时不渲染）
    await page.keyboard.press("Control+Shift+h");
    await sleep(700);
    const restored = await page.evaluate(() => ({
      toolbar: !!document.querySelector(".mm-toolbar"),
      sidebar: !!document.querySelector(".sidebar"),
      exitPillGone: !document.querySelector(".immersive-exit"),
    }));
    log("[phase16] Ctrl+Shift+H 退出:", restored,
      restored.toolbar && restored.exitPillGone && restored.sidebar === sidebarBefore ? "✓" : "✗");
  }

  // ---------- Phase 17: 宽度/高度回缩 + 字号增长 + 检查器常开（回归守卫） ----------
  {
    await page.evaluate(() => { window.__phase = "p17"; });
    // 清场：点空白取消选中（不动历史节点，测量一律取最后节点）
    await page.mouse.click(120, 250);
    await sleep(300);
    // 新建节点 → 粘贴 500 字 → 提交
    await page.evaluate(() => {
      Array.from(document.querySelectorAll("button")).find((x) =>
        (x.getAttribute("aria-label") || "").includes("新建文本框"))?.click();
    });
    await sleep(800);
    const MID = "中篇粘贴压力测试：这段文字用来验证五百字级别的粘贴后节点是否按内容自适应尺寸。".repeat(12);
    await page.evaluate((t) => navigator.clipboard.writeText(t).catch(() => {}), MID);
    await sleep(300);
    await page.keyboard.press("Control+v");
    await sleep(1300);
    await page.keyboard.press("Shift+Enter");
    await sleep(1000);
    const m17a = await page.evaluate(() => {
      const n = Array.from(document.querySelectorAll(".mm-node")).pop();
      const c = n?.querySelector(".mm-content");
      if (!n || !c) return null;
      const r = n.getBoundingClientRect();
      return { w: Math.round(r.width), h: Math.round(r.height), textH: c.scrollHeight };
    });
    log("[phase17] 500字粘贴:", m17a, m17a && m17a.w >= 280 && m17a.textH > 200 ? "✓" : "✗");

    // 缩短为 1 字 → 宽高都应回缩贴合
    await page.keyboard.press("Enter");
    await sleep(600);
    await page.keyboard.press("Control+a");
    await sleep(150);
    await page.keyboard.type("一", { delay: 0 });
    await sleep(400);
    await page.keyboard.press("Shift+Enter");
    await sleep(1000);
    const m17b = await page.evaluate(() => {
      const n = Array.from(document.querySelectorAll(".mm-node")).pop();
      const c = n?.querySelector(".mm-content");
      if (!n || !c) return null;
      const r = n.getBoundingClientRect();
      return { w: Math.round(r.width), h: Math.round(r.height), textH: c.scrollHeight, len: (c.textContent || "").length };
    });
    // 修复前宽度卡死 310；修复后应回缩到 ~120（MIN_W）
    log("[phase17] 缩为1字:", m17b,
      m17b && m17b.len === 1 && m17b.w <= 160 && m17b.h <= 120 ? "✓ 宽高回缩" : "✗ 宽度/高度未回缩");

    // 点击节点：选中保持 + 检查器常开（修复前：打开面板的同一击会把它关掉并清空选中）
    const nc17 = await page.evaluate(() => {
      const el = Array.from(document.querySelectorAll(".mm-node")).pop();
      const r = el.getBoundingClientRect();
      return { x: r.left + r.width / 2, y: r.top + 30 };
    });
    await page.mouse.click(nc17.x, nc17.y);
    await sleep(800);
    const m17c = await page.evaluate(() => ({
      sel: (document.querySelector(".mm-status")?.textContent || "").match(/选中 (\d+)/)?.[1] ?? "?",
      insp: !!document.querySelector(".inspector"),
    }));
    log("[phase17] 点击节点后:", m17c, m17c.sel === "1" && m17c.insp ? "✓ 检查器常开" : "✗ 选中被清/面板被关");

    // 重新粘贴长文 → 字号+3 → 高度应随字号增长（若检查器被关则步进失败）
    await page.keyboard.press("Enter");
    await sleep(600);
    await page.evaluate((t) => navigator.clipboard.writeText(t).catch(() => {}), MID);
    await sleep(300);
    await page.keyboard.press("Control+v");
    await sleep(1300);
    await page.keyboard.press("Shift+Enter");
    await sleep(1000);
    const h0 = await page.evaluate(() => {
      const els = document.querySelectorAll(".mm-node");
      return Math.round(els[els.length - 1].getBoundingClientRect().height);
    });
    for (let i = 0; i < 3; i++) {
      await page.evaluate(() => {
        const insp = document.querySelector(".inspector");
        (insp?.querySelectorAll(".stepper button")[1] ?? insp?.querySelector(".stepper button"))?.click();
      });
      await sleep(500);
    }
    const m17d = await page.evaluate(() => {
      const n = Array.from(document.querySelectorAll(".mm-node")).pop();
      const c = n?.querySelector(".mm-content");
      const r = n.getBoundingClientRect();
      return { h: Math.round(r.height), font: c ? getComputedStyle(c).fontSize : null, textH: c?.scrollHeight ?? 0 };
    });
    log("[phase17] 字号15→18:", { h0, ...m17d },
      m17d.font === "18px" && m17d.h > h0 + 40 ? "✓ 高度随字号增长" : "✗ 字号/高度未联动");
  }
} catch (e) {
  log("!! 异常:", e.message);
} finally {
  log("=== page errors ===");
  log(pageErrors.slice(0, 8).join(" | ") || "(无)");
  try { await page.screenshot({ path: "shots/final.png", timeout: 5000 }); } catch {}
  await browser.close();
}
