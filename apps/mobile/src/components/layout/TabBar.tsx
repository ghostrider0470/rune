import React from "react";
import { Text, View } from "react-native";
import { BottomTabBar, type BottomTabBarProps } from "@react-navigation/bottom-tabs";
import { useApprovalsBadge } from "../../hooks/use-approvals-badge";

export function TabBar(props: BottomTabBarProps) {
  const { count, visible } = useApprovalsBadge();

  return (
    <View>
      <BottomTabBar {...props} />
      {visible ? (
        <View
          pointerEvents="none"
          style={{
            position: "absolute",
            right: 24,
            top: 8,
          }}
        >
          <View
            style={{
              alignItems: "center",
              backgroundColor: "#dc2626",
              borderRadius: 999,
              justifyContent: "center",
              minWidth: 20,
              paddingHorizontal: 6,
              height: 20,
            }}
          >
            <Text style={{ color: "#fff", fontSize: 12, fontWeight: "700" }}>{count}</Text>
          </View>
        </View>
      ) : null}
    </View>
  );
}
