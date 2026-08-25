import { beforeEach, describe, expect, it } from "vitest";
import { useNotificationStore } from "./notifications";

describe("notification store", () => {
  beforeEach(() => useNotificationStore.getState().clear());

  it("merges repeated notifications within the deduplication window", () => {
    const input = { tone: "error" as const, title: "Save failed", detail: "disk full" };
    const first = useNotificationStore.getState().push(input);
    const second = useNotificationStore.getState().push(input);

    expect(second).toBe(first);
    expect(useNotificationStore.getState().notifications).toHaveLength(1);
    expect(useNotificationStore.getState().notifications[0].occurrences).toBe(2);
  });

  it("keeps at most three visible notifications and supports dismissal", () => {
    const store = useNotificationStore.getState();
    for (let index = 0; index < 4; index += 1) {
      store.push({ tone: "info", title: `Notice ${index}` });
    }
    const notifications = useNotificationStore.getState().notifications;
    expect(notifications.map((notification) => notification.title)).toEqual([
      "Notice 1",
      "Notice 2",
      "Notice 3",
    ]);

    useNotificationStore.getState().dismiss(notifications[1].id);
    expect(useNotificationStore.getState().notifications).toHaveLength(2);
  });
});
