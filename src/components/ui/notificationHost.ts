// Keep one mounted Toaster while moving its DOM into the active Radix focus scope.
// Scheduling and toast state remain owned by NotificationViewport, not the dialogs.
const modalHosts = new Set<HTMLElement>();
let notificationHost: HTMLElement | undefined;

function placeNotificationHost() {
  if (!notificationHost) return;
  const active = Array.from(modalHosts)
    .filter((host) => host.isConnected)
    .at(-1);
  const parent = active ?? document.body;
  if (notificationHost.parentElement !== parent) parent.append(notificationHost);
}

export function attachNotificationHost(host: HTMLElement) {
  notificationHost = host;
  placeNotificationHost();
  return () => {
    if (notificationHost === host) notificationHost = undefined;
    host.remove();
  };
}

export function registerModalHost(host: HTMLElement) {
  modalHosts.add(host);
  placeNotificationHost();
  return () => {
    modalHosts.delete(host);
    placeNotificationHost();
  };
}
