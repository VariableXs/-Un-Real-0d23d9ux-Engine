// ═══════════════════════════════════════════════════════════════════
// 待发队列 / 循环发布 / 内容来源
//
// ★ 边界原则（与主流程一致）★
// 前端**不自己判断"这条发成功了吗"**——那必须由后端用客观证据判定
// （编辑器清空 / 消息入流）。前端只负责展示后端结论。
// ★ 也不自己数轮数★：无限循环在后端是 u32::MAX，
//   前端传 0 表示无限，避免大数字在前端丢精度。
// ═══════════════════════════════════════════════════════════════════

// ══════════════════════════════════════════════════════════════════
// ★★★ 共享契约：惰性取，不用顶层解构 ★★★
//
// 现象：HTML 里onclick="enqueueOne()" 点了没反应，
// 而同一行的「预览 / 填入」正常（它们在 app.js 里，模块内直接可用）。
//
// 根因：两个文件都是 <script type="module">，浏览器**并行执行**，
// 谁先跑完不保证。loop_ui.js 常先跑完，而 app.js 是在它自己第 507 行
// 才 （共 556 行）——于是这行顶层解构拿到 undefined，
// 抛 TypeError ⇒ **整个模块中断** ⇒ 后面全没跑 ⇒
// Object.assign(window, VAPUI) 从未执行 ⇒ 内联 onclick 找不到函数。
//
// 「一半按钮能用一半不能用」正是本 bug 的签名。
//
// ★ 为什么不用顶层 await ★
// 顶层 await 一旦被拒绝，整个模块就死——本项目已因此栽过一次
//（await import('/__TAURI__/core.js') 失败，GUI 永久停在「连接中…」）。
//
// 正解：**惰性取值**。api/$/esc 用到时才从 window.VAP 拿，
// 拿不到时给出明确错误而不是崩在启动阶段。
// ══════════════════════════════════════════════════════════════════
const Q = {
  picked: '',        // 选中的对话 conv_id（'' = 当前对话）
  pickedTitle: '当前选中的对话',
  loopRunning: false,
  loopTexts: { first: '', rest: '' },
  curTab: 'first',
  skills: [],
};

// ── 对话点选 ────────────────────────────────────────────
function pickConv(convId, title) {
  Q.picked = convId || '';
  Q.pickedTitle = title || '当前选中的对话';
  // 重绘卡片（只改 picked 类，不重建——避免打断悬停）
  for (const el of document.querySelectorAll('.conv')) {
    el.classList.toggle('picked', (el.dataset.cid || '') === Q.picked && !!Q.picked);
  }
  const sel = window.VAP.$('lp-conv');
  if (sel) sel.options[0].textContent = '当前选中：' + Q.pickedTitle.slice(0, 22);
  const n = window.VAP.$('pick-note');
  if (n) n.textContent = Q.picked ? '已选：' + Q.pickedTitle.slice(0, 26) : '未选（发到当前对话）';
}

// ═══ 导出到window ═══
// 模块作用域互不可见，本文件里的函数要��� app.js 与 HTML 的 onclick 用，
// 就必须挂到 window。统一挂一份VAPUI，避免散落多个全局。
// ★ 教训（这轮踩的）：跨模块调用若只挂一半，
//   页面会报 "xxx is not defined"，而 build 与 node --check 全绿 ★
const VAPUI = {
  pickConv, enqueueOne, renderQueue, refreshQueue, clearQueue,
  switchLoopTab, loopTexts, startLoop, stopLoop, readMd, probeSkills, appendSkill,
};
window.VAPUI = VAPUI;
// ★★ 必须同时挂到 window 顶层 ★★
// HTML 里的 onclick="startLoop()" 是在**全局作用域**里找 startLoop，
// 只挂 window.VAPUI.startLoop 它找不到——
// 实测症状：VAPUI 有 12 个键，但点击报 "startLoop is not defined"。
// 这类错 build 与 node --check 都发现不了，只有真点一下才暴露。
Object.assign(window, VAPUI);

