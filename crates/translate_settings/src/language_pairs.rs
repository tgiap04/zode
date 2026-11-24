//! The language pairs are the keybindings of `translate::TranslateSelection`:
//! the keymap is the only place they are stored, so this reads them from the
//! active keymap and writes changes to the user keymap the way the keymap
//! editor does, keeping comments and order.

use std::rc::Rc;
use std::sync::Arc;

use anyhow::{Context as _, Result};
use fs::Fs;
use gpui::{
    App, KeyBindingContextPredicate, KeyContext, KeybindingKeystroke, PlatformKeyboardMapper,
};
use keymap_editor::keystrokes_match_exactly;
use serde_json::Value;
use settings::{
    KeybindSource, KeybindUpdateOperation, KeybindUpdateTarget, KeymapFile, infer_json_indent_size,
};

/// Named rather than imported so this crate does not depend on the crate that
/// handles the action.
pub const TRANSLATE_SELECTION_ACTION: &str = "translate::TranslateSelection";

/// The contexts the bundled keymaps bind every pair in: editors, terminals,
/// and every view showing `Markdown`.
const PAIR_CONTEXTS: [&str; 3] = ["Editor", "Terminal", "Markdown"];

/// One keymap entry of a pair.
#[derive(Clone, Debug, PartialEq)]
pub struct PairBinding {
    pub context: Option<String>,
    pub source: KeybindSource,
}

/// A language pair with its shortcut. One pair can be bound in several
/// contexts with the same keystrokes; those entries are edited together.
#[derive(Clone, Debug, PartialEq)]
pub struct LanguagePair {
    pub from: String,
    pub to: String,
    pub keystrokes: Vec<KeybindingKeystroke>,
    pub bindings: Vec<PairBinding>,
}

impl LanguagePair {
    pub fn arguments(&self) -> String {
        action_arguments(&self.from, &self.to)
    }
}

pub fn action_arguments(from: &str, to: &str) -> String {
    serde_json::json!({ "from": from, "to": to }).to_string()
}

/// The pairs that are live in the active keymap, in keymap order. Entries
/// switched off by a later `unbind` are left out.
pub fn active_language_pairs(cx: &App) -> Vec<LanguagePair> {
    let key_bindings = cx.key_bindings();
    let key_bindings = key_bindings.borrow();
    let all_bindings = key_bindings.bindings().collect::<Vec<_>>();
    let mut pairs: Vec<LanguagePair> = Vec::new();
    for (index, binding) in all_bindings.iter().enumerate() {
        if binding.action().name() != TRANSLATE_SELECTION_ACTION
            || keymap_editor::binding_is_unbound_by_unbind(binding, index, &all_bindings)
        {
            continue;
        }
        let Some((from, to)) = binding
            .action_input()
            .and_then(|input| parse_arguments(&input))
        else {
            continue;
        };
        let pair_binding = PairBinding {
            context: binding.predicate().map(|predicate| predicate.to_string()),
            source: binding
                .meta()
                .map(KeybindSource::from_meta)
                .unwrap_or(KeybindSource::Unknown),
        };
        let existing = pairs.iter_mut().find(|pair| {
            pair.from == from
                && pair.to == to
                && keystrokes_match_exactly(&pair.keystrokes, binding.keystrokes())
        });
        match existing {
            Some(pair) => pair.bindings.push(pair_binding),
            None => pairs.push(LanguagePair {
                from,
                to,
                keystrokes: binding.keystrokes().to_vec(),
                bindings: vec![pair_binding],
            }),
        }
    }
    pairs
}

/// The contexts a new pair is bound in: the bundled ones, so it works
/// wherever the default pairs do even after those were removed, plus any
/// other context the user binds pairs in.
pub fn contexts_for_new_pair(pairs: &[LanguagePair]) -> Vec<Option<String>> {
    let mut contexts: Vec<Option<String>> = PAIR_CONTEXTS
        .iter()
        .map(|context| Some(context.to_string()))
        .collect();
    for binding in pairs.iter().flat_map(|pair| &pair.bindings) {
        if !contexts.contains(&binding.context) {
            contexts.push(binding.context.clone());
        }
    }
    contexts
}

