import React from "react";
import { ActivityIndicator, Alert, FlatList, SafeAreaView, Text, View } from "react-native";
import { ApprovalCard } from "../../src/components/approvals/ApprovalCard";
import { useApprovals } from "../../src/hooks/use-approvals";
import { useTheme } from "../../src/hooks/use-theme";

export default function ApprovalsScreen() {
  const colors = useTheme();
  const { approvals, loading, decidingId, decide } = useApprovals();

  const handleDecision = async (id: string, decision: "allow_once" | "deny") => {
    try {
      await decide(id, decision);
    } catch (error) {
      Alert.alert("Approval failed", error instanceof Error ? error.message : "Unknown error");
    }
  };

  return (
    <SafeAreaView style={{ backgroundColor: colors.background, flex: 1 }}>
      <View style={{ borderBottomWidth: 1, borderColor: colors.border, gap: 4, padding: 16 }}>
        <Text style={{ color: colors.text, fontSize: 24, fontWeight: "700" }}>Approvals</Text>
        <Text style={{ color: colors.textMuted }}>Review and unblock pending tool calls.</Text>
      </View>

      {loading ? (
        <View style={{ alignItems: "center", flex: 1, justifyContent: "center" }}>
          <ActivityIndicator color={colors.primary} />
        </View>
      ) : (
        <FlatList
          contentContainerStyle={{ flexGrow: approvals.length === 0 ? 1 : undefined, gap: 12, padding: 16 }}
          data={approvals}
          keyExtractor={(item) => item.id}
          ListEmptyComponent={
            <View style={{ alignItems: "center", flex: 1, justifyContent: "center", padding: 24 }}>
              <Text style={{ color: colors.text, fontSize: 18, fontWeight: "600" }}>No pending approvals</Text>
              <Text style={{ color: colors.textMuted, marginTop: 8, textAlign: "center" }}>
                Approval requests will show up here when a tool call needs operator input.
              </Text>
            </View>
          }
          renderItem={({ item }) => (
            <ApprovalCard
              approval={item}
              busy={decidingId === item.id}
              onApprove={() => void handleDecision(item.id, "allow_once")}
              onDeny={() => void handleDecision(item.id, "deny")}
            />
          )}
        />
      )}
    </SafeAreaView>
  );
}
