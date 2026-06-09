import type { HeadroomPricingStatus } from "./types";

export async function maybeFireTrialNotifications(
  _status: HeadroomPricingStatus
): Promise<void> {
  // Open-source builds have no trial or subscription paywall.
}
