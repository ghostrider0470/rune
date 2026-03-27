import { getToken } from "../lib/auth";
import { getGatewayUrl } from "../store/app-store";

export interface ApiClientOptions extends RequestInit {
  baseUrl?: string;
  token?: string | null;
  headers?: HeadersInit;
}

function trimTrailingSlash(value: string): string {
  return value.replace(/\/+$/, "");
}

export function resolveBaseUrl(explicitBaseUrl?: string): string {
  const baseUrl = explicitBaseUrl ?? getGatewayUrl();
  if (!baseUrl) {
    throw new Error("Gateway base URL is not configured");
  }
  return trimTrailingSlash(baseUrl);
}

export async function buildAuthHeaders(
  headers?: HeadersInit,
  explicitToken?: string | null,
): Promise<Headers> {
  const nextHeaders = new Headers(headers);
  const token = explicitToken ?? (await getToken());
  if (token) {
    nextHeaders.set("Authorization", `Bearer ${token}`);
  }
  if (!nextHeaders.has("Accept")) {
    nextHeaders.set("Accept", "application/json");
  }
  return nextHeaders;
}

export async function apiFetch(path: string, options: ApiClientOptions = {}): Promise<Response> {
  const baseUrl = resolveBaseUrl(options.baseUrl);
  const headers = await buildAuthHeaders(options.headers, options.token);
  const url = path.startsWith("http://") || path.startsWith("https://")
    ? path
    : `${baseUrl}${path.startsWith("/") ? path : `/${path}`}`;

  return fetch(url, {
    ...options,
    headers,
  });
}
