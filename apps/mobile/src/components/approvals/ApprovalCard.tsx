import React, { useMemo, useState } from "react";
import { ActivityIndicator, Pressable, Text, View } from "react-native";
import type { ApprovalRequestResponse } from "../../api/api-types";
import { useTheme } from "../../hooks/use-theme";

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

function getStatusLabel(approval: ApprovalRequestResponse): string {
  if (approval.approval_status) {
    return approval.approval_status.replace(/_/g, " ");
  }
  if (approval.decision) {
    return approval.decision.replace(/_/g, " ");
  }
  return "pending";
}

export function ApprovalCard({ approval, busy = false, onApprove, onDeny }: ApprovalCardProps) {
  const colors = useTheme();
  const [touchStartX, setTouchStartX] = useState<number | null>(null);
  const summary = useMemo(() => getSummary(approval), [approval]);
  const statusLabel = useMemo(() => getStatusLabel(approval), [approval]);

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
        backgroundColor: colors.surface,
        borderColor: colors.border,
        borderRadius: 16,
        borderWidth: 1,
        gap: 12,
        padding: 16,
      }}
    >
      <View style={{ gap: 6 }}>
        <View style={{ alignItems: "center", flexDirection: "row", justifyContent: "space-between" }}>
          <Text style={{ color: colors.textMuted, fontSize: 12, fontWeight: "600", textTransform: "uppercase" }}>
            {approval.subject_type.replace(/_/g, " ")}
          </Text>
          <View
            style={{
              backgroundColor: colors.surfaceMuted,
              borderRadius: 999,
              paddingHorizontal: 10,
              paddingVertical: 4,
            }}
          >
            <Text style={{ color: colors.text, fontSize: 12, fontWeight: "700", textTransform: "capitalize" }}>
              {statusLabel}
            </Text>
          </View>
        </View>
        <Text style={{ color: colors.text, fontSize: 18, fontWeight: "700" }}>{approval.reason}</Text>
        <Text style={{ color: colors.text, fontFamily: "monospace" }}>{summary}</Text>
        {approval.resume_result_summary ? (
          <Text style={{ color: colors.textMuted, fontSize: 12 }}>{approval.resume_result_summary}</Text>
        ) : null}
      </View>

      <Text style={{ color: colors.textMuted, fontSize: 12 }}>
        Swipe right to approve • Swipe left to deny
      </Text>

      <View style={{ flexDirection: "row", gap: 12 }}>
        <Pressable
          disabled={busy}
          onPress={onDeny}
          style={{
            alignItems: "center",
            backgroundColor: colors.surfaceMuted,
            borderRadius: 12,
            flex: 1,
            paddingVertical: 14,
          }}
        >
          {busy ? <ActivityIndicator color={colors.text} /> : <Text style={{ color: colors.danger, fontWeight: "700" }}>Deny</Text>}
        </Pressable>
        <Pressable
          disabled={busy}
          onPress={onApprove}
          style={{
            alignItems: "center",
            backgroundColor: colors.primary,
            borderRadius: 12,
            flex: 1,
            paddingVertical: 14,
          }}
        >
          {busy ? <ActivityIndicator color={colors.onPrimary} /> : <Text style={{ color: colors.onPrimary, fontWeight: "700" }}>Approve</Text>}
        </Pressable>
      </View>
    </View>
  );
}
