// VarixAutoPilot · 前端逻辑
//
// 只做四件事：轮询快照渲染、编辑模板、触发执行、把后端来的三要素错误如实呈现。
//
// ★ 边界原则 ★
// 前端**不自己判断"成功了吗"**——那必须由后端用客观证据
// （编辑器字符数 / 消息是否入流）判定。前端只负责展示后端的结论。
// 前端若自己猜，就是给自己造幻觉。
//
// ★ 双模式 ★
// Tauri 环境：走 invoke 调 Rust 后端。
// 浏览器环境（Tauri 全局不存在）：走内置 mock，用于**独立视觉预览与回归**——
//   没有 mock 的话，每次改样式都得先 cargo build 才能看效果，太慢。
// mock 数据是**真实结构**（7 个对话 / 模型 / 目录 / 发送态），
// 所以它既能验布局，也能在没有 WorkBuddy 时演示。

const $ = (id) => document.getElementById(id);

// ══════════════════════════════════════════════════════════════
// ★ 内容来源指示器（常驻显示，不是出错才显示）★
//
// 要解决的问题：早先「勾了自由文本但框是空的」只在出错时
// 于 hint 区显示一行，极易被忽略 ⇒ 表现为「点了没反应」。
//
// 这里改成**常驻**：勾了什么、当前会不会入不了队、勾了干跑会不会真发，
// 全部直接写在界面上。不出错也显示。
// ══════════════════════════════════════════════════════════════
window.__VAP_SRC_NOTE__ = function updateSrcNote() {
  const box = document.getElementById('src-note');
  if (box) {
    const free = document.getElementById('o-free');
    const ft = document.getElementById('free-text');
    if (free && free.checked) {
      const n = (ft && ft.value || '').trim().length;
      box.innerHTML = n > 0
        ? '<span class="ok">当前内容来源：自由文本（' + n + ' 字符）· 点「加入待发」会入队</span>'
        : '<span class="bad">★ 已勾「用这段自由文本」但框是空的 —— 现在点「加入待发」会失败。'
          + '写点内容，或取消那个勾选。</span>';
    } else {
      const rounds = document.getElementById('lp-first') || document.getElementById('lp-text-1');
      const tpl = document.getElementById('tpl');
      const n = tpl ? tpl.value.length : 0;
      box.innerHTML = '<span class="ok">当前内容来源：模板（' + n + ' 字符）· 点「加入待发」会入队</span>';
    }
  }
  // 干跑醒目条
  const dry = document.getElementById('o-dry');
  const note = document.getElementById('dry-note');
  if (dry && note) {
    note.hidden = !dry.checked;
  }
  return true;
};

// ── 后端调用：Tauri 全局 or mock ────────────────────────────
// ★ 为什么用 window.__TAURI__ 而不是 import('/__TAURI__/core.js')★
// Tauri 2 默认不把 API 打进全局，文档给的是动态 import
// `/__TAURI__/core.js`。但那条路实测失败：
//   「取不到 Tauri invoke 模块：Failed to fetch dynamically imported
//     module: http://tauri.localhost/__TAURI__/core.js」
// 原因：CSP 的 `script-src 'self'` 拦住了这个非相对路径的模块请求。
//
// 两条可行路线，本项目选第一条：
//   A. tauri.conf.json 设 `app.withGlobalTauri: true`
//      → API 注入 window.__TAURI__，**无需任何 import**，也不受 CSP 约束。
//      代价：多注入几 KB（可忽略）。★ 这是本项目采用的方式★
//   B. 放开 CSP 再动态 import
//      → 要写 `script-src 'self' http://tauri.localhost`，
//      等于把 CSP 的防护面开大，不划算。
//
// ★ 教训★：报"Failed to fetch dynamically imported module"时，
//   先怀疑 CSP，别急着换写法。用全局注入是更省事也更安全的路。
const IS_TAURI =
  typeof window.__TAURI_INTERNALS__ !== 'undefined' ||
  typeof window.__TAURI__ !== 'undefined';

