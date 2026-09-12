/** Avatar skins available in the companion UI. */
export type AvatarSkinId = "buddy" | "orb" | "familiar";

export type AvatarState = "idle" | "thinking" | "speaking" | "notification" | "error" | "sleeping";

export const AVATAR_STATE_LABEL: Record<AvatarState, string> = {
  idle: "In ascolto",
  thinking: "Sto pensando…",
  speaking: "Ti rispondo",
  notification: "Hai un avviso",
  error: "Qualcosa non va",
  sleeping: "Sto riposando…",
};

export interface AvatarSkinMeta {
  id: AvatarSkinId;
  name: string;
  blurb: string;
}

export const AVATAR_SKINS: AvatarSkinMeta[] = [
  {
    id: "buddy",
    name: "Buddy",
    blurb: "Robot 3D — raso e visore, sguardo, braccia, cammina.",
  },
  {
    id: "orb",
    name: "Lumina",
    blurb: "Occhio-compagno crema e ambra, alone soft.",
  },
  {
    id: "familiar",
    name: "Familiar",
    blurb: "Silhouette a focolare: corpo morbido, occhi e alone verde-oro.",
  },
];

export const DEFAULT_AVATAR_SKIN: AvatarSkinId = "buddy";

export function normalizeAvatarSkin(raw: unknown): AvatarSkinId {
  if (raw === "familiar" || raw === "orb" || raw === "buddy") return raw;
  return DEFAULT_AVATAR_SKIN;
}
