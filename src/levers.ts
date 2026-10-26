import type { PowerState, ProfileInfo } from "./api";
import { t } from "./i18n";

/** Les champs que portent à la fois un profil et un état relu. */
type LeverKey = "boostMode" | "throttleMax" | "epp" | "throttleMax1" | "epp1";

/**
 * Les leviers CPU, sous leur alias `powercfg` — celui que le diagnostic affiche — et
 * leur champ dans `ProfileInfo` et `PowerState`. Même ordre que `power::LEVERS`.
 */
export const LEVERS: { name: string; key: LeverKey; label: () => string }[] = [
  { name: "PERFBOOSTMODE", key: "boostMode", label: () => t.leverBoost },
  { name: "PROCTHROTTLEMAX", key: "throttleMax", label: () => t.leverCapE },
  { name: "PERFEPP", key: "epp", label: () => t.leverEppE },
  { name: "PROCTHROTTLEMAX1", key: "throttleMax1", label: () => t.leverCapP },
  { name: "PERFEPP1", key: "epp1", label: () => t.leverEppP },
];

/**
 * Ce qui sépare une machine relue du profil qu'on lui a demandé, levier par levier. Un
 * levier que le profil ne fixe pas ne compte pas — c'est la règle de `profiles::identify`.
 *
 * C'est la réponse à « bridage externe » : sans elle, on sait que la machine ne
 * ressemble à aucun profil, pas pourquoi.
 */
export function mismatches(profile: ProfileInfo, power: PowerState): string[] {
  return LEVERS.flatMap(({ name, key, label }) => {
    const want = profile[key];
    const got = power[key];
    if (want === null || got === want) return [];
    const policy = power.policyLocked.includes(name) ? ` ${t.byPolicy}` : "";
    return [`${label()} ${got ?? "?"} ≠ ${want}${policy}`];
  });
}
