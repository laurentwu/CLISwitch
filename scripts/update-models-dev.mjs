#!/usr/bin/env node

import { createHash } from "node:crypto";
import { mkdtemp, rename, rm, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const output = join(root, "src-tauri", "catalog", "models.dev.json");
const metadataOutput = join(root, "src-tauri", "catalog", "models.dev.meta.json");
const url = "https://models.dev/api.json";
const maxBytes = 16 * 1024 * 1024;

const response = await fetch(url, { redirect: "error", headers: { accept: "application/json" } });
if (!response.ok) throw new Error(`models.dev returned HTTP ${response.status}`);
const length = response.headers.get("content-length");
if (length && Number(length) > maxBytes) throw new Error("models.dev response is too large");
const bytes = new Uint8Array(await response.arrayBuffer());
if (bytes.byteLength > maxBytes) throw new Error("models.dev response is too large");

const text = new TextDecoder().decode(bytes);
const document = JSON.parse(text);
if (!document || Array.isArray(document) || typeof document !== "object") {
  throw new Error("models.dev response must be a provider object");
}
const entries = Object.entries(document);
if (!entries.length) throw new Error("models.dev response contains no providers");
for (const [id, provider] of entries) {
  if (
    !provider ||
    (provider.id !== undefined && provider.id !== id) ||
    typeof provider.name !== "string" ||
    !provider.name
  ) {
    throw new Error(`invalid models.dev provider ${id}`);
  }
  if (
    provider.env !== undefined &&
    (!Array.isArray(provider.env) || provider.env.some((name) => typeof name !== "string" || !name))
  ) {
    throw new Error(`models.dev provider ${id} has invalid credential environment names`);
  }
  if (typeof provider.npm !== "string" || !provider.npm) {
    throw new Error(`models.dev provider ${id} has no npm adapter`);
  }
  if (!provider.models || Array.isArray(provider.models) || typeof provider.models !== "object") {
    throw new Error(`models.dev provider ${id} has no model map`);
  }
  for (const [modelId, model] of Object.entries(provider.models)) {
    if (
      !model ||
      (model.id !== undefined && model.id !== modelId) ||
      typeof model.name !== "string" ||
      !model.name
    ) {
      throw new Error(`invalid models.dev model ${id}/${modelId}`);
    }
  }
}

const digest = createHash("sha256").update(bytes).digest("hex");
const temporaryDirectory = await mkdtemp(join(dirname(output), ".models-dev-"));
const temporaryOutput = join(temporaryDirectory, "models.dev.json");
const temporaryMetadata = join(temporaryDirectory, "models.dev.meta.json");
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
        providerCount: entries.length,
        modelCount: entries.reduce(
          (sum, [, provider]) => sum + Object.keys(provider.models).length,
          0,
        ),
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
console.log(
  `updated ${output}: ${entries.length} providers, ${entries.reduce((sum, [, provider]) => sum + Object.keys(provider.models).length, 0)} models, sha256=${digest}`,
);
