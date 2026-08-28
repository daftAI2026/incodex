export type UiProbeInput = {
  incognito: boolean;
  buttonPresent: boolean;
  bannerPresent: boolean;
  bannerDismissed: boolean;
  tooltipPresent: boolean;
};

export type UiProbeSnapshot = {
  button: "present" | "missing";
  banner: "not-applicable" | "present" | "missing" | "dismissed";
  tooltip: "present" | "missing";
  accepted: boolean;
};

export function deriveUiProbe(input: UiProbeInput): UiProbeSnapshot {
  const button = input.buttonPresent ? "present" : "missing";
  const tooltip = input.tooltipPresent ? "present" : "missing";
  let banner: UiProbeSnapshot["banner"];

  if (!input.incognito) {
    banner = "not-applicable";
  } else if (input.bannerPresent) {
    banner = "present";
  } else if (input.bannerDismissed) {
    banner = "dismissed";
  } else {
    banner = "missing";
  }

  return {
    button,
    banner,
    tooltip,
    accepted: button === "present" && tooltip === "present" && banner !== "missing",
  };
}
