#!/usr/bin/env node

import { createHash } from "node:crypto";
import { mkdtemp, rename, rm, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const output = join(root, "src-tauri", "catalog", "providers.json");
const metadataOutput = join(root, "src-tauri", "catalog", "providers.meta.json");
const url = "https://laurentwu.github.io/CLIAdapter/providers.json";
const maxBytes = 1024 * 1024;
const supportedProtocols = new Set(["anthropic-messages", "openai-compatible", "responses"]);

const response = await fetch(url, { redirect: "error", headers: { accept: "application/json" } });
if (!response.ok) throw new Error(`CLIAdapter returned HTTP ${response.status}`);
const length = response.headers.get("content-length");
if (length && Number(length) > maxBytes) throw new Error("CLIAdapter response is too large");
const bytes = new Uint8Array(await response.arrayBuffer());
if (bytes.byteLength > maxBytes) throw new Error("CLIAdapter response is too large");

const document = JSON.parse(new TextDecoder().decode(bytes));
if (!Array.isArray(document)) throw new Error("CLIAdapter response must be a provider array");
if (!document.length) throw new Error("CLIAdapter response contains no providers");
const providerIds = new Set();
for (const provider of document) {
  if (
    !provider ||
    typeof provider.id !== "string" ||
    !provider.id ||
    provider.id.length > 256 ||
    /\s|[\x00-\x1f\x7f]/u.test(provider.id) ||
    typeof provider.name !== "string" ||
    !provider.name.trim()
  ) {
    throw new Error("CLIAdapter response contains an invalid provider");
  }
  if (providerIds.has(provider.id)) throw new Error(`duplicate CLIAdapter provider ${provider.id}`);
  providerIds.add(provider.id);
  if (
    !Array.isArray(provider.env) ||
    provider.env.some((name) => typeof name !== "string" || !name.trim()) ||
    new Set(provider.env).size !== provider.env.length
  ) {
    throw new Error(`CLIAdapter provider ${provider.id} has invalid environment names`);
  }
  if (
    !Array.isArray(provider.endpoints) ||
    provider.endpoints.length < 1 ||
    provider.endpoints.length > 3
  ) {
    throw new Error(`CLIAdapter provider ${provider.id} must have one to three endpoints`);
  }
  const protocols = new Set();
  for (const endpoint of provider.endpoints) {
    if (
      !endpoint ||
      typeof endpoint.protocol !== "string" ||
      !supportedProtocols.has(endpoint.protocol) ||
      protocols.has(endpoint.protocol) ||
      typeof endpoint.url !== "string"
    ) {
      throw new Error(`CLIAdapter provider ${provider.id} has an invalid endpoint`);
    }
    protocols.add(endpoint.protocol);
    const parsed = new URL(endpoint.url);
    if (
      parsed.protocol !== "https:" ||
      parsed.username ||
      parsed.password ||
      parsed.search ||
      parsed.hash
    ) {
      throw new Error(`CLIAdapter provider ${provider.id} has an unsafe endpoint URL`);
    }
  }
}

const digest = createHash("sha256").update(bytes).digest("hex");
const temporaryDirectory = await mkdtemp(join(dirname(output), ".providers-"));
const temporaryOutput = join(temporaryDirectory, "providers.json");
const temporaryMetadata = join(temporaryDirectory, "providers.meta.json");
try {
  await writeFile(temporaryOutput, bytes);
  await writeFile(
    temporaryMetadata,
    `${JSON.stringify(
      {
        source: "bundled",
        fetchedAt: new Date().toISOString(),
        etag: response.headers.get("etag"),
        digest,
        providerCount: document.length,
        lastError: null,
      },
      null,
      2,
    )}\n`,
  );
  await rename(temporaryOutput, output);
  await rename(temporaryMetadata, metadataOutput);
} finally {
  await rm(temporaryDirectory, { recursive: true, force: true });
}
console.log(`updated ${output}: ${document.length} providers, sha256=${digest}`);