// ═══ 取「本轮要发什么」═══
// 三个来源，按用户勾选/填写状态决定：
//   ① 勾了「用自由文本」→ 直接用那段字（绕过模板与占位符）
//   ② 循环框有内容 → 用它（首轮/后续轮按当前标签）
//   ③ 否则 → 用主模板渲染
// ★ 统一在这里取，循环与单发共用，不会出现两套判断 ★
async function pickContent() {
  if (window.VAP.$('o-free').checked) {
    const t = window.VAP.$('free-text').value;
    if (!t.trim()) {
      const e = new Error('勾了用自由文本，但框是空的');
      e.what = '自由文本框是空的'; e.why = '勾了「用这段自由文本」但没写内容';
      e.next = '写点内容，或取消那个勾选';
      throw e;
    }
    return { text: t, chars: t.length, missing: [], 源: '自由文本' };
  }
  const lt = loopTexts();
  if (lt.length) {
    const t = Q.curTab === 'first' ? lt[0] : lt[1];
    return { text: t, chars: t.length, missing: [], 源: Q.curTab === 'first' ? '首轮' : '后续每轮' };
  }
  const tpl = window.VAP.$('tpl').value;
  const r = await window.VAP.api.preview(tpl, { '任务': window.VAP.$('v-task').value, '上下文': window.VAP.$('v-ctx').value });
  return { text: r.text, chars: r.chars, missing: r.missing, 源: '模板' };
}

// ── 入队一条 ──────────────────────────────────────────────
async function enqueueOne() {
  try {
    const r = await pickContent();
    // ★ 允许带未填占位符入队 ★
    // 早先在这里硬拦，未填 {{任务}} 时队列永远是空的——
    // 而模板的既定语义就是「未填的原样保留」，那不该拦住入队，
    // 只该**提示**。拦下来反而让人以为功能坏了。
    const v = await window.VAP.api.enqueue(r.text, Q.picked, 0);
    renderQueue(v.items);
    if (r.missing.length) {
      setHint(
        `已入队 · ${r.chars} 字符（来自${r.源}；${r.missing.length} 个占位符没填：`
        + r.missing.map((k) => `{{${k}}}`).join('、')
        + '，会原样发出去）',
        'warn'
      );
    } else {
      setHint(`已入队 · ${r.chars} 字符（来自${r.源}）· 对方空闲时会自动发`, 'ok');
    }
  } catch (e) {
    window.VAP.renderErr(e);
  }
}

// ── 队列渲染 ──────────────────────────────────────────────
const ST_TEXT = { pending: '等待', sending: '发送中', done: '已发', failed: '失败', canceled: '已取消' };

function renderQueue(items) {
  const box = window.VAP.$('qlist');
  box.innerHTML = '';
  if (!items || !items.length) {
    box.innerHTML = '<div class="empty">队列为空。点「加入待发」把内容排进来，或在下面设循环。</div>';
    window.VAP.$('q-sum').textContent = '0 条';
    return;
  }
  for (const it of items) {
    const d = document.createElement('div');
    d.className = 'qi';
    const rd = document.createElement('div');
    rd.className = 'rd';
    rd.textContent = it.round > 0 ? '第' + it.round + '轮' : '单发';
    const pv = document.createElement('div');
    pv.className = 'pv';
    pv.textContent = it.preview;
    pv.title = (it.err ? it.err + ' · ' : '') + it.text;
    const st = document.createElement('div');
    st.className = 'st ' + it.state;
    st.textContent = ST_TEXT[it.state] || it.state;
    const x = document.createElement('button');
    x.className = 'x';
    x.textContent = '×';
    x.title = it.state === 'pending' ? '取消这条' : '这条已不在等待中，不能撤';
    x.disabled = it.state !== 'pending';
    x.onclick = async () => {
      try {
        const v = await window.VAP.api.queueCancel(it.id);
        renderQueue(v.items);
      } catch (e) {
        window.VAP.renderErr(e);
      }
    };
    d.append(rd, pv, st, x);
    box.append(d);
  }
  const wait = items.filter((i) => i.state === 'pending').length;
  const done = items.filter((i) => i.state === 'done').length;
  window.VAP.$('q-sum').textContent = `待发 ${wait} · 已发 ${done} · 共 ${items.length}`;
  window.VAP.$('q-sum').className = 'chip' + (wait ? ' busy' : done ? ' on' : '');
}

