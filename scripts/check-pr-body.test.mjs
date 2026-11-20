import assert from "node:assert/strict";
import test from "node:test";
import { bodyViolations, footerMarker } from "./check-pr-body.mjs";

test("pull requests 14 and 17 fail on the agent footer comment", () => {
  const violations = bodyViolations(`Summary\n${footerMarker}_BEGIN -->\n`);
  assert.equal(violations.length, 1);
  assert.match(violations[0], /agent footer comment/);
  assert.match(violations[0], /Remove the footer/);
});

test("an ordinary pull request body passes", () => {
  assert.deepEqual(bodyViolations("Explain the change and the check you ran."), []);
});

test("prose that names the footer marker passes", () => {
  assert.deepEqual(bodyViolations("The check rejects a CURSOR_AGENT_PR_BODY comment."), []);
});