/// The actions already on `keystrokes` wherever a binding in one of
/// `contexts` would fire. That covers bindings in the same context, in wider
/// ones that hold it (`Workspace`, `Pane`, or no context at all), and in
/// narrower ones (`Editor && mode == full`, `Terminal && selection` for
/// `Terminal`). Bindings switched off by a later `unbind`, and
/// `null` bindings, fire nothing and are left out.
pub fn conflicts_for(
    keystrokes: &[KeybindingKeystroke],
    contexts: &[Option<String>],
    cx: &App,
) -> Vec<&'static str> {
    let targets = contexts
        .iter()
        .map(|context| {
            let predicate = context
                .as_deref()
                .and_then(|context| KeyBindingContextPredicate::parse(context).ok());
            let stack = predicate.as_ref().map(representative_stack);
            (predicate, stack)
        })
        .collect::<Vec<_>>();
    let key_bindings = cx.key_bindings();
    let key_bindings = key_bindings.borrow();
    let all_bindings = key_bindings.bindings().collect::<Vec<_>>();
    let mut names = Vec::new();
    for (index, binding) in all_bindings.iter().enumerate() {
        if gpui::is_unbind(binding.action())
            || gpui::is_no_action(binding.action())
            || !keystrokes_match_exactly(binding.keystrokes(), keystrokes)
            || keymap_editor::binding_is_unbound_by_unbind(binding, index, &all_bindings)
        {
            continue;
        }
        let other = binding.predicate();
        let overlaps = targets.iter().any(|(target, stack)| {
            match (target, stack, other.as_deref()) {
                (Some(target), Some(stack), Some(other)) => {
                    other.depth_of(stack).is_some() || target.is_superset(other)
                }
                // A binding with no context fires everywhere, and a pair bound
                // with no context fires wherever the other one does.
                _ => true,
            }
        });
        if overlaps && !names.contains(&binding.action().name()) {
            names.push(binding.action().name());
        }
    }
    names
}

/// The context stack a view matching `predicate` usually sits in: the
/// workspace, a pane, then the view itself with the identifiers and values the
/// predicate asks for. A binding that matches anywhere in this stack competes
/// with one on `predicate`.
fn representative_stack(predicate: &KeyBindingContextPredicate) -> Vec<KeyContext> {
    let mut workspace = KeyContext::new_with_defaults();
    workspace.add("Workspace");
    let mut pane = KeyContext::default();
    pane.add("Pane");
    let mut stack = vec![workspace, pane];
    let mut leaf = KeyContext::default();
    add_required_parts(predicate, &mut stack, &mut leaf);
    stack.push(leaf);
    stack
}

fn add_required_parts(
    predicate: &KeyBindingContextPredicate,
    ancestors: &mut Vec<KeyContext>,
    leaf: &mut KeyContext,
) {
    match predicate {
        KeyBindingContextPredicate::Identifier(name) => leaf.add(name.clone()),
        KeyBindingContextPredicate::Equal(key, value) => leaf.set(key.clone(), value.clone()),
        KeyBindingContextPredicate::And(left, right) => {
            add_required_parts(left, ancestors, leaf);
            add_required_parts(right, ancestors, leaf);
        }
        KeyBindingContextPredicate::Descendant(parent, child) => {
            let mut parent_context = KeyContext::default();
            add_required_parts(parent, ancestors, &mut parent_context);
            ancestors.push(parent_context);
            add_required_parts(child, ancestors, leaf);
        }
        KeyBindingContextPredicate::NotEqual(..)
        | KeyBindingContextPredicate::Not(_)
        | KeyBindingContextPredicate::Or(..) => {}
    }
}

pub enum PairEdit {
    Add {
        from: String,
        to: String,
        keystrokes: Vec<KeybindingKeystroke>,
        contexts: Vec<Option<String>>,
    },
    Reassign {
        pair: LanguagePair,
        keystrokes: Vec<KeybindingKeystroke>,
    },
    Remove {
        pair: LanguagePair,
    },
}