async function refreshQueue() {
  try {
    const v = await window.VAP.api.queueView();
    renderQueue(v.items);
  } catch (e) {
    window.VAP.renderErr(e);
  }
}

async function clearQueue() {
  try {
    const v = await window.VAP.api.queueClear();
    renderQueue(v.items);
  } catch (e) {
    window.VAP.renderErr(e);
  }
}

// ── 循环 ─────────────────────────────────────────────────
function switchLoopTab(which) {
  // ★切换标签前先存当前框的内容 ★
  Q.loopTexts[Q.curTab] = window.VAP.$('loop-text').value;
  Q.curTab = which;
  window.VAP.$('loop-text').value = Q.loopTexts[which] || '';
  for (const b of document.querySelectorAll('.tab[data-lt]')) {
    b.classList.toggle('on', b.dataset.lt === which);
  }
}

function loopTexts() {
  Q.loopTexts[Q.curTab] = window.VAP.$('loop-text').value;
  // 首轮为空时退回用后续轮的（常见用法：一套内容循环发）
  const first = (Q.loopTexts.first || '').trim();
  const rest = (Q.loopTexts.rest || '').trim();
  if (first && rest) return [first, rest];
  if (first) return [first, first];
  if (rest) return [rest];
  return [];
}

async function startLoop() {
  // ★ 内容来源与单发统一走 pickContent ★
  // 早先只看 loopTexts()，于是「用自由文本」或「用主模板」时循环框是空的 ⇒
  // 直接报「循环没内容」。而用户明明已经填了内容——这属于「明明做了却报没做」。
  const p0 = await pickContent();
  const texts = loopTexts();
  const rounds_ = texts.length ? texts : [p0.text];
  if (!rounds_[0] || !rounds_[0].trim()) {
    return setHint('循环没内容：先在上面的框里写点东西', 'err');
  }
  const inf = window.VAP.$('lp-inf').checked;
  const rounds = inf ? 0 : Math.max(1, Math.min(99999, parseInt(window.VAP.$('lp-rounds').value, 10) || 1));
  try {
    const v = await window.VAP.api.loopStart(
      rounds_,
      Q.picked,
      rounds,
      window.VAP.$('lp-trigger').value,
      Math.max(1, parseInt(window.VAP.$('lp-interval').value, 10) || 30),
      600
    );
    renderQueue(v.items);
    Q.loopRunning = true;
    window.VAP.$('loop-state').textContent = inf ? '运行中 · 无限' : '运行中 · ' + rounds + ' 轮';
    window.VAP.$('loop-state').className = 'chip on';
    window.VAP.$('loop-note').textContent = (inf
      ? '无限循环：会一直发下去，停止请点「停止」'
      : '共 ' + rounds + ' 轮（首轮 + 后续 ' + (rounds - 1) + ' 轮）')
      + ' · 内容来自' + p0.源;
    setHint(inf ? '无限循环已开始' : '循环已开始，共 ' + rounds + ' 轮', 'ok');
  } catch (e) {
    window.VAP.renderErr(e);
  }
}

async function stopLoop() {
  try {
    const v = await window.VAP.api.loopStop();
    renderQueue(v.items);
    Q.loopRunning = false;
    window.VAP.$('loop-state').textContent = '已停止';
    window.VAP.$('loop-state').className = 'chip';
    window.VAP.$('loop-note').textContent = '队列里未发的项仍保留，可继续或清空';
    setHint('循环已停。未发的项还在队列里', 'ok');
  } catch (e) {
    window.VAP.renderErr(e);
  }
}