const MOCK = {
  version: '5.6.2',
  conversation_title: '自动领任务直至验收并归档',
  current_model: 'Space-Bunny',
  sending: false,
  send_label: '',
  editor_chars: 0,
  editor_visible: true,
  session_count: 276,
  convs: [
    { index: 0, conv_id: 'a09df76e', title: '# 下一轮施工提示词（自动生成，勿手改）', rel_time: '4分钟前', model: '', selected: false, cwd: 'C:\\Users\\varia\\WorkBuddy\\2026-10-04-17-48-14', cwd_confidence: '精确（conversation-id 命中）' },
    { index: 1, conv_id: '6a70cd4d', title: '破解软件咨询', rel_time: '7小时前', model: '', selected: false, cwd: 'C:\\Users\\varia\\WorkBuddy\\2026-10-03-15-11-44', cwd_confidence: '精确（conversation-id 命中）' },
    { index: 2, conv_id: '44dc2b80', title: '重构原 MD 并输出改进版文件', rel_time: '14小时前', model: '', selected: false, cwd: 'C:\\Users\\varia\\WorkBuddy\\2026-10-03-22-15-25', cwd_confidence: '精确（conversation-id 命中）' },
    { index: 3, conv_id: 'e042f2d0', title: '自动领任务直至验收并归档', rel_time: '', model: 'Space-Bunny', selected: true, cwd: 'D:\\2\\14\\-Un-Real-0d23d9ux-Engine-main', cwd_confidence: '精确（conversation-id 命中）' },
    { index: 4, conv_id: '3b7c9a12', title: '检查 varix 库测试结果', rel_time: '14分钟前', model: '', selected: false, cwd: 'D:\\2\\14\\-Un-Real-0d23d9ux-Engine-main', cwd_confidence: '精确（conversation-id 命中）' },
    { index: 5, conv_id: 'a1b2c3d4', title: '核查后台命令输出', rel_time: '', model: '', selected: false, cwd: 'D:\\2\\14\\-Un-Real-0d23d9ux-Engine-main\\VarixTaskOps', cwd_confidence: '精确（conversation-id 命中）' },
    { index: 6, conv_id: 'ffeeddcc', title: '自动化任务领取与验收', rel_time: '1小时前', model: '', selected: false, cwd: '', cwd_confidence: '未查到该 session' },
  ],
  cwd_histogram: [],
  queue: [],
};

/** 前端本地模板渲染（仅 mock 模式用；真实模式由 Rust 渲染）。 */
function localRender(tpl, vars) {
  const now = new Date();
  const p = (n) => String(n).padStart(2, '0');
  const sys = { 时间: `${now.getFullYear()}-${p(now.getMonth() + 1)}-${p(now.getDate())} ${p(now.getHours())}:${p(now.getMinutes())}:${p(now.getSeconds())}` };
  const missing = [];
  const text = String(tpl).replace(/\{\{\s*([^{}]+?)\s*\}\}/g, (_, k) => {
    const key = k.trim();
    if (key in sys) return sys[key];
    const v = vars[key];
    if (v === undefined || v === null || v === '') {
      if (!missing.includes(key)) missing.push(key);
      return `{{${key}}}`;
    }
    return String(v);
  });
  return { text, missing, chars: [...text].length };
}
const api = IS_TAURI
  ? {
      probe: () => invoke('probe'),
      preview: (tpl, vars) => invoke('preview', { tpl, vars }),
      send: (tpl, vars, dryRun, openNew) =>
        invoke('send', { tpl, vars, dryRun, openNew }),
    }
  : {
      probe: async () => MOCK,
      preview: async (tpl, vars) => localRender(tpl, vars),
      // ── 队列 / 循环 / 内容来源（mock 也要能用，否则改样式时看不到新面板）──
      enqueue: async (text, convId, round) => {
        const r = { text: text, chars: text.length, missing: [] };
        MOCK.queue.push({
          id: MOCK.queue.length + 1,
          preview: text.slice(0, 60) + (text.length > 60 ? '…' : ''),
          text, conv_id: convId || '', round: round || 0,
          state: 'pending', evidence: '', err: '', enqueued_at: '',
        });
        return { items: MOCK.queue, id: MOCK.queue.length };
      },
      queueView: async () => ({ items: MOCK.queue, id: 0 }),
      queueCancel: async (id) => {
        const it = MOCK.queue.find((q) => q.id === id && q.state === 'pending');
        if (it) it.state = 'canceled';
        else {
          const e = new Error('这条已在发送中');
          e.what = '这条取消不了'; e.why = '内容已写入输入框'; e.next = '等这轮落地再取消下一条';
        }
        return { items: MOCK.queue, id };
      },
      queueClear: async () => {
        MOCK.queue = MOCK.queue.filter((q) => q.state === 'pending' || q.state === 'sending');
        return { items: MOCK.queue, id: 0 };
      },
      loopStart: async (texts, convId, rounds) => {
        // 模拟：把首轮与后续轮灌进队列
        MOCK.queue = [];
        for (let i = 0; i < Math.min(rounds === 0 ? 3 : rounds, 6); i++) {
          const t = texts[Math.min(i, texts.length - 1)];
          MOCK.queue.push({
            id: i + 1, preview: t.slice(0, 60), text: t, conv_id: convId || '',
            round: i + 1, state: 'pending', evidence: '', err: '', enqueued_at: '',
          });
        }
        return { items: MOCK.queue, id: 0 };
      },
      loopStop: async () => ({ items: MOCK.queue, id: 0 }),
      idleCheck: async () => ({ idle: true, reason: 'mock', by_btn: true, by_stop_btn: true, by_anim: true }),
      readTextFile: async (path) => {
        const e = new Error('mock 不读文件');
        e.what = '这是浏览器预览模式'; e.why = '读文件要后端'; e.next = '启动 VarixAutoPilot.exe 才有';
        throw e;
      },
      probeSkills: async () => {
        // ★ mock 的技能名是「示例」，界面必须显示为 mock，不能冒充真实清单 ★
        return { items: ['/示例技能A  这是演示数据', '/示例技能B  非真实清单'], cleaned: true };
      },
      send: async (tpl, vars, dryRun) => {
        const r = localRender(tpl, vars);
        if (r.missing.length) {
          return {
            ok: false, dry_run: dryRun, chars: 0, evidence: '',
            err: {
              what: `有 ${r.missing.length} 个占位符没填，拒绝发送`,
              why: r.missing.map((k) => `{{${k}}}`).join('、'),
              next: '在「变量」区填好，或把模板里那些占位符删掉',
            },
          };
        }
        await new Promise((r) => setTimeout(r, 600));
        return {
          ok: true, dry_run: dryRun, chars: r.chars,
          evidence: dryRun ? '内容已就位，未发送' : '输入框已清空（剩余 5 字符）',
        };
      },
    };