/// Applies `edit` to the user keymap file and writes it back.
pub async fn write_pair_edit(
    edit: PairEdit,
    fs: Arc<dyn Fs>,
    keyboard_mapper: Rc<dyn PlatformKeyboardMapper>,
) -> Result<()> {
    let keymap_contents = KeymapFile::load_keymap_file(&fs)
        .await
        .context("Failed to load keymap file")?;
    let updated = apply_pair_edit(&edit, keymap_contents, keyboard_mapper.as_ref())?;
    fs.write(paths::keymap_file().as_path(), updated.as_bytes())
        .await
        .context("Failed to write keymap file")
}

fn apply_pair_edit(
    edit: &PairEdit,
    mut keymap_contents: String,
    keyboard_mapper: &dyn PlatformKeyboardMapper,
) -> Result<String> {
    let tab_size = infer_json_indent_size(&keymap_contents);
    let apply = |operation: KeybindUpdateOperation<'_>, contents: String| {
        KeymapFile::update_keybinding(operation, contents, tab_size, keyboard_mapper)
            .context("Could not save the shortcut")
    };
    match edit {
        PairEdit::Add {
            from,
            to,
            keystrokes,
            contexts,
        } => {
            let arguments = action_arguments(from, to);
            for context in contexts {
                let source = target(context.as_deref(), keystrokes, &arguments);
                keymap_contents = apply(KeybindUpdateOperation::add(source), keymap_contents)?;
            }
        }
        PairEdit::Reassign { pair, keystrokes } => {
            let arguments = pair.arguments();
            for binding in &pair.bindings {
                let old = target(binding.context.as_deref(), &pair.keystrokes, &arguments);
                let new = target(binding.context.as_deref(), keystrokes, &arguments);
                if binding.source == KeybindSource::User {
                    // Not replaced in place: an `unbind` of these keys written
                    // when a default pair was moved away may sit after the old
                    // entry, and an `unbind` switches off every earlier
                    // binding of the action on those keys. Appending puts the
                    // new binding after any such `unbind`.
                    let add = KeybindUpdateOperation::Add {
                        source: new,
                        from: Some(old.clone()),
                    };
                    keymap_contents = apply(add, keymap_contents)?;
                    let remove = KeybindUpdateOperation::Remove {
                        target: old,
                        target_keybind_source: KeybindSource::User,
                    };
                    keymap_contents = apply(remove, keymap_contents)?;
                } else {
                    // A non-user binding becomes an appended binding plus an
                    // `unbind` of the old keys, so it is already last.
                    let operation = KeybindUpdateOperation::Replace {
                        target: old,
                        target_keybind_source: binding.source,
                        source: new,
                    };
                    keymap_contents = apply(operation, keymap_contents)?;
                }
            }
        }
        PairEdit::Remove { pair } => {
            let arguments = pair.arguments();
            for binding in &pair.bindings {
                let operation = KeybindUpdateOperation::Remove {
                    target: target(binding.context.as_deref(), &pair.keystrokes, &arguments),
                    target_keybind_source: binding.source,
                };
                keymap_contents = apply(operation, keymap_contents)?;
            }
        }
    }
    Ok(keymap_contents)
}

fn target<'a>(
    context: Option<&'a str>,
    keystrokes: &'a [KeybindingKeystroke],
    arguments: &'a str,
) -> KeybindUpdateTarget<'a> {
    KeybindUpdateTarget {
        context,
        keystrokes,
        action_name: TRANSLATE_SELECTION_ACTION,
        action_arguments: Some(arguments),
    }
}

fn parse_arguments(input: &str) -> Option<(String, String)> {
    let value: Value = serde_json::from_str(input).ok()?;
    let from = value.get("from")?.as_str()?.to_string();
    let to = value.get("to")?.as_str()?.to_string();
    Some((from, to))
}
