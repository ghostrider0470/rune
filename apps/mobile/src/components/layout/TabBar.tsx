import React from "react";
import { Text, View } from "react-native";
import { BottomTabBar, type BottomTabBarProps } from "@react-navigation/bottom-tabs";
import { useApprovalsBadge } from "../../hooks/use-approvals-badge";
import { useTheme } from "../../hooks/use-theme";

export function TabBar(props: BottomTabBarProps) {
  const colors = useTheme();
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
              backgroundColor: colors.danger,
              borderRadius: 999,
              justifyContent: "center",
              minWidth: 20,
              paddingHorizontal: 6,
              height: 20,
            }}
          >
            <Text style={{ color: colors.onPrimary, fontSize: 12, fontWeight: "700" }}>{count}</Text>
          </View>
        </View>
      ) : null}
    </View>
  );
}
