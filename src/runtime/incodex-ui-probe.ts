export type UiProbeInput = {
  incognito: boolean;
  buttonPresent: boolean;
  bannerPresent: boolean;
  bannerDismissed: boolean;
};

export type UiProbeSnapshot = {
  button: "present" | "missing";
  banner: "not-applicable" | "present" | "missing" | "dismissed";
  accepted: boolean;
};

export function deriveUiProbe(input: UiProbeInput): UiProbeSnapshot {
  const button = input.buttonPresent ? "present" : "missing";
  let banner: UiProbeSnapshot["banner"];

  if (!input.incognito) banner = "not-applicable";
  else if (input.bannerPresent) banner = "present";
  else if (input.bannerDismissed) banner = "dismissed";
  else banner = "missing";

  return {
    button,
    banner,
    accepted: button === "present" && banner !== "missing",
  };
}
