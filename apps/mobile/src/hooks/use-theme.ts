import { useColorScheme } from "react-native";
import { getTheme, type ThemeMode } from "../theme";
import { useAppStore } from "../store/app-store";

export function useThemePreference(): ThemeMode {
  const preference = useAppStore((state) => state.themePreference);
  const systemScheme = useColorScheme();

  if (preference === "system") {
    return systemScheme === "light" ? "light" : "dark";
  }

  return preference;
}

export function useTheme() {
  return getTheme(useThemePreference());
}
