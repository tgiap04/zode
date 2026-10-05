//! Whether a tab holds a finished turn its user has not looked at.
//!
//! Pure on purpose, like the turn tracker it reads from: no gpui, no focus, no
//! clock. Whether the user is looking arrives as an argument, which is what
//! lets each rule below be pinned by a test.
//!
//! It is a flag the user can take back, unlike a posted notification, so it
//! shows the moment a turn ends and holds nothing back for confirmation: a
//! follow-up turn or an interrupt retracts it a moment later.

use crate::turn_tracker::TurnEvent;

#[derive(Default)]
pub(crate) struct AgentAttention {
    unread: bool,
}

impl AgentAttention {
    /// Folds one turn event. `in_view` is whether the user is looking at the
    /// tab right now.
    ///
    /// A turn that ends under the user's eyes is never unread. An interrupt
    /// arrives right behind the `Ended` of the turn it cut short, so it takes
    /// the flag back rather than leaving a finished turn nobody wants to hear
    /// about.
    ///
    /// The approval events are not this fold's business: a pending dialog is
    /// read straight from the tracker, because looking at it does not answer it.
    pub(crate) fn on_turn(&mut self, event: TurnEvent, in_view: bool) {
        match event {
            TurnEvent::Ended => self.unread = !in_view,
            TurnEvent::Started | TurnEvent::Interrupted => self.unread = false,
            TurnEvent::ApprovalNeeded | TurnEvent::ApprovalCleared => {}
        }
    }

    /// Takes the flag back: the user looked at the tab, or the agent ended,
    /// restarted or began a new session and whatever was unread belonged to a
    /// conversation that is gone.
    pub(crate) fn clear(&mut self) {
        self.unread = false;
    }

    pub(crate) fn unread(&self) -> bool {
        self.unread
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_turn_ending_out_of_view_is_unread() {
        let mut attention = AgentAttention::default();
        attention.on_turn(TurnEvent::Ended, false);
        assert!(attention.unread());
    }

    #[test]
    fn a_turn_ending_in_view_is_not_unread() {
        let mut attention = AgentAttention::default();
        attention.on_turn(TurnEvent::Ended, true);
        assert!(!attention.unread());
    }

    #[test]
    fn an_interrupt_right_behind_the_end_leaves_nothing_unread() {
        let mut attention = AgentAttention::default();
        attention.on_turn(TurnEvent::Ended, false);
        attention.on_turn(TurnEvent::Interrupted, false);
        assert!(!attention.unread());
    }

    #[test]
    fn a_new_turn_retracts_an_unread_end() {
        let mut attention = AgentAttention::default();
        attention.on_turn(TurnEvent::Ended, false);
        attention.on_turn(TurnEvent::Started, false);
        assert!(!attention.unread());
    }

    #[test]
    fn approval_events_leave_the_flag_alone() {
        let mut attention = AgentAttention::default();
        attention.on_turn(TurnEvent::Ended, false);
        attention.on_turn(TurnEvent::ApprovalNeeded, false);
        attention.on_turn(TurnEvent::ApprovalCleared, false);
        assert!(attention.unread());
    }

    #[test]
    fn clear_takes_the_flag_back() {
        let mut attention = AgentAttention::default();
        attention.on_turn(TurnEvent::Ended, false);
        attention.clear();
        assert!(!attention.unread());
    }
}
