const STORAGE_KEY = "rune-auth-token";

let token: string | null = null;

export async function getToken(): Promise<string | null> {
  return token;
}

export async function setToken(nextToken: string): Promise<void> {
  token = nextToken;
}

export async function clearToken(): Promise<void> {
  token = null;
}

export async function isAuthenticated(): Promise<boolean> {
  return !!(await getToken());
}

export { STORAGE_KEY };