// 取 Tauri invoke。
//
// ★★ 这里绝不能用「顶层 await import()」★★
// 早先写成 `const core = await import('/__TAURI__/core.js')` 放在模块顶层，
// 结果整个 GUI 停在「连接中…」，五个统计位全是「–」。
// 原因：**顶层 await 一旦被拒绝，整个模块执行中断**，
// 于是后面的 init()、tick() 全都不跑——而界面看起来"正常"（HTML 是静态的），
// 极具欺骗性。
//
// 现在改为读 window.__TAURI__（由 withGlobalTauri 注入），
// 失败时把原因记下来并抛出，由 renderErr 展示，绝不静默。
let coreErr = null;
function getCore() {
  const g = window.__TAURI__;
  const inv = (g && g.core && g.core.invoke) || (g && typeof g.invoke === 'function' ? g.invoke : null);
  if (!inv) {
    coreErr = coreErr || 'window.__TAURI__ 未注入。'
      + '请确认 tauri.conf.json 里 app.withGlobalTauri = true，并已重新构建。';
    throw new Error(coreErr);
  }
  // ★★ 命令名映射：camelCase → snake_case ★★
  // Tauri 的 invoke **不做命名风格转换**：前端写 queueView、
  // 后端注册的是 queue_view，运行时直接报
  // 「Command queueView not found」（实测踩过，界面只显示这一句红字）。
  //
  // 为什么在这里统一转换、而不把前端改成 snake_case：
  // 前端是 JS，camelCase 是本分；后端是 Rust，snake_case 也是本分。
  // 让一层适配去做翻译，两边都保持各自惯例——
  // 而且这层表是**白名单**，新增命令漏写会立刻报错（可发现），
  // 比静默失败好。
  const SNAKE = {
    enqueue: 'enqueue',
    queueView: 'queue_view',
    queueCancel: 'queue_cancel',
    queueClear: 'queue_clear',
    loopStart: 'loop_start',
    loopStop: 'loop_stop',
    idleCheck: 'idle_check',
    readTextFile: 'read_text_file',
    probeSkills: 'probe_skills',
    diag: 'diag',
    uiClick: 'ui_click',
    probe: 'probe', preview: 'preview', send: 'send', busy: 'busy',
    // ★ 打断发送：后端 command 名 force_send，映射名 forceSend ★
    forceSend: 'force_send',
  };
  return { invoke: (cmd, args) => {
    const real = SNAKE[cmd];
    if (!real) {
      coreErr = coreErr || ('未知命令 ' + cmd
        + '——它不在 SNAKE 映射表里。改了后端命令名就要同步这张表。');
      throw new Error(coreErr);
    }
    return inv(real, args);
  } };
}

if (IS_TAURI) {
  api.probe = () => getCore().invoke('probe');
  api.diag = (msg) => getCore().invoke('diag', { msg });
  api.uiClick = (selector) => getCore().invoke('ui_click', { selector });
  api.enqueue = (text, convId, round) =>
    getCore().invoke('enqueue', { text, convId, round });
  api.queueView = () => getCore().invoke('queueView');
  api.queueCancel = (id) => getCore().invoke('queueCancel', { id });
  api.queueClear = () => getCore().invoke('queueClear');
  api.loopStart = (texts, convId, rounds, trigger, intervalS, idleTimeoutS) =>
    getCore().invoke('loopStart', { texts, convId, rounds, trigger, intervalS, idleTimeoutS });
  api.loopStop = () => getCore().invoke('loopStop');
  api.idleCheck = () => getCore().invoke('idleCheck');
  api.readTextFile = (path) => getCore().invoke('readTextFile', { path });
  api.probeSkills = () => getCore().invoke('probeSkills');
  api.preview = (tpl, vars) => getCore().invoke('preview', { tpl, vars });
  api.send = (tpl, vars, dryRun, openNew) =>
    getCore().invoke('send', { tpl, vars, dryRun, openNew });
}



