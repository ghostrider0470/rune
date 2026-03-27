import React from "react";
import { ActivityIndicator, Alert, FlatList, SafeAreaView, Text, View } from "react-native";
import { ApprovalCard } from "../../src/components/approvals/ApprovalCard";
import { useApprovals } from "../../src/hooks/use-approvals";

export default function ApprovalsScreen() {
  const { approvals, loading, decidingId, decide } = useApprovals();

  const handleDecision = async (id: string, decision: "allow_once" | "deny") => {
    try {
      await decide(id, decision);
    } catch (error) {
      Alert.alert("Approval failed", error instanceof Error ? error.message : "Unknown error");
    }
  };

  return (
    <SafeAreaView style={{ backgroundColor: "#f9fafb", flex: 1 }}>
      <View style={{ borderBottomWidth: 1, borderColor: "#e5e7eb", gap: 4, padding: 16 }}>
        <Text style={{ fontSize: 24, fontWeight: "700" }}>Approvals</Text>
        <Text style={{ color: "#6b7280" }}>Review and unblock pending tool calls.</Text>
      </View>

      {loading ? (
        <View style={{ alignItems: "center", flex: 1, justifyContent: "center" }}>
          <ActivityIndicator />
        </View>
      ) : (
        <FlatList
          contentContainerStyle={{ flexGrow: approvals.length === 0 ? 1 : undefined, gap: 12, padding: 16 }}
          data={approvals}
          keyExtractor={(item) => item.id}
          ListEmptyComponent={
            <View style={{ alignItems: "center", flex: 1, justifyContent: "center", padding: 24 }}>
              <Text style={{ color: "#111827", fontSize: 18, fontWeight: "600" }}>No pending approvals</Text>
              <Text style={{ color: "#6b7280", marginTop: 8, textAlign: "center" }}>
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
