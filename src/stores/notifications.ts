import { create } from "zustand";

export type NotificationTone = "success" | "info" | "warning" | "error";

export interface NotificationAction {
  label: string;
  run: () => void;
}

export interface UserNotification {
  id: number;
  tone: NotificationTone;
  title: string;
  description?: string;
  detail?: string;
  code?: string;
  action?: NotificationAction;
  dedupeKey: string;
  createdAt: number;
  occurrences: number;
}

export type NotificationInput = Omit<
  UserNotification,
  "id" | "dedupeKey" | "createdAt" | "occurrences"
> & {
  dedupeKey?: string;
};

interface NotificationState {
  notifications: UserNotification[];
  push: (notification: NotificationInput) => number;
  dismiss: (id: number) => void;
  clear: () => void;
}

const MAX_VISIBLE_NOTIFICATIONS = 3;
const DEDUPE_WINDOW_MS = 3_000;
let nextNotificationId = 1;

function fingerprint(notification: NotificationInput): string {
  return (
    notification.dedupeKey ??
    [notification.tone, notification.title, notification.description, notification.detail].join(
      "\0",
    )
  );
}

export const useNotificationStore = create<NotificationState>((set, get) => ({
  notifications: [],
  push: (input) => {
    const now = Date.now();
    const dedupeKey = fingerprint(input);
    const existing = get().notifications.find(
      (notification) =>
        notification.dedupeKey === dedupeKey && now - notification.createdAt <= DEDUPE_WINDOW_MS,
    );
    if (existing) {
      set((state) => ({
        notifications: state.notifications.map((notification) =>
          notification.id === existing.id
            ? {
                ...notification,
                ...input,
                dedupeKey,
                createdAt: now,
                occurrences: notification.occurrences + 1,
              }
            : notification,
        ),
      }));
      return existing.id;
    }

    const id = nextNotificationId;
    nextNotificationId += 1;
    const notification: UserNotification = {
      ...input,
      id,
      dedupeKey,
      createdAt: now,
      occurrences: 1,
    };
    set((state) => ({
      notifications: [...state.notifications, notification].slice(-MAX_VISIBLE_NOTIFICATIONS),
    }));
    return id;
  },
  dismiss: (id) =>
    set((state) => ({
      notifications: state.notifications.filter((notification) => notification.id !== id),
    })),
  clear: () => set({ notifications: [] }),
}));
