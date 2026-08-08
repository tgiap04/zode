use client::TelemetrySettings;
use fs::Fs;
use gpui::{App, IntoElement};
use project::project_settings::ProjectSettings;
use settings::{BaseKeymap, Settings, update_settings_file};
use terminal::terminal_settings::TerminalSettings;
use theme::Appearance;
use theme_settings::{ThemeAppearanceMode, ThemeSettings};
use ui::{
    Divider, PopoverMenu, SwitchField,
    ToggleButtonGroup, ToggleButtonGroupSize, ToggleButtonSimple, ToggleButtonWithIcon, Tooltip,
    prelude::*,
};
use vim_mode_setting::VimModeSetting;

/// Only the 2026 family ships, so there is nothing to choose between -- the
/// mode toggle stays because Light/Dark/System is still a real choice, and
/// System is the one that follows the OS.
fn render_theme_section(tab_index: &mut isize, cx: &mut App) -> impl IntoElement {
    let theme_selection = ThemeSettings::get_global(cx).theme.clone();
    let system_appearance = theme::SystemAppearance::global(cx);

    let theme_mode = theme_selection
        .mode()
        .unwrap_or_else(|| match *system_appearance {
            Appearance::Light => ThemeAppearanceMode::Light,
            Appearance::Dark => ThemeAppearanceMode::Dark,
        });

    return h_flex().justify_between().child(Label::new("Theme")).child(
        ToggleButtonGroup::single_row(
            "theme-selector-onboarding-dark-light",
            [
                ThemeAppearanceMode::Light,
                ThemeAppearanceMode::Dark,
                ThemeAppearanceMode::System,
            ]
            .map(|mode| {
                const MODE_NAMES: [SharedString; 3] = [
                    SharedString::new_static("Light"),
                    SharedString::new_static("Dark"),
                    SharedString::new_static("System"),
                ];
                ToggleButtonSimple::new(MODE_NAMES[mode as usize].clone(), move |_, _, cx| {
                    write_mode_change(mode, cx);

                    telemetry::event!(
                        "Welcome Theme mode Changed",
                        from = theme_mode,
                        to = mode
                    );
                })
            }),
        )
        .size(ToggleButtonGroupSize::Medium)
        .tab_index(tab_index)
        .selected_index(theme_mode as usize)
        .style(ui::ToggleButtonGroupStyle::Outlined)
        .width(rems_from_px(3. * 64.)),
    );

    fn write_mode_change(mode: ThemeAppearanceMode, cx: &mut App) {
        let fs = <dyn Fs>::global(cx);
        update_settings_file(fs, cx, move |settings, _cx| {
            theme_settings::set_mode(settings, mode);
        });
    }
}

/// Font family and size, the two settings almost everyone changes on day one.
///
/// The pickers are built inside `PopoverMenu::menu`, which only runs when the
/// menu is opened. That matters: this page re-renders on every settings change
/// (`Onboarding` observes `SettingsStore`), and enumerating system fonts on each
/// of those would be paid for nothing. `FontPickerDelegate` reads the list from
/// `FontFamilyCache`, which `Onboarding::new` has already prefetched.
fn render_font_section(tab_index: &mut isize, cx: &mut App) -> impl IntoElement {
    let theme_settings = ThemeSettings::get_global(cx);
    let ui_font = theme_settings.ui_font.family.clone();
    let buffer_font = theme_settings.buffer_font.family.clone();
    let current_size = f32::from(theme_settings.buffer_font_size(cx)).round() as i32;
    // `None` here is not "unset" to the user -- the terminal renders the buffer
    // font in that case, so showing the buffer font is what is actually on screen.
    let terminal_font = TerminalSettings::get_global(cx)
        .font_family
        .as_ref()
        .map_or_else(
            || buffer_font.clone(),
            |family| SharedString::from(family.0.clone()),
        );

    v_flex()
        .gap_2()
        .child(Label::new("Fonts"))
        .child(
            h_flex()
                .justify_between()
                .child(Label::new("Interface").color(Color::Muted))
                .child(render_font_menu(
                    "onboarding-ui-font",
                    ui_font,
                    tab_index,
                    |name, settings| settings.theme.ui_font_family = Some(name),
                )),
        )
        .child(
            h_flex()
                .justify_between()
                .child(Label::new("Editor").color(Color::Muted))
                .child(render_font_menu(
                    "onboarding-buffer-font",
                    buffer_font,
                    tab_index,
                    |name, settings| settings.theme.buffer_font_family = Some(name),
                )),
        )
        .child(
            h_flex()
                .justify_between()
                .child(Label::new("Terminal").color(Color::Muted))
                .child(render_font_menu(
                    "onboarding-terminal-font",
                    terminal_font,
                    tab_index,
                    |name, settings| {
                        settings.terminal.get_or_insert_default().font_family = Some(name)
                    },
                )),
        )
        .child(
            h_flex()
                .justify_between()
                .child(Label::new("Size").color(Color::Muted))
                .child(render_font_size_toggle(current_size, tab_index)),
        )
}

