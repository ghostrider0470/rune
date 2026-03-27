import React, { useMemo, useState } from "react";
import { ActivityIndicator, Pressable, Text, View } from "react-native";
import type { ApprovalRequestResponse } from "../../api/api-types";

interface ApprovalCardProps {
  approval: ApprovalRequestResponse;
  busy?: boolean;
  onApprove: () => void;
  onDeny: () => void;
}

function getSummary(approval: ApprovalRequestResponse): string {
  if (approval.command) {
    return approval.command;
  }

  if (approval.presented_payload && typeof approval.presented_payload === "object") {
    const payload = approval.presented_payload as Record<string, unknown>;
    const command = payload.command;
    if (typeof command === "string" && command.length > 0) {
      return command;
    }
  }

  return approval.reason;
}

export function ApprovalCard({ approval, busy = false, onApprove, onDeny }: ApprovalCardProps) {
  const [touchStartX, setTouchStartX] = useState<number | null>(null);
  const summary = useMemo(() => getSummary(approval), [approval]);

  return (
    <View
      onTouchStart={(event) => setTouchStartX(event.nativeEvent.pageX)}
      onTouchEnd={(event) => {
        if (touchStartX === null || busy) return;
        const deltaX = event.nativeEvent.pageX - touchStartX;
        setTouchStartX(null);
        if (deltaX > 72) {
          onApprove();
        } else if (deltaX < -72) {
          onDeny();
        }
      }}
      style={{
        backgroundColor: "#fff",
        borderColor: "#e5e7eb",
        borderRadius: 16,
        borderWidth: 1,
        gap: 12,
        padding: 16,
      }}
    >
      <View style={{ gap: 6 }}>
        <Text style={{ color: "#6b7280", fontSize: 12, fontWeight: "600", textTransform: "uppercase" }}>
          {approval.subject_type.replace(/_/g, " ")}
        </Text>
        <Text style={{ color: "#111827", fontSize: 18, fontWeight: "700" }}>{approval.reason}</Text>
        <Text style={{ color: "#374151", fontFamily: "monospace" }}>{summary}</Text>
      </View>

      <Text style={{ color: "#9ca3af", fontSize: 12 }}>
        Swipe right to approve • Swipe left to deny
      </Text>

      <View style={{ flexDirection: "row", gap: 12 }}>
        <Pressable
          disabled={busy}
          onPress={onDeny}
          style={{
            alignItems: "center",
            backgroundColor: "#fee2e2",
            borderRadius: 12,
            flex: 1,
            paddingVertical: 14,
          }}
        >
          {busy ? <ActivityIndicator /> : <Text style={{ color: "#b91c1c", fontWeight: "700" }}>Deny</Text>}
        </Pressable>
        <Pressable
          disabled={busy}
          onPress={onApprove}
          style={{
            alignItems: "center",
            backgroundColor: "#dcfce7",
            borderRadius: 12,
            flex: 1,
            paddingVertical: 14,
          }}
        >
          {busy ? <ActivityIndicator /> : <Text style={{ color: "#15803d", fontWeight: "700" }}>Approve</Text>}
        </Pressable>
      </View>
    </View>
  );
}
