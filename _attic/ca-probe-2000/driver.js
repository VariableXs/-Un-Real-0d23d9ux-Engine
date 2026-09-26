/* CA-PROBE-2000 驱动器：Playwright 逐类执行 + 异常捕获 + 视口排布 */
async (page) => {
  const errs = [];
  page.on("console", (m) => { if (m.type() === "error") errs.push(m.text().slice(0, 160)); });
  page.on("pageerror", (e) => errs.push("PAGEERROR:" + String(e).slice(0, 160)));
  page.on("filechooser", (fc) => { fc.setFiles([]).catch(() => {}); });
  await page.setViewportSize({ width: 1680, height: 920 });
  await page.goto("http://127.0.0.1:8977/?v=probe", { waitUntil: "load", timeout: 20000 }).catch(() => {});
  await page.waitForTimeout(2200);
  await page.addScriptTag({ path: "D:/2/14/-Un-Real-0d23d9ux-Engine-main/_attic/ca-probe-2000/checks.js" });
  const order = ["structure", "funcs", "buttons", "ifaceOps", "keyOps", "zoomOps", "searchOps", "themeOps", "styleOps", "randomOps", "viewsLevels", "perNode", "irOps", "projSwitch", "refs"];
  const out = {};
  for (const c of order) {
    try {
      out[c] = await page.evaluate((c) => window.__CA_PROBE.P[c](), c);
    } catch (e) {
      out[c] = { n: 0, p: 0, f: ["DRIVER:" + String(e).slice(0, 120)] };
    }
  }
  out.viewport = [];
  for (const [w, h] of [[2560, 1400], [1680, 920], [1280, 800], [900, 700], [520, 700]]) {
    await page.setViewportSize({ width: w, height: h });
    await page.waitForTimeout(350);
    out.viewport.push(await page.evaluate(([w, h]) => window.__CA_PROBE.P.layout(w, h), [w, h]));
  }
  await page.setViewportSize({ width: 1680, height: 920 });
  const R = await page.evaluate(() => {
    const r = window.__CA_PROBE.R;
    return { n: r.n, p: r.p, fcount: r.f.length + Object.keys(r.cat).reduce((a, k) => a + r.cat[k].f.length, 0), f: Object.keys(r.cat).flatMap((k) => r.cat[k].f.slice(0, 40).map((x) => k + " :: " + x)), errs: r.errs.slice(0, 60) };
  });
  return { R, viewport: out.viewport, consoleErrors: errs.slice(0, 40), cats: Object.fromEntries(order.map((c) => [c, out[c]])) };
}