/// A few presets rather than a free number field: the page's other controls are
/// toggle groups, and a stepper here would be the only thing on the screen
/// needing its own state.
const FONT_SIZE_PRESETS: [i32; 4] = [12, 13, 14, 16];

/// Split out so a test can reach it: the closure it is called from lives inside
/// a toggle button and cannot be invoked without a window.
///
/// Writes both sizes deliberately -- the user picks one number, and setting only
/// the editor leaves the chrome at its default with nothing on screen to say so.
fn write_font_size(size: i32, settings: &mut settings::SettingsContent) {
    let size = Some(settings::FontSize(size as f32));
    settings.theme.buffer_font_size = size;
    settings.theme.ui_font_size = size;
}

fn render_font_size_toggle(current_size: i32, tab_index: &mut isize) -> impl IntoElement {
    let selected = FONT_SIZE_PRESETS
        .iter()
        .position(|size| *size == current_size);

    ToggleButtonGroup::single_row(
        "onboarding-font-size",
        FONT_SIZE_PRESETS.map(|size| {
            ToggleButtonSimple::new(SharedString::from(size.to_string()), move |_, _, cx| {
                let fs = <dyn Fs>::global(cx);
                update_settings_file(fs, cx, move |settings, _cx| {
                    write_font_size(size, settings);
                });
            })
        }),
    )
    .size(ToggleButtonGroupSize::Medium)
    .tab_index(tab_index)
    .when_some(selected, |group, index| group.selected_index(index))
    .style(ui::ToggleButtonGroupStyle::Outlined)
    .width(rems_from_px(4. * 48.))
}

fn render_font_menu(
    id: &'static str,
    current: SharedString,
    tab_index: &mut isize,
    write: fn(settings::FontFamilyName, &mut settings::SettingsContent),
) -> impl IntoElement {
    *tab_index += 1;
    let trigger_index = *tab_index;

    PopoverMenu::new(id)
        .trigger(
            Button::new(SharedString::from(id), current.clone())
                .tab_index(trigger_index)
                .style(ButtonStyle::Outlined)
                .size(ButtonSize::Medium)
                .end_icon(
                    Icon::new(IconName::ChevronUpDown)
                        .size(IconSize::Small)
                        .color(Color::Muted),
                ),
        )
        .menu(move |window, cx: &mut App| {
            let current = current.clone();
            Some(cx.new(move |cx| {
                settings_ui::font_picker(
                    current,
                    move |font_name, _window, cx| {
                        let fs = <dyn Fs>::global(cx);
                        update_settings_file(fs, cx, move |settings, _cx| {
                            write(font_name.to_string().into(), settings);
                        });
                    },
                    window,
                    cx,
                )
            }))
        })
        .anchor(gpui::Anchor::TopRight)
        .into_any_element()
}

