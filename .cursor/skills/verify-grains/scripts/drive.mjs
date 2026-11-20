#!/usr/bin/env node
import { spawn, execFileSync } from "node:child_process";
import { createServer } from "node:http";
import { createHash, randomBytes } from "node:crypto";
import { createConnection, createServer as createSocketServer } from "node:net";
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { mkdir, readFile, writeFile, rm } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const skillDir = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const repoRoot = resolve(skillDir, "../../..");
const runDir = join(skillDir, "run");
const socketPath = join(runDir, "drive.sock");
const sessionPath = join(runDir, "session.json");
const toolsDir = join(skillDir, ".tools");
const evidenceRoot = join(skillDir, "evidence");
const evidenceDirs = ["paint", "pause", "erase", "clear", "save-load"].map((name) =>
  join(evidenceRoot, name),
);

const keys = {
  Space: "Space",
  1: "Digit1",
  2: "Digit2",
  3: "Digit3",
  4: "Digit4",
  5: "Digit5",
  6: "Digit6",
  7: "Digit7",
  8: "Digit8",
  9: "Digit9",
  0: "Digit0",
  c: "KeyC",
  s: "KeyS",
  l: "KeyL",
  b: "KeyB",
};

function parseArgs(argv) {
  const out = { _: [] };
  for (let i = 0; i < argv.length; i += 1) {
    const token = argv[i];
    if (!token.startsWith("--")) {
      out._.push(token);
      continue;
    }
    const name = token.slice(2);
    const next = argv[i + 1];
    if (next === undefined || next.startsWith("--")) {
      out[name] = true;
    } else {
      out[name] = next;
      i += 1;
    }
  }
  return out;
}

function repoPath(value) {
  if (!value) return value;
  return resolve(repoRoot, value);
}

function fail(message) {
  console.error(message);
  process.exit(1);
}

function cmdline(pid) {
  try {
    return readFileSync(`/proc/${pid}/cmdline`, "utf8").replaceAll("\0", " ");
  } catch {
    return "";
  }
}

function browserPid(display) {
  for (const entry of readdirSync("/proc")) {
    if (!/^\d+$/.test(entry)) continue;
    const command = cmdline(entry);
    if (!command.includes("chrome") || command.includes("--type=")) continue;
    if (!command.includes("--use-vulkan=native")) continue;
    try {
      const env = readFileSync(`/proc/${entry}/environ`).toString("utf8");
      if (env.includes(`DISPLAY=${display}`)) return Number(entry);
    } catch {
      continue;
    }
  }
  return null;
}

