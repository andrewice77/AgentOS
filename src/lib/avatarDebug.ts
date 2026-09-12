import { invoke } from "@tauri-apps/api/core";

export type Pixel = { r: number; g: number; b: number; a: number };

export interface BufferTest {
  name: string;
  /** True if the old sprite pixel was overwritten in the *canvas bitmap*. */
  cleared: boolean;
  oldPixel: Pixel;
  note: string;
}

export interface AvatarProbeReport {
  when: string;
  ua: string;
  dpr: number;
  inner: [number, number];
  overlay: boolean;
  chromaFilter: boolean;
  present: Record<string, unknown> | null;
  gl: { vendor: string; renderer: string } | null;
  tests: BufferTest[];
  /** BUFFER_TRAILS = our canvas is dirty. COMPOSITOR_TRAILS = canvas is clean, window is not. */
  verdict: "BUFFER_TRAILS" | "COMPOSITOR_TRAILS" | "INCONCLUSIVE";
}

function px(d: Uint8ClampedArray): Pixel {
  return { r: d[0], g: d[1], b: d[2], a: d[3] };
}

function isMagenta(p: Pixel): boolean {
  return p.r > 200 && p.b > 200 && p.g < 40;
}

function isWhite(p: Pixel): boolean {
  return p.r > 220 && p.g > 220 && p.b > 220;
}

export async function avatarLog(message: string, level: "info" | "warn" | "error" = "info") {
  try {
    await invoke("push_debug_log", { level, message: `avatar:${message}` });
  } catch {
    console[level === "error" ? "error" : "info"](`avatar:${message}`);
  }
}

function test2dOpaque(): BufferTest {
  const c = document.createElement("canvas");
  c.width = 64;
  c.height = 64;
  const ctx = c.getContext("2d", { alpha: false });
  if (!ctx) {
    return {
      name: "2d-opaque",
      cleared: false,
      oldPixel: { r: 0, g: 0, b: 0, a: 0 },
      note: "context missing",
    };
  }
  ctx.fillStyle = "#ff00ff";
  ctx.fillRect(0, 0, 64, 64);
  ctx.fillStyle = "#ffffff";
  ctx.fillRect(8, 8, 10, 10);
  ctx.fillStyle = "#ff00ff";
  ctx.fillRect(0, 0, 64, 64);
  ctx.fillStyle = "#ffffff";
  ctx.fillRect(40, 40, 10, 10);
  const oldPixel = px(ctx.getImageData(12, 12, 1, 1).data);
  return {
    name: "2d-opaque",
    cleared: isMagenta(oldPixel),
    oldPixel,
    note: "old white square must become magenta after full fillRect",
  };
}

function test2dAlpha(): BufferTest {
  const c = document.createElement("canvas");
  c.width = 64;
  c.height = 64;
  const ctx = c.getContext("2d", { alpha: true });
  if (!ctx) {
    return {
      name: "2d-alpha",
      cleared: false,
      oldPixel: { r: 0, g: 0, b: 0, a: 0 },
      note: "context missing",
    };
  }
  ctx.clearRect(0, 0, 64, 64);
  ctx.fillStyle = "#ffffff";
  ctx.fillRect(8, 8, 10, 10);
  ctx.clearRect(0, 0, 64, 64);
  ctx.fillStyle = "#ffffff";
  ctx.fillRect(40, 40, 10, 10);
  const oldPixel = px(ctx.getImageData(12, 12, 1, 1).data);
  return {
    name: "2d-alpha",
    cleared: oldPixel.a < 8,
    oldPixel,
    note: "old white square must be alpha 0 after clearRect",
  };
}

function testGlOpaque(): BufferTest {
  const c = document.createElement("canvas");
  c.width = 64;
  c.height = 64;
  const gl = c.getContext("webgl", { alpha: false, preserveDrawingBuffer: true });
  if (!gl) {
    return {
      name: "gl-opaque",
      cleared: false,
      oldPixel: { r: 0, g: 0, b: 0, a: 0 },
      note: "webgl missing",
    };
  }
  gl.viewport(0, 0, 64, 64);
  gl.clearColor(1, 0, 1, 1);
  gl.clear(gl.COLOR_BUFFER_BIT);
  gl.enable(gl.SCISSOR_TEST);
  gl.scissor(8, 8, 10, 10);
  gl.clearColor(1, 1, 1, 1);
  gl.clear(gl.COLOR_BUFFER_BIT);
  gl.disable(gl.SCISSOR_TEST);
  gl.clearColor(1, 0, 1, 1);
  gl.clear(gl.COLOR_BUFFER_BIT);
  gl.enable(gl.SCISSOR_TEST);
  gl.scissor(40, 40, 10, 10);
  gl.clearColor(1, 1, 1, 1);
  gl.clear(gl.COLOR_BUFFER_BIT);
  const buf = new Uint8Array(4);
  gl.pixelStorei(gl.PACK_ALIGNMENT, 1);
  gl.readPixels(12, 12, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, buf);
  const oldPixel = { r: buf[0], g: buf[1], b: buf[2], a: buf[3] };
  return {
    name: "gl-opaque",
    cleared: isMagenta(oldPixel),
    oldPixel,
    note: "WebGL clear of full buffer must wipe the old white scissor",
  };
}

