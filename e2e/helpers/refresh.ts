import { $, browser } from "@wdio/globals";

export async function refreshApp() {
  // The embedded driver's refresh resolves before the new document is loaded.
  // Mark the old document so its already-rendered UI cannot satisfy our wait.
  await browser.execute(() => {
    document.documentElement.setAttribute("data-e2e-reload-pending", "true");
  });
  await browser.refresh();
  await browser.waitUntil(
    async () => (await $("html").getAttribute("data-e2e-reload-pending")) === null,
    { timeoutMsg: "Expected refresh to replace the previous document" },
  );
  await $("main h1").waitForExist();
}
