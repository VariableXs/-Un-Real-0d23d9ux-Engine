// 视觉验收专用：Tauri IPC mock（仅 agent-browser --init-script 注入，不改产品代码）
(() => {
  const w = window;
  w.__TAURI_INTERNALS__ = {
    metadata: {
      currentWindow: { label: "desktop" },
      currentWebview: { label: "desktop", windowLabel: "desktop" },
    },
    plugins: {},
    transformCallback: (cb) => cb,
    invoke: (cmd, args) => {
      if (cmd === "get_all_settings") {
        return Promise.resolve({ oobeDone: "1", wizardDone: "1" });
      }
      if (cmd === "app_bootstrap" || cmd === "bootstrap") {
        return Promise.resolve({
          dataDir: "C:\\mock-data",
          dbPath: "C:\\mock-data\\db.sqlite",
          mediaDir: "C:\\mock-data\\media",
          backupsDir: "C:\\mock-data\\backups",
          version: "1.0.0-va-mock",
          schemaVersion: 1,
          portable: false,
        });
      }
      if (cmd === "boot_replay") {
        // 事件回放：seq 递增 + progress 推满（BootScreen apply() 单调不减）。
        const evs = [];
        for (let i = 1; i <= 10; i++) {
          evs.push({ seq: i, progress: i / 10, elapsedMs: i * 300, fileCount: i * 137 });
        }
        return Promise.resolve(evs);
      }
      if (cmd && cmd.startsWith("plugin:event|")) {
        return Promise.resolve(Math.floor(Math.random() * 1e6));
      }
      return Promise.reject(new Error("mock-unhandled:" + cmd));
    },
  };
})();
