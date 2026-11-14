/**
 * Whether a connection's host is a domain name decryption can be turned on for: not an IP address, with a dot, made
 * of letters, digits, dots and hyphens. The backend checks again (`mappings::normalize_host`).
 */
export function isDecryptableHost(host: string | null | undefined): host is string {
  if (!host) return false;
  const name = host.replace(/:\d+$/, "").replace(/\.$/, "").toLowerCase();
  if (!/^[a-z0-9.-]+$/.test(name) || !name.includes(".") || /^[.-]/.test(name)) return false;
  // An IPv4 address is digits and dots only; top-level domains are never all digits.
  return !/^[0-9.]+$/.test(name);
}

/** The domain without a port, lower-cased, as the decrypt list keeps it. */
export function decryptHostOf(host: string): string {
  return host.replace(/:\d+$/, "").replace(/\.$/, "").toLowerCase();
}

/** Whether `host` is decrypted by an entry of the list: the same name, or a `*.` entry that covers it. */
export function coveredBy(host: string, hosts: string[]): boolean {
  const name = decryptHostOf(host);
  return hosts.some((entry) => entry === name || (entry.startsWith("*.") && name.endsWith(entry.slice(1))));
}

/** One step of getting HTTPS requests into the list, in the order they have to happen. */
export type SetupStep = "connect" | "certificate" | "decryption" | "domains" | "capture";

export interface SetupState {
  connected: boolean;
  /** Null where the app cannot tell (Android installs the certificate in its own settings). */
  trusted: boolean | null;
  decrypting: boolean;
  /** Decrypted domains: the list's, the plugins' and the mappings'. */
  domains: number;
  capturing: boolean;
}

/** The steps still to do; empty once HTTPS requests can show up. A step the app cannot check is not listed. */
export function pendingSteps(state: SetupState): SetupStep[] {
  const steps: SetupStep[] = [];
  if (!state.connected) steps.push("connect");
  if (state.trusted === false) steps.push("certificate");
  if (!state.decrypting) steps.push("decryption");
  if (state.domains === 0) steps.push("domains");
  if (!state.capturing) steps.push("capture");
  return steps;
}
