export type AvatarDisplayProfile = {
  os: string;
  /** Per-pixel window alpha. False on Linux (WebKitGTK compositor trails). */
  glass: boolean;
};

export function guessAvatarGlassFromUa(): boolean {
  return typeof navigator !== "undefined" && !/Linux/i.test(navigator.userAgent);
}

export function applyAvatarShellClass(glass: boolean) {
  document.documentElement.classList.toggle("shell-avatar-glass", glass);
}

export async function fetchAvatarDisplayProfile(): Promise<AvatarDisplayProfile> {
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    return await invoke<AvatarDisplayProfile>("avatar_display_profile");
  } catch {
    const glass = guessAvatarGlassFromUa();
    return { os: glass ? "unknown" : "linux", glass };
  }
}
