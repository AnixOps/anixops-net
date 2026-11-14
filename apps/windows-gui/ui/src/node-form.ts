import { t } from "./i18n";

/** What is wrong with the typed server and port, with the rules of the iOS form; `null` when nothing. */
export function nodeFormProblem(server: string, port: string): string | null {
  const host = server.trim();
  if (!host) return t("请填写服务器地址");
  if (/[\s/]/.test(host)) return t("服务器地址不能包含空格或 /");
  const value = Number(port.trim());
  if (!/^\d+$/.test(port.trim()) || value < 1 || value > 65535) return t("端口应为 1–65535");
  return null;
}
