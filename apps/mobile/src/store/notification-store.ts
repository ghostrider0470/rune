import { create } from "zustand";

export interface NotificationPreferences {
  approvalsEnabled: boolean;
  approvalsDeepLinkPath: string;
}

interface NotificationState extends NotificationPreferences {
  setApprovalsEnabled: (approvalsEnabled: boolean) => void;
}

const DEFAULT_DEEP_LINK_PATH = "/(tabs)/approvals";

export const useNotificationStore = create<NotificationState>((set) => ({
  approvalsEnabled: true,
  approvalsDeepLinkPath: DEFAULT_DEEP_LINK_PATH,
  setApprovalsEnabled: (approvalsEnabled) => set({ approvalsEnabled }),
}));

export function getNotificationPreferences(): NotificationPreferences {
  const { approvalsEnabled, approvalsDeepLinkPath } = useNotificationStore.getState();
  return { approvalsEnabled, approvalsDeepLinkPath };
}

export { DEFAULT_DEEP_LINK_PATH };
