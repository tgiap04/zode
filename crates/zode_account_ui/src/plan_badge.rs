use gpui::{Context, Entity, IntoElement, Render, Subscription, Window};
use ui::{Chip, Tooltip, prelude::*};
use workspace::{ItemHandle, StatusItemView};
use zode_account::{Account, PlanSummary, PlanTier};

use crate::{ManagePlan, UpgradePlan, plan_summary};

/// A tag at the right end of the status bar naming the account's plan: grey for
/// Free, the brand orange for a paid plan. Signed out, or a plan not read yet,
/// shows nothing: the tag only ever states what the server said, never a guess.
pub struct PlanBadge {
    account: Option<Entity<Account>>,
    _account_subscription: Option<Subscription>,
}

impl PlanBadge {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let account = Account::global(cx);
        let subscription = account
            .as_ref()
            .map(|account| cx.observe(account, |_, _, cx| cx.notify()));
        Self {
            account,
            _account_subscription: subscription,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlanBadgeKind {
    pub label: &'static str,
    /// Paid plans wear the brand colour and open billing; Free is grey and
    /// opens the plans page.
    pub paid: bool,
}

/// The tag for a plan, or `None` when there is no plan to name.
pub fn plan_badge(plan: Option<&PlanSummary>) -> Option<PlanBadgeKind> {
    let (label, paid) = match plan?.tier {
        PlanTier::Free => ("FREE", false),
        PlanTier::Pro => ("PRO", true),
        PlanTier::Team => ("TEAM", true),
    };
    Some(PlanBadgeKind { label, paid })
}

impl Render for PlanBadge {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let plan = self
            .account
            .as_ref()
            .and_then(|account| account.read(cx).plan());
        let Some((badge, summary)) =
            plan.and_then(|plan| Some((plan_badge(Some(plan))?, plan_summary(plan))))
        else {
            return div().into_any_element();
        };

        let (label_color, border_color, hint) = if badge.paid {
            // `brand_accent`, not `Color::Accent`: the theme's accent is blue, and
            // a paid plan is a fact about the app, coloured the same in every theme.
            let orange = ui::brand_accent(cx);
            (
                Color::Custom(orange),
                orange.opacity(0.5),
                "Manage your plan on the web",
            )
        } else {
            (
                Color::Muted,
                cx.theme().colors().border_variant,
                "See plans on the web",
            )
        };

        div()
            .id("plan-badge")
            .cursor_pointer()
            .child(
                Chip::new(badge.label)
                    .label_color(label_color)
                    .border_color(border_color),
            )
            .tooltip(move |_window, cx| Tooltip::with_meta(summary.clone(), None, hint, cx))
            .on_click(move |_, window, cx| {
                if badge.paid {
                    window.dispatch_action(Box::new(ManagePlan), cx)
                } else {
                    window.dispatch_action(Box::new(UpgradePlan), cx)
                }
            })
            .into_any_element()
    }
}

impl StatusItemView for PlanBadge {
    /// Nothing to follow: the plan is the same whichever tab is in front.
    fn set_active_pane_item(
        &mut self,
        _active_pane_item: Option<&dyn ItemHandle>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan_with_tier(tier: &str) -> PlanSummary {
        let source = if tier == "free" {
            "free"
        } else {
            "subscription"
        };
        let body = format!(
            r#"{{
                "tier": "{tier}",
                "source": "{source}",
                "status": "active",
                "limits": {{
                    "syncDevices": {{ "used": 1, "max": null }},
                    "envSync": true,
                    "relayDailyBytes": 0,
                    "teamVault": false
                }}
            }}"#
        );
        PlanSummary::parse(&body, "https://zodekit.site").expect("a plan the server could send")
    }

    #[test]
    fn every_known_plan_gets_a_tag_and_only_paid_ones_are_branded() {
        assert_eq!(plan_badge(None), None, "a plan not read yet is not guessed");
        assert_eq!(
            plan_badge(Some(&plan_with_tier("free"))),
            Some(PlanBadgeKind {
                label: "FREE",
                paid: false
            })
        );
        assert_eq!(
            plan_badge(Some(&plan_with_tier("pro"))),
            Some(PlanBadgeKind {
                label: "PRO",
                paid: true
            })
        );
        assert_eq!(
            plan_badge(Some(&plan_with_tier("team"))),
            Some(PlanBadgeKind {
                label: "TEAM",
                paid: true
            })
        );
    }
}