// ── 内容来源：MD 文件 ─────────────────────────────────────
async function readMd() {
  const path = window.VAP.$('md-path').value.trim();
  if (!path) return setHint('先填文件路径，或把文件拖到路径框里', 'err');
  try {
    const f = await window.VAP.api.readTextFile(path);
    window.VAP.$('md-info').innerHTML =
      `<span class="e-next">${window.VAP.esc(f.name)} · ${f.chars} 字符 / ${f.bytes} 字节 · 编码 ${window.VAP.esc(f.encoding)}</span>`;
    // 填进「首轮」与「后续轮」——但只在它们为空时填，不覆盖用户已写的
    if (!Q.loopTexts.first.trim()) { Q.loopTexts.first = f.text; Q.loopTexts.rest = f.text; }
    else if (!Q.loopTexts.rest.trim()) { Q.loopTexts.rest = f.text; }
    window.VAP.$('loop-text').value = Q.loopTexts[Q.curTab];
    setHint(`已读入 ${f.name}（${f.chars} 字符）。若下面框里已有内容，我没覆盖它`, 'ok');
  } catch (e) {
    window.VAP.renderErr(e);
  }
}

// ── 内容来源：技能（斜杠命令） ───────────────────────────
async function probeSkills() {
  const info = window.VAP.$('skill-info');
  info.innerHTML = '<span class="e-what">正在探测…（会往输入框打一个斜杠再清掉）</span>';
  try {
    const r = await window.VAP.api.probeSkills();
    Q.skills = r.items || [];
    const sel = window.VAP.$('skill-pick');
    sel.innerHTML = '<option value="">— 选择技能 —</option>';
    for (const s of Q.skills) {
      const o = document.createElement('option');
      o.value = s;
      o.textContent = s;
      sel.append(o);
    }
    if (!r.cleaned) {
      // ★ 显式提示残留，不静默 ★
      info.innerHTML =
        '<span class="e-what">探测后输入框有残留</span>' +
        '<span class="e-why">清理动作没成功，那一个斜杠可能还在</span>' +
        '<span class="e-next">切到 WorkBuddy 窗口手动删掉那个 /，再重新探测</span>';
      setHint('探测完成，但清理不干净——见技能区提示', 'err');
    } else {
      info.innerHTML = Q.skills.length
        ? `<span class="e-next">找到 ${Q.skills.length} 个可用命令（读自你的 WorkBuddy，不是预置列表）</span>`
        : '<span class="e-what">没抓到技能清单</span>' +
          '<span class="e-why">WorkBuddy 的补全面板可能换了结构</span>' +
          '<span class="e-next">可以直接在内容框里手打 /命令名 试试</span>';
      setHint(Q.skills.length ? `探测到 ${Q.skills.length} 个可用命令` : '没抓到技能清单', Q.skills.length ? 'ok' : 'err');
    }
  } catch (e) {
    info.innerHTML = '';
    window.VAP.renderErr(e);
  }
}

function appendSkill() {
  const s = window.VAP.$('skill-pick').value;
  if (!s) return setHint('先选一个技能', 'err');
  //技能名是「/命令名 描述」形态，插入时只取命令部分
  const cmd = s.trim().split(/\s+/)[0];
  Q.loopTexts[Q.curTab] = window.VAP.$('loop-text').value;
  Q.loopTexts[Q.curTab] += (Q.loopTexts[Q.curTab] ? '\n' : '') + cmd + ' ';
  window.VAP.$('loop-text').value = Q.loopTexts[Q.curTab];
  setHint('已插入 ' + cmd + ' 到「' + (Q.curTab === 'first' ? '首轮' : '后续每轮') + '」', 'ok');
}

// ── 统一提示（与主流程同一套三要素样式）─────────────────
function setHint(text, kind) {
  const h = window.VAP.$('hint');
  // ★ 三态：ok / warn / err ★
  // warn 用于「做成了但有瑕疵」（如带未填占位符入队），
  // 与 err（做不成）区分开，用户一眼能分清是哪种情况。
  h.className = 'note'
    + (kind === 'err' ? '' : kind === 'ok' ? ' ok' : kind === 'warn' ? ' warn' : '');
  h.textContent = text;
}
