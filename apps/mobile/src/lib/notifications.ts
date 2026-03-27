import { router } from "expo-router";
import { getNotificationPreferences, DEFAULT_DEEP_LINK_PATH } from "../store/notification-store";

export interface NotificationPayload {
  pendingCount: number;
  href?: string;
}

let configured = false;

export async function configureNotifications(): Promise<void> {
  configured = true;
}

export function notificationsConfigured(): boolean {
  return configured;
}

export async function notifyPendingApprovalsIncreased(previousCount: number, nextCount: number): Promise<void> {
  const { approvalsEnabled, approvalsDeepLinkPath } = getNotificationPreferences();
  if (!approvalsEnabled || nextCount <= previousCount) {
    return;
  }

  await scheduleLocalNotification({
    pendingCount: nextCount,
    href: approvalsDeepLinkPath || DEFAULT_DEEP_LINK_PATH,
  });
}

export async function scheduleLocalNotification(payload: NotificationPayload): Promise<void> {
  void payload;
}

export function handleNotificationResponse(payload: NotificationPayload | null | undefined): void {
  if (!payload) return;
  const href = payload.href || DEFAULT_DEEP_LINK_PATH;
  router.push(href as never);
}
