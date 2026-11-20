import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

export const footerMarker = "<!-- CURSOR_AGENT_PR_BODY";

export function bodyViolations(body) {
  if (!body || !body.includes(footerMarker)) return [];
  return [
    "Pull request body contains an agent footer comment. Remove the footer.",
  ];
}

function readBody(argv, env) {
  const fileFlag = argv.indexOf("--file");
  if (fileFlag === -1) return env.PR_BODY ?? "";
  const path = argv[fileFlag + 1];
  if (!path) throw new Error("Pass a file path after --file.");
  return readFileSync(path, "utf8");
}

function isDirectRun() {
  const entry = process.argv[1];
  return Boolean(entry) && import.meta.url === pathToFileURL(entry).href;
}

if (isDirectRun()) {
  const violations = bodyViolations(readBody(process.argv, process.env));
  if (violations.length > 0) {
    for (const violation of violations) console.error(violation);
    process.exit(1);
  }
}
