import React from "react";
import { Tabs } from "expo-router";
import { TabBar } from "../../src/components/layout/TabBar";

export default function TabsLayout() {
  return (
    <Tabs screenOptions={{ headerShown: false }} tabBar={(props) => <TabBar {...props} />}>
      <Tabs.Screen name="index" options={{ title: "Chat" }} />
      <Tabs.Screen name="approvals" options={{ title: "Approvals" }} />
    </Tabs>
  );
}
