//! "Import from URL" on a chosen engine's form: paste the URL the database
//! was handed out with, and the form's own fields fill in from it.

use crate::connection_modal::{ConnectionModal, Step};
use gpui::{AnyElement, Context, Window};
use ui::{Tooltip, prelude::*};

impl ConnectionModal {
    /// Reads the pasted URL into the form's fields. Fields the URL says nothing
    /// about keep what they held -- a driver's default port, say. A URL for
    /// another engine, or text that is not a URL, changes nothing and says why.
    pub(crate) fn fill_from_url(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Step::Filling { form, inputs, .. } = &self.step else {
            return;
        };
        let url = self.url_import.read(cx).text(cx);
        let imported = match form.import_url(&url) {
            Ok(imported) => imported,
            Err(message) => {
                self.error = Some(message.into());
                self.url_import_note = None;
                cx.notify();
                return;
            }
        };

        let mut filled = Vec::new();
        for (key, value) in &imported.values {
            let Some((field, input)) = form
                .fields
                .iter()
                .zip(inputs)
                .find(|(field, _)| &field.key == key)
            else {
                continue;
            };
            input.update(cx, |input, cx| input.set_text(value, window, cx));
            filled.push(field.label.clone());
        }

        let mut note = if filled.is_empty() {
            "The URL held nothing this form asks for.".to_string()
        } else {
            format!("Filled {}.", filled.join(", "))
        };
        if !imported.left_out.is_empty() {
            note.push_str(&format!(
                " Left out, as the form has no field for it: {}.",
                imported.left_out.join(", ")
            ));
        }
        self.error = None;
        self.url_import_note = Some(note.into());
        // The connection the form now describes has not been tried.
        self.test = crate::connection_modal::TestState::Untested;
        cx.notify();
    }

    /// The paste box above the driver's fields. Only on a form built from a
    /// real URL: SQLite's is a file path, and the URL-only form *is* the URL.
    pub(crate) fn render_url_import(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let Step::Filling { form, .. } = &self.step else {
            return None;
        };
        if !form.url_template.contains("://") || form.fields.len() < 2 {
            return None;
        }

        Some(
            v_flex()
                .id("database-url-import")
                .debug_selector(|| "database-url-import".into())
                .gap_1p5()
                .child(Label::new("Import from URL").size(LabelSize::Small))
                .child(
                    h_flex()
                        .gap_1()
                        .child(div().flex_1().min_w_0().child(self.url_import.clone()))
                        .child(
                            Button::new("database-url-import-fill", "Fill")
                                .start_icon(Icon::new(IconName::Link).size(IconSize::Small))
                                .tooltip(|_window, cx| {
                                    Tooltip::simple("Fill the fields below from this URL", cx)
                                })
                                .on_click(cx.listener(|this, _event, window, cx| {
                                    this.fill_from_url(window, cx);
                                })),
                        ),
                )
                .when_some(self.url_import_note.clone(), |element, note| {
                    element.child(Label::new(note).size(LabelSize::XSmall).color(Color::Muted))
                })
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database_panel_tests::modal_for_test;
    use crate::driver_registry::CatalogueEntry;
    use database::protocol::{ConnectionField, ConnectionForm};

    fn postgres() -> CatalogueEntry {
        CatalogueEntry {
            driver: "postgres".into(),
            name: "PostgreSQL".into(),
            description: "A server".into(),
            group: "Relational".into(),
            installed: true,
            downloaded: false,
            hand_placed: false,
        }
    }

    fn field(key: &str, default: Option<&str>) -> ConnectionField {
        ConnectionField {
            key: key.into(),
            label: key.into(),
            default: default.map(Into::into),
            url_encoded: true,
            ..Default::default()
        }
    }

    /// The shape of the Postgres driver's form.
    fn postgres_form() -> ConnectionForm {
        ConnectionForm {
            fields: vec![
                field("host", Some("localhost")),
                field("port", Some("5432")),
                field("database", None),
                field("user", None),
                ConnectionField {
                    key: "password".into(),
                    label: "Password".into(),
                    secret: true,
                    ..Default::default()
                },
            ],
            url_template: "postgres://{user}@{host}:{port}/{database}".into(),
        }
    }

    fn texts(modal: &ConnectionModal, cx: &gpui::App) -> Vec<String> {
        let Step::Filling { inputs, .. } = &modal.step else {
            panic!("the form should be showing");
        };
        inputs.iter().map(|input| input.read(cx).text(cx)).collect()
    }

    #[gpui::test]
    async fn a_pasted_url_fills_the_form_and_its_password_stays_out_of_the_saved_url(
        cx: &mut gpui::TestAppContext,
    ) {
        let (modal, cx) = modal_for_test(cx, &["postgres"]).await;
        modal.update_in(cx, |modal, window, cx| {
            modal.show_form(postgres(), Some(postgres_form()), window, cx);
            modal.url_import.update(cx, |input, cx| {
                input.set_text(
                    "postgres://ada:pw@db.example:6543/shop?sslmode=require",
                    window,
                    cx,
                )
            });
            modal.fill_from_url(window, cx);

            assert_eq!(
                texts(modal, cx),
                vec!["db.example", "6543", "shop", "ada", "pw"]
            );
            assert!(modal.error.is_none(), "{:?}", modal.error);
            let note = modal
                .url_import_note
                .clone()
                .expect("what was left out is said");
            assert!(note.contains("sslmode=require"), "{note}");

            let Ok(filled) = modal.filled(cx) else {
                panic!("the form is complete");
            };
            assert_eq!(filled.url, "postgres://ada@db.example:6543/shop");
            assert_eq!(filled.secret.as_deref(), Some("pw"));
        });
    }

    /// Another engine's URL is refused and leaves the form as it was.
    #[gpui::test]
    async fn another_engines_url_changes_nothing_and_says_why(cx: &mut gpui::TestAppContext) {
        let (modal, cx) = modal_for_test(cx, &["postgres"]).await;
        modal.update_in(cx, |modal, window, cx| {
            modal.show_form(postgres(), Some(postgres_form()), window, cx);
            let before = texts(modal, cx);
            modal.url_import.update(cx, |input, cx| {
                input.set_text("mysql://ada@db.example:3306/shop", window, cx)
            });
            modal.fill_from_url(window, cx);

            assert_eq!(texts(modal, cx), before);
            let error = modal.error.clone().expect("the refusal is shown");
            assert!(error.contains("mysql"), "{error}");
        });
    }
}