const DEFAULT_TPL = `# {{任务}} · 下一轮施工指令

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

读上面点名的文件，按锚点原文逐条落实判据。做完交活、回写任务板、归档过程产物。`;

const S = { snap: null, busy: false, timer: 0 };

// ─────────────────────────── 工具 ───────────────────────────
const esc = (s) =>
  String(s == null ? '' : s).replace(/[&<>"']/g, (x) =>
    ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[x]));

/** 压短长路径：…\父\子，完整值挂 title。 */
function shortPath(p, max = 42) {
  if (!p) return '';
  if (p.length <= max) return p;
  const parts = p.split(/[\\/]/).filter(Boolean);
  return '…\\' + parts.slice(-2).join('\\');
}

/** 场景切到 body[data-scene]，驱动整套配色与光晕。 */
function setScene(name) {
  if (document.body.dataset.scene !== name) document.body.dataset.scene = name;
}

function conn(kind, msg) {
  $('s-dot').className = 'dot ' + kind;
  $('s-msg').textContent = msg;
}

// ─────────────────────────── 快照 ───────────────────────────
async function tick() {
  if (S.busy) return void (S.timer = setTimeout(tick, 1200));
  try {
    const s = await api.probe();
    S.snap = s;
    render(s);
    conn('ok', '已连接');
  } catch (e) {
    renderErr(e);
    conn('err', '采集失败');
  }
  S.timer = setTimeout(tick, 1500);
}

/** 把后端的三要素错误渲染出来（不裸奔异常码）。 */
function renderErr(e) {
  const p = e && typeof e === 'object' ? e : null;
  const what = p?.what || p?.message || String(e);
  $('hint').className = 'note';
  $('hint').innerHTML =
    `<span class="e-what">${esc(what)}</span>` +
    (p?.why ? `<span class="e-why">${esc(p.why)}</span>` : '') +
    (p?.next ? `<span class="e-next">${esc(p.next)}</span>` : '');
  setScene('alarm');
}

function render(s) {
  $('s-conv').textContent = s.convs.length;
  $('s-model').textContent = s.current_model || '未知';
  $('s-cwd').textContent = shortPath(s.convs.find((c) => c.selected)?.cwd || '') || '未知';
  $('s-ver').textContent = s.version || '?';
  $('s-send').innerHTML = s.sending
    ? '<span class="tag bad">生成中</span>'
    : '<span class="tag now">空闲</span>';

  // 场景：忙 → 琥珀；否则回到空闲
  if (s.sending) setScene('busy');
  else if (document.body.dataset.scene === 'busy') setScene('idle');

  $('conv-count').textContent = s.convs.length + ' 个';
  const box = $('convs');
  // ★ 增量更新，不要整块重建 ★
  // 早先每次轮询都 `innerHTML = ''` 再全量重建，实测两个副作用：
  //   1. 用户悬停看 title 的那一刻，卡片被换掉 ⇒ tooltip 闪掉
  //   2. 列表滚动位置跳回顶部（重建后浏览器重排）
  // 1.5 秒一次轮询 ⇒ 这不是偶发，是必然。
  // 正解：按 key（conv_id，无则退回 index）复用节点，只更新变化的字段。
  const seen = new Map();   // key -> {el, sig}
  if (!s.convs.length) {
    box.innerHTML = '<div class="empty">没有读到对话。确认 WorkBuddy 已启动且开着对话窗口。</div>';
    return;
  }
  let prev = null;          // 用于把选中的卡片插到正确顺序
  for (const c of s.convs) {
    const key = c.conv_id || 'idx' + c.index;
    const sig = [c.title, c.rel_time, c.model, c.cwd, c.selected].join('\u0001');
    let rec = seen.get(key);

    if (!rec) {
      rec = { el: buildConvCard(c), sig: '' };
      rec.el.dataset.cid = c.conv_id || '';
      seen.set(key, rec);
    }
    if (rec.sig !== sig) {
      updateConvCard(rec.el, c);
      rec.sig = sig;
    }
    // 按数据顺序插入（append 已有节点等于移动，天然去重）
    if (prev) prev.after(rec.el); else box.prepend(rec.el);
    prev = rec.el;
  }
  // 移除已消失的对话
  for (const child of [...box.children]) {
    if (![...seen.values()].some((r) => r.el === child)) child.remove();
  }
}

