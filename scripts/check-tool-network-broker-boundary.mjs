#!/usr/bin/env node
// check-tool-network-broker-boundary.mjs
//
// Scans production Rust source under crates/jyowo-harness-tool/src/ for
// direct reqwest::Client or reqwest::ClientBuilder construction outside the
// authorized HTTP broker module (network_broker.rs). Ignores #[cfg(test)]
// blocks and test directories.
//
// Exit 0 when boundary is clean, 1 when violations are found.

import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, extname } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = fileURLToPath(new URL("..", import.meta.url));
const TOOL_SRC = join(ROOT, "crates", "jyowo-harness-tool", "src");
const BROKER_FILE = "network_broker.rs";

const REQWEST_PATTERNS = [
  // Direct construction
  { pattern: /\breqwest::Client::(builder|new)\s*\(/g, label: "reqwest::Client construction" },
  // ClientBuilder construction
  { pattern: /\breqwest::ClientBuilder::new\s*\(/g, label: "reqwest::ClientBuilder construction" },
  // Import of reqwest::Client or ClientBuilder (catches aliases via `use reqwest::Client as Foo`)
  { pattern: /^\s*use\s+reqwest::(Client|ClientBuilder)\b/gm, label: "reqwest::Client / ClientBuilder import" },
];

const violations = [];

function scanFile(filePath) {
  const content = readFileSync(filePath, "utf-8");
  const lines = content.split("\n");

  // Remove #[cfg(test)] blocks — replace with blank lines to preserve line numbers.
  const stripped = stripTestBlocks(lines);

  for (const { pattern, label } of REQWEST_PATTERNS) {
    // Reset lastIndex for global regex.
    pattern.lastIndex = 0;

    // Check each line.
    for (let i = 0; i < stripped.length; i++) {
      const line = stripped[i];
      pattern.lastIndex = 0;
      const match = pattern.exec(line);
      if (match) {
        violations.push({
          file: filePath,
          line: i + 1,
          label,
          snippet: lines[i].trim(),
        });
      }
    }
  }
}

function stripTestBlocks(lines) {
  const result = [...lines];
  let depth = 0;
  let inTest = false;

  for (let i = 0; i < result.length; i++) {
    const line = result[i];

    // Track #[cfg(test)] attribute and mod tests blocks.
    if (/^\s*#\[cfg\s*\(\s*test\s*\)\]/.test(line)) {
      inTest = true;
      depth = 1;
      result[i] = "";
      continue;
    }

    if (inTest) {
      // Count braces to find end of test block
      const openBraces = (line.match(/\{/g) || []).length;
      const closeBraces = (line.match(/\}/g) || []).length;
      depth += openBraces - closeBraces;
      result[i] = "";
      if (depth <= 0) {
        inTest = false;
        depth = 0;
      }
      continue;
    }

    // Also skip `mod tests {` blocks
    if (/^\s*mod\s+tests\s*\{/.test(line)) {
      inTest = true;
      depth = 1;
      result[i] = "";
      continue;
    }
  }

  return result;
}

function walkDir(dir) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const fullPath = join(dir, entry.name);
    if (entry.isDirectory()) {
      // Skip test directories and the broker module.
      if (entry.name === "tests" || entry.name === "test") continue;
      walkDir(fullPath);
    } else if (entry.isFile() && extname(entry.name) === ".rs") {
      // Skip the broker module itself.
      if (entry.name === BROKER_FILE) continue;
      // Skip test files (file names ending in _test or in test directories).
      if (fullPath.includes("/tests/")) continue;
      scanFile(fullPath);
    }
  }
}

// Also scan provider_media.rs since it's in scope.
walkDir(TOOL_SRC);

if (violations.length > 0) {
  console.error("Network broker boundary violations found:\n");
  for (const v of violations) {
    const relPath = v.file.replace(ROOT + "/", "");
    console.error(`  ${relPath}:${v.line} — ${v.label}`);
    console.error(`    ${v.snippet}`);
  }
  console.error(`\n${violations.length} violation(s) found.`);
  console.error(
    "Production code outside network_broker.rs must not construct or import reqwest::Client."
  );
  console.error(
    "Use ToolNetworkBrokerCap::execute_json() for authorized HTTP dispatch."
  );
  process.exit(1);
}

console.log("Network broker boundary clean — no raw reqwest usage found outside broker module.");
process.exit(0);
