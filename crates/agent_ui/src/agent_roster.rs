//! The one table an id, an icon and a colour are read from, for every surface
//! that offers the built-in agents -- the rail, the scratch-window menu, and
//! the create-worktree form.
//!
//! Those three used to each hold their own copy of this list, and the copies
//! had already drifted: the rail said "Claude Code", the worktree form said
//! "Claude". A table cannot un-say something it was never asked to hold, so
//! the label is not here -- every consumer reads it from
//! `project::builtin_agent`, which is the one place it is written down.

use gpui::Hsla;
use ui::IconName;

/// The mark one built-in agent is drawn with.
///
/// Only what `project` cannot carry. `project` sits below `icons` in the crate
/// graph, so the id, the display name and the install line live there while the
/// glyph and the hue live here. There is deliberately no `label` field: every
/// consumer reads the name from `project::builtin_agent`, which is what stops
/// the two tables disagreeing about what an agent is called. They already did --
/// the rail said "Claude Code" while the worktree form said "Claude".
pub struct AgentMark {
    pub id: &'static str,
    pub icon: IconName,
    /// Packed rgb rather than `Hsla`, because `Hsla` has no const constructor
    /// and this table has to be a `const`.
    pub color: u32,
}

/// The built-in agents, in rail order.
///
/// A linear scan over five entries, run once per drawn agent mark, is the case
/// `CLAUDE.md` asks to leave alone rather than promoting to a `HashMap` -- a
/// small, bounded collection with no repeated-lookup pressure behind it.
const AGENT_MARKS: &[AgentMark] = &[
    AgentMark {
        id: project::CLAUDE_CODE_AGENT_ID,
        icon: IconName::AiClaude,
        // Anthropic's clay orange.
        color: 0xD97757,
    },
    AgentMark {
        id: project::CODEX_AGENT_ID,
        icon: IconName::AiOpenAi,
        // OpenAI's green.
        color: 0x10A37F,
    },
    AgentMark {
        id: project::ANTIGRAVITY_AGENT_ID,
        icon: IconName::AiAntigravity,
        color: 0x4285F4,
    },
    AgentMark {
        id: project::COPILOT_AGENT_ID,
        icon: IconName::AiCopilot,
        // GitHub's mark is monochrome, so this is a chosen hue rather than a
        // brand one -- picked to stay apart from the other three.
        color: 0x8957E5,
    },
    AgentMark {
        id: project::OPENCODE_AGENT_ID,
        icon: IconName::AiOpencode,
        // opencode's mark is an ASCII wordmark, not a coloured logo, so like
        // Copilot's this is a chosen hue rather than a brand one -- picked to
        // stay apart from the four above.
        color: 0xE8A33D,
    },
];

/// Every built-in agent a surface should offer, in rail order: the id, its glyph,
/// and the name `project` records for it.
///
/// An entry whose id no longer names a built-in is skipped rather than drawn
/// under a placeholder: a button dispatching an id `AgentServerStore` cannot
/// resolve is a button that does nothing.
/// `every_agent_mark_is_a_registered_builtin` is what keeps that branch
/// unreachable in practice.
pub fn agent_marks() -> impl Iterator<Item = (&'static str, IconName, &'static str)> {
    AGENT_MARKS.iter().filter_map(|mark| {
        project::builtin_agent(mark.id).map(|builtin| (mark.id, mark.icon, builtin.display_name))
    })
}

/// The rail draws hard-coded buttons for the built-in agents, and the tab has to
/// carry the same glyph. A lookup through this table rather than
/// `AgentServerStore`: `project` cannot depend on the icon crate, and adding an
/// agent is a deliberate choice of icon, not something to be derived.
pub fn agent_icon(agent: &str) -> IconName {
    AGENT_MARKS
        .iter()
        .find(|mark| mark.id == agent)
        .map_or(IconName::Sparkle, |mark| mark.icon)
}

/// The vendor's own colour for an agent's mark.
///
/// A brand colour is the one case where detaching from the theme is right:
/// Claude's orange is Claude's orange on a light theme and a dark one, and
/// recolouring it by theme would make the mark stop being the mark. Everything
/// else in this crate uses semantic `Color` variants.
pub fn agent_color(agent: &str) -> Hsla {
    AGENT_MARKS
        .iter()
        .find(|mark| mark.id == agent)
        .map_or(gpui::rgb(0x9A9A9A).into(), |mark| {
            gpui::rgb(mark.color).into()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every mark must name an agent the store actually knows about, or the
    /// click resolves to nothing and the user gets a button that does nothing.
    #[test]
    fn every_agent_mark_is_a_registered_builtin() {
        for mark in AGENT_MARKS {
            assert!(
                project::builtin_agent(mark.id).is_some(),
                "the roster draws `{}`, which no built-in agent claims",
                mark.id
            );
        }
    }

    /// The reverse of the guard above: a builtin with no mark is how an agent
    /// would be half-added -- registered in `project` but invisible on every
    /// surface that reads this table.
    #[test]
    fn every_builtin_has_exactly_one_agent_mark() {
        for builtin in project::BUILTIN_AGENTS {
            let count = AGENT_MARKS
                .iter()
                .filter(|mark| mark.id == builtin.id)
                .count();
            assert_eq!(
                count, 1,
                "`{}` must have exactly one row in AGENT_MARKS, found {count}",
                builtin.id
            );
        }
    }

    /// `agent_icon` and `agent_color` fall back to the placeholder mark for an
    /// id no built-in claims, rather than panicking on it.
    #[test]
    fn unknown_agent_falls_back_to_the_placeholder_mark() {
        assert_eq!(agent_icon("not-a-real-agent"), IconName::Sparkle);
        assert_eq!(agent_color("not-a-real-agent"), gpui::rgb(0x9A9A9A).into());
    }
}