/** 造一张对话卡片（结构只建一次，后续只改文本）。 */
function buildConvCard(c) {
  const d = document.createElement('div');
  d.className = 'conv';

  const t = document.createElement('div');
  t.className = 't';
  const txt = document.createElement('span');
  txt.className = 'txt';
  t.append(txt);
  const tag = document.createElement('span');
  tag.className = 'tag now';
  tag.textContent = '当前';
  tag.hidden = true;
  t.append(tag);

  const rt = document.createElement('div');
  rt.className = 'rt';

  const meta = document.createElement('div');
  meta.className = 'meta';
  const s1 = document.createElement('span');   // 模型行
  const s2 = document.createElement('span');   // 目录行
  // ★ 必须 append，否则 meta 是空的 ★
  // 重写增量更新时漏了这两行，实测表现：卡片只有标题，
  // 「模型 / 目录」两行整个不见了——而静默无错。
  meta.append(s1, s2);

  // 点选标记（Variable 要的「选任意对话发布」）
  const pick = document.createElement('div');
  pick.className = 'pick';
  pick.textContent = '✓';
  pick.title = '点这张卡片 = 之后的发送/循环都发到这个对话';

  d.append(t, rt, meta, pick);
  d._parts = { t, txt, tag, rt, meta, s1, s2 };
  // ★ 整卡可点 ★：点卡片 = 选中该对话（发送走选中项）
  d.addEventListener('click', () => {
    window.pickConv && window.pickConv(d.dataset.cid || '', d._parts.txt.textContent);
  });
  return d;
}

/** 更新卡片内容。只碰变化，不重建节点。 */
function updateConvCard(d, c) {
  const p = d._parts;
  d.className = 'conv' + (c.selected ? ' sel' : '');
  if (p.txt.textContent !== c.title) p.txt.textContent = c.title;
  p.t.title = c.title;                 // 完整值挂 tooltip
  p.tag.hidden = !c.selected;
  if (p.rt.textContent !== (c.rel_time || '')) p.rt.textContent = c.rel_time || '';

  p.s1.innerHTML = c.model
    ? `<b>模型</b>${esc(c.model)}`
    : `<b>模型</b><span class="tag">侧栏不显示 · 点开可读</span>`;
  p.s2.innerHTML = c.cwd
    ? `<b>目录</b><span class="dir" title="${esc(c.cwd)}">${esc(shortPath(c.cwd))}</span>`
    : `<b>目录</b><span class="tag bad">${esc(c.cwd_confidence)}</span>`;
}
// ─────────────────────────── 模板 ───────────────────────────
const collect = () => ({
  tpl: $('tpl').value,
  vars: { '任务': $('v-task').value, '上下文': $('v-ctx').value },
  openNew: $('o-new').checked,
  dryRun: $('o-dry').checked,
});

async function doPreview() {
  try {
    const r = await api.preview(collect().tpl, collect().vars);
    $('preview').innerHTML = esc(r.text).replace(/\{\{\s*([^{}]+?)\s*\}\}/g,
      '<span class="ph">{{$1}}</span>');
    const h = $('hint');
    if (r.missing.length) {
      h.className = 'note';
      h.innerHTML =
        `<span class="e-what">还有 ${r.missing.length} 个占位符没填：` +
        r.missing.map((k) => `{{${esc(k)}}}`).join('、') + `</span>` +
        `<span class="e-next">未填的会原样保留（不会变成空段落），发送会被拦下。填好或删掉即可。</span>`;
    } else {
      h.className = 'note ok';
      h.textContent = `预览就绪 · ${r.chars} 字符 · ` +
        (collect().dryRun ? '干跑模式（只填不发）' : '将真发送');
    }
  } catch (e) {
    __why('app', e);
    renderErr(e);
  }
}

