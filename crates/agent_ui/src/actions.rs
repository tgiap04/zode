//! The one action the agent tab still needs.
//!
//! This file used to carry ~30 actions, most of them for the chat UI — a thread
//! menu, a model selector, a mode selector, mention handling, diff review. That
//! whole surface is gone; the agent runs in a terminal and nothing about it is
//! toggled or configured from a menu.
//!
//! `RenameAgent` survives because renaming the *tab* is not a chat feature: two
//! terminal sessions of one agent still need telling apart.

use gpui::actions;

actions!(
    agent,
    [
        /// Renames the agent tab, so two sessions of one agent can be told apart.
        RenameAgent,
    ]
);
