//! Shared UI language. Engine history stores canonical message keys.
use eframe::egui;
use std::{collections::BTreeMap, sync::OnceLock};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Locale {
    #[default]
    Spanish,
    English,
}

impl Locale {
    pub const ALL: [Self; 2] = [Self::Spanish, Self::English];
    pub fn name(self) -> &'static str {
        match self {
            Self::Spanish => "Español",
            Self::English => "English",
        }
    }
    pub fn text(self, key: &str) -> &str {
        static ES: OnceLock<BTreeMap<String, String>> = OnceLock::new();
        static EN: OnceLock<BTreeMap<String, String>> = OnceLock::new();
        let (cache, json) = match self {
            Self::Spanish => (&ES, include_str!("../locales/es.json")),
            Self::English => (&EN, include_str!("../locales/en.json")),
        };
        cache
            .get_or_init(|| serde_json::from_str(json).expect("valid embedded locale catalog"))
            .get(key)
            .map(String::as_str)
            .unwrap_or(key)
    }
}

pub fn locale(ctx: &egui::Context) -> Locale {
    ctx.data(|data| data.get_temp::<Locale>(egui::Id::new("ui-language")))
        .unwrap_or_default()
}
pub fn set_locale(ctx: &egui::Context, locale: Locale) {
    ctx.data_mut(|data| data.insert_temp(egui::Id::new("ui-language"), locale));
    ctx.request_repaint();
}
pub fn tr<'a>(ctx: &egui::Context, key: &'a str) -> &'a str {
    locale(ctx).text(key)
}
pub fn message(ctx: &egui::Context, key: &str, args: &[(&str, &str)]) -> String {
    let mut text = tr(ctx, key).to_owned();
    for (name, value) in args {
        text = text.replace(&format!("{{{name}}}"), value);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalogs_have_matching_keys_and_placeholders() {
        let en: BTreeMap<String, String> =
            serde_json::from_str(include_str!("../locales/en.json")).unwrap();
        let es: BTreeMap<String, String> =
            serde_json::from_str(include_str!("../locales/es.json")).unwrap();
        assert_eq!(en.keys().collect::<Vec<_>>(), es.keys().collect::<Vec<_>>());
        for (key, value) in &en {
            assert!(!es[key].is_empty(), "empty translation: {key}");
            let placeholders = |s: &str| {
                s.split('{')
                    .skip(1)
                    .filter_map(|p| p.split_once('}').map(|(p, _)| p.to_owned()))
                    .collect::<std::collections::BTreeSet<_>>()
            };
            assert_eq!(
                placeholders(value),
                placeholders(&es[key]),
                "placeholders for {key}"
            );
        }
    }
    #[test]
    fn language_changes_are_shared_and_unknown_keys_fall_back() {
        let ctx = egui::Context::default();
        assert_eq!(tr(&ctx, "File"), "Archivo");
        set_locale(&ctx, Locale::English);
        assert_eq!(tr(&ctx, "File"), "File");
        assert_eq!(tr(&ctx, "custom document.png"), "custom document.png");
    }
}
