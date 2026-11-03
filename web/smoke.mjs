// Headed smoke test. A displayed WebGPU canvas drops draws that arrive in a
// later submit, which is how the panel disappeared, so this runs under Xvfb
// rather than headless. Screenshots stay empty; the page publishes three
// canvas pixels at window.grainsPixels when opened with ?probe=1.
import { spawn } from "node:child_process";
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { join } from "node:path";

const { chromium } = await import(process.env.PLAYWRIGHT_MODULE ?? "playwright");

const root = process.env.GRAINS_WEB;
if (!root) {
  console.error("GRAINS_WEB must point at the wasm-bindgen output");
  process.exit(1);
}

const display = process.env.GRAINS_DISPLAY || ":98";
const port = process.env.GRAINS_PORT || "8876";
const linux = process.platform === "linux";
const xvfb = linux
  ? spawn("Xvfb", [display, "-screen", "0", "1280x800x24"], { stdio: "ignore" })
  : null;
const assets = new Map([
  ["/", ["index.html", "text/html"]],
  ["/grains.js", ["grains.js", "text/javascript"]],
  ["/grains_bg.wasm", ["grains_bg.wasm", "application/wasm"]],
]);
const server = createServer(async (request, response) => {
  const asset = assets.get(request.url.split("?")[0]);
  if (!asset) return response.writeHead(404).end();
  try {
    const data = await readFile(join(root, asset[0]));
    response.writeHead(200, { "Content-Type": asset[1] }).end(data);
  } catch {
    response.writeHead(500).end();
  }
});
await new Promise((resolve) => server.listen(Number(port), "127.0.0.1", resolve));
if (linux) await new Promise((resolve) => setTimeout(resolve, 800));

const failures = [];
const consoleLines = [];
let page;
let browser;
try {
  browser = await chromium.launch({
    headless: false,
    channel: process.platform === "win32" ? "chrome" : undefined,
    env: {
      ...process.env,
      DISPLAY: display,
    },
    args: [
      "--enable-unsafe-webgpu",
      ...(linux ? ["--enable-features=Vulkan", "--use-angle=vulkan", "--use-vulkan=native"] : []),
      "--ignore-gpu-blocklist",
      "--disable-gpu-sandbox",
      "--no-sandbox",
    ],
  });
  page = await browser.newPage({
    viewport: { width: 1024, height: 600 },
    deviceScaleFactor: 1.5,
    colorScheme: "dark",
  });
  page.on("console", (msg) => {
    const text = msg.text();
    consoleLines.push(`${msg.type()}: ${text}`);
    if (text.includes("favicon") || text.includes("404")) {
      return;
    }
    if (msg.type() === "error" || /WGSL|panic|ShaderModule/i.test(text)) {
      failures.push(`${msg.type()}: ${text}`);
    }
  });
  page.on("pageerror", (err) => failures.push(err.message));
  await page.goto(`http://127.0.0.1:${port}/?probe=1`, {
    waitUntil: "domcontentloaded",
  });
  // winit makes the canvas focusable when it installs its input handlers.
  // Pause before waiting for readback so the brush probe is deterministic
  // even when software WebGPU takes several seconds to render a frame.
  await page.locator("#grains[tabindex]").focus();
  await page.keyboard.press("Space");
  await page.keyboard.press("2");
  await page.waitForFunction(
    () => {
      const pixels = window.grainsPixels;
      if (!pixels?.corner || !pixels.panel || !pixels.stroke) return false;
      // The sky is blue-tinted. The panel fill is neutral in both egui themes.
      const spread = (pixel) => Math.max(...pixel.slice(0, 3)) - Math.min(...pixel.slice(0, 3));
      return spread(pixels.corner) > 8 && spread(pixels.panel) <= 3;
    },
    null,
    { timeout: 120000 },
  );
  const before = await page.evaluate(() => window.grainsPixels.stroke.slice());
  await page.mouse.move(700, 300);
  await page.mouse.down();
  await page.waitForTimeout(500);
  await page.mouse.move(760, 360, { steps: 20 });
  await page.mouse.move(748, 348);
  await page.waitForFunction(
    (previous) =>
      window.grainsPixels.stroke.some((channel, index) => Math.abs(channel - previous[index]) > 20),
    before,
    { timeout: 30000 },
  );
  await page.mouse.up();

  const unavailable = await browser.newPage();
  unavailable.on("pageerror", (err) => failures.push(err.message));
  await unavailable.addInitScript(() => {
    const getContext = HTMLCanvasElement.prototype.getContext;
    HTMLCanvasElement.prototype.getContext = function (kind, ...options) {
      return kind === "webgpu" ? null : getContext.call(this, kind, ...options);
    };
  });
  await unavailable.goto(`http://127.0.0.1:${port}/`);
  await unavailable.locator("#grains-webgpu").waitFor({ state: "visible", timeout: 15000 });
  await unavailable.close();
} catch (err) {
  failures.push(String(err));
  const dump = await page
    ?.evaluate(() => ({
      pixels: window.grainsPixels ?? null,
      search: location.search,
      width: document.querySelector("#grains")?.width ?? 0,
      height: document.querySelector("#grains")?.height ?? 0,
    }))
    .catch((dumpErr) => String(dumpErr));
  failures.push(JSON.stringify(dump));
  failures.push(consoleLines.slice(-15).join("\n"));
} finally {
  await browser?.close();
  xvfb?.kill();
  server.close();
}

if (failures.length > 0) {
  console.error(failures.join("\n"));
  process.exit(1);
}
console.log("web smoke ok");
