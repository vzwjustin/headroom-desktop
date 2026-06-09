use chrono::{DateTime, Utc};
use reqwest::blocking::Client;
use serde_json::Value;

use crate::models::{
    ClaudeAccountProfile, ClaudeAuthMethod, ClaudePlanTier, ClaudeUsage, ClaudeUsageWindow,
};
use crate::state::AppState;

#[derive(Debug, Clone)]
struct ClaudeOauthProfile {
    account: ClaudeOauthProfileAccount,
    organization: Option<ClaudeOauthProfileOrganization>,
}

#[derive(Debug, Clone)]
struct ClaudeOauthProfileAccount {
    uuid: Option<String>,
    email: Option<String>,
    display_name: Option<String>,
    created_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
struct ClaudeOauthProfileOrganization {
    uuid: Option<String>,
    billing_type: Option<String>,
    subscription_created_at: Option<DateTime<Utc>>,
    has_extra_usage_enabled: bool,
    /// e.g. "claude_pro", "claude_max", "claude_enterprise"
    organization_type: Option<String>,
    /// e.g. "default_claude_ai", "claude_max_5x", "claude_max_20x",
    /// "default_claude_max_x5", "default_claude_max_x20" (Anthropic ships both
    /// the `_5x`/`_20x` and `_x5`/`_x20` orderings in the wild)
    rate_limit_tier: Option<String>,
}

pub fn fetch_claude_usage(state: &AppState) -> Result<ClaudeUsage, String> {
    let access_token = state.current_bearer_token().ok_or_else(|| {
        "No Claude AI token captured yet — make sure Claude Code is running and authenticated via Claude AI (not an API key), then try again after the first request passes through the proxy.".to_string()
    })?;

    let resp = http_client()?
        .get("https://api.anthropic.com/api/oauth/usage")
        .header("Authorization", format!("Bearer {access_token}"))
        .header("anthropic-beta", "oauth-2025-04-20")
        .header("Content-Type", "application/json")
        .send()
        .map_err(|e| format!("Request failed: {e}"))?;

    let body: serde_json::Value = resp
        .json()
        .map_err(|e| format!("Failed to parse response: {e}"))?;

    parse_claude_usage_response(&body)
}

/// Pure parser for the Anthropic OAuth usage endpoint response. Extracted so
/// schema-drift tests don't need a live HTTP server.
pub(crate) fn parse_claude_usage_response(body: &serde_json::Value) -> Result<ClaudeUsage, String> {
    use chrono::DateTime;

    if let Some(err) = body.get("error") {
        return Err(format!(
            "API error: {}",
            err["message"].as_str().unwrap_or("unknown")
        ));
    }

    let parse_window = |v: &serde_json::Value| -> Option<ClaudeUsageWindow> {
        let utilization = v.get("utilization")?.as_f64()?;
        let resets_at_str = v.get("resets_at")?.as_str()?;
        let resets_at = DateTime::parse_from_rfc3339(resets_at_str).ok()?.to_utc();
        Some(ClaudeUsageWindow {
            utilization,
            resets_at,
        })
    };

    let five_hour = body.get("five_hour").and_then(parse_window);
    let seven_day = body.get("seven_day").and_then(parse_window);

    let extra_usage = body.get("extra_usage").and_then(|e| {
        Some(crate::models::ClaudeExtraUsage {
            is_enabled: e.get("is_enabled")?.as_bool()?,
            monthly_limit: e.get("monthly_limit").and_then(|v| v.as_f64()),
            used_credits: e.get("used_credits").and_then(|v| v.as_f64()),
            utilization: e.get("utilization").and_then(|v| v.as_f64()),
        })
    });

    Ok(ClaudeUsage {
        five_hour,
        seven_day,
        extra_usage,
    })
}

pub fn detect_claude_profile(state: &AppState) -> ClaudeAccountProfile {
    state.cached_claude_profile()
}

pub fn detect_claude_profile_uncached(state: &AppState) -> ClaudeAccountProfile {
    let Some(token) = state.current_bearer_token() else {
        // No token yet — proxy hasn't seen a request through. Return a minimal
        // profile so the app can show "send a message first" messaging.
        return ClaudeAccountProfile {
            auth_method: ClaudeAuthMethod::Unknown,
            email: None,
            display_name: None,
            account_uuid: None,
            organization_uuid: None,
            billing_type: None,
            account_created_at: None,
            subscription_created_at: None,
            has_extra_usage_enabled: false,
            plan_tier: ClaudePlanTier::Unknown,
            plan_detection_source: None,
            organization_type: None,
            rate_limit_tier: None,
            weekly_utilization_pct: None,
            five_hour_utilization_pct: None,
            extra_usage_monthly_limit: None,
            profile_fetch_error: None,
        };
    };

    let (profile, profile_fetch_error) = match fetch_oauth_profile(&token) {
        Ok(p) => (Some(p), None),
        Err(msg) => (None, Some(msg)),
    };
    let usage = fetch_claude_usage(state).ok();

    let (plan_tier, plan_detection_source) = if let Some(ref p) = profile {
        detect_plan_tier_from_profile(p)
    } else {
        (ClaudePlanTier::Unknown, None)
    };

    // Persist the classifier output when it carries real signal so the
    // pricing gate can fall back to it next time Anthropic returns a sparse
    // profile and we'd otherwise classify as Unknown. The helper filters
    // Unknown internally.
    state.record_known_good_plan_tier(&plan_tier);

    ClaudeAccountProfile {
        auth_method: ClaudeAuthMethod::ClaudeAiOauth,
        email: profile.as_ref().and_then(|p| p.account.email.clone()),
        display_name: profile
            .as_ref()
            .and_then(|p| p.account.display_name.clone()),
        account_uuid: profile.as_ref().and_then(|p| p.account.uuid.clone()),
        organization_uuid: profile
            .as_ref()
            .and_then(|p| p.organization.as_ref().and_then(|o| o.uuid.clone())),
        billing_type: profile
            .as_ref()
            .and_then(|p| p.organization.as_ref().and_then(|o| o.billing_type.clone())),
        account_created_at: profile.as_ref().and_then(|p| p.account.created_at),
        subscription_created_at: profile.as_ref().and_then(|p| {
            p.organization
                .as_ref()
                .and_then(|o| o.subscription_created_at)
        }),
        has_extra_usage_enabled: profile
            .as_ref()
            .and_then(|p| p.organization.as_ref().map(|o| o.has_extra_usage_enabled))
            .unwrap_or(false),
        plan_tier,
        plan_detection_source,
        organization_type: profile
            .as_ref()
            .and_then(|p| p.organization.as_ref().and_then(|o| o.organization_type.clone())),
        rate_limit_tier: profile
            .as_ref()
            .and_then(|p| p.organization.as_ref().and_then(|o| o.rate_limit_tier.clone())),
        weekly_utilization_pct: usage
            .as_ref()
            .and_then(|u| u.seven_day.as_ref().map(|w| w.utilization)),
        five_hour_utilization_pct: usage
            .as_ref()
            .and_then(|u| u.five_hour.as_ref().map(|w| w.utilization)),
        extra_usage_monthly_limit: usage
            .as_ref()
            .and_then(|u| u.extra_usage.as_ref().and_then(|e| e.monthly_limit)),
        profile_fetch_error,
    }
}

fn fetch_oauth_profile(token: &str) -> Result<ClaudeOauthProfile, String> {
    let response = http_client()?
        .get("https://api.anthropic.com/api/oauth/profile")
        .header("Authorization", format!("Bearer {token}"))
        .header("anthropic-beta", "oauth-2025-04-20")
        .header("Content-Type", "application/json")
        .send()
        .map_err(|_| {
            "Couldn't reach Anthropic to refresh your Claude plan. Check your internet connection \
             and we'll try again shortly."
                .to_string()
        })?;

    if !response.status().is_success() {
        let status = response.status().as_u16();
        let user_msg = if status >= 500 {
            format!(
                "Anthropic is having trouble serving your Claude plan right now (HTTP {status}). \
                 We'll keep trying."
            )
        } else if status == 401 || status == 403 {
            "Anthropic rejected our request for your Claude plan. Try signing out of Claude Code \
             and back in."
                .to_string()
        } else {
            format!(
                "Anthropic returned an unexpected response for your Claude plan (HTTP {status}). \
                 We'll try again shortly."
            )
        };
        return Err(user_msg);
    }

    let body: serde_json::Value = response.json().map_err(|err| {
        sentry::capture_message(
            &format!("Could not parse Claude OAuth profile: {err}"),
            sentry::Level::Error,
        );
        "We couldn't read the response from Anthropic for your Claude plan. Please report this if \
         it keeps happening."
            .to_string()
    })?;

    parse_oauth_profile_value(&body).ok_or_else(|| {
        "Anthropic's response didn't include your Claude account details. Please report this if \
         it keeps happening."
            .to_string()
    })
}

fn parse_oauth_profile_value(value: &serde_json::Value) -> Option<ClaudeOauthProfile> {
    let root = value
        .get("profile")
        .or_else(|| value.get("data"))
        .unwrap_or(value);
    let account_value = root.get("account").unwrap_or(root);

    Some(ClaudeOauthProfile {
        account: ClaudeOauthProfileAccount {
            uuid: json_string(account_value, &["uuid", "account_uuid"]),
            email: json_string(account_value, &["email", "email_address"]),
            display_name: json_string(account_value, &["display_name", "displayName"]),
            created_at: json_datetime(account_value, &["created_at", "createdAt"]),
        },
        organization: root
            .get("organization")
            .and_then(parse_oauth_profile_organization),
    })
}

fn parse_oauth_profile_organization(
    value: &serde_json::Value,
) -> Option<ClaudeOauthProfileOrganization> {
    Some(ClaudeOauthProfileOrganization {
        uuid: json_string(value, &["uuid", "organization_uuid"]),
        billing_type: json_string(value, &["billing_type", "billingType"]),
        subscription_created_at: json_datetime(
            value,
            &["subscription_created_at", "subscriptionCreatedAt"],
        ),
        has_extra_usage_enabled: json_bool(
            value,
            &["has_extra_usage_enabled", "hasExtraUsageEnabled"],
        )
        .unwrap_or(false),
        organization_type: json_string(value, &["organization_type", "organizationType"]),
        rate_limit_tier: json_string(value, &["rate_limit_tier", "rateLimitTier"]),
    })
}

fn detect_plan_tier_from_profile(profile: &ClaudeOauthProfile) -> (ClaudePlanTier, Option<String>) {
    let Some(org) = profile.organization.as_ref() else {
        return (ClaudePlanTier::Free, Some("oauth_profile.account".into()));
    };

    if let Some(rate_limit_tier) = org.rate_limit_tier.as_deref() {
        let normalized = rate_limit_tier.trim().to_ascii_lowercase();
        // Anthropic ships both orderings in the wild: "claude_max_20x" and
        // "default_claude_max_x20" (same for 5x/x5). Match either.
        if normalized.contains("20x") || normalized.contains("x20") {
            return (
                ClaudePlanTier::Max20x,
                Some("oauth_profile.organization.rateLimitTier".into()),
            );
        }
        if normalized.contains("5x") || normalized.contains("x5") {
            return (
                ClaudePlanTier::Max5x,
                Some("oauth_profile.organization.rateLimitTier".into()),
            );
        }
        // Anthropic's internal label for Team-plan rate limits. Show Max20x
        // pricing rather than falling through to Pro.
        if normalized.contains("raven") {
            return (
                ClaudePlanTier::Max20x,
                Some("oauth_profile.organization.rateLimitTier".into()),
            );
        }
        if normalized == "default_claude_ai" {
            let organization_type = org.organization_type.as_deref().unwrap_or_default();
            if organization_type.eq_ignore_ascii_case("claude_max") {
                return (
                    ClaudePlanTier::Max5x,
                    Some("oauth_profile.organization.organizationType".into()),
                );
            }
            if organization_type.eq_ignore_ascii_case("claude_pro")
                || organization_type.eq_ignore_ascii_case("claude_enterprise")
            {
                return (
                    ClaudePlanTier::Pro,
                    Some("oauth_profile.organization.organizationType".into()),
                );
            }
        }
    }

    if let Some(organization_type) = org.organization_type.as_deref() {
        let normalized = organization_type.trim().to_ascii_lowercase();
        if normalized == "claude_max" {
            return (
                ClaudePlanTier::Max5x,
                Some("oauth_profile.organization.organizationType".into()),
            );
        }
        if normalized == "claude_pro" || normalized == "claude_enterprise" {
            return (
                ClaudePlanTier::Pro,
                Some("oauth_profile.organization.organizationType".into()),
            );
        }
        if normalized == "claude_free" || normalized == "free" {
            return (
                ClaudePlanTier::Free,
                Some("oauth_profile.organization.organizationType".into()),
            );
        }
    }

    if org.subscription_created_at.is_none() {
        return (
            ClaudePlanTier::Free,
            Some("oauth_profile.organization.subscriptionCreatedAt".into()),
        );
    }

    log_unknown_plan_tier_once(profile);
    (
        ClaudePlanTier::Unknown,
        Some("oauth_profile.organization".into()),
    )
}

/// Capture the raw classification fields whenever `detect_plan_tier_from_profile`
/// falls into the `Unknown` branch — i.e., the user has an Anthropic
/// organization with `subscription_created_at` set but neither
/// `organization_type` nor `rate_limit_tier` matches our enum. Almost
/// certainly Team/Workspace/Enterprise plans we haven't enumerated.
///
/// Currently those users bypass the pricing gate entirely, which means
/// paying Anthropic customers get Headroom for free. Goal of this telemetry
/// is to learn which taxonomy strings to add to the detection (or to a new
/// "treat as Pro" fallback) before changing the gate policy.
///
/// Deduped on content — Sentry sees one event per distinct
/// (organization_type, rate_limit_tier, has_subscription, billing_type)
/// combo across the lifetime of the desktop process.
fn log_unknown_plan_tier_once(profile: &ClaudeOauthProfile) {
    use std::collections::HashSet;
    use std::hash::{DefaultHasher, Hash, Hasher};
    use std::sync::OnceLock;

    static SEEN: OnceLock<parking_lot::Mutex<HashSet<u64>>> = OnceLock::new();

    let org = profile.organization.as_ref();
    let organization_type = org
        .and_then(|o| o.organization_type.as_deref())
        .unwrap_or("");
    let rate_limit_tier = org.and_then(|o| o.rate_limit_tier.as_deref()).unwrap_or("");
    let billing_type = org.and_then(|o| o.billing_type.as_deref()).unwrap_or("");
    let has_subscription_created_at = org
        .and_then(|o| o.subscription_created_at.as_ref())
        .is_some();

    let mut hasher = DefaultHasher::new();
    organization_type.hash(&mut hasher);
    rate_limit_tier.hash(&mut hasher);
    billing_type.hash(&mut hasher);
    has_subscription_created_at.hash(&mut hasher);
    let key = hasher.finish();

    let seen = SEEN.get_or_init(|| parking_lot::Mutex::new(HashSet::new()));
    if !seen.lock().insert(key) {
        return;
    }

    let payload = serde_json::json!({
        "organization_type": organization_type,
        "rate_limit_tier": rate_limit_tier,
        "billing_type": billing_type,
        "has_subscription_created_at": has_subscription_created_at,
    });
    sentry::capture_message(
        &format!("plan_tier_unknown: {payload}"),
        sentry::Level::Warning,
    );
}

fn json_string(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(|entry| entry.as_str()))
        .map(str::to_string)
}

