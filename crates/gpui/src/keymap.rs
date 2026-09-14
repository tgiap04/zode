mod binding;
mod context;

pub use binding::*;
pub use context::*;

use crate::{Action, AsKeystroke, Keystroke, Unbind, is_no_action, is_unbind};
use collections::{HashMap, HashSet};
use smallvec::SmallVec;
use std::any::TypeId;

/// An opaque identifier of which version of the keymap is currently active.
/// The keymap's version is changed whenever bindings are added or removed.
#[derive(Copy, Clone, Eq, PartialEq, Default)]
pub struct KeymapVersion(usize);

/// A collection of key bindings for the user's application.
#[derive(Default)]
pub struct Keymap {
    bindings: Vec<KeyBinding>,
    binding_indices_by_action_id: HashMap<TypeId, SmallVec<[usize; 3]>>,
    disabled_binding_indices: Vec<usize>,
    version: KeymapVersion,
}

/// Index of a binding within a keymap.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct BindingIndex(usize);

fn disabled_binding_matches_context(disabled_binding: &KeyBinding, binding: &KeyBinding) -> bool {
    match (
        &disabled_binding.context_predicate,
        &binding.context_predicate,
    ) {
        (None, _) => true,
        (Some(_), None) => false,
        (Some(disabled_predicate), Some(predicate)) => disabled_predicate.is_superset(predicate),
    }
}

fn binding_is_unbound(disabled_binding: &KeyBinding, binding: &KeyBinding) -> bool {
    disabled_binding.keystrokes == binding.keystrokes
        && disabled_binding
            .action()
            .as_any()
            .downcast_ref::<Unbind>()
            .is_some_and(|unbind| unbind.0.as_ref() == binding.action.name())
}

fn is_disable_binding(binding: &KeyBinding) -> bool {
    is_no_action(&*binding.action) || is_unbind(&*binding.action)
}

/// True when `binding`'s meta names the highest-precedence ("user") source: unset, or explicitly
/// `KeybindSource::User` (index 0). `None` reads as user because most bindings — ad-hoc test
/// bindings, `cx.bind_keys` call sites with no source tracking — never stamp a source at all, and
/// defaulting them to the lowest tier would silently lose to any labeled binding.
fn has_user_source(binding: &KeyBinding) -> bool {
    match binding.meta {
        None => true,
        Some(meta) => meta.0 == 0,
    }
}

/// Orders matched bindings, highest precedence first. Shared by `bindings_for_input` and
/// `possible_next_bindings_for_input` so the dispatcher and the which-key modal can never disagree
/// about order.
///
/// Depth-then-registration is the historical rule, and it stays the whole rule whenever a disable
/// binding is competing for the chord: `NoAction` and `Unbind` suppress by reaching a context, so
/// letting anything outrank them would change what a keymap disables — a shallow `"key": null`
/// could reach past its own context, or a real binding could escape one aimed at it.
///
/// With no disable in play, a binding the user wrote outranks a shipped one even when the shipped
/// one names a deeper context. That is the case this exists for: the keymap editor writes contexts,
/// and a user who names `Workspace` should not lose to a default that named `Editor`.
///
/// The rule is chosen once for the whole slice rather than per pair, and that is load-bearing. Per
/// pair the two do not compose into a total order: a user binding outranks a deeper default, a
/// deeper disable outranks that user binding, and that deeper default outranks the disable — a
/// cycle, which `sort_by` is under no obligation to survive and is permitted to reject outright.
fn sort_by_precedence(matched: &mut [(usize, BindingIndex, &KeyBinding)]) {
    let disable_in_play = matched
        .iter()
        .any(|(_, _, binding)| is_disable_binding(binding));

    if disable_in_play {
        matched.sort_by(|(depth_a, ix_a, _), (depth_b, ix_b, _)| {
            depth_b.cmp(depth_a).then(ix_b.cmp(ix_a))
        });
    } else {
        matched.sort_by(|(depth_a, ix_a, binding_a), (depth_b, ix_b, binding_b)| {
            has_user_source(binding_b)
                .cmp(&has_user_source(binding_a))
                .then(depth_b.cmp(depth_a))
                .then(ix_b.cmp(ix_a))
        });
    }
}

impl Keymap {
    /// Create a new keymap with the given bindings.
    pub fn new(bindings: Vec<KeyBinding>) -> Self {
        let mut this = Self::default();
        this.add_bindings(bindings);
        this
    }

    /// Get the current version of the keymap.
    pub fn version(&self) -> KeymapVersion {
        self.version
    }

    /// Add more bindings to the keymap.
    pub fn add_bindings<T: IntoIterator<Item = KeyBinding>>(&mut self, bindings: T) {
        for binding in bindings {
            let action_id = binding.action().as_any().type_id();
            if is_no_action(&*binding.action) || is_unbind(&*binding.action) {
                self.disabled_binding_indices.push(self.bindings.len());
            } else {
                self.binding_indices_by_action_id
                    .entry(action_id)
                    .or_default()
                    .push(self.bindings.len());
            }
            self.bindings.push(binding);
        }

        self.version.0 += 1;
    }

    /// Reset this keymap to its initial state.
    pub fn clear(&mut self) {
        self.bindings.clear();
        self.binding_indices_by_action_id.clear();
        self.disabled_binding_indices.clear();
        self.version.0 += 1;
    }

    /// Iterate over all bindings, in the order they were added.
    pub fn bindings(&self) -> impl DoubleEndedIterator<Item = &KeyBinding> + ExactSizeIterator {
        self.bindings.iter()
    }

