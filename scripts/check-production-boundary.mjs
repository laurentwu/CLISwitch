import { readdir, readFile } from "node:fs/promises";
import { extname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../dist/", import.meta.url));
const forbidden = [
  "@wdio/tauri-plugin",
  "@wdio/tauri-service",
  "tauri-plugin-wdio",
  "wdio-webdriver",
  "webdriverio",
];
const textExtensions = new Set([".css", ".html", ".js", ".json", ".map", ".svg"]);

async function files(directory) {
  const entries = await readdir(directory, { withFileTypes: true });
  const nested = await Promise.all(
    entries.map((entry) => {
      const path = join(directory, entry.name);
      return entry.isDirectory() ? files(path) : [path];
    }),
  );
  return nested.flat();
}

for (const path of await files(root)) {
  if (!textExtensions.has(extname(path))) continue;
  const content = await readFile(path, "utf8");
  for (const marker of forbidden) {
    if (content.toLowerCase().includes(marker.toLowerCase())) {
      throw new Error(`Production output contains test-only marker ${marker} in ${path}`);
    }
  }
}

process.stdout.write("Production output contains no WebDriver test bridge.\n");
