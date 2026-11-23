/**
 * Whether a plugin's decryption list reaches every site: `*`, `*.*`, or every name under a top-level domain
 * (`*.com`). The review before a plugin is turned on says so plainly. A leading `-` marks an exclusion, which
 * decrypts nothing.
 */
export function decryptsBroadly(hosts: string[]): boolean {
  return hosts.some((host) => {
    const pattern = host.trim();
    if (pattern.startsWith("-")) return false;
    return pattern === "*" || pattern === "*.*" || /^\*\.[^.*]+$/.test(pattern);
  });
}