    /// Iterate over all bindings for the given action, in the order they were added. For display,
    /// the last binding should take precedence.
    pub fn bindings_for_action<'a>(
        &'a self,
        action: &'a dyn Action,
    ) -> impl 'a + DoubleEndedIterator<Item = &'a KeyBinding> {
        let action_id = action.type_id();
        let binding_indices = self
            .binding_indices_by_action_id
            .get(&action_id)
            .map_or(&[] as _, SmallVec::as_slice)
            .iter();

        binding_indices.filter_map(|ix| {
            let binding = &self.bindings[*ix];
            if !binding.action().partial_eq(action) {
                return None;
            }

            for disabled_ix in &self.disabled_binding_indices {
                if disabled_ix > ix {
                    let disabled_binding = &self.bindings[*disabled_ix];
                    if disabled_binding.keystrokes != binding.keystrokes {
                        continue;
                    }

                    if is_no_action(&*disabled_binding.action) {
                        if disabled_binding_matches_context(disabled_binding, binding) {
                            return None;
                        }
                    } else if is_unbind(&*disabled_binding.action)
                        && disabled_binding_matches_context(disabled_binding, binding)
                        && binding_is_unbound(disabled_binding, binding)
                    {
                        return None;
                    }
                }
            }

            Some(binding)
        })
    }

    /// Returns all bindings that might match the input without checking context. The bindings
    /// returned in precedence order (reverse of the order they were added to the keymap).
    pub fn all_bindings_for_input(&self, input: &[Keystroke]) -> Vec<KeyBinding> {
        self.bindings()
            .rev()
            .filter(|binding| {
                binding
                    .match_keystrokes(input)
                    .is_some_and(|pending| !pending)
            })
            .cloned()
            .collect()
    }

    /// Returns a list of bindings that match the given input, and a boolean indicating whether or
    /// not more bindings might match if the input was longer. Bindings are returned in precedence
    /// order (higher precedence first, reverse of the order they were added to the keymap).
    ///
    /// Precedence is decided by three keys, in order:
    /// 1. Source tier: a binding from `KeybindSource::User` (or with no source set at all) outranks
    ///    any binding from `Vim`, `Base`, `Default` or `Unknown`, regardless of context depth. This
    ///    is what lets a user's own keymap win over a non-user binding at a deeper context for the
    ///    same chord.
    /// 2. Context depth: within a tier, matches on the Editor take precedence over matches on the
    ///    Pane, then the Workspace, etc. Bindings with no context are treated as the same as the
    ///    deepest context.
    /// 3. Registration order: bindings added to the keymap later take precedence over earlier ones
    ///    at the same tier and depth. User bindings are added after built-in bindings.
    ///
    /// `"key": null` (`NoAction`) and unbind bindings are excluded from the source-tier promotion
    /// and always sort by depth then registration order, so a user's shallow disable entry cannot
    /// reach past its own context and suppress a deeper built-in binding the user never touched. If
    /// a user has disabled a binding with `"x": null` it will not be returned. Disabled bindings are
    /// evaluated with the same precedence rules so you can disable a rule in a given context only.
    pub fn bindings_for_input(
        &self,
        input: &[impl AsKeystroke],
        context_stack: &[KeyContext],
    ) -> (SmallVec<[KeyBinding; 1]>, bool) {
        let mut matched_bindings = SmallVec::<[(usize, BindingIndex, &KeyBinding); 1]>::new();
        let mut pending_bindings = SmallVec::<[(BindingIndex, &KeyBinding); 1]>::new();

        for (ix, binding) in self.bindings().enumerate().rev() {
            let Some(depth) = self.binding_enabled(binding, context_stack) else {
                continue;
            };
            let Some(pending) = binding.match_keystrokes(input) else {
                continue;
            };

            if !pending {
                matched_bindings.push((depth, BindingIndex(ix), binding));
            } else {
                pending_bindings.push((BindingIndex(ix), binding));
            }
        }

        sort_by_precedence(&mut matched_bindings);

        let mut bindings: SmallVec<[_; 1]> = SmallVec::new();
        let mut first_binding_index = None;
        let mut unbound_bindings: Vec<&KeyBinding> = Vec::new();

        for (_, ix, binding) in matched_bindings {
            if is_no_action(&*binding.action) {
                // Only break if this is a user-defined NoAction binding. This allows user keymaps
                // to override base keymap NoAction bindings; `has_user_source` is the same "meta
                // unset or User" check the sort comparator uses.
                if has_user_source(binding) {
                    break;
                }
                // For non-user NoAction bindings, continue searching for user overrides
                continue;
            }

            if is_unbind(&*binding.action) {
                unbound_bindings.push(binding);
                continue;
            }

            if unbound_bindings
                .iter()
                .any(|disabled_binding| binding_is_unbound(disabled_binding, binding))
            {
                continue;
            }

            bindings.push(binding.clone());
            first_binding_index.get_or_insert(ix);
        }

        let mut pending = HashSet::default();
        for (ix, binding) in pending_bindings.into_iter().rev() {
            if let Some(binding_ix) = first_binding_index
                && binding_ix > ix
            {
                continue;
            }
            if is_no_action(&*binding.action) || is_unbind(&*binding.action) {
                pending.remove(&&binding.keystrokes);
                continue;
            }
            pending.insert(&binding.keystrokes);
        }

        (bindings, !pending.is_empty())
    }
    /// Check if the given binding is enabled, given a certain key context.
    /// Returns the deepest depth at which the binding matches, or None if it doesn't match.
    fn binding_enabled(&self, binding: &KeyBinding, contexts: &[KeyContext]) -> Option<usize> {
        if let Some(predicate) = &binding.context_predicate {
            predicate.depth_of(contexts)
        } else {
            Some(contexts.len())
        }
    }

    /// Find the bindings that can follow the current input sequence.
    pub fn possible_next_bindings_for_input(
        &self,
        input: &[Keystroke],
        context_stack: &[KeyContext],
    ) -> Vec<KeyBinding> {
        let mut bindings = self
            .bindings()
            .enumerate()
            .rev()
            .filter_map(|(ix, binding)| {
                let depth = self.binding_enabled(binding, context_stack)?;
                let pending = binding.match_keystrokes(input);
                match pending {
                    None => None,
                    Some(is_pending) => {
                        if !is_pending
                            || is_no_action(&*binding.action)
                            || is_unbind(&*binding.action)
                        {
                            return None;
                        }
                        Some((depth, BindingIndex(ix), binding))
                    }
                }
            })
            .collect::<Vec<_>>();

        sort_by_precedence(&mut bindings);

        bindings
            .into_iter()
            .map(|(_, _, binding)| binding.clone())
            .collect::<Vec<_>>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate as gpui;
    use gpui::{NoAction, Unbind};

    actions!(
        test_only,
        [ActionAlpha, ActionBeta, ActionGamma, ActionDelta,]
    );

    #[test]
    fn test_keymap() {
        let bindings = [
            KeyBinding::new("ctrl-a", ActionAlpha {}, None),
            KeyBinding::new("ctrl-a", ActionBeta {}, Some("pane")),
            KeyBinding::new("ctrl-a", ActionGamma {}, Some("editor && mode==full")),
        ];

        let mut keymap = Keymap::default();
        keymap.add_bindings(bindings.clone());

        // global bindings are enabled in all contexts
        assert_eq!(keymap.binding_enabled(&bindings[0], &[]), Some(0));
        assert_eq!(
            keymap.binding_enabled(&bindings[0], &[KeyContext::parse("terminal").unwrap()]),
            Some(1)
        );

        // contextual bindings are enabled in contexts that match their predicate
        assert_eq!(
            keymap.binding_enabled(&bindings[1], &[KeyContext::parse("barf x=y").unwrap()]),
            None
        );
        assert_eq!(
            keymap.binding_enabled(&bindings[1], &[KeyContext::parse("pane x=y").unwrap()]),
            Some(1)
        );

        assert_eq!(
            keymap.binding_enabled(&bindings[2], &[KeyContext::parse("editor").unwrap()]),
            None
        );
        assert_eq!(
            keymap.binding_enabled(
                &bindings[2],
                &[KeyContext::parse("editor mode=full").unwrap()]
            ),
            Some(1)
        );
    }

    #[test]
    fn test_depth_precedence() {
        let bindings = [
            KeyBinding::new("ctrl-a", ActionBeta {}, Some("pane")),
            KeyBinding::new("ctrl-a", ActionGamma {}, Some("editor")),
        ];

        let mut keymap = Keymap::default();
        keymap.add_bindings(bindings);

        let (result, pending) = keymap.bindings_for_input(
            &[Keystroke::parse("ctrl-a").unwrap()],
            &[
                KeyContext::parse("pane").unwrap(),
                KeyContext::parse("editor").unwrap(),
            ],
        );

        assert!(!pending);
        assert_eq!(result.len(), 2);
        assert!(result[0].action.partial_eq(&ActionGamma {}));
        assert!(result[1].action.partial_eq(&ActionBeta {}));
    }

    #[test]
    fn test_keymap_disabled() {
        let bindings = [
            KeyBinding::new("ctrl-a", ActionAlpha {}, Some("editor")),
            KeyBinding::new("ctrl-b", ActionAlpha {}, Some("editor")),
            KeyBinding::new("ctrl-a", NoAction {}, Some("editor && mode==full")),
            KeyBinding::new("ctrl-b", NoAction {}, None),
        ];

        let mut keymap = Keymap::default();
        keymap.add_bindings(bindings);

        // binding is only enabled in a specific context
        assert!(
            keymap
                .bindings_for_input(
                    &[Keystroke::parse("ctrl-a").unwrap()],
                    &[KeyContext::parse("barf").unwrap()],
                )
                .0
                .is_empty()
        );
        assert!(
            !keymap
                .bindings_for_input(
                    &[Keystroke::parse("ctrl-a").unwrap()],
                    &[KeyContext::parse("editor").unwrap()],
                )
                .0
                .is_empty()
        );

        // binding is disabled in a more specific context
        assert!(
            keymap
                .bindings_for_input(
                    &[Keystroke::parse("ctrl-a").unwrap()],
                    &[KeyContext::parse("editor mode=full").unwrap()],
                )
                .0
                .is_empty()
        );

        // binding is globally disabled
        assert!(
            keymap
                .bindings_for_input(
                    &[Keystroke::parse("ctrl-b").unwrap()],
                    &[KeyContext::parse("barf").unwrap()],
                )
                .0
                .is_empty()
        );
    }

    #[test]
    /// Tests for https://github.com/zed-industries/zed/issues/30259
    fn test_multiple_keystroke_binding_disabled() {
        let bindings = [
            KeyBinding::new("space w w", ActionAlpha {}, Some("workspace")),
            KeyBinding::new("space w w", NoAction {}, Some("editor")),
        ];

        let mut keymap = Keymap::default();
        keymap.add_bindings(bindings);

        let space = || Keystroke::parse("space").unwrap();
        let w = || Keystroke::parse("w").unwrap();

        let space_w = [space(), w()];
        let space_w_w = [space(), w(), w()];

        let workspace_context = || [KeyContext::parse("workspace").unwrap()];

        let editor_workspace_context = || {
            [
                KeyContext::parse("workspace").unwrap(),
                KeyContext::parse("editor").unwrap(),
            ]
        };

        // Ensure `space` results in pending input on the workspace, but not editor
        let space_workspace = keymap.bindings_for_input(&[space()], &workspace_context());
        assert!(space_workspace.0.is_empty());
        assert!(space_workspace.1);

        let space_editor = keymap.bindings_for_input(&[space()], &editor_workspace_context());
        assert!(space_editor.0.is_empty());
        assert!(!space_editor.1);

        // Ensure `space w` results in pending input on the workspace, but not editor
        let space_w_workspace = keymap.bindings_for_input(&space_w, &workspace_context());
        assert!(space_w_workspace.0.is_empty());
        assert!(space_w_workspace.1);

        let space_w_editor = keymap.bindings_for_input(&space_w, &editor_workspace_context());
        assert!(space_w_editor.0.is_empty());
        assert!(!space_w_editor.1);

        // Ensure `space w w` results in the binding in the workspace, but not in the editor
        let space_w_w_workspace = keymap.bindings_for_input(&space_w_w, &workspace_context());
        assert!(!space_w_w_workspace.0.is_empty());
        assert!(!space_w_w_workspace.1);

        let space_w_w_editor = keymap.bindings_for_input(&space_w_w, &editor_workspace_context());
        assert!(space_w_w_editor.0.is_empty());
        assert!(!space_w_w_editor.1);

        // Now test what happens if we have another binding defined AFTER the NoAction
        // that should result in pending
        let bindings = [
            KeyBinding::new("space w w", ActionAlpha {}, Some("workspace")),
            KeyBinding::new("space w w", NoAction {}, Some("editor")),
            KeyBinding::new("space w x", ActionAlpha {}, Some("editor")),
        ];
        let mut keymap = Keymap::default();
        keymap.add_bindings(bindings);

        let space_editor = keymap.bindings_for_input(&[space()], &editor_workspace_context());
        assert!(space_editor.0.is_empty());
        assert!(space_editor.1);

        // Now test what happens if we have another binding defined BEFORE the NoAction
        // that should result in pending
        let bindings = [
            KeyBinding::new("space w w", ActionAlpha {}, Some("workspace")),
            KeyBinding::new("space w x", ActionAlpha {}, Some("editor")),
            KeyBinding::new("space w w", NoAction {}, Some("editor")),
        ];
        let mut keymap = Keymap::default();
        keymap.add_bindings(bindings);

        let space_editor = keymap.bindings_for_input(&[space()], &editor_workspace_context());
        assert!(space_editor.0.is_empty());
        assert!(space_editor.1);

        // Now test what happens if we have another binding defined at a higher context
        // that should result in pending
        let bindings = [
            KeyBinding::new("space w w", ActionAlpha {}, Some("workspace")),
            KeyBinding::new("space w x", ActionAlpha {}, Some("workspace")),
            KeyBinding::new("space w w", NoAction {}, Some("editor")),
        ];
        let mut keymap = Keymap::default();
        keymap.add_bindings(bindings);

        let space_editor = keymap.bindings_for_input(&[space()], &editor_workspace_context());
        assert!(space_editor.0.is_empty());
        assert!(space_editor.1);
    }

    #[test]
    fn test_override_multikey() {
        let bindings = [
            KeyBinding::new("ctrl-w left", ActionAlpha {}, Some("editor")),
            KeyBinding::new("ctrl-w", NoAction {}, Some("editor")),
        ];

        let mut keymap = Keymap::default();
        keymap.add_bindings(bindings);

        // Ensure `space` results in pending input on the workspace, but not editor
        let (result, pending) = keymap.bindings_for_input(
            &[Keystroke::parse("ctrl-w").unwrap()],
            &[KeyContext::parse("editor").unwrap()],
        );
        assert!(result.is_empty());
        assert!(pending);

        let bindings = [
            KeyBinding::new("ctrl-w left", ActionAlpha {}, Some("editor")),
            KeyBinding::new("ctrl-w", ActionBeta {}, Some("editor")),
        ];

        let mut keymap = Keymap::default();
        keymap.add_bindings(bindings);

        // Ensure `space` results in pending input on the workspace, but not editor
        let (result, pending) = keymap.bindings_for_input(
            &[Keystroke::parse("ctrl-w").unwrap()],
            &[KeyContext::parse("editor").unwrap()],
        );
        assert_eq!(result.len(), 1);
        assert!(!pending);
    }

    #[test]
    fn test_simple_disable() {
        let bindings = [
            KeyBinding::new("ctrl-x", ActionAlpha {}, Some("editor")),
            KeyBinding::new("ctrl-x", NoAction {}, Some("editor")),
        ];

        let mut keymap = Keymap::default();
        keymap.add_bindings(bindings);

        // Ensure `space` results in pending input on the workspace, but not editor
        let (result, pending) = keymap.bindings_for_input(
            &[Keystroke::parse("ctrl-x").unwrap()],
            &[KeyContext::parse("editor").unwrap()],
        );
        assert!(result.is_empty());
        assert!(!pending);
    }

    #[test]
    fn test_fail_to_disable() {
        // disabled at the wrong level
        let bindings = [
            KeyBinding::new("ctrl-x", ActionAlpha {}, Some("editor")),
            KeyBinding::new("ctrl-x", NoAction {}, Some("workspace")),
        ];

        let mut keymap = Keymap::default();
        keymap.add_bindings(bindings);

        // Ensure `space` results in pending input on the workspace, but not editor
        let (result, pending) = keymap.bindings_for_input(
            &[Keystroke::parse("ctrl-x").unwrap()],
            &[
                KeyContext::parse("workspace").unwrap(),
                KeyContext::parse("editor").unwrap(),
            ],
        );
        assert_eq!(result.len(), 1);
        assert!(!pending);
    }

    #[test]
    fn test_disable_deeper() {
        let bindings = [
            KeyBinding::new("ctrl-x", ActionAlpha {}, Some("workspace")),
            KeyBinding::new("ctrl-x", NoAction {}, Some("editor")),
        ];

        let mut keymap = Keymap::default();
        keymap.add_bindings(bindings);

        // Ensure `space` results in pending input on the workspace, but not editor
        let (result, pending) = keymap.bindings_for_input(
            &[Keystroke::parse("ctrl-x").unwrap()],
            &[
                KeyContext::parse("workspace").unwrap(),
                KeyContext::parse("editor").unwrap(),
            ],
        );
        assert_eq!(result.len(), 0);
        assert!(!pending);
    }

    #[test]
    fn test_pending_match_enabled() {
        let bindings = [
            KeyBinding::new("ctrl-x", ActionBeta, Some("vim_mode == normal")),
            KeyBinding::new("ctrl-x 0", ActionAlpha, Some("Workspace")),
        ];
        let mut keymap = Keymap::default();
        keymap.add_bindings(bindings);

        let matched = keymap.bindings_for_input(
            &[Keystroke::parse("ctrl-x")].map(Result::unwrap),
            &[
                KeyContext::parse("Workspace"),
                KeyContext::parse("Pane"),
                KeyContext::parse("Editor vim_mode=normal"),
            ]
            .map(Result::unwrap),
        );
        assert_eq!(matched.0.len(), 1);
        assert!(matched.0[0].action.partial_eq(&ActionBeta));
        assert!(matched.1);
    }

    #[test]
    fn test_pending_match_enabled_extended() {
        let bindings = [
            KeyBinding::new("ctrl-x", ActionBeta, Some("vim_mode == normal")),
            KeyBinding::new("ctrl-x 0", NoAction, Some("Workspace")),
        ];
        let mut keymap = Keymap::default();
        keymap.add_bindings(bindings);

        let matched = keymap.bindings_for_input(
            &[Keystroke::parse("ctrl-x")].map(Result::unwrap),
            &[
                KeyContext::parse("Workspace"),
                KeyContext::parse("Pane"),
                KeyContext::parse("Editor vim_mode=normal"),
            ]
            .map(Result::unwrap),
        );
        assert_eq!(matched.0.len(), 1);
        assert!(matched.0[0].action.partial_eq(&ActionBeta));
        assert!(!matched.1);
        let bindings = [
            KeyBinding::new("ctrl-x", ActionBeta, Some("Workspace")),
            KeyBinding::new("ctrl-x 0", NoAction, Some("vim_mode == normal")),
        ];
        let mut keymap = Keymap::default();
        keymap.add_bindings(bindings);

        let matched = keymap.bindings_for_input(
            &[Keystroke::parse("ctrl-x")].map(Result::unwrap),
            &[
                KeyContext::parse("Workspace"),
                KeyContext::parse("Pane"),
                KeyContext::parse("Editor vim_mode=normal"),
            ]
            .map(Result::unwrap),
        );
        assert_eq!(matched.0.len(), 1);
        assert!(matched.0[0].action.partial_eq(&ActionBeta));
        assert!(!matched.1);
    }

    #[test]
    fn test_overriding_prefix() {
        let bindings = [
            KeyBinding::new("ctrl-x 0", ActionAlpha, Some("Workspace")),
            KeyBinding::new("ctrl-x", ActionBeta, Some("vim_mode == normal")),
        ];
        let mut keymap = Keymap::default();
        keymap.add_bindings(bindings);

        let matched = keymap.bindings_for_input(
            &[Keystroke::parse("ctrl-x")].map(Result::unwrap),
            &[
                KeyContext::parse("Workspace"),
                KeyContext::parse("Pane"),
                KeyContext::parse("Editor vim_mode=normal"),
            ]
            .map(Result::unwrap),
        );
        assert_eq!(matched.0.len(), 1);
        assert!(matched.0[0].action.partial_eq(&ActionBeta));
        assert!(!matched.1);
    }

    #[test]
    fn test_context_precedence_with_same_source() {
        // Test case: User has both Workspace and Editor bindings for the same key
        // Editor binding should take precedence over Workspace binding
        let bindings = [
            KeyBinding::new("cmd-r", ActionAlpha {}, Some("Workspace")),
            KeyBinding::new("cmd-r", ActionBeta {}, Some("Editor")),
        ];

        let mut keymap = Keymap::default();
        keymap.add_bindings(bindings);

        // Test with context stack: [Workspace, Editor] (Editor is deeper)
        let (result, _) = keymap.bindings_for_input(
            &[Keystroke::parse("cmd-r").unwrap()],
            &[
                KeyContext::parse("Workspace").unwrap(),
                KeyContext::parse("Editor").unwrap(),
            ],
        );

        // Both bindings should be returned, but Editor binding should be first (highest precedence)
        assert_eq!(result.len(), 2);
        assert!(result[0].action.partial_eq(&ActionBeta {})); // Editor binding first
        assert!(result[1].action.partial_eq(&ActionAlpha {})); // Workspace binding second
    }

    #[test]
    fn test_bindings_for_action() {
        let bindings = [
            KeyBinding::new("ctrl-a", ActionAlpha {}, Some("pane")),
            KeyBinding::new("ctrl-b", ActionBeta {}, Some("editor && mode == full")),
            KeyBinding::new("ctrl-c", ActionGamma {}, Some("workspace")),
            KeyBinding::new("ctrl-a", NoAction {}, Some("pane && active")),
            KeyBinding::new("ctrl-b", NoAction {}, Some("editor")),
        ];

        let mut keymap = Keymap::default();
        keymap.add_bindings(bindings);

        assert_bindings(&keymap, &ActionAlpha {}, &["ctrl-a"]);
        assert_bindings(&keymap, &ActionBeta {}, &[]);
        assert_bindings(&keymap, &ActionGamma {}, &["ctrl-c"]);

        #[track_caller]
        fn assert_bindings(keymap: &Keymap, action: &dyn Action, expected: &[&str]) {
            let actual = keymap
                .bindings_for_action(action)
                .map(|binding| binding.keystrokes[0].inner().unparse())
                .collect::<Vec<_>>();
            assert_eq!(actual, expected, "{:?}", action);
        }
    }

    #[test]
    fn test_targeted_unbind_ignores_target_context() {
        let bindings = [
            KeyBinding::new("tab", ActionAlpha {}, Some("Editor")),
            KeyBinding::new("tab", ActionBeta {}, Some("Editor && showing_completions")),
            KeyBinding::new(
                "tab",
                Unbind("test_only::ActionAlpha".into()),
                Some("Editor && edit_prediction"),
            ),
        ];

        let mut keymap = Keymap::default();
        keymap.add_bindings(bindings);

        let (result, pending) = keymap.bindings_for_input(
            &[Keystroke::parse("tab").unwrap()],
            &[KeyContext::parse("Editor showing_completions edit_prediction").unwrap()],
        );

        assert!(!pending);
        assert_eq!(result.len(), 1);
        assert!(result[0].action.partial_eq(&ActionBeta {}));
    }

    #[test]
    fn test_bindings_for_action_keeps_binding_for_narrower_targeted_unbind() {
        let bindings = [
            KeyBinding::new("tab", ActionAlpha {}, Some("Editor")),
            KeyBinding::new(
                "tab",
                Unbind("test_only::ActionAlpha".into()),
                Some("Editor && edit_prediction"),
            ),
            KeyBinding::new("tab", ActionBeta {}, Some("Editor && showing_completions")),
        ];

        let mut keymap = Keymap::default();
        keymap.add_bindings(bindings);

        assert_bindings(&keymap, &ActionAlpha {}, &["tab"]);
        assert_bindings(&keymap, &ActionBeta {}, &["tab"]);

        #[track_caller]
        fn assert_bindings(keymap: &Keymap, action: &dyn Action, expected: &[&str]) {
            let actual = keymap
                .bindings_for_action(action)
                .map(|binding| binding.keystrokes[0].inner().unparse())
                .collect::<Vec<_>>();
            assert_eq!(actual, expected, "{:?}", action);
        }
    }

    #[test]
    fn test_bindings_for_action_removes_binding_for_broader_targeted_unbind() {
        let bindings = [
            KeyBinding::new("tab", ActionAlpha {}, Some("Editor && edit_prediction")),
            KeyBinding::new(
                "tab",
                Unbind("test_only::ActionAlpha".into()),
                Some("Editor"),
            ),
        ];

        let mut keymap = Keymap::default();
        keymap.add_bindings(bindings);

        assert!(keymap.bindings_for_action(&ActionAlpha {}).next().is_none());
    }

    #[test]
    fn test_source_precedence_sorting() {
        // KeybindSource precedence: User (0) > Vim (1) > Base (2) > Default (3)
        // Test that user keymaps take precedence over default keymaps regardless of context depth
        let mut keymap = Keymap::default();

        // Add a default keymap binding first
        let mut default_binding = KeyBinding::new("cmd-r", ActionAlpha {}, Some("Editor"));
        default_binding.set_meta(KeyBindingMetaIndex(3)); // Default source
        keymap.add_bindings([default_binding]);

        // Add a user keymap binding
        let mut user_binding = KeyBinding::new("cmd-r", ActionBeta {}, Some("Editor"));
        user_binding.set_meta(KeyBindingMetaIndex(0)); // User source
        keymap.add_bindings([user_binding]);

        // Test with Editor context stack
        let (result, _) = keymap.bindings_for_input(
            &[Keystroke::parse("cmd-r").unwrap()],
            &[KeyContext::parse("Editor").unwrap()],
        );

        // User binding should take precedence over default binding
        assert_eq!(result.len(), 2);
        assert!(result[0].action.partial_eq(&ActionBeta {}));
        assert!(result[1].action.partial_eq(&ActionAlpha {}));
    }

    #[test]
    fn test_user_binding_promoted_over_deeper_default() {
        // This is the reported defect: a user binding that names a shallow context used to lose to
        // a non-user binding at a deeper context, because source never entered the sort.
        let bindings = [
            KeyBinding::new("cmd-r", ActionAlpha {}, Some("Workspace"))
                .with_meta(KeyBindingMetaIndex(0)),
            KeyBinding::new("cmd-r", ActionBeta {}, Some("Editor"))
                .with_meta(KeyBindingMetaIndex(3)),
        ];

        let mut keymap = Keymap::default();
        keymap.add_bindings(bindings);

        let (result, _) = keymap.bindings_for_input(
            &[Keystroke::parse("cmd-r").unwrap()],
            &[
                KeyContext::parse("Workspace").unwrap(),
                KeyContext::parse("Editor").unwrap(),
            ],
        );

        assert_eq!(result.len(), 2);
        assert!(result[0].action.partial_eq(&ActionAlpha {}));
        assert!(result[1].action.partial_eq(&ActionBeta {}));
    }

    #[test]
    fn test_user_binding_deep_beats_default_shallow() {
        let bindings = [
            KeyBinding::new("cmd-r", ActionAlpha {}, Some("Workspace"))
                .with_meta(KeyBindingMetaIndex(3)),
            KeyBinding::new("cmd-r", ActionBeta {}, Some("Editor"))
                .with_meta(KeyBindingMetaIndex(0)),
        ];

        let mut keymap = Keymap::default();
        keymap.add_bindings(bindings);

        let (result, _) = keymap.bindings_for_input(
            &[Keystroke::parse("cmd-r").unwrap()],
            &[
                KeyContext::parse("Workspace").unwrap(),
                KeyContext::parse("Editor").unwrap(),
            ],
        );

        assert_eq!(result.len(), 2);
        assert!(result[0].action.partial_eq(&ActionBeta {}));
        assert!(result[1].action.partial_eq(&ActionAlpha {}));
    }

    #[test]
    fn test_vim_deep_loses_to_user_shallow() {
        // Only the User tier is promoted; Vim still loses to a user binding even though Vim's
        // context is deeper, because the promotion is source-primary.
        let bindings = [
            KeyBinding::new("cmd-r", ActionAlpha {}, Some("Editor"))
                .with_meta(KeyBindingMetaIndex(1)),
            KeyBinding::new("cmd-r", ActionBeta {}, Some("Workspace"))
                .with_meta(KeyBindingMetaIndex(0)),
        ];

        let mut keymap = Keymap::default();
        keymap.add_bindings(bindings);

        let (result, _) = keymap.bindings_for_input(
            &[Keystroke::parse("cmd-r").unwrap()],
            &[
                KeyContext::parse("Workspace").unwrap(),
                KeyContext::parse("Editor").unwrap(),
            ],
        );

        assert_eq!(result.len(), 2);
        assert!(result[0].action.partial_eq(&ActionBeta {}));
        assert!(result[1].action.partial_eq(&ActionAlpha {}));
    }

    #[test]
    fn test_same_source_depth_ordering_preserved() {
        let bindings = [
            KeyBinding::new("cmd-r", ActionAlpha {}, Some("Workspace"))
                .with_meta(KeyBindingMetaIndex(3)),
            KeyBinding::new("cmd-r", ActionBeta {}, Some("Editor"))
                .with_meta(KeyBindingMetaIndex(3)),
        ];

        let mut keymap = Keymap::default();
        keymap.add_bindings(bindings);

        let (result, _) = keymap.bindings_for_input(
            &[Keystroke::parse("cmd-r").unwrap()],
            &[
                KeyContext::parse("Workspace").unwrap(),
                KeyContext::parse("Editor").unwrap(),
            ],
        );

        assert_eq!(result.len(), 2);
        assert!(result[0].action.partial_eq(&ActionBeta {}));
        assert!(result[1].action.partial_eq(&ActionAlpha {}));
    }

    #[test]
    fn test_non_user_tiers_keep_depth_ordering_against_each_other() {
        // No defect was reported between Vim/Base/Default, so their relative order stays
        // depth-only; only the User tier is promoted above depth.
        let bindings = [
            KeyBinding::new("cmd-r", ActionAlpha {}, Some("Workspace"))
                .with_meta(KeyBindingMetaIndex(1)),
            KeyBinding::new("cmd-r", ActionBeta {}, Some("Editor"))
                .with_meta(KeyBindingMetaIndex(3)),
        ];

        let mut keymap = Keymap::default();
        keymap.add_bindings(bindings);

        let (result, _) = keymap.bindings_for_input(
            &[Keystroke::parse("cmd-r").unwrap()],
            &[
                KeyContext::parse("Workspace").unwrap(),
                KeyContext::parse("Editor").unwrap(),
            ],
        );

        assert_eq!(result.len(), 2);
        assert!(result[0].action.partial_eq(&ActionBeta {}));
        assert!(result[1].action.partial_eq(&ActionAlpha {}));
    }

    #[test]
    fn test_unset_meta_sits_in_user_tier() {
        // A binding that never stamps `meta` (the common case: ad-hoc `bind_keys` calls and most
        // test setups) must be treated as the highest-precedence tier, not the lowest.
        let bindings = [
            KeyBinding::new("cmd-r", ActionAlpha {}, Some("Workspace")),
            KeyBinding::new("cmd-r", ActionBeta {}, Some("Editor"))
                .with_meta(KeyBindingMetaIndex(3)),
        ];

        let mut keymap = Keymap::default();
        keymap.add_bindings(bindings);

        let (result, _) = keymap.bindings_for_input(
            &[Keystroke::parse("cmd-r").unwrap()],
            &[
                KeyContext::parse("Workspace").unwrap(),
                KeyContext::parse("Editor").unwrap(),
            ],
        );

        assert_eq!(result.len(), 2);
        assert!(result[0].action.partial_eq(&ActionAlpha {}));
        assert!(result[1].action.partial_eq(&ActionBeta {}));
    }

    #[test]
    fn test_two_user_bindings_fall_back_to_depth() {
        let bindings = [
            KeyBinding::new("cmd-r", ActionAlpha {}, Some("Workspace"))
                .with_meta(KeyBindingMetaIndex(0)),
            KeyBinding::new("cmd-r", ActionBeta {}, Some("Editor"))
                .with_meta(KeyBindingMetaIndex(0)),
        ];

        let mut keymap = Keymap::default();
        keymap.add_bindings(bindings);

        let (result, _) = keymap.bindings_for_input(
            &[Keystroke::parse("cmd-r").unwrap()],
            &[
                KeyContext::parse("Workspace").unwrap(),
                KeyContext::parse("Editor").unwrap(),
            ],
        );

        assert_eq!(result.len(), 2);
        assert!(result[0].action.partial_eq(&ActionBeta {}));
        assert!(result[1].action.partial_eq(&ActionAlpha {}));
    }

    #[test]
    fn test_shallow_user_no_action_does_not_suppress_deeper_default() {
        // `NoAction`/`Unbind` are excluded from the promotion, so a user's shallow `"key": null`
        // cannot reach past its own context and silently cancel a deeper built-in binding.
        let bindings = [
            KeyBinding::new("cmd-r", ActionBeta {}, Some("Editor"))
                .with_meta(KeyBindingMetaIndex(3)),
            KeyBinding::new("cmd-r", NoAction {}, Some("Workspace"))
                .with_meta(KeyBindingMetaIndex(0)),
        ];

        let mut keymap = Keymap::default();
        keymap.add_bindings(bindings);

        let (result, _) = keymap.bindings_for_input(
            &[Keystroke::parse("cmd-r").unwrap()],
            &[
                KeyContext::parse("Workspace").unwrap(),
                KeyContext::parse("Editor").unwrap(),
            ],
        );

        assert_eq!(result.len(), 1);
        assert!(result[0].action.partial_eq(&ActionBeta {}));
    }

    #[test]
    fn test_same_context_user_no_action_still_suppresses() {
        let bindings = [
            KeyBinding::new("cmd-r", ActionBeta {}, Some("Editor"))
                .with_meta(KeyBindingMetaIndex(3)),
            KeyBinding::new("cmd-r", NoAction {}, Some("Editor"))
                .with_meta(KeyBindingMetaIndex(0)),
        ];

        let mut keymap = Keymap::default();
        keymap.add_bindings(bindings);

        let (result, _) = keymap.bindings_for_input(
            &[Keystroke::parse("cmd-r").unwrap()],
            &[
                KeyContext::parse("Workspace").unwrap(),
                KeyContext::parse("Editor").unwrap(),
            ],
        );

        assert!(result.is_empty());
    }

    #[test]
    fn test_promoted_user_binding_suppresses_pending_default_prefix() {
        // Registration order mirrors real load order: built-in bindings first, user keymap last.
        // Before this change, the deepest matched binding (the default `ctrl-w`) would win and its
        // low registration index would leave the default's own `ctrl-w x` prefix pending. Promoting
        // the user binding makes it the winner instead, which must suppress that pending prefix —
        // otherwise the dispatcher waits out the chord timeout instead of firing immediately.
        let bindings = [
            KeyBinding::new("ctrl-w", ActionGamma {}, Some("Editor"))
                .with_meta(KeyBindingMetaIndex(3)),
            KeyBinding::new("ctrl-w x", ActionDelta {}, Some("Editor"))
                .with_meta(KeyBindingMetaIndex(3)),
            KeyBinding::new("ctrl-w", ActionAlpha {}, Some("Workspace"))
                .with_meta(KeyBindingMetaIndex(0)),
        ];

        let mut keymap = Keymap::default();
        keymap.add_bindings(bindings);

        let (result, pending) = keymap.bindings_for_input(
            &[Keystroke::parse("ctrl-w").unwrap()],
            &[
                KeyContext::parse("Workspace").unwrap(),
                KeyContext::parse("Editor").unwrap(),
            ],
        );

        assert!(!result.is_empty());
        assert!(result[0].action.partial_eq(&ActionAlpha {}));
        assert!(
            !pending,
            "a promoted user chord must not leave a shadowed default prefix pending"
        );
    }

    /// The three-way case the two precedence rules cannot both satisfy at once.
    ///
    /// A user binding outranks a deeper default; a deeper disable outranks that user binding; and
    /// that deeper default outranks the disable. Applied per pair those form a cycle, and a
    /// comparator carrying one leaves `sort_by` free to return any order at all, or to reject it.
    /// So the rule is picked once per slice: a disable anywhere in the matched set means depth
    /// governs the whole set, which is the behaviour that predates promotion.
    ///
    /// Sorting every permutation is what makes this a proof rather than a description. A cyclic
    /// comparator still returns *an* order for any single input -- it just returns a different one
    /// depending on where each element started, which no amount of asserting one expected sequence
    /// will reveal.
    #[test]
    fn test_a_disable_between_two_tiers_keeps_a_total_order() {
        let user_shallow = KeyBinding::new("ctrl-w", ActionAlpha {}, Some("Workspace"))
            .with_meta(KeyBindingMetaIndex(0));
        let disable_middle =
            KeyBinding::new("ctrl-w", NoAction {}, Some("Editor")).with_meta(KeyBindingMetaIndex(3));
        let default_deep =
            KeyBinding::new("ctrl-w", ActionBeta {}, Some("Pane")).with_meta(KeyBindingMetaIndex(3));

        let entries = [
            (1usize, BindingIndex(0), &user_shallow),
            (2usize, BindingIndex(1), &disable_middle),
            (3usize, BindingIndex(2), &default_deep),
        ];

        let permutations = [
            [0, 1, 2],
            [0, 2, 1],
            [1, 0, 2],
            [1, 2, 0],
            [2, 0, 1],
            [2, 1, 0],
        ];
        let mut settled: Option<Vec<BindingIndex>> = None;
        for permutation in permutations {
            let mut slice = permutation.map(|position| entries[position]);
            sort_by_precedence(&mut slice);
            let order: Vec<BindingIndex> = slice.iter().map(|(_, ix, _)| *ix).collect();
            match &settled {
                None => settled = Some(order),
                Some(first) => assert_eq!(
                    first, &order,
                    "the order depends on where the elements started, so the comparator holds a cycle"
                ),
            }
        }

        // Depth governs, so the deepest binding leads regardless of tier.
        assert_eq!(
            settled.expect("six permutations were sorted"),
            vec![BindingIndex(2), BindingIndex(1), BindingIndex(0)]
        );
    }
}