// ─────────────────────────── 执行 ───────────────────────────
async function doSend(real) {
  if (S.busy) return;
  S.busy = true;
  $('b-fill').disabled = $('b-send').disabled = true;
  const h = $('hint');
  h.className = 'note';
  h.textContent = real ? '正在填入并发送…' : '正在填入（不发送）…';
  try {
    const c = collect();
    const r = await api.send(c.tpl, c.vars, !real, c.openNew);
    if (r.ok) {
      h.className = 'note ok';
      h.textContent = r.dry_run
        ? `已填入 ${r.chars} 字符，未发送。`
        : `已发送 · ${r.chars} 字符 · ${r.evidence}`;
      if (real) {
        setScene('sent');
        const f = $('flash');
        f.classList.remove('go');
        void f.offsetWidth;          // 强制重排以重启动画
        f.classList.add('go');
        setTimeout(() => setScene('idle'), 1400);
      }
    } else if (r.err?.busy && real) {
      // ★★ 忙时自动降级为入队，不让用户失败 ★★
      // 按钮叫「填入并发送」，那就该保证发出去。
      // 忙时唯一的正确做法是排队——「加入待发」能排，
      // 凭什么点了「填入并发送」反而失败、还要用户自己换个按钮？
      try {
        const c2 = collect();
        const pv = await api.preview(c2.tpl, c2.vars);
        // ★ 对话 id 从当前快照里取（collect() 里没有这个字段）★
        let cid = '';
        try {
          const sel = (S.snap && S.snap.convs || []).find((c) => c.selected);
          cid = (sel && sel.conv_id) || '';
        } catch (_e) { /* 取不到就留空，后端会回退到当前会话 */ }
        const q = await api.enqueue(pv.text, cid, 0);
        h.className = 'note ok';
        h.textContent =
          `已排队 · ${pv.chars} 字符 · 对方正在生成中，生成完会自动发` +
          (pv.missing && pv.missing.length
            ? `（${pv.missing.length} 个占位符没填，会原样发出去）`
            : '');
        // 队列面板同步刷新（它在 loop_ui.js 里）
        if (window.VAPUI && typeof window.VAPUI.renderQueue === 'function') {
          window.VAPUI.renderQueue(q.items);
        }
        if (window.VAPUI && typeof window.VAPUI.refreshQueue === 'function') {
          window.VAPUI.refreshQueue();
        }
        setScene('busy');
      } catch (e2) {
        // 降级也失败（如 enqueue 后端不可用）⇒ 说清两层原因
        __why('doSend-降级入队', e2);
        h.className = 'note busy';
        h.innerHTML =
          `<span class="e-what">对方正在生成中，且自动排队也失败了</span>` +
          `<span class="e-why">${esc(r.err.why || '')}；排队失败：${esc(e2 && e2.message ? e2.message : String(e2))}</span>` +
          `<span class="e-next">先点「加入待发」把内容排进去，等它空闲后自动发。</span>`;
      }
    } else {
      h.className = 'note' + (r.err?.busy ? ' busy' : '');
      h.innerHTML = r.err
        ? `<span class="e-what">${esc(r.err.what)}</span>` +
          (r.err.why ? `<span class="e-why">${esc(r.err.why)}</span>` : '') +
          (r.err.next ? `<span class="e-next">${esc(r.err.next)}</span>` : '')
        : '执行失败（后端未给出原因）';
      if (r.err?.busy) setScene('busy');
    }
  } catch (e) {
    __why('app', e);
    renderErr(e);
  } finally {
    S.busy = false;
    $('b-fill').disabled = $('b-send').disabled = false;
    tick();
  }
}

// ═══════════════════════════════════════════════════════════════════
// ★★★ 打断发送 ★★★
//
// Variable 明确要求（2026-10-06）：
//   「发不了不要管当前是什么，全部停止然后使用，不然怎么老是这样」
//
// ★ 我之前把「不打断对方」当成红线，于是永远在等—— ★
// 而等待没有上限、没有进度，正好就是他抱怨的「老是这样没反应」。
// 现在给出第三条路：**先停止，立刻发**。
//
// ★ 保留原行为 ★
// 「加入待发」「填入并发送」两条安全路径完全不受影响，仍可选用。
// 打断是显式选择，不是默认行为。
//
// ⚠ 代价是明说的：它会中断对方当前正在生成的内容。
async function forceSend() {
  if (S.busy) return;
  S.busy = true;
  const btn = $('b-force');
  btn.disabled = true;
  const h = $('hint');
  h.className = 'note';
  h.textContent = '正在打断：停止对方生成 → 填入 → 发送…';
  try {
    const c = collect();
    // ★ 用 preview 先渲染好文本（它已实现变量替换 + 缺占位符检出）★
    //   后端 force_send 只收渲染好的 text，不做替换 ——
    //   避免两处各实现一遍、行为不一致。
    const pv = await api.preview(c.tpl, c.vars);
    const r = await api.forceSend(pv.text);
    h.className = 'note ok';
    h.textContent = (r.stopped_first
      ? '★ 已打断对方并发送'
      : '★ 已发送（对方本来就没在生成）')
      + ' · ' + pv.chars + ' 字符 · ' + r.evidence;
    setScene('sent');
    const f = $('flash');
    f.classList.remove('go');
    void f.offsetWidth;
    f.classList.add('go');
    setTimeout(() => setScene('idle'), 1400);
  } catch (e) {
    __why('forceSend', e);
    h.className = 'note busy';
    // ErrPayload 有 what/why/next 三字段，全展示，不裸抛
    const what = e && e.what ? e.what : '打断发送失败';
    const why = e && e.why ? e.why : (e && e.message ? e.message : String(e));
    const next = e && e.next ? e.next : '再点一次，或改用「加入待发」排队等空闲。';
    h.innerHTML =
      '<span class="e-what">' + esc(what) + '</span>' +
      '<span class="e-why">' + esc(why) + '</span>' +
      '<span class="e-next">' + esc(next) + '</span>';
  } finally {
    S.busy = false;
    btn.disabled = false;
    tick();
  }
}

