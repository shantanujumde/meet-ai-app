//! The languages each whisper model understands, for the Settings (i) button
//! (TUR-94).
//!
//! One list, in OpenAI's order. Whisper's multilingual models before large-v3
//! know the first 99 entries; large-v3 and large-v3-turbo add Cantonese
//! (`yue`), the 100th. In `tokenizer.py`, `get_encoding` defaults
//! `num_languages` to 99, and `whisper/model.py` sets it to
//! `n_vocab - 51765 - 1`, which is 100 for the 51866-token large-v3 vocabulary.
//! The English-only (`.en`) models know English alone.
//!
//! Names are the upstream ones in title case ("haitian creole" becomes
//! "Haitian Creole"); the codes are unchanged.

/// Which language list a model uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LanguageSet {
    /// The `.en` models: English only.
    EnglishOnly,
    /// tiny to large-v2 and medium: the first 99 of [`WHISPER_LANGUAGES`].
    Multilingual99,
    /// large-v3 and large-v3-turbo: all 100, with Cantonese.
    Multilingual100,
}

impl LanguageSet {
    /// The `(code, English name)` pairs this set covers, in upstream order.
    pub fn languages(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::EnglishOnly => &WHISPER_LANGUAGES[..1],
            Self::Multilingual99 => &WHISPER_LANGUAGES[..99],
            Self::Multilingual100 => WHISPER_LANGUAGES,
        }
    }
}

// Source: openai/whisper whisper/tokenizer.py @ 86098128c0b4f24f0e2aa2994de830614b474227 (MIT)
// Adapted from github.com/openai/whisper/whisper/tokenizer.py @ 86098128c0b4f24f0e2aa2994de830614b474227 (MIT)
// Copyright (c) 2022 OpenAI. The `LANGUAGES` table, names title-cased.
/// Every language Whisper has a token for, `(code, English name)`.
pub const WHISPER_LANGUAGES: &[(&str, &str)] = &[
    ("en", "English"),
    ("zh", "Chinese"),
    ("de", "German"),
    ("es", "Spanish"),
    ("ru", "Russian"),
    ("ko", "Korean"),
    ("fr", "French"),
    ("ja", "Japanese"),
    ("pt", "Portuguese"),
    ("tr", "Turkish"),
    ("pl", "Polish"),
    ("ca", "Catalan"),
    ("nl", "Dutch"),
    ("ar", "Arabic"),
    ("sv", "Swedish"),
    ("it", "Italian"),
    ("id", "Indonesian"),
    ("hi", "Hindi"),
    ("fi", "Finnish"),
    ("vi", "Vietnamese"),
    ("he", "Hebrew"),
    ("uk", "Ukrainian"),
    ("el", "Greek"),
    ("ms", "Malay"),
    ("cs", "Czech"),
    ("ro", "Romanian"),
    ("da", "Danish"),
    ("hu", "Hungarian"),
    ("ta", "Tamil"),
    ("no", "Norwegian"),
    ("th", "Thai"),
    ("ur", "Urdu"),
    ("hr", "Croatian"),
    ("bg", "Bulgarian"),
    ("lt", "Lithuanian"),
    ("la", "Latin"),
    ("mi", "Maori"),
    ("ml", "Malayalam"),
    ("cy", "Welsh"),
    ("sk", "Slovak"),
    ("te", "Telugu"),
    ("fa", "Persian"),
    ("lv", "Latvian"),
    ("bn", "Bengali"),
    ("sr", "Serbian"),
    ("az", "Azerbaijani"),
    ("sl", "Slovenian"),
    ("kn", "Kannada"),
    ("et", "Estonian"),
    ("mk", "Macedonian"),
    ("br", "Breton"),
    ("eu", "Basque"),
    ("is", "Icelandic"),
    ("hy", "Armenian"),
    ("ne", "Nepali"),
    ("mn", "Mongolian"),
    ("bs", "Bosnian"),
    ("kk", "Kazakh"),
    ("sq", "Albanian"),
    ("sw", "Swahili"),
    ("gl", "Galician"),
    ("mr", "Marathi"),
    ("pa", "Punjabi"),
    ("si", "Sinhala"),
    ("km", "Khmer"),
    ("sn", "Shona"),
    ("yo", "Yoruba"),
    ("so", "Somali"),
    ("af", "Afrikaans"),
    ("oc", "Occitan"),
    ("ka", "Georgian"),
    ("be", "Belarusian"),
    ("tg", "Tajik"),
    ("sd", "Sindhi"),
    ("gu", "Gujarati"),
    ("am", "Amharic"),
    ("yi", "Yiddish"),
    ("lo", "Lao"),
    ("uz", "Uzbek"),
    ("fo", "Faroese"),
    ("ht", "Haitian Creole"),
    ("ps", "Pashto"),
    ("tk", "Turkmen"),
    ("nn", "Nynorsk"),
    ("mt", "Maltese"),
    ("sa", "Sanskrit"),
    ("lb", "Luxembourgish"),
    ("my", "Myanmar"),
    ("bo", "Tibetan"),
    ("tl", "Tagalog"),
    ("mg", "Malagasy"),
    ("as", "Assamese"),
    ("tt", "Tatar"),
    ("haw", "Hawaiian"),
    ("ln", "Lingala"),
    ("ha", "Hausa"),
    ("ba", "Bashkir"),
    ("jw", "Javanese"),
    ("su", "Sundanese"),
    ("yue", "Cantonese"),
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn the_table_has_the_100_languages_upstream_lists() {
        assert_eq!(WHISPER_LANGUAGES.len(), 100);
        assert_eq!(WHISPER_LANGUAGES[0], ("en", "English"));
        assert_eq!(WHISPER_LANGUAGES[99], ("yue", "Cantonese"));
        assert!(WHISPER_LANGUAGES.contains(&("hi", "Hindi")));
    }

    #[test]
    fn codes_and_names_are_unique() {
        let codes: HashSet<_> = WHISPER_LANGUAGES.iter().map(|(c, _)| c).collect();
        let names: HashSet<_> = WHISPER_LANGUAGES.iter().map(|(_, n)| n).collect();
        assert_eq!(codes.len(), WHISPER_LANGUAGES.len());
        assert_eq!(names.len(), WHISPER_LANGUAGES.len());
    }

    #[test]
    fn each_set_has_the_size_upstream_gives_it() {
        assert_eq!(LanguageSet::EnglishOnly.languages(), &[("en", "English")]);
        assert_eq!(LanguageSet::Multilingual99.languages().len(), 99);
        assert!(
            !LanguageSet::Multilingual99
                .languages()
                .iter()
                .any(|(code, _)| *code == "yue")
        );
        assert_eq!(LanguageSet::Multilingual100.languages().len(), 100);
    }
}