function glInfo(): { vendor: string; renderer: string } | null {
  const c = document.createElement("canvas");
  const gl = c.getContext("webgl");
  if (!gl) return null;
  const ext = gl.getExtension("WEBGL_debug_renderer_info");
  if (!ext) {
    return { vendor: gl.getParameter(gl.VENDOR), renderer: gl.getParameter(gl.RENDERER) };
  }
  return {
    vendor: String(gl.getParameter(ext.UNMASKED_VENDOR_WEBGL)),
    renderer: String(gl.getParameter(ext.UNMASKED_RENDERER_WEBGL)),
  };
}

function presentSnapshot(): Record<string, unknown> | null {
  const hook = (window as unknown as { __agentosAvatarDebug?: { snapshot: () => Record<string, unknown> } })
    .__agentosAvatarDebug;
  if (hook?.snapshot) return hook.snapshot();
  const stage = document.querySelector(".present-wrap canvas") as HTMLCanvasElement | null;
  if (!stage) return null;
  return { w: stage.width, h: stage.height, cssW: stage.clientWidth, cssH: stage.clientHeight };
}

export function probeAvatarCompositor(): AvatarProbeReport {
  const tests = [test2dOpaque(), test2dAlpha(), testGlOpaque()];
  const opaqueOk = tests.filter((t) => t.name !== "2d-alpha").every((t) => t.cleared);
  const verdict: AvatarProbeReport["verdict"] = opaqueOk ? "COMPOSITOR_TRAILS" : "BUFFER_TRAILS";

  return {
    when: new Date().toISOString(),
    ua: navigator.userAgent,
    dpr: window.devicePixelRatio || 1,
    inner: [window.innerWidth, window.innerHeight],
    overlay: !!document.querySelector(".float-root.overlay"),
    chromaFilter: !!document.getElementById("agentos-buddy-chroma"),
    present: presentSnapshot(),
    gl: glInfo(),
    tests,
    verdict,
  };
}

export async function runAndLogAvatarProbe(reason: string) {
  const report = probeAvatarCompositor();
  let desktop: { win_w: number; win_h: number } | null = null;
  try {
    desktop = await invoke<{ win_w: number; win_h: number }>("get_avatar_desktop");
  } catch {
    /* not in avatar shell */
  }
  await avatarLog(`probe reason=${reason} verdict=${report.verdict}`);
  await avatarLog(`probe ua=${report.ua}`);
  await avatarLog(
    `probe dpr=${report.dpr} inner=${report.inner.join("x")} overlay=${report.overlay} chroma=${report.chromaFilter}`,
  );
  if (desktop) {
    await avatarLog(`probe desktop win=${desktop.win_w}x${desktop.win_h}`);
  }
  if (report.gl) {
    await avatarLog(`probe gl vendor=${report.gl.vendor} renderer=${report.gl.renderer}`);
  }
  await avatarLog(`probe present=${JSON.stringify(report.present)}`);
  for (const t of report.tests) {
    await avatarLog(
      `probe test ${t.name} cleared=${t.cleared} pixel=${t.oldPixel.r},${t.oldPixel.g},${t.oldPixel.b},${t.oldPixel.a} (${t.note})`,
    );
  }
  if (report.verdict === "COMPOSITOR_TRAILS") {
    await avatarLog(
      "probe hint: buffer puliti. Se vedi ancora cloni, restano pixel alpha nella finestra. Su Linux la mode attesa è opaque-window; su Windows/macOS è glass.",
      "warn",
    );
  } else if (report.verdict === "BUFFER_TRAILS") {
    await avatarLog(
      "probe hint: il framebuffer 2D/WebGL NON si pulisce. I cloni nascono prima del compositor.",
      "warn",
    );
  }
  return report;
}