// ═══════════════════════════════════════════════════════════════════
// 共享契约（供 loop_ui.js 使用）
//
// ★ 为什么要显式挂 window ★
// app.js 与 loop_ui.js 是两个 ES module，**模块作用域互不可见**。
// 而 queue/循环/内容来源这些功能的 UI 逻辑体量不小，塞进app.js 会让
// 它超过 700 行、难维护；拆成独立文件又需要共享 api/$/esc。
//
// 显式挂 window 的好处：**共享面是白名单式的**——
// 只有列在这里的才对外可见，其余仍是模块私有。
// 另起一个 shared.js 反而多一次 import，且 Circular 依赖更难查。
// ═══════════════════════════════════════════════════════════════════
window.VAP = { api, $, esc, renderErr, setScene, conn };

// app.js 侧三个按钮同样改成 addEventListener（理由见 loop_ui 的 __BIND 注释）。
(function __bindApp() {
  const pairs = [
    ['b-preview', () => window.VAP.api && doPreview()],
    ['b-fill', () => doSend(false)],
    ['b-send', () => doSend(true)],
  // ★ 打断发送 ★（必须走 addEventListener，勿改回内联 onclick ——
  //   今晚已确诊：module作用域下内联 onclick 静默失效）
  ['b-force', () => forceSend()],
  ];
  const ok = pairs.filter(([id]) => !!document.getElementById(id)).length;
  for (const [id, fn] of pairs) {
    const el = document.getElementById(id);
    if (el) el.addEventListener('click', fn);
  }
  // ★ 报绑定结果 ★
  // 早先这段只报 loop_ui 的 10 个，app.js 侧这4 个绑没绑上完全不可见——
  // 而「按钮点了没反应」正是这类缺失的典型表现。
  // 静默绑不上= 换UI 改id 时最容易踩的坑，必须让它显形。
  try {
    window.VAP.api.diag('app 按钮绑定 ' + ok + '/' + pairs.length).catch(() => {});
  } catch (_e) { /* 排障通道失败不影响界面 */ }
})();


  // ★ 把异常写进后端日志（排障唯一可见的地方）★
  // 没有它，失败只显示在界面 hint 区 ⇒ 表现为「点了没反应」，
  // 排障者看不到原因。这条通道本项目已因此栽过一次。
  const __why = (where, e) => {
    try {
      const w = window.VAP && window.VAP.api && window.VAP.api.diag;
      if (!w) return;
      const parts = [
        where + ' 失败',
        'msg=' + (e && e.message ? e.message : String(e)),
        'what=' + (e && e.what ? e.what : '-'),
        'why=' + (e && e.why ? e.why : '-'),
        'next=' + (e && e.next ? e.next : '-'),
        'at=' + (e && e.stack && e.stack.split('\n')[1] ? String(e.stack).split('\n')[1].trim() : '-'),
      ];
      w(parts.join(' | ')).catch(() => {});
    } catch (_x) { /* 排障通道失败不影响主流程 */ }
  };


// ════════════════════════════════════════════════════════════════
// ★★★ 必须把自己的函数挂到裸全局 ★★★
//
// 现象（实测check_buttons_live.mjs）：
//   点「预览」「填入」「填入并发送」→ console 报
//   **ReferenceError: doPreview is not defined**
//   而同一界面上loop_ui.js 的按钮（加入待发 / 刷新 / 首轮 / 读取）**全部正常**。
//
// 原因：HTML 里的内联 onclick 写的是裸名字（onclick="doPreview()"），
// 浏览器去**全局作用域链**上找。而 app.js 是 ES module，
// 模块里的函数**不在全局**——只有显式 window.doPreview= 才是。
// loop_ui.js 做了这件事（Object.assign(window, VAPUI)），
// app.js 一直没做，于是它的三个按钮全废。
//
// 「一半按钮能用一半不能」正是这个 bug 的签名。
// ★ 而且它**静默**：界面上看不出按钮坏了，只有控制台有话说。
window.doPreview = doPreview;
window.forceSend = forceSend;
window.doSend = doSend;
window.render = render;
window.setScene = setScene;

