import { checkPermissions, invoke, requestPermissions } from "@tauri-apps/api/core";

/**
 * Android's camera scanner (tauri-plugin-barcode-scanner). The camera preview sits behind the window, so while it
 * runs the page turns transparent (`html.qr-scanning`) and shows only the scan frame and its cancel button.
 */
export type ScanOutcome = { content: string } | { denied: true } | { cancelled: true };

type PermissionState = "granted" | "denied" | "prompt" | "prompt-with-rationale";

const scanningClass = "qr-scanning";

/** Asks for the camera when needed, then scans one QR code. */
export async function scanQrCode(): Promise<ScanOutcome> {
  let permission = await checkPermissions<{ camera: PermissionState }>("barcode-scanner");
  if (permission.camera !== "granted") permission = await requestPermissions<{ camera: PermissionState }>("barcode-scanner");
  if (permission.camera !== "granted") return { denied: true };
  document.documentElement.classList.add(scanningClass);
  try {
    const result = await invoke<{ content: string }>("plugin:barcode-scanner|scan", { windowed: true, formats: ["QR_CODE"] });
    return { content: result.content.trim() };
  } catch (error) {
    if (String(error).includes("cancel")) return { cancelled: true };
    throw error;
  } finally {
    document.documentElement.classList.remove(scanningClass);
  }
}

/** Stops a running scan; `scanQrCode` then resolves as cancelled. */
export function cancelQrScan(): Promise<void> {
  return invoke("plugin:barcode-scanner|cancel");
}