function alive(pid) {
  if (!pid) return false;
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

function pixelDelta(left, right) {
  let max = 0;
  for (let i = 0; i < 3; i += 1) max = Math.max(max, Math.abs(left[i] - right[i]));
  return max;
}

async function call(message) {
  return new Promise((resolvePromise, reject) => {
    const socket = createConnection(socketPath);
    let buffer = "";
    socket.on("data", (chunk) => {
      buffer += chunk;
      if (!buffer.includes("\n")) return;
      socket.end();
      resolvePromise(JSON.parse(buffer));
    });
    socket.on("error", reject);
    socket.write(`${JSON.stringify(message)}\n`);
  });
}

function ensureLavapipe() {
  const root = join(toolsDir, "lavapipe");
  const icd = join(root, "usr/share/vulkan/icd.d/lvp_icd.json");
  const lib = join(root, "usr/lib/x86_64-linux-gnu");
  if (existsSync(icd) && existsSync(join(lib, "libvulkan_lvp.so"))) {
    console.log("lavapipe=cached");
    return { icd, lib };
  }
  mkdirSync(root);
  execFileSync("apt-get", ["download", "mesa-vulkan-drivers", "libdisplay-info3"], {
    cwd: root,
    stdio: "inherit",
  });
  for (const deb of readdirSync(root).filter((name) => name.endsWith(".deb"))) {
    execFileSync("dpkg-deb", ["-x", join(root, deb), root], { stdio: "inherit" });
  }
  if (!existsSync(icd)) fail(`lavapipe ICD missing under ${root}`);
  console.log("lavapipe=installed");
  return { icd, lib };
}

function mkdirSync(path) {
  execFileSync("mkdir", ["-p", path]);
}

async function ensurePlaywright() {
  const root = join(toolsDir, "playwright");
  const entry = join(root, "node_modules/playwright/index.mjs");
  if (existsSync(entry)) {
    console.log("playwright=cached");
    return entry;
  }
  mkdirSync(root);
  execFileSync("npm", ["install", "--silent", "--prefix", root, "playwright@1.62.0"], {
    stdio: "inherit",
  });
  execFileSync("npx", ["--prefix", root, "playwright", "install", "chromium"], {
    stdio: "inherit",
  });
  console.log("playwright=installed");
  return entry;
}

function startServer(dist, port, nonce) {
  const assets = new Map([
    ["/", ["index.html", "text/html"]],
    ["/index.html", ["index.html", "text/html"]],
    ["/grains.js", ["grains.js", "text/javascript"]],
    ["/grains_bg.wasm", ["grains_bg.wasm", "application/wasm"]],
  ]);
  const server = createServer(async (request, response) => {
    const asset = assets.get(request.url.split("?")[0]);
    if (!asset) {
      response.writeHead(404).end();
      return;
    }
    try {
      const data = await readFile(join(dist, asset[0]));
      response.writeHead(200, {
        "Content-Type": asset[1],
        "Content-Length": data.length,
        "X-Grains-Verify": nonce,
      });
      response.end(data);
    } catch {
      response.writeHead(500).end();
    }
  });
  return new Promise((resolvePromise, reject) => {
    server.once("error", reject);
    server.listen(port, "127.0.0.1", () => resolvePromise(server));
  });
}

async function readPixels(page) {
  const pixels = await page.evaluate(() => {
    const value = window.grainsPixels;
    if (!value) return null;
    const copy = (pixel) => (pixel ? Array.from(pixel).slice(0, 4) : null);
    return { corner: copy(value.corner), panel: copy(value.panel), stroke: copy(value.stroke) };
  });
  if (!pixels?.corner || !pixels.panel || !pixels.stroke) {
    throw new Error("grainsPixels is missing a corner, panel, or stroke sample");
  }
  return pixels;
}

async function waitForFrame(page) {
  const canvasCount = await page.locator("#grains").count();
  if (canvasCount !== 1) throw new Error(`#grains matched ${canvasCount} elements`);
  const fallbackCount = await page.locator("#grains-webgpu").count();
  if (fallbackCount !== 0) {
    const text = await page.locator("#grains-webgpu").innerText();
    throw new Error(`WebGPU fallback is showing: ${text}`);
  }
  await page.waitForFunction(
    () => {
      const pixels = window.grainsPixels;
      if (!pixels?.corner || !pixels.panel || !pixels.stroke) return false;
      const spread = (pixel) => Math.max(...pixel.slice(0, 3)) - Math.min(...pixel.slice(0, 3));
      return spread(pixels.corner) > 8 && spread(pixels.panel) <= 3;
    },
    null,
    { timeout: 120000 },
  );
  return readPixels(page);
}

async function openPage(browser, url) {
  const page = await browser.newPage({
    viewport: { width: 1024, height: 600 },
    deviceScaleFactor: 1.5,
    colorScheme: "dark",
  });
  const consoleLines = [];
  page.on("console", (msg) => {
    const text = msg.text();
    consoleLines.push(`${msg.type()}: ${text}`);
    if (text.includes("favicon") || text.includes("404")) return;
    if (msg.type() === "error" || /WGSL|panic|ShaderModule/i.test(text)) {
      page._grainsFailure = `${msg.type()}: ${text}`;
    }
  });
  page.on("pageerror", (err) => {
    page._grainsFailure = err.message;
    consoleLines.push(`pageerror: ${err.message}`);
  });
  page._grainsConsole = consoleLines;
  await page.goto(url, { waitUntil: "domcontentloaded" });
  return page;
}

function assertQuiet(page) {
  if (page._grainsFailure) throw new Error(page._grainsFailure);
}

async function runActions(page, actions, evidenceDir) {
  const state = {
    baseline: null,
    mark: null,
    after: null,
    held: null,
    saved: null,
    trace: [],
    markAt: -1,
  };
  await mkdir(evidenceDir, { recursive: true });
  for (const action of actions) {
    assertQuiet(page);
    state.trace.push(action);
    if (action === "reload") {
      await page.goto(page.url(), { waitUntil: "domcontentloaded" });
      state.baseline = null;
      state.mark = null;
      state.after = null;
      state.held = null;
      continue;
    }
    if (action === "wait-input") {
      await page.locator("#grains[tabindex]").waitFor({ state: "attached", timeout: 30000 });
      const canvasCount = await page.locator("#grains").count();
      const inputCount = await page.locator("#grains[tabindex]").count();
      if (canvasCount !== 1 || inputCount !== 1) {
        throw new Error(`#grains matched ${canvasCount} and #grains[tabindex] matched ${inputCount}`);
      }
      await page.locator("#grains[tabindex]").focus();
      continue;
    }
    if (action === "wait-frame") {
      state.ready = await waitForFrame(page);
      continue;
    }
    if (action.startsWith("key:")) {
      const name = action.slice(4);
      const key = keys[name];
      if (!key) throw new Error(`unknown key ${name}`);
      await page.keyboard.press(key);
      continue;
    }
    if (action === "settle") {
      const first = await readPixels(page);
      await page.waitForTimeout(400);
      const second = await readPixels(page);
      for (const spot of ["corner", "panel", "stroke"]) {
        const delta = pixelDelta(first[spot], second[spot]);
        if (delta > 5) throw new Error(`${spot} still moving by ${delta} before the stroke`);
      }
      state.baseline = second;
      continue;
    }
    if (action.startsWith("drag:") || action.startsWith("erase:")) {
      if (!state.baseline) throw new Error("settle did not run before the pointer move");
      const erase = action.startsWith("erase:");
      const points = action
        .split(":")
        .slice(1)
        .map((value) => Number(value));
      if (points.length !== 6 || points.some((value) => Number.isNaN(value))) {
        throw new Error(`pointer action needs six numbers: ${action}`);
      }
      if (erase) await page.keyboard.down("Shift");
      await page.mouse.move(points[0], points[1]);
      if (erase) await page.mouse.down({ button: "right" });
      else await page.mouse.down();
      await page.waitForTimeout(500);
      await page.mouse.move(points[2], points[3], { steps: 20 });
      await page.mouse.move(points[4], points[5], { steps: 8 });
      const previous = state.baseline.stroke;
      await page.waitForFunction(
        (before) =>
          window.grainsPixels.stroke.some((channel, index) => Math.abs(channel - before[index]) > 20),
        previous,
        { timeout: 30000 },
      );
      state.after = await readPixels(page);
      if (erase) await page.mouse.up({ button: "right" });
      else await page.mouse.up();
      if (erase) await page.keyboard.up("Shift");
      continue;
    }
    if (action === "assert-paint") {
      if (!state.baseline || !state.after) throw new Error("the stroke was not sampled");
      await page.waitForTimeout(400);
      state.held = await readPixels(page);
      const stroke = pixelDelta(state.baseline.stroke, state.held.stroke);
      const corner = pixelDelta(state.baseline.corner, state.held.corner);
      const panel = pixelDelta(state.baseline.panel, state.held.panel);
      if (corner > 5 || panel > 5) {
        throw new Error(
          `corner moved ${corner} and panel moved ${panel}, so this is not a brush mark`,
        );
      }
      if (stroke <= 20) throw new Error(`stroke pixel changed by ${stroke}, which is not a mark`);
      state.mark = state.held.stroke;
      state.markAt = state.trace.length;
      continue;
    }
    if (action === "assert-falls") {
      if (!state.mark || !state.baseline) throw new Error("assert-paint did not record a mark");
      await page.waitForFunction(
        (mark) =>
          window.grainsPixels.stroke.some((channel, index) => Math.abs(channel - mark[index]) > 20),
        state.mark,
        { timeout: 8000 },
      );
      await page.waitForTimeout(400);
      const now = await readPixels(page);
      const stroke = pixelDelta(state.mark, now.stroke);
      const corner = pixelDelta(state.baseline.corner, now.corner);
      if (stroke <= 20) throw new Error("the sand mark did not leave the sample after resume");
      if (corner > 5) throw new Error(`corner moved ${corner} during the fall`);
      state.fell = now;
      continue;
    }
    if (action === "assert-cleared" || action === "assert-erased") {
      if (!state.baseline || !state.mark) throw new Error("assert-paint did not record a mark");
      const later = state.trace.slice(state.markAt);
      const erased = later.some((item) => item.startsWith("erase:"));
      const cleared = later.includes("key:c");
      const painted = later.some((item) => item.startsWith("drag:"));
      if (action === "assert-cleared" && (!cleared || erased || painted)) {
        throw new Error("clear only counts when KeyC is the edit after the mark");
      }
      if (action === "assert-erased" && (!erased || cleared)) {
        throw new Error("erase only counts when the shift-right drag is the edit after the mark");
      }
      await page.waitForFunction(
        (sky) =>
          window.grainsPixels.stroke.every((channel, index) => Math.abs(channel - sky[index]) <= 20),
        state.baseline.stroke,
        { timeout: 10000 },
      );
      const now = await readPixels(page);
      const fromMark = pixelDelta(state.mark, now.stroke);
      const corner = pixelDelta(state.baseline.corner, now.corner);
      if (fromMark <= 20) throw new Error("the mark is still in the stroke sample");
      if (corner > 5) throw new Error(`corner moved ${corner}`);
      state.restored = now;
      continue;
    }
    if (action === "save") {
      const downloadPromise = page.waitForEvent("download", { timeout: 20000 });
      await page.keyboard.press("KeyS");
      const download = await downloadPromise;
      if (download.suggestedFilename() !== "world.grains") {
        throw new Error(`download name is ${download.suggestedFilename()}`);
      }
      state.saved = join(evidenceDir, "world.grains");
      await download.saveAs(state.saved);
      const bytes = await readFile(state.saved);
      if (bytes.subarray(0, 4).toString() !== "GRNS") throw new Error("saved file magic is not GRNS");
      continue;
    }
    if (action === "load") {
      if (!state.saved) throw new Error("save did not run");
      const chooserPromise = page.waitForEvent("filechooser", { timeout: 10000 });
      await page.keyboard.press("KeyL");
      const chooser = await chooserPromise;
      await chooser.setFiles(state.saved);
      continue;
    }
    if (action === "assert-mark") {
      if (!state.mark) throw new Error("assert-paint did not record a mark");
      await page.waitForFunction(
        (mark) =>
          window.grainsPixels.stroke.every((channel, index) => Math.abs(channel - mark[index]) <= 20),
        state.mark,
        { timeout: 20000 },
      );
      state.reloaded = await readPixels(page);
      continue;
    }
    throw new Error(`unknown action ${action}`);
  }
  assertQuiet(page);
  const beforeShot = await page.screenshot({ type: "png" });
  const beforeName = join(evidenceDir, "screenshot.png");
  await writeFile(beforeName, beforeShot);
  const result = {
    pass: true,
    url: page.url(),
    actions,
    baseline: state.baseline,
    after: state.after,
    held: state.held,
    fell: state.fell ?? null,
    restored: state.restored ?? null,
    reloaded: state.reloaded ?? null,
    screenshotBytes: beforeShot.length,
    screenshotSha256: createHash("sha256").update(beforeShot).digest("hex"),
  };
  await writeFile(join(evidenceDir, "pixels.json"), `${JSON.stringify(result, null, 2)}\n`);
  await writeFile(join(evidenceDir, "console.txt"), `${page._grainsConsole.join("\n")}\n`);
  return result;
}

async function launch(options) {
  const started = Date.now();
  const dist = repoPath(options.dist || "dist");
  const port = Number(options.port || 8876);
  const display = options.display || ":97";
  if (!existsSync(join(dist, "grains.js")) || !existsSync(join(dist, "grains_bg.wasm"))) {
    fail(`dist is missing grains.js. Build it with the commands in SKILL.md. Looked in ${dist}`);
  }
  if (existsSync(sessionPath)) fail(`a session already exists at ${sessionPath}`);
  if (existsSync(`/tmp/.X11-unix/X${display.slice(1)}`)) fail(`display ${display} is already in use`);
  const lavapipe = ensureLavapipe();
  const playwrightEntry = await ensurePlaywright();
  const { chromium } = await import(pathToFileURL(playwrightEntry).href);
  const nonce = randomBytes(8).toString("hex");
  const url = `http://127.0.0.1:${port}/?probe=1`;
  await mkdir(runDir, { recursive: true });
  const xvfb = spawn("Xvfb", [display, "-screen", "0", "1280x800x24"], { stdio: "ignore" });
  let server;
  let browser;
  let page;
  const shutdown = async () => {
    const chrome = browserPid(display);
    if (chrome) {
      try {
        process.kill(chrome, "SIGTERM");
      } catch {}
    }
    if (xvfb.pid) {
      try {
        xvfb.kill("SIGTERM");
      } catch {}
    }
    server?.close();
    await Promise.race([
      browser?.close().catch(() => {}),
      new Promise((resolvePromise) => setTimeout(resolvePromise, 1000)),
    ]);
    await rm(socketPath, { force: true });
    await rm(sessionPath, { force: true });
  };
  process.on("SIGINT", () => {
    shutdown().finally(() => process.exit(0));
  });
  process.on("SIGTERM", () => {
    shutdown().finally(() => process.exit(0));
  });
  try {
    await new Promise((resolvePromise) => setTimeout(resolvePromise, 300));
    server = await startServer(dist, port, nonce);
    browser = await chromium.launch({
      headless: false,
      env: {
        ...process.env,
        DISPLAY: display,
        VK_DRIVER_FILES: lavapipe.icd,
        VK_ICD_FILENAMES: lavapipe.icd,
        LD_LIBRARY_PATH: `${lavapipe.lib}:${process.env.LD_LIBRARY_PATH ?? ""}`,
      },
      args: [
        "--enable-unsafe-webgpu",
        "--enable-features=Vulkan",
        "--use-angle=vulkan",
        "--use-vulkan=native",
        "--disable-gpu-watchdog",
        "--ignore-gpu-blocklist",
        "--disable-gpu-sandbox",
        "--no-sandbox",
      ],
    });
    page = await openPage(browser, url);
    await page.locator("#grains[tabindex]").waitFor({ state: "attached", timeout: 30000 });
    const pixels = await waitForFrame(page);
    const session = {
      pid: process.pid,
      xvfb: xvfb.pid,
      browser: browserPid(display),
      port,
      display,
      url,
      nonce,
      dist,
    };
    await writeFile(sessionPath, `${JSON.stringify(session, null, 2)}\n`);
    const socketServer = createSocketServer((socket) => {
      let buffer = "";
      socket.on("data", (chunk) => {
        buffer += chunk;
        if (!buffer.includes("\n")) return;
        const message = JSON.parse(buffer);
        buffer = "";
        handle(message)
          .then((response) => {
            socket.write(`${JSON.stringify(response)}\n`);
            if (message.op === "cleanup") {
              setImmediate(() => shutdown().finally(() => process.exit(0)));
            }
          })
          .catch((err) => {
            socket.end(`${JSON.stringify({ ok: false, error: String(err) })}\n`);
          });
      });
    });
    await mkdir(runDir, { recursive: true });
    await rm(socketPath, { force: true });
    await new Promise((resolvePromise) => socketServer.listen(socketPath, resolvePromise));
    const setupMs = Date.now() - started;
    console.log(
      `ready url=${url} corner=${pixels.corner.slice(0, 3).join(",")} panel=${pixels.panel.slice(0, 3).join(",")} stroke=${pixels.stroke.slice(0, 3).join(",")} setup_ms=${setupMs}`,
    );
    await new Promise(() => {});
  } catch (err) {
    console.error(String(err));
    if (page?._grainsConsole) console.error(page._grainsConsole.slice(-20).join("\n"));
    await shutdown();
    process.exit(1);
  }

  async function handle(message) {
    if (message.op === "doctor") {
      const pixels = await readPixels(page);
      const canvasCount = await page.locator("#grains").count();
      const fallbackCount = await page.locator("#grains-webgpu").count();
      return { ok: true, pixels, canvasCount, fallbackCount, url: page.url() };
    }
    if (message.op === "run") {
      const evidenceDir = repoPath(message.evidence);
      const actions = message.actions;
      const result = await runActions(page, actions, evidenceDir);
      return { ok: true, evidence: evidenceDir, result };
    }
    if (message.op === "cleanup") return { ok: true };
    throw new Error(`unknown op ${message.op}`);
  }
}

async function doctor() {
  if (!existsSync(sessionPath)) fail("no session. Launch first.");
  const session = JSON.parse(await readFile(sessionPath, "utf8"));
  if (!alive(session.pid) || !cmdline(session.pid).includes("drive.mjs")) {
    fail(`session pid ${session.pid} is not drive.mjs`);
  }
  const response = await fetch(session.url);
  const header = response.headers.get("x-grains-verify");
  const html = await response.text();
  const wasm = await fetch(new URL("/grains_bg.wasm", session.url));
  const wasmBody = Buffer.from(await wasm.arrayBuffer());
  const wasmBytes = wasmBody.length;
  const canvasCount = html.split('id="grains"').length - 1;
  if (header !== session.nonce) fail("the port is not the verify server");
  if (canvasCount !== 1) fail(`the page has ${canvasCount} canvas id grains`);
  if (wasm.status !== 200 || wasmBytes < 100000) fail(`wasm status ${wasm.status} size ${wasmBytes}`);
  const live = await call({ op: "doctor" });
  if (!live.ok) fail(live.error);
  if (live.canvasCount !== 1 || live.fallbackCount !== 0) {
    fail(`page has ${live.canvasCount} canvases and ${live.fallbackCount} fallback nodes`);
  }
  const spread = (pixel) => Math.max(...pixel.slice(0, 3)) - Math.min(...pixel.slice(0, 3));
  if (spread(live.pixels.corner) <= 8 || spread(live.pixels.panel) > 3) {
    fail(`frame is not ready corner=${live.pixels.corner} panel=${live.pixels.panel}`);
  }
  console.log(
    `doctor ok url=${session.url} pid=${session.pid} port=${session.port} canvas=1 wasm=${wasmBytes}`,
  );
}

async function run(options) {
  const started = Date.now();
  if (!options.evidence || !options.actions) fail("run needs --evidence and --actions");
  const actions = options.actions.split(",").filter(Boolean);
  const response = await call({ op: "run", evidence: options.evidence, actions });
  if (!response.ok) fail(response.error);
  const verifyMs = Date.now() - started;
  console.log(`pass evidence=${response.evidence} verify_ms=${verifyMs}`);
}

async function cleanup() {
  if (existsSync(socketPath)) {
    await call({ op: "cleanup" }).catch(() => {});
  }
  if (existsSync(sessionPath)) await killSession();
  for (const dir of evidenceDirs) {
    console.log(`evidence ${dir}`);
  }
  console.log("cleanup ok");
}

async function killSession() {
  const session = JSON.parse(readFileSync(sessionPath, "utf8"));
  for (const [pid, needle] of [
    [session.browser, "chrom"],
    [session.xvfb, session.display],
    [session.pid, "drive.mjs"],
  ]) {
    if (!alive(pid)) continue;
    if (!cmdline(pid).includes(needle)) continue;
    process.kill(pid, "SIGTERM");
  }
  await rm(socketPath, { force: true });
  await rm(sessionPath, { force: true });
}

const options = parseArgs(process.argv.slice(3));
const command = process.argv[2];
if (command === "launch") await launch(options);
else if (command === "doctor") await doctor();
else if (command === "run") await run(options);
else if (command === "cleanup") await cleanup();
else {
  fail("usage: drive.mjs launch|doctor|run|cleanup");
}
