/**
 * VarixAutoPilot · 本地服务
 *
 * 一个零依赖的 Node HTTP 服务，给可视化面板提供：
 *   GET  /api/snapshot        当前快照（对话列表 / 模型 / 工作目录 / 发送态）
 *   GET  /api/settings        提示词模板与设置
 *   POST /api/settings        保存提示词模板与设置
 *   POST /api/preview         预览模板渲染结果（不填不发送）
 *   POST /api/send            执行填入（+可选发送）
 *   GET  /api/log             读日志
 *
 * 设计取舍：
 * - **单文件零依赖**：只用 node: 内置模块。不装 express，不引包，
 *   避免"为跑一个面板先搞一串依赖"；
 * - **常驻浏览器连接**：CDP 连一次就留着，轮询复用。
 *   每次重连都开一条新连接会让端口上的会话句柄堆积；
 * - **异常零静默**：每个失败都带可操作指引，且日志里带时间戳与分级。
 */

import { createServer } from 'node:http';
import { appendFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { chromium } from 'playwright-core';

import { CDP_ENDPOINT, probe } from './collect.mjs';
import { runFlow } from './engine.mjs';
import { renderTemplate } from './template.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const PORT = Number(process.env.AUTOPILOT_PORT || 8768);
const SETTINGS_PATH = join(HERE, 'settings.json');
const LOG_PATH = join(HERE, 'autopilot.log');
const SHOT_DIR = join(HERE, 'shots');

/** 默认提示词模板。
 *
 * 占位符用 {{键}} 语法，渲染时逐个替换；**未提供的键保留原样**
 * （不替换成空串）—— 让缺失一眼可见，比悄悄变空更安全。
 *
 * 【系统内置变量（自动注入，不需填）】
 *   {{时间}} = 发送时刻
 *
 * 【你填的变量（面板左侧）】默认给了两个，可自行增删：
 *   {{任务}} {{上下文}}
 */
const DEFAULT_TEMPLATE = `# {{任务}} · 下一轮施工指令

> 由 VarixAutoPilot 于 {{时间}} 填入。
> 改写这段话：直接在输入框里改完再发——本工具只负责送到，不替你决定。

## 目标

在 **Agent 模式** 下施工 **{{任务}}**。

## 必读上下文

{{上下文}}

## 纪律（凌驾任务目标）

1. 源码只增不减不移动——推前必数文件数与行数，数不对不推
2. 过程产物全归 \`_attic/\`，原位清零，不入库
3. 只推纯功能代码——测试脚本与截图一律不推
4. 只 add 显式路径，绝不 \`git add .\`
5. 异常零静默——失败必带「发生了什么/为什么/下一步怎么办」
6. 完成度铁律——不半途交付、不留占位符、不写 TODO
7. 修测试不迁就实现，修实现不迁就测试；但要先归因是哪一边错

## 现在开始

读上面点名的文件，按锚点原文逐条落实判据。做完交活、回写任务板、归档过程产物。
`;

/** 默认设置。 */
const DEFAULT_SETTINGS = {
  template: DEFAULT_TEMPLATE,
  // 变量表：渲染时按这些键取值。空字符串表示「本次没有该信息」，
  // 面板上会显示成占位符提醒，不会静默产出空段落。
  vars: {
    任务: '',
    上下文: '',
  },
  // 行为开关
  openNew: true,      // 发送前先点「新建任务」
  dryRun: true,       // ★ 默认只填不发★：真发要手动关掉
  timeoutMs: 8000,
  autoRefreshMs: 1500, // 面板轮询间隔
};

let settings = structuredClone(DEFAULT_SETTINGS);
let browser = null;
let browserErr = '';
const logBuf = [];

/** 记日志（同时写文件与内存环形缓冲）。 */
function log(level, msg) {
  const line = `[${new Date().toISOString().slice(11, 19)}] [${level}] ${msg}`;
  console.log(line);
  logBuf.push(line);
  if (logBuf.length > 500) logBuf.splice(0, logBuf.length - 500);
  try {
    appendFileSync(LOG_PATH, line + '\n', 'utf8');
  } catch { /* 日志写失败不掩盖主流程 */ }
}

// ---------------------------------------------------------------------------
// 浏览器连接（常驻）
// ---------------------------------------------------------------------------

/** 取得浏览器连接；不可用时如实记错并返回 null（不崩服务）。 */
async function ensureBrowser() {
  if (browser) return browser;
  try {
    browser = await chromium.connectOverCDP(CDP_ENDPOINT);
    log('OK', 'CDP 已连接 ' + CDP_ENDPOINT);
    return browser;
  } catch (e) {
    browserErr = String(e && e.message ? e.message : e);
    log('ERR', `CDP 连接失败：${browserErr}`);
    return null;
  }
}

/** 断开重连（连接僵死时用）。 */
async function reconnect() {
  try {
    if (browser) await browser.close();
  } catch { /* 关不关都无所谓 */ }
  browser = null;
  return ensureBrowser();
}

// ---------------------------------------------------------------------------
// 提示词模板渲染
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// HTTP
// ---------------------------------------------------------------------------

const MIME = { '.html': 'text/html; charset=utf-8', '.mjs': 'text/javascript', '.json': 'application/json; charset=utf-8' };

function sendJSON(res, code, obj) {
  const body = JSON.stringify(obj);
  res.writeHead(code, { 'Content-Type': MIME['.json'], 'Content-Length': Buffer.byteLength(body) });
  res.end(body);
}

function readBody(req) {
  return new Promise((resolve, reject) => {
    let b = '';
    req.on('data', (c) => {
      b += c;
      if (b.length > 4 * 1024 * 1024) reject(new Error('请求体过大（>4MB）'));
    });
    req.on('end', () => {
      try {
        resolve(b ? JSON.parse(b) : {});
      } catch (e) {
        reject(new Error('请求体不是合法 JSON：' + e.message));
      }
    });
    req.on('error', reject);
  });
}

const server = createServer(async (req, res) => {
  const url = new URL(req.url, 'http://127.0.0.1');
  const p = url.pathname;

  try {
    // favicon：浏览器会自动请求 /favicon.ico，不给就是 404。
    // 返 204 而不是 404 ——面板控制台里那条"资源加载失败"是噪音，
    // 而噪音会掩盖真的错误。异常零静默不等于让假异常混进来。
    if (req.method === 'GET' && p === '/favicon.ico') {
      res.writeHead(204);
      return res.end();
    }

    if (req.method === 'GET' && p === '/') {
      const f = join(HERE, 'panel.html');
      if (!existsSync(f)) return sendJSON(res, 500, { ok: false, err: 'panel.html 不存在' });
      const body = readFileSync(f);
      res.writeHead(200, { 'Content-Type': MIME['.html'], 'Content-Length': body.length });
      return res.end(body);
    }

    if (req.method === 'GET' && p === '/panel.js') {
      const f = join(HERE, 'panel.js');
      const body = readFileSync(f);
      res.writeHead(200, { 'Content-Type': MIME['.mjs'], 'Content-Length': body.length });
      return res.end(body);
    }

    if (req.method === 'GET' && p === '/api/snapshot') {
      const b = await ensureBrowser();
      if (!b) return sendJSON(res, 200, { ok: false, err: browserErr, hint: '确认 WorkBuddy 带 --remote-debugging-port=9222 启动' });
      try {
        const s = await probe(b);
        return sendJSON(res, 200, { ok: true, snapshot: s });
      } catch (e) {
        // 连接僵死时自动重连一次再试（一次性，不循环）
        log('WARN', '快照失败，尝试重连：' + e.message);
        const b2 = await reconnect();
        if (!b2) return sendJSON(res, 200, { ok: false, err: browserErr });
        try {
          const s = await probe(b2);
          return sendJSON(res, 200, { ok: true, snapshot: s });
        } catch (e2) {
          return sendJSON(res, 200, { ok: false, err: String(e2.message || e2) });
        }
      }
    }

    if (req.method === 'GET' && p === '/api/settings') {
      return sendJSON(res, 200, { ok: true, settings });
    }

    if (req.method === 'POST' && p === '/api/settings') {
      const body = await readBody(req);
      if (typeof body.template === 'string') settings.template = body.template;
      if (body.vars && typeof body.vars === 'object') settings.vars = { ...settings.vars, ...body.vars };
      for (const k of ['openNew', 'dryRun']) {
        if (typeof body[k] === 'boolean') settings[k] = body[k];
      }
      for (const k of ['timeoutMs', 'autoRefreshMs']) {
        const n = Number(body[k]);
        if (Number.isFinite(n) && n > 0) settings[k] = n;
      }
      writeFileSync(SETTINGS_PATH, JSON.stringify(settings, null, 2), 'utf8');
      log('OK', '设置已保存到 settings.json');
      return sendJSON(res, 200, { ok: true, settings });
    }

    if (req.method === 'POST' && p === '/api/preview') {
      const body = await readBody(req);
      const tpl = body.template !== undefined ? body.template : settings.template;
      const vars = body.vars || settings.vars || {};
      const r = renderTemplate(tpl, vars);
      return sendJSON(res, 200, { ok: true, ...r, chars: r.text.length });
    }

    if (req.method === 'POST' && p === '/api/send') {
      const body = await readBody(req);
      const tpl = body.template !== undefined ? body.template : settings.template;
      const vars = body.vars || settings.vars || {};
      const r = renderTemplate(tpl, vars);
      if (r.missing.length) {
        return sendJSON(res, 200, {
          ok: false,
          err: `有 ${r.missing.length} 个占位符没填：${r.missing.map((k) => '{{' + k + '}}').join('、')}`,
          hint: '在左侧「变量」区填好，或把模板里的占位符删掉',
          missing: r.missing,
        });
      }
      const dryRun = body.dryRun !== undefined ? !!body.dryRun : settings.dryRun;
      const openNew = body.openNew !== undefined ? !!body.openNew : settings.openNew;
      const b = await ensureBrowser();
      if (!b) return sendJSON(res, 200, { ok: false, err: browserErr });

      log('INFO', `开始${dryRun ? '干跑（只填不发）' : '真发'}，${r.text.length} 字符，openNew=${openNew}`);
      try {
        const out = await runFlow(b, { text: r.text, dryRun, openNew, timeoutMs: settings.timeoutMs });
        log(out.ok ? 'OK' : 'ERR', `执行结果 ok=${out.ok}${out.evidence ? ' · ' + out.evidence : ''}${out.dryRun ? ' · dry-run' : ''}`);
        return sendJSON(res, 200, { ok: true, ...out, chars: r.text.length });
      } catch (e) {
        const code = e && e.code === 'BUSY' ? 409 : 500;
        const msg = String(e && e.message ? e.message : e);
        log('ERR', '执行失败：' + msg);
        // ★ hint 必须与实际行为一致 ★
        // 早先忙时返回的 hint写「内容已留在输入框里，不会丢」——
        // 那时的实现确实是"先填后检"，但改成"忙时提前退出"之后这句话
        // 就变成**假的**。提示与行为不符比没有提示更坏：人会照着它
        // 去找那段根本不存在的内容。所以这里按当前实现给准确说法。
        return sendJSON(res, 200, {
          ok: false, busy: code === 409, err: msg,
          hint: code === 409
            ? '本轮未写入任何内容（输入框保持原样）。等对方空闲后再点发送。'
            : '看 shots/ 目录下的截图确认界面状态；若是选择器失效，跑 node probe.mjs --list 核对',
        });
      }
    }

    if (req.method === 'GET' && p === '/api/log') {
      return sendJSON(res, 200, { ok: true, lines: logBuf.slice(-120) });
    }

    return sendJSON(res, 404, { ok: false, err: '无此端点：' + p });
  } catch (e) {
    const msg = String(e && e.message ? e.message : e);
    log('ERR', '请求处理异常：' + msg);
    return sendJSON(res, 500, { ok: false, err: msg });
  }
});

// ---------------------------------------------------------------------------
// 启动
// ---------------------------------------------------------------------------

function loadSettings() {
  if (!existsSync(SETTINGS_PATH)) {
    writeFileSync(SETTINGS_PATH, JSON.stringify(DEFAULT_SETTINGS, null, 2), 'utf8');
    log('OK', '已生成默认 settings.json');
    return;
  }
  try {
    const d = JSON.parse(readFileSync(SETTINGS_PATH, 'utf8'));
    settings = { ...structuredClone(DEFAULT_SETTINGS), ...d };
    settings.vars = { ...DEFAULT_SETTINGS.vars, ...(d.vars || {}) };
    log('OK', '已载入 settings.json');
  } catch (e) {
    log('ERR', `settings.json 解析失败，用默认值：${e.message}`);
    settings = structuredClone(DEFAULT_SETTINGS);
  }
}

loadSettings();
mkdirSync(SHOT_DIR, { recursive: true });

server.listen(PORT, '127.0.0.1', () => {
  console.log('');
  console.log('  VarixAutoPilot已启动');
  console.log(`  面板http://127.0.0.1:${PORT}/`);
  console.log(`  CDP      ${CDP_ENDPOINT}`);
  console.log(`  截图     ${SHOT_DIR}`);
  console.log('');
  log('OK', `服务启动于 127.0.0.1:${PORT}`);
  ensureBrowser();
});

process.on('unhandledRejection', (e) => log('ERR', '未捕获的Promise 拒绝：' + (e && e.stack ? e.stack : e)));
process.on('SIGINT', async () => {
  log('INFO', '收到 SIGINT，断开 CDP 并退出');
  try { if (browser) await browser.close(); } catch { /* 忽略 */ }
  process.exit(0);
});
