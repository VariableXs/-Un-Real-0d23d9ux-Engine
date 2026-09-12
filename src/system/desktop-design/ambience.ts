/**
 * AURORA-10000 · AI-11~AI-15 车道 · WebAudio 程序化音景（族0074 剧场）。
 * 全部本地合成（噪声源 + 滤波 + LFO），零外部音频资产、零出站；
 * 用户点「进入剧场」后才开始（浏览器自动播放策略合规）。
 */

export type AmbienceKind =
  | "none" | "rain" | "waves" | "fire" | "wind" | "crickets" | "city" | "cafe";

export class AmbienceSynth {
  private ctx: AudioContext | null = null;
  private src: AudioBufferSourceNode | null = null;
  private gain: GainNode | null = null;
  private lfo: OscillatorNode | null = null;
  private kind: AmbienceKind = "none";

  play(kind: AmbienceKind): void {
    if (kind === this.kind) return;
    this.stop();
    this.kind = kind;
    if (kind === "none") return;
    try {
      const Ctor = window.AudioContext ?? (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
      if (!Ctor) return;
      this.ctx = new Ctor();
      const ctx = this.ctx;
      void ctx.resume();
      // 2s 白噪声 buffer
      const buf = ctx.createBuffer(1, ctx.sampleRate * 2, ctx.sampleRate);
      const data = buf.getChannelData(0);
      let brown = 0;
      for (let i = 0; i < data.length; i++) {
        const white = Math.random() * 2 - 1;
        brown = (brown + 0.02 * white) / 1.02;
        data[i] = kind === "rain" || kind === "crickets" ? white : brown * 3.5;
      }
      this.src = ctx.createBufferSource();
      this.src.buffer = buf;
      this.src.loop = true;
      const filter = ctx.createBiquadFilter();
      this.gain = ctx.createGain();
      switch (kind) {
        case "rain": filter.type = "bandpass"; filter.frequency.value = 1800; filter.Q.value = 0.6; this.gain.gain.value = 0.12; break;
        case "waves": filter.type = "lowpass"; filter.frequency.value = 420; this.gain.gain.value = 0.2; break;
        case "fire": filter.type = "lowpass"; filter.frequency.value = 900; this.gain.gain.value = 0.16; break;
        case "wind": filter.type = "lowpass"; filter.frequency.value = 300; this.gain.gain.value = 0.12; break;
        case "crickets": filter.type = "bandpass"; filter.frequency.value = 4200; filter.Q.value = 6; this.gain.gain.value = 0.05; break;
        case "city": filter.type = "lowpass"; filter.frequency.value = 220; this.gain.gain.value = 0.14; break;
        case "cafe": filter.type = "lowpass"; filter.frequency.value = 700; this.gain.gain.value = 0.1; break;
        default: break;
      }
      this.src.connect(filter).connect(this.gain).connect(ctx.destination);
      // 慢速起伏（海浪/风）
      if (kind === "waves" || kind === "wind") {
        this.lfo = ctx.createOscillator();
        this.lfo.frequency.value = kind === "waves" ? 0.12 : 0.07;
        const lg = ctx.createGain();
        lg.gain.value = this.gain.gain.value * 0.6;
        this.lfo.connect(lg).connect(this.gain.gain);
        this.lfo.start();
      }
      this.src.start();
    } catch {
      this.stop();
    }
  }

  stop(): void {
    try {
      this.lfo?.stop();
      this.src?.stop();
      void this.ctx?.close();
    } catch {
      /* ignore */
    }
    this.lfo = null;
    this.src = null;
    this.ctx = null;
    this.kind = "none";
  }
}