fn render_telemetry_section(tab_index: &mut isize, cx: &App) -> impl IntoElement {
    let fs = <dyn Fs>::global(cx);

    v_flex()
        .gap_4()
        .child(
            SwitchField::new(
                "onboarding-telemetry-metrics",
                None::<&str>,
                Some("Help improve Zode by sending anonymous usage data".into()),
                if TelemetrySettings::get_global(cx).metrics {
                    ui::ToggleState::Selected
                } else {
                    ui::ToggleState::Unselected
                },
                {
                    let fs = fs.clone();
                    move |selection, _, cx| {
                        let enabled = match selection {
                            ToggleState::Selected => true,
                            ToggleState::Unselected => false,
                            ToggleState::Indeterminate => {
                                return;
                            }
                        };

                        update_settings_file(fs.clone(), cx, move |setting, _| {
                            setting.telemetry.get_or_insert_default().metrics = Some(enabled);
                        });

                        // This telemetry event shouldn't fire when it's off. If it does we'll be alerted
                        // and can fix it in a timely manner to respect a user's choice.
                        telemetry::event!(
                            "Welcome Page Telemetry Metrics Toggled",
                            options = if enabled { "on" } else { "off" }
                        );
                    }
                },
            )
            .tab_index({
                *tab_index += 1;
                *tab_index
            }),
        )
        .child(
            SwitchField::new(
                "onboarding-telemetry-crash-reports",
                None::<&str>,
                Some(
                    "Help fix Zode by sending crash reports so we can fix critical issues fast"
                        .into(),
                ),
                if TelemetrySettings::get_global(cx).diagnostics {
                    ui::ToggleState::Selected
                } else {
                    ui::ToggleState::Unselected
                },
                {
                    let fs = fs.clone();
                    move |selection, _, cx| {
                        let enabled = match selection {
                            ToggleState::Selected => true,
                            ToggleState::Unselected => false,
                            ToggleState::Indeterminate => {
                                return;
                            }
                        };

                        update_settings_file(fs.clone(), cx, move |setting, _| {
                            setting.telemetry.get_or_insert_default().diagnostics = Some(enabled);
                        });

                        // This telemetry event shouldn't fire when it's off. If it does we'll be alerted
                        // and can fix it in a timely manner to respect a user's choice.
                        telemetry::event!(
                            "Welcome Page Telemetry Diagnostics Toggled",
                            options = if enabled { "on" } else { "off" }
                        );
                    }
                },
            )
            .tab_index({
                *tab_index += 1;
                *tab_index
            }),
        )
}

fn render_base_keymap_section(tab_index: &mut isize, cx: &mut App) -> impl IntoElement {
    let base_keymap = match BaseKeymap::get_global(cx) {
        BaseKeymap::VSCode => Some(0),
        BaseKeymap::JetBrains => Some(1),
        BaseKeymap::SublimeText => Some(2),
        BaseKeymap::Atom => Some(3),
        BaseKeymap::Emacs => Some(4),
        BaseKeymap::Cursor => Some(5),
        BaseKeymap::TextMate | BaseKeymap::None => None,
    };

    return v_flex().gap_2().child(Label::new("Base Keymap")).child(
        ToggleButtonGroup::two_rows(
            "base_keymap_selection",
            [
                ToggleButtonWithIcon::new("VS Code", IconName::EditorVsCode, |_, _, cx| {
                    write_keymap_base(BaseKeymap::VSCode, cx);
                }),
                ToggleButtonWithIcon::new("JetBrains", IconName::EditorJetBrains, |_, _, cx| {
                    write_keymap_base(BaseKeymap::JetBrains, cx);
                }),
                ToggleButtonWithIcon::new("Sublime Text", IconName::EditorSublime, |_, _, cx| {
                    write_keymap_base(BaseKeymap::SublimeText, cx);
                }),
            ],
            [
                ToggleButtonWithIcon::new("Atom", IconName::EditorAtom, |_, _, cx| {
                    write_keymap_base(BaseKeymap::Atom, cx);
                }),
                ToggleButtonWithIcon::new("Emacs", IconName::EditorEmacs, |_, _, cx| {
                    write_keymap_base(BaseKeymap::Emacs, cx);
                }),
                ToggleButtonWithIcon::new("Cursor", IconName::EditorCursor, |_, _, cx| {
                    write_keymap_base(BaseKeymap::Cursor, cx);
                }),
            ],
        )
        .when_some(base_keymap, |this, base_keymap| {
            this.selected_index(base_keymap)
        })
        .full_width()
        .tab_index(tab_index)
        .size(ui::ToggleButtonGroupSize::Medium)
        .style(ui::ToggleButtonGroupStyle::Outlined),
    );

    fn write_keymap_base(keymap_base: BaseKeymap, cx: &App) {
        let fs = <dyn Fs>::global(cx);

        update_settings_file(fs, cx, move |setting, _| {
            setting.base_keymap = Some(keymap_base.into());
        });

        telemetry::event!("Welcome Keymap Changed", keymap = keymap_base);
    }
}

