import React from "react";
import { Text, View } from "react-native";

interface OfflineBannerProps {
  queuedCount: number;
}

export function OfflineBanner({ queuedCount }: OfflineBannerProps) {
  if (queuedCount <= 0) return null;

  const label = queuedCount === 1 ? "message" : "messages";

  return (
    <View
      style={{
        backgroundColor: "#fef3c7",
        borderBottomColor: "#f59e0b",
        borderBottomWidth: 1,
        paddingHorizontal: 16,
        paddingVertical: 10,
      }}
    >
      <Text style={{ color: "#92400e", fontWeight: "600" }}>
        Offline queue active — {queuedCount} {label} waiting to send.
      </Text>
    </View>
  );
}