fn json_bool(value: &serde_json::Value, keys: &[&str]) -> Option<bool> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(|entry| entry.as_bool()))
}

fn json_datetime(value: &serde_json::Value, keys: &[&str]) -> Option<DateTime<Utc>> {
    keys.iter().find_map(|key| {
        value
            .get(*key)
            .and_then(|entry| entry.as_str())
            .and_then(|entry| DateTime::parse_from_rfc3339(entry).ok())
            .map(|entry| entry.to_utc())
    })
}

fn http_client() -> Result<Client, String> {
    Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .map_err(|err| format!("Could not build HTTP client: {err}"))
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};

    use super::{detect_plan_tier_from_profile, parse_claude_usage_response};
    use crate::models::ClaudePlanTier;
    use super::{ClaudeOauthProfile, ClaudeOauthProfileAccount, ClaudeOauthProfileOrganization};

    fn oauth_profile(
        rate_limit_tier: Option<&str>,
        organization_type: Option<&str>,
        subscription_created_at: Option<DateTime<Utc>>,
    ) -> ClaudeOauthProfile {
        ClaudeOauthProfile {
            account: ClaudeOauthProfileAccount {
                uuid: None,
                email: None,
                display_name: None,
                created_at: None,
            },
            organization: Some(ClaudeOauthProfileOrganization {
                uuid: None,
                billing_type: None,
                subscription_created_at,
                has_extra_usage_enabled: false,
                organization_type: organization_type.map(str::to_string),
                rate_limit_tier: rate_limit_tier.map(str::to_string),
            }),
        }
    }
    #[test]
    fn detect_plan_tier_rate_limit_20x_wins() {
        let p = oauth_profile(Some("claude_max_20x"), Some("claude_pro"), Some(Utc::now()));
        assert!(matches!(
            detect_plan_tier_from_profile(&p).0,
            ClaudePlanTier::Max20x
        ));
    }

    #[test]
    fn detect_plan_tier_rate_limit_5x_wins() {
        let p = oauth_profile(Some("claude_max_5x"), Some("claude_pro"), Some(Utc::now()));
        assert!(matches!(
            detect_plan_tier_from_profile(&p).0,
            ClaudePlanTier::Max5x
        ));
    }

    #[test]
    fn detect_plan_tier_rate_limit_x5_variant_is_max5x() {
        let p = oauth_profile(
            Some("default_claude_max_x5"),
            Some("default_claude"),
            Some(Utc::now()),
        );
        assert!(matches!(
            detect_plan_tier_from_profile(&p).0,
            ClaudePlanTier::Max5x
        ));
    }

    #[test]
    fn detect_plan_tier_rate_limit_x20_variant_is_max20x() {
        let p = oauth_profile(
            Some("default_claude_max_x20"),
            Some("default_claude"),
            Some(Utc::now()),
        );
        assert!(matches!(
            detect_plan_tier_from_profile(&p).0,
            ClaudePlanTier::Max20x
        ));
    }

    #[test]
    fn detect_plan_tier_default_raven_is_max20x() {
        let p = oauth_profile(
            Some("default_raven"),
            Some("claude_team"),
            Some(Utc::now()),
        );
        assert!(matches!(
            detect_plan_tier_from_profile(&p).0,
            ClaudePlanTier::Max20x
        ));
    }

    #[test]
    fn detect_plan_tier_raven_substring_is_max20x() {
        let p = oauth_profile(Some("default_raven_x"), Some("claude_team"), Some(Utc::now()));
        assert!(matches!(
            detect_plan_tier_from_profile(&p).0,
            ClaudePlanTier::Max20x
        ));
    }

    #[test]
    fn detect_plan_tier_default_rate_limit_with_claude_max_is_max5x() {
        let p = oauth_profile(
            Some("default_claude_ai"),
            Some("claude_max"),
            Some(Utc::now()),
        );
        assert!(matches!(
            detect_plan_tier_from_profile(&p).0,
            ClaudePlanTier::Max5x
        ));
    }

    #[test]
    fn detect_plan_tier_default_rate_limit_with_claude_pro_is_pro() {
        let p = oauth_profile(
            Some("default_claude_ai"),
            Some("claude_pro"),
            Some(Utc::now()),
        );
        assert!(matches!(
            detect_plan_tier_from_profile(&p).0,
            ClaudePlanTier::Pro
        ));
    }

    #[test]
    fn detect_plan_tier_organization_type_claude_free_is_free() {
        let p = oauth_profile(None, Some("claude_free"), Some(Utc::now()));
        assert!(matches!(
            detect_plan_tier_from_profile(&p).0,
            ClaudePlanTier::Free
        ));
    }

    #[test]
    fn detect_plan_tier_missing_organization_is_free() {
        let p = ClaudeOauthProfile {
            account: ClaudeOauthProfileAccount {
                uuid: None,
                email: None,
                display_name: None,
                created_at: None,
            },
            organization: None,
        };
        assert!(matches!(
            detect_plan_tier_from_profile(&p).0,
            ClaudePlanTier::Free
        ));
    }

    #[test]
    fn detect_plan_tier_no_subscription_created_at_is_free() {
        let p = oauth_profile(None, None, None);
        assert!(matches!(
            detect_plan_tier_from_profile(&p).0,
            ClaudePlanTier::Free
        ));
    }

    #[test]
    fn detect_plan_tier_with_subscription_but_no_identifying_fields_is_unknown() {
        let p = oauth_profile(None, None, Some(Utc::now()));
        assert!(matches!(
            detect_plan_tier_from_profile(&p).0,
            ClaudePlanTier::Unknown
        ));
    }

    #[test]
    fn remote_account_clamps_invite_bonus_to_50() {
        let raw = RemoteAccountResponse {
            email: "a@b".into(),
            trial_started_at: None,
            trial_ends_at: None,
            trial_active: false,
            subscription_active: false,
            subscription_tier: None,
            subscription_started_at: None,
            subscription_renews_at: None,
            subscription_amount_cents: None,
            subscription_billing_period: None,
            subscription_discount_duration: None,
            subscription_discount_duration_in_months: None,
            invite_code: None,
            accepted_invites_count: 0,
            invite_bonus_percent: 999.0,
        };
        assert_eq!(remote_account_to_profile(raw).invite_bonus_percent, 50.0);
    }

    #[test]
    fn remote_account_clamps_negative_invite_bonus_to_zero() {
        let raw = RemoteAccountResponse {
            email: "a@b".into(),
            trial_started_at: None,
            trial_ends_at: None,
            trial_active: false,
            subscription_active: false,
            subscription_tier: None,
            subscription_started_at: None,
            subscription_renews_at: None,
            subscription_amount_cents: None,
            subscription_billing_period: None,
            subscription_discount_duration: None,
            subscription_discount_duration_in_months: None,
            invite_code: None,
            accepted_invites_count: 0,
            invite_bonus_percent: -10.0,
        };
        assert_eq!(remote_account_to_profile(raw).invite_bonus_percent, 0.0);
    }

    // ── Anthropic OAuth usage parser ────────────────────────────────────────

    #[test]
    fn parse_claude_usage_response_decodes_full_payload() {
        let body = serde_json::json!({
            "five_hour": {
                "utilization": 42.5,
                "resets_at": "2026-04-25T15:00:00Z"
            },
            "seven_day": {
                "utilization": 18.75,
                "resets_at": "2026-04-30T00:00:00Z"
            },
            "extra_usage": {
                "is_enabled": true,
                "monthly_limit": 50.0,
                "used_credits": 12.5,
                "utilization": 25.0
            }
        });
        let usage = parse_claude_usage_response(&body).expect("parse usage");

        let five = usage.five_hour.expect("five-hour window");
        assert!((five.utilization - 42.5).abs() < f64::EPSILON);

        let seven = usage.seven_day.expect("seven-day window");
        assert!((seven.utilization - 18.75).abs() < f64::EPSILON);

        let extra = usage.extra_usage.expect("extra-usage block");
        assert!(extra.is_enabled);
        assert_eq!(extra.monthly_limit, Some(50.0));
        assert_eq!(extra.used_credits, Some(12.5));
        assert_eq!(extra.utilization, Some(25.0));
    }

    #[test]
    fn parse_claude_usage_response_returns_error_on_api_error_envelope() {
        let body = serde_json::json!({
            "error": { "message": "rate limit exceeded" }
        });
        let err = parse_claude_usage_response(&body).expect_err("api error");
        assert!(
            err.contains("rate limit exceeded"),
            "expected rate-limit message, got: {err}"
        );
    }

    #[test]
    fn parse_claude_usage_response_returns_error_with_unknown_message_when_message_missing() {
        let body = serde_json::json!({ "error": {} });
        let err = parse_claude_usage_response(&body).expect_err("api error");
        assert!(err.contains("unknown"));
    }

    #[test]
    fn parse_claude_usage_response_skips_windows_missing_required_fields() {
        // Schema-drift smoke: a window object missing `resets_at` should be
        // dropped rather than producing a panic.
        let body = serde_json::json!({
            "five_hour": { "utilization": 10.0 },
            "seven_day": { "resets_at": "2026-04-30T00:00:00Z" },
            "extra_usage": null
        });
        let usage = parse_claude_usage_response(&body).expect("parse usage");
        assert!(usage.five_hour.is_none(), "no resets_at → window dropped");
        assert!(usage.seven_day.is_none(), "no utilization → window dropped");
        assert!(usage.extra_usage.is_none());
    }

    #[test]
    fn parse_claude_usage_response_skips_extra_usage_missing_required_field() {
        let body = serde_json::json!({
            "extra_usage": { "monthly_limit": 50.0 }  // missing is_enabled
        });
        let usage = parse_claude_usage_response(&body).expect("parse");
        assert!(
            usage.extra_usage.is_none(),
            "extra_usage without is_enabled should be dropped"
        );
    }

    #[test]
    fn parse_claude_usage_response_skips_window_with_malformed_resets_at() {
        let body = serde_json::json!({
            "five_hour": { "utilization": 10.0, "resets_at": "not-a-date" }
        });
        let usage = parse_claude_usage_response(&body).expect("parse");
        assert!(usage.five_hour.is_none());
    }

}
