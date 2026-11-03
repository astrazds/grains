// Headed smoke test. A displayed WebGPU canvas drops draws that arrive in a
// later submit, which is how the panel disappeared, so this runs under Xvfb
// rather than headless. Screenshots stay empty; the page publishes three
// canvas pixels at window.grainsPixels when opened with ?probe=1.
import { spawn } from "node:child_process";

const { chromium } = await import(process.env.PLAYWRIGHT_MODULE ?? "playwright");

const root = process.env.GRAINS_WEB;
if (!root) {
  console.error("GRAINS_WEB must point at the wasm-bindgen output");
  process.exit(1);
}

const display = process.env.GRAINS_DISPLAY || ":98";
const port = process.env.GRAINS_PORT || "8876";
const children = [];

function start(command, args, options = {}) {
  const child = spawn(command, args, { stdio: "ignore", ...options });
  children.push(child);
  return child;
}

function stop() {
  for (const child of children) {
    child.kill("SIGTERM");
  }
}

start("Xvfb", [display, "-screen", "0", "1280x800x24"]);
start("python3", ["-m", "http.server", port, "--bind", "127.0.0.1"], { cwd: root });
await new Promise((resolve) => setTimeout(resolve, 800));

const failures = [];
const browser = await chromium.launch({
  headless: false,
  env: {
    ...process.env,
    DISPLAY: display,
    VK_DRIVER_FILES: process.env.VK_DRIVER_FILES ?? "",
    LD_LIBRARY_PATH: process.env.LD_LIBRARY_PATH ?? "",
  },
  args: [
    "--enable-unsafe-webgpu",
    "--enable-features=Vulkan",
    "--use-angle=vulkan",
    "--use-vulkan=native",
    "--ignore-gpu-blocklist",
    "--disable-gpu-sandbox",
    "--no-sandbox",
  ],
});

const consoleLines = [];
let page;
try {
  page = await browser.newPage({ viewport: { width: 1024, height: 600 } });
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
  await page.waitForFunction(
    () => {
      const pixels = window.grainsPixels;
      // Corner is the tonemapped sky, so the frame was drawn.
      // The panel pixel has to be the light egui window, not that sky.
      if (!pixels || pixels.corner[0] >= 80) {
        return false;
      }
      return pixels.panel[0] > 160 && pixels.panel[1] > 160 && pixels.panel[2] > 160;
    },
    null,
    { timeout: 120000 },
  );
  await page.mouse.click(80, 20);
  await page.keyboard.press("Space");
  await page.keyboard.press("2");
  const before = await page.evaluate(() => window.grainsPixels.stroke.slice());
  await page.mouse.move(700, 300);
  await page.mouse.down();
  await page.mouse.move(760, 360, { steps: 12 });
  await page.mouse.up();
  await page.waitForFunction(
    (previous) =>
      window.grainsPixels.stroke.some((channel, index) => Math.abs(channel - previous[index]) > 20),
    before,
    { timeout: 30000 },
  );
} catch (err) {
  failures.push(String(err));
  const dump = await page
    .evaluate(() => ({
      pixels: window.grainsPixels ?? null,
      search: location.search,
      width: document.querySelector("#grains")?.width ?? 0,
      height: document.querySelector("#grains")?.height ?? 0,
    }))
    .catch((dumpErr) => String(dumpErr));
  failures.push(JSON.stringify(dump));
  failures.push(consoleLines.slice(-15).join("\n"));
} finally {
  await browser.close();
  stop();
}

if (failures.length > 0) {
  console.error(failures.join("\n"));
  process.exit(1);
}
console.log("web smoke ok");
