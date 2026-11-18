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

/**
 * Whether `pattern` matches `host`: `*` any run of characters (dots too), `?` one character, as the engine reads
 * `[MITM] hostname` items. `*.example.com` covers the subdomains, not `example.com` itself.
 */
export function matchesPattern(pattern: string, host: string): boolean {
  const p = pattern.trim().toLowerCase();
  const text = decryptHostOf(host);
  let pi = 0;
  let ti = 0;
  let star = -1;
  let mark = 0;
  while (ti < text.length) {
    if (pi < p.length && (p[pi] === "?" || p[pi] === text[ti])) {
      pi += 1;
      ti += 1;
    } else if (pi < p.length && p[pi] === "*") {
      star = pi;
      mark = ti;
      pi += 1;
    } else if (star >= 0) {
      pi = star + 1;
      mark += 1;
      ti = mark;
    } else {
      return false;
    }
  }
  while (pi < p.length && p[pi] === "*") pi += 1;
  return pi === p.length;
}

/** Whether an exclusion of `excluded` names `host`. Exclusions win over every include. */
export function excludedBy(host: string, excluded: string[]): boolean {
  return excluded.some((pattern) => matchesPattern(pattern, host));
}

/** Whether `host` is decrypted: a pattern of `hosts` matches it and no exclusion of `excluded` does. */
export function coveredBy(host: string, hosts: string[], excluded: string[] = []): boolean {
  return hosts.some((pattern) => matchesPattern(pattern, host)) && !excludedBy(host, excluded);
}

/** The engine's decryption result for one domain, as `mitm_hosts` gives it. */
export interface HostOutcome {
  host: string;
  decrypted: number;
  failed: number;
  last_failure?: string;
  last_failure_at_ms?: number;
  last_decrypted_at_ms?: number;
}

/** The domain's last connection failed: nothing was decrypted since the last failure. */
export function isFailing(outcome: HostOutcome): boolean {
  if (outcome.failed === 0 || outcome.last_failure_at_ms === undefined) return false;
  return outcome.last_decrypted_at_ms === undefined || outcome.last_decrypted_at_ms <= outcome.last_failure_at_ms;
}

/** Why decryption failed, in words; certificate pinning first, since excluding the domain is then the only fix. */
export function failureReason(code: string | undefined): string {
  switch (code) {
    case "client_rejected_certificate":
      return "App 拒绝了证书：可能固定了证书，或没有信任根证书";
    case "client_closed":
      return "App 在握手时断开：常见于证书固定";
    case "client_handshake_failed":
      return "与 App 的 TLS 握手失败";
    case "client_timeout":
      return "App 没有在限定时间内完成握手";
    case "origin_tls":
      return "源站 TLS 握手失败，例如它的证书与域名不符";
    case "origin_unreachable":
      return "连不上源站，或源站没有及时响应";
    default:
      return "解密失败";
  }
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
