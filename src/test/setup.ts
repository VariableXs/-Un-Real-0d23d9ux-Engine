// 测试全局环境兜底：node 环境缺 localStorage / window（vwm/timeline/rules/scenes/
// pip/colorBand/motion 的持久化与调度通道依赖）。仅在缺失时注入，不影响浏览器真机。
// window 定时器用 getter 动态转发全局，保证 vi.useFakeTimers 的补丁同样生效。
const store = new Map<string, string>();
const g = globalThis as { localStorage?: unknown; window?: unknown };
if (!g.localStorage) {
  g.localStorage = {
    getItem: (k: string) => (store.has(k) ? (store.get(k) as string) : null),
    setItem: (k: string, v: string) => {
      store.set(k, String(v));
    },
    removeItem: (k: string) => {
      store.delete(k);
    },
    clear: () => {
      store.clear();
    },
    key: (i: number) => [...store.keys()][i] ?? null,
    get length() {
      return store.size;
    },
  };
}
if (!g.window) {
  g.window = {
    get setTimeout() {
      return globalThis.setTimeout;
    },
    get clearTimeout() {
      return globalThis.clearTimeout;
    },
    get setInterval() {
      return globalThis.setInterval;
    },
    get clearInterval() {
      return globalThis.clearInterval;
    },
  };
}