fn render_vim_mode_switch(tab_index: &mut isize, cx: &mut App) -> impl IntoElement {
    let toggle_state = if VimModeSetting::get_global(cx).0 {
        ui::ToggleState::Selected
    } else {
        ui::ToggleState::Unselected
    };
    SwitchField::new(
        "onboarding-vim-mode",
        Some("Vim Mode"),
        Some("Coming from Neovim? Use our first-class implementation of Vim Mode".into()),
        toggle_state,
        {
            let fs = <dyn Fs>::global(cx);
            move |&selection, _, cx| {
                let vim_mode = match selection {
                    ToggleState::Selected => true,
                    ToggleState::Unselected => false,
                    ToggleState::Indeterminate => {
                        return;
                    }
                };
                update_settings_file(fs.clone(), cx, move |setting, _| {
                    setting.vim_mode = Some(vim_mode);
                });

                telemetry::event!(
                    "Welcome Vim Mode Toggled",
                    options = if vim_mode { "on" } else { "off" },
                );
            }
        },
    )
    .tab_index({
        *tab_index += 1;
        *tab_index - 1
    })
}

fn render_worktree_auto_trust_switch(tab_index: &mut isize, cx: &mut App) -> impl IntoElement {
    let toggle_state = if ProjectSettings::get_global(cx).session.trust_all_worktrees {
        ui::ToggleState::Selected
    } else {
        ui::ToggleState::Unselected
    };

    let tooltip_description = "Zode can only allow services like language servers, project settings, and MCP servers to run after you mark a new project as trusted.";

    SwitchField::new(
        "onboarding-auto-trust-worktrees",
        Some("Trust All Projects By Default"),
        Some("Automatically mark all new projects as trusted to unlock all Zode's features".into()),
        toggle_state,
        {
            let fs = <dyn Fs>::global(cx);
            move |&selection, _, cx| {
                let trust = match selection {
                    ToggleState::Selected => true,
                    ToggleState::Unselected => false,
                    ToggleState::Indeterminate => {
                        return;
                    }
                };
                update_settings_file(fs.clone(), cx, move |setting, _| {
                    setting.session.get_or_insert_default().trust_all_worktrees = Some(trust);
                });

                telemetry::event!(
                    "Welcome Page Worktree Auto Trust Toggled",
                    options = if trust { "on" } else { "off" }
                );
            }
        },
    )
    .tab_index({
        *tab_index += 1;
        *tab_index - 1
    })
    .tooltip(Tooltip::text(tooltip_description))
}

pub(crate) fn render_basics_page(cx: &mut App) -> impl IntoElement {
    let mut tab_index = 0;

    v_flex()
        .id("basics-page")
        .gap_6()
        .child(render_theme_section(&mut tab_index, cx))
        .child(render_font_section(&mut tab_index, cx))
        .child(render_base_keymap_section(&mut tab_index, cx))
        .child(render_vim_mode_switch(&mut tab_index, cx))
        .child(render_worktree_auto_trust_switch(&mut tab_index, cx))
        .child(Divider::horizontal().color(ui::DividerColor::BorderVariant))
        .child(render_telemetry_section(&mut tab_index, cx))
}

#[cfg(test)]
mod tests {
    use gpui::TestAppContext;
    use settings::Settings as _;

    /// The first screen a user ever sees must not be able to abort the process.
    ///
    /// It used to: the theme section looked its themes up by name and
    /// `unwrap()`ed the result, so a registry without them took the app down on
    /// launch. `LoadThemes::JustBase` reproduces exactly that -- it loads the
    /// base theme only, so "Dark 2026" and "Light 2026" are genuinely absent.
    #[gpui::test]
    fn the_page_renders_without_the_themes_it_expects(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let settings_store = settings::SettingsStore::test(cx);
            cx.set_global(settings_store);
            theme_settings::init(theme::LoadThemes::JustBase, cx);
            project::DisableAiSettings::register(cx);
            client::TelemetrySettings::register(cx);
            project::project_settings::ProjectSettings::register(cx);
            settings::BaseKeymap::register(cx);
            vim_mode_setting::VimModeSetting::register(cx);
            <dyn fs::Fs>::set_global(fs::FakeFs::new(cx.background_executor().clone()), cx);

            // Building the element tree is the whole assertion: anything that
            // panics on missing data does it here.
            let _ = super::render_basics_page(cx);
        });
    }

    /// A font size is one number to the user but two settings underneath, and
    /// writing only the editor one leaves the chrome at its default with nothing
    /// on screen to say so.
    #[gpui::test]
    fn a_font_size_writes_both_the_editor_and_the_interface(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let settings_store = settings::SettingsStore::test(cx);
            cx.set_global(settings_store);

            let mut content = settings::SettingsContent::default();
            super::write_font_size(13, &mut content);

            assert_eq!(content.theme.buffer_font_size, Some(settings::FontSize(13.0)));
            assert_eq!(content.theme.ui_font_size, Some(settings::FontSize(13.0)));
        });
    }
}