// 启动自检：把「前端执行到哪一步」写进后端日志。
// 后端 logx 写到 %TEMP%/varix-autopilot.log ——
// 排障时第一件事就是 tail 它，不用猜、不用开 DevTools。
// ★★ 排障代码绝不能有能力搞挂主流程 ★★
// .catch() 只捕获 Promise 拒绝，**捕获不了同步 throw**——
// 而 api.diag 内部会调 getCore()，它在 Tauri 未注入时是同步抛的。
// 后果实测过：这一行同步抛 ⇒ 整个模块顶层中断 ⇒ init() 从未执行
// ⇒ 模板空、tick 不跑、界面永远停在「连接中…」。
// 所以：try + .catch 双保险，且失败也只静默（它是纯排障通道）。
try {
  api.diag('app.js 已就绪').catch(() => {});
} catch (_e) { /* 排障通道失败不影响主流程 */ }

// ═══════════════════════════════════════════════════════════════════
// ★ 共享契约的时机（这一段是本项目最隐蔽的坑，值得写清楚）★
//
// app.js 与 loop_ui.js 都是 <script type="module">，
// 浏览器会**并行下载、并行执行**，谁先跑完不保证。
//
// 早先 loop_ui.js 顶层写 ，
// 实测它常常**先**跑完 —— 那一刻 window.VAP 还没建（本文��� 507 行才赋值），
// 于是解构 undefined 抛 TypeError → **整个 loop_ui 模块中断** →
// 后面所有函数都没定义、Object.assign(window, VAPUI) 从未执行 →
// HTML 里 onclick="enqueueOne()" 找不到函数
// → **「加入待发」点了完全没反应**。
//
// 而同一行的「预览 / 填入」是好的——它们在 app.js 里，模块内直接可用。
// **「一半按钮能用、一半不能」正是这个 bug 的签名。**
//
// 现在 loop_ui.js 里改成**在函数体内**才取 window.VAP.xxx
//（回调被调用时 VAP 必然已就绪），顶层不再有任何依赖。
// 这样两个模块谁先跑都无所谓——**顺序不再是问题**。
//
// 另：HTML 的内联 onclick 在**全局作用域**找函数，
// 所以 loop_ui.js 的实现仍需 Object.assign 到 window（那边已加自检）。
// ═══════════════════════════════════════════════════════════════════

// ─────────────────────────── 启动 ───────────────────────────
(async function init() {
  try {
    $('tpl').value = DEFAULT_TPL;
  // ★ 内容来源指示器：初始化 + 勾选变化时立刻刷新 ★
  if (typeof window.__VAP_SRC_NOTE__ === 'function') window.__VAP_SRC_NOTE__();
  for (const i of ['o-free', 'o-dry', 'free-text', 'tpl']) {
    const el = document.getElementById(i);
    if (el) el.addEventListener('input', () => window.__VAP_SRC_NOTE__());
    if (el) el.addEventListener('change', () => window.__VAP_SRC_NOTE__());
  }
  } catch (e) {
  }
  // ★ 每一段都独立 try：早先 `await doPreview()` 一抛错，
  //  后面的 tick() 就不执行，界面永远停在初始态却看起来"正常"。
  //  这是"异常零静默"在启动链上的落实。
  try {
    await doPreview();
  } catch (e) {
    __why('app', e);
    renderErr(e);
  }
  tick();
  // ★ 走 window 取 ★：refreshQueue 在 loop_ui.js 里，
  //   两个 ES module 作用域互不可见（这正是本轮踩的坑）
  window.VAPUI && window.VAPUI.refreshQueue();

  // ★ 自检兜底：8 秒后若仍显示「连接中…」，把原因摊到界面上。
  //   目的：宁可吵一点，也不要"看着正常其实没在跑"。
  setTimeout(() => {
    if (S.snap) return;                       // 已连上，不管
    const d = $('s-dot');
    if (d.classList.contains('ok')) return;    // 已连接，不管
    const msg = $('s-msg');
    if (msg.textContent === '连接中…') {
      conn('err', '未连上 WorkBuddy');
      const h = $('hint');
      h.className = 'note';
      h.innerHTML =
        `<span class="e-what">8 秒内没有取得任何快照</span>` +
        `<span class="e-why">${esc(coreErr || '可能是 9222 端口未开、或 WorkBuddy 未带调试参数启动')}</span>` +
        `<span class="e-next">先双击桌面的「VarixAutoPilot 开端口.bat」，再重新启动本程序。</span>`;
      setScene('alarm');
    }
  }, 8000);

  // 模板改动 300ms 防抖后自动预览（不打断输入）
  let t = 0;
  $('tpl').addEventListener('input', () => {
    clearTimeout(t);
    t = setTimeout(doPreview, 300);
  });
})();

// 快捷键：Ctrl+Enter 真发，Ctrl+P 预览
document.addEventListener('keydown', (e) => {
  if (e.ctrlKey && e.key === 'Enter') { e.preventDefault(); doSend(true); }
  if (e.ctrlKey && e.key === 'p') { e.preventDefault(); doPreview(); }
});
