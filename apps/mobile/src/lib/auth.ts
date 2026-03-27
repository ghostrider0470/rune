import * as SecureStore from "expo-secure-store";

const STORAGE_KEY = "rune-auth-token";

let token: string | null = null;
let hydrated = false;

async function hydrateToken(): Promise<void> {
  if (hydrated) {
    return;
  }

  token = await SecureStore.getItemAsync(STORAGE_KEY);
  hydrated = true;
}

export async function getToken(): Promise<string | null> {
  await hydrateToken();
  return token;
}

export async function setToken(nextToken: string): Promise<void> {
  token = nextToken;
  hydrated = true;
  await SecureStore.setItemAsync(STORAGE_KEY, nextToken);
}

export async function clearToken(): Promise<void> {
  token = null;
  hydrated = true;
  await SecureStore.deleteItemAsync(STORAGE_KEY);
}

export async function isAuthenticated(): Promise<boolean> {
  return !!(await getToken());
}

export { STORAGE_KEY };
