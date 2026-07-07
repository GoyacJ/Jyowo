import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { mkdtempSync, mkdirSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

const REPO_ROOT = execFileSync("git", ["rev-parse", "--show-toplevel"], {
  encoding: "utf-8",
}).trim();
const SCRIPT = join(REPO_ROOT, "scripts", "check-tool-network-broker-boundary.mjs");

function fixtureRoot(name) {
  const root = mkdtempSync(join(tmpdir(), `jyowo-network-boundary-${name}-`));
  mkdirSync(join(root, "crates", "jyowo-harness-tool", "src"), {
    recursive: true,
  });
  return root;
}

test("reports production direct network paths while ignoring broker and cfg(test) code", () => {
  const root = fixtureRoot("violations");
  const src = join(root, "crates", "jyowo-harness-tool", "src");

  writeFileSync(
    join(src, "network_broker.rs"),
    "fn broker() { let _ = reqwest::Client::new().get(\"https://example.com\").send(); }\n",
  );
  writeFileSync(
    join(src, "test_only.rs"),
    `
#[cfg(test)]
mod tests {
  enum Transport {
    Direct(reqwest::Client),
  }

  async fn direct(client: reqwest::Client) {
    let _ = client.get("https://example.com").send().await;
    let _ = AuthorizedNetworkPermit::for_test();
  }
}
`,
  );
  writeFileSync(
    join(src, "violating.rs"),
    `
enum Transport {
  Direct(reqwest::Client),
}

async fn direct(builder: reqwest::RequestBuilder) {
  let _ = builder.send().await;
}

fn permit() {
  let _ = AuthorizedNetworkPermit::for_test();
}
`,
  );

  const result = spawnSync(process.execPath, [SCRIPT], {
    cwd: REPO_ROOT,
    env: {
      ...process.env,
      JYOWO_TOOL_NETWORK_BROKER_BOUNDARY_ROOT: root,
    },
    encoding: "utf-8",
  });

  assert.equal(result.status, 1);
  assert.match(result.stderr, /violating\.rs:3 .*direct reqwest transport/);
  assert.match(result.stderr, /violating\.rs:7 .*raw HTTP send/);
  assert.match(result.stderr, /violating\.rs:11 .*test network permit/);
  assert.doesNotMatch(result.stderr, /src\/network_broker\.rs/);
  assert.doesNotMatch(result.stderr, /src\/test_only\.rs/);
});
