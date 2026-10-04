//! The catalogue of whisper models the fallback engine can use.
//!
//! Pure data and filesystem lookups: which models exist, where they live, and
//! whether one is already on disk. That is everything [`crate::registry`]
//! needs to answer "what can this machine do right now, offline".
//!
//! **Downloading is deliberately not here.** It lives in the `modelfetch`
//! crate, so `crates/stt` has no HTTP stack anywhere in its dependency graph
//! and transcription cannot reach the network even by accident (L9/L10/L11).
//! The pinned URL and SHA-256 stay here, next to the rest of the catalogue,
//! because they describe *what* a model is rather than how it is fetched.
//!
//! On macOS 26+ nothing is ever downloaded at all — Apple's engine ships with
//! the OS and the first-run download is zero bytes (SPEC §2.4).

use std::path::{Path, PathBuf};

use crate::Error;
use crate::hardware::{Gpu, Hardware};
use crate::languages::LanguageSet;

/// The Parakeet engine's model, a folder of three files (TUR-62).
pub mod parakeet;

/// A model we are willing to download.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelSpec {
    /// The id used in `config.jsonc` (`transcription.model`).
    pub id: &'static str,
    /// Filename on disk, inside [`model_dir`].
    pub filename: &'static str,
    /// Pinned download URL.
    pub url: &'static str,
    /// Expected SHA-256 of the finished file, lowercase hex.
    pub sha256: &'static str,
    /// Expected size in bytes, for progress reporting and a cheap sanity check.
    pub bytes: u64,
    /// What the model is good for, in plain words, for the Settings rows
    /// (TUR-79). Data, so the screen never spells out a model's merits itself.
    pub facts: ModelFacts,
}

/// One word about a model, drawn as a small chip. The UI owns the label for
/// each, so the wording stays the same on every row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "kebab-case")]
pub enum ModelTag {
    Fast,
    Light,
    MostAccurate,
    Multilingual,
    EnglishOnly,
    Slower,
}

/// The plain-word facts about one model.
///
/// Honest words only: speed and accuracy relative to the other models in this
/// list, never a made-up number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelFacts {
    /// The name on the row, e.g. "Small (English only)". No quantisation or
    /// file-format jargon; the full id is shown in the detail line instead.
    pub display_name: &'static str,
    /// One line on when to pick it.
    pub good_for: &'static str,
    pub tags: &'static [ModelTag],
    /// The languages it understands, for the Settings (i) button (TUR-94),
    /// and what [`ModelSpec::whisper_language`] is read from.
    pub languages: LanguageSet,
}

impl ModelFacts {
    /// No facts at all, for test fixtures that are not in [`MODELS`]. The
    /// catalogue test fails if a real entry uses it.
    pub const NONE: Self = Self {
        display_name: "",
        good_for: "",
        tags: &[],
        languages: LanguageSet::EnglishOnly,
    };
}

impl ModelSpec {
    /// The language to tell whisper the audio is in (TUR-94): `en` for an
    /// English-only model, `None` (auto-detect) for every multilingual one.
    ///
    /// Telling a multilingual model "this is English" makes it write Hindi
    /// speech as made-up English, so no recording path may fall back to
    /// `WhisperConfig::default()`'s `en` for one of these.
    pub fn whisper_language(&self) -> Option<&'static str> {
        match self.facts.languages {
            LanguageSet::EnglishOnly => Some("en"),
            LanguageSet::Multilingual99 | LanguageSet::Multilingual100 => None,
        }
    }
}

/// [`ModelSpec::whisper_language`] for a model file on disk.
///
/// A file from [`MODELS`] uses its entry. Any other file (one pointed at with
/// `MEET_WHISPER_MODEL`) follows whisper.cpp's naming: `.en` in the name means
/// English only, anything else is treated as multilingual and auto-detects.
pub fn whisper_language_for_file(path: &Path) -> Option<&'static str> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    match MODELS.iter().find(|spec| spec.filename == name) {
        Some(spec) => spec.whisper_language(),
        None if name.contains(".en") => Some("en"),
        None => None,
    }
}

/// The models SPEC §2.4 names, plus Medium and Large (TUR-94).
///
/// URLs point at `resolve/main` on Hugging Face, which is the canonical
/// distribution point for `whisper.cpp` GGML weights. The digests are the LFS
/// object ids Hugging Face publishes for these exact files, read from its API
/// on 2026-09-27 (small, large turbo) and 2026-10-04 (medium, large), not
/// computed from a local download, which would only prove the bytes matched
/// themselves.
///
/// Reading an id: `.en` means English only, no `.en` means multilingual (99
/// languages, 100 for large-v3 and its turbo; see [`crate::languages`]);
/// `q5_0` / `q5_1` mean compressed to 5 bits, at near-full quality. None of
/// that reaches a row's main line; see [`ModelFacts`].
///
/// Accuracy words follow OpenAI's README: turbo is "an optimized version of
/// large-v3 that offers faster transcription speed with a minimal degradation
/// in accuracy", and medium is listed at ~2x the speed of large, turbo at ~8x.
pub const MODELS: &[ModelSpec] = &[
    ModelSpec {
        id: "small.en-q5_1",
        filename: "ggml-small.en-q5_1.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.en-q5_1.bin",
        sha256: "bfdff4894dcb76bbf647d56263ea2a96645423f1669176f4844a1bf8e478ad30",
        bytes: 190_098_681,
        facts: ModelFacts {
            display_name: "Small (English only)",
            good_for: "Lightweight and fast. Good for clear English calls; less accurate with \
                       accents, cross-talk or jargon.",
            tags: &[ModelTag::Fast, ModelTag::Light, ModelTag::EnglishOnly],
            languages: LanguageSet::EnglishOnly,
        },
    },
    ModelSpec {
        id: "medium-q5_0",
        filename: "ggml-medium-q5_0.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-medium-q5_0.bin",
        sha256: "19fea4b380c3a618ec4723c3eef2eb785ffba0d0538cf43f8f235e7b3b34220f",
        bytes: 539_212_467,
        facts: ModelFacts {
            display_name: "Medium (multilingual)",
            good_for: "Mid-size. Understands 99 languages. Faster and lighter on memory than \
                       Large, less accurate on hard audio. Large turbo is faster still.",
            tags: &[ModelTag::Multilingual],
            languages: LanguageSet::Multilingual99,
        },
    },
    ModelSpec {
        id: "large-v3-turbo-q5_0",
        filename: "ggml-large-v3-turbo-q5_0.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin",
        sha256: "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2",
        // SPEC §2.4 estimates ~1.6 GB for this file. The real q5_0 turbo build
        // is 574 MB; the 1.6 GB figure belongs to an unquantized large-v3.
        bytes: 574_041_195,
        facts: ModelFacts {
            display_name: "Large turbo (multilingual)",
            good_for: "Close to Large in accuracy and much faster. Understands 100 languages. \
                       Slightly less accurate than Large for some languages other than English.",
            tags: &[ModelTag::Multilingual],
            languages: LanguageSet::Multilingual100,
        },
    },
    ModelSpec {
        id: "large-v3-q5_0",
        filename: "ggml-large-v3-q5_0.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-q5_0.bin",
        sha256: "d75795ecff3f83b5faa89d1900604ad8c780abd5739fae406de19f23ecd98ad1",
        bytes: 1_081_140_203,
        facts: ModelFacts {
            display_name: "Large (multilingual)",
            good_for: "Most accurate. Understands 100 languages; the best choice for languages \
                       other than English and mixed-language calls, such as Hindi and English. \
                       Biggest download, slowest, and uses the most memory.",
            tags: &[
                ModelTag::MostAccurate,
                ModelTag::Multilingual,
                ModelTag::Slower,
            ],
            languages: LanguageSet::Multilingual100,
        },
    },
];

/// Look up a model by its `config.jsonc` id.
pub fn find(id: &str) -> Option<&'static ModelSpec> {
    MODELS.iter().find(|model| model.id == id)
}

/// The id of the small model, picked when this machine has no GPU whisper can
/// use, or is short of memory.
pub const SMALL_MODEL: &str = "small.en-q5_1";
/// The id of the large model, picked with a GPU whisper can use and enough
/// memory.
pub const LARGE_MODEL: &str = "large-v3-turbo-q5_0";

/// The memory the large model is recommended from: 16 GB.
///
/// Initial thresholds, tune with measured numbers (TUR-61): this is the line
/// TUR-79 drew for Apple silicon, reused for Vulkan GPUs until someone times
/// both models on real Windows and Linux machines.
pub const LARGE_MODEL_MIN_MEMORY: u64 = 16 * 1024 * 1024 * 1024;

/// Which model to suggest for this machine, and why, in one line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recommendation {
    pub model_id: &'static str,
    pub reason: String,
}

/// The hardware tier's model (TUR-61): large turbo with a GPU whisper can use
/// and at least [`LARGE_MODEL_MIN_MEMORY`], small otherwise. It is the model
/// Settings marks "Recommended" and the default when `transcription.model` is
/// not set; a model set in config always wins.
///
/// Pure, with the machine passed in, so every tier is tested on every OS. The
/// app reads the machine once with [`crate::hardware::Hardware::detect`].
pub fn recommended(hardware: &Hardware) -> Recommendation {
    let machine = crate::platform::MACHINE;
    let gb = gigabytes(hardware.total_memory_bytes);
    match hardware.gpu {
        Gpu::Usable if hardware.total_memory_bytes >= LARGE_MODEL_MIN_MEMORY => Recommendation {
            model_id: LARGE_MODEL,
            reason: format!(
                "{machine} has {} and {gb} GB of memory, enough for Large turbo, close to the \
                 most accurate model and much faster.",
                crate::platform::GPU_YES
            ),
        },
        Gpu::Usable => Recommendation {
            model_id: SMALL_MODEL,
            reason: format!(
                "{machine} has {gb} GB of memory; the lighter model leaves room for the call."
            ),
        },
        Gpu::None => Recommendation {
            model_id: SMALL_MODEL,
            reason: format!(
                "{machine} has {}, so the lighter model keeps up better.",
                crate::platform::GPU_NONE
            ),
        },
        Gpu::CrashedBefore => Recommendation {
            model_id: SMALL_MODEL,
            reason: format!(
                "{machine}'s graphics chip crashed during transcription before, so transcription \
                 runs on the processor, where the lighter model keeps up better."
            ),
        },
    }
}

/// The model to use when `transcription.model` is not set: the
/// [`recommended`] one, unless only other models are downloaded, in which case
/// the first of those in catalogue order. A user who downloaded a model before
/// the tiers existed keeps using it instead of being told to download another.
pub fn default_model(
    recommended: &'static str,
    is_installed: impl Fn(&ModelSpec) -> bool,
) -> &'static str {
    let installed = |id: &str| find(id).is_some_and(&is_installed);
    if installed(recommended) {
        return recommended;
    }
    MODELS
        .iter()
        .find(|spec| is_installed(spec))
        .map_or(recommended, |spec| spec.id)
}

/// Whole gigabytes, rounded down, so a reason never claims more than the line.
fn gigabytes(bytes: u64) -> u64 {
    bytes / (1024 * 1024 * 1024)
}

/// The variable both this crate and the app read to point meet-ai at a
/// different meetings root — a fixture folder in development, most often. An
/// empty value means unset, in both places.
pub const MEETINGS_ROOT_ENV: &str = "MEET_AI_MEETINGS_ROOT";

/// Where models live: `<meetings root>/.app/models/` (SPEC §3.1).
///
/// The root is an argument because it is not always `~/Meetings`: the user can
/// move it from Settings, and `change_root` carries `.app/` — models included —
/// along with every meeting. A directory derived here from `~` would keep
/// pointing at the old place after that move, so a model the user had already
/// downloaded would read as missing and be fetched a second time into a folder
/// the app no longer owns. The app passes its own resolved root
/// (`meetings::root()`); only tools with no app to ask use
/// [`default_model_dir`].
pub fn model_dir(meetings_root: &Path) -> PathBuf {
    meeting_format::layout::models_dir(meetings_root)
}

/// [`model_dir`] under the fallback root, for callers that have no app to ask.
///
/// That is the `meet-stt-model` CLI, the `offline_meeting` example and the
/// tests — nothing that runs inside the app. The fallback root is
/// `MEET_AI_MEETINGS_ROOT` when set (the same override the app's
/// `meetings::root()` honours first), otherwise `~/Meetings`. It cannot see a
/// folder the user picked in Settings: that choice is recorded by the app, and
/// reading it from here would put a second copy of the app's root rules in a
/// crate that should not know them. Pass `--dir` to the CLI in that case.
///
/// Built with `dirs` + `PathBuf::join` and no literal `~`, per the Windows seam
/// in SPEC §8.2.
pub fn default_model_dir() -> Result<PathBuf, Error> {
    let root = fallback_meetings_root(std::env::var_os(MEETINGS_ROOT_ENV), dirs::home_dir())?;
    Ok(model_dir(&root))
}

/// The fallback root, with its two inputs passed in so the precedence can be
/// tested without mutating the process environment under parallel tests.
fn fallback_meetings_root(
    env_override: Option<std::ffi::OsString>,
    home: Option<PathBuf>,
) -> Result<PathBuf, Error> {
    if let Some(custom) = env_override.filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(custom));
    }
    let home =
        home.ok_or_else(|| Error::Engine("could not determine the home directory".into()))?;
    Ok(meeting_format::layout::default_root(&home))
}

/// Is this model already present and verified?
///
/// Only checks for the finished file. A stale `.part` is not "present"; it is
/// the thing `modelfetch::ensure` resumes.
pub fn is_installed(spec: &ModelSpec, dir: &Path) -> bool {
    dir.join(spec.filename).is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pinned_model_has_a_plausible_digest_and_https_url() {
        for spec in MODELS {
            assert_eq!(spec.sha256.len(), 64, "{} digest is not 32 bytes", spec.id);
            assert!(
                spec.sha256.chars().all(|c| c.is_ascii_hexdigit()),
                "{} digest is not hex",
                spec.id
            );
            assert!(
                spec.url
                    .starts_with("https://huggingface.co/ggerganov/whisper.cpp/"),
                "{} is not pinned to the expected host",
                spec.id
            );
            assert!(spec.bytes > 0);
        }
    }

    #[test]
    fn models_are_found_by_their_config_id() {
        assert!(find("small.en-q5_1").is_some());
        assert!(find("large-v3-turbo-q5_0").is_some());
        assert!(find("not-a-model").is_none());
        assert!(find(SMALL_MODEL).is_some());
        assert!(find(LARGE_MODEL).is_some());
    }

    #[test]
    fn every_catalogue_model_says_what_it_is_good_for_in_plain_words() {
        // TUR-79: a model added without its facts fails here, not on screen.
        for spec in MODELS {
            let facts = spec.facts;
            assert_ne!(facts, ModelFacts::NONE, "{} has no facts", spec.id);
            assert!(
                !facts.display_name.trim().is_empty(),
                "{} has no name",
                spec.id
            );
            assert!(
                !facts.good_for.trim().is_empty(),
                "{} has no good-for line",
                spec.id
            );
            assert!(!facts.tags.is_empty(), "{} has no tags", spec.id);
            // The id's jargon stays in the detail line, never the main one.
            for jargon in ["q5", "ggml", "RTF", ".bin", spec.id] {
                assert!(
                    !facts.display_name.contains(jargon) && !facts.good_for.contains(jargon),
                    "{}'s main line says {jargon:?}",
                    spec.id
                );
            }
            // `.en` = English only, no `.en` = multilingual.
            let english_only = spec.id.contains(".en");
            assert_eq!(
                facts.tags.contains(&ModelTag::EnglishOnly),
                english_only,
                "{}",
                spec.id
            );
            assert_eq!(
                facts.tags.contains(&ModelTag::Multilingual),
                !english_only,
                "{}",
                spec.id
            );
        }
    }

    #[test]
    fn the_catalogue_facts_are_the_ones_the_ticket_names() {
        let small = find("small.en-q5_1").unwrap().facts;
        assert_eq!(small.display_name, "Small (English only)");
        assert_eq!(
            small.tags,
            &[ModelTag::Fast, ModelTag::Light, ModelTag::EnglishOnly]
        );
        let turbo = find("large-v3-turbo-q5_0").unwrap().facts;
        assert_eq!(turbo.display_name, "Large turbo (multilingual)");
        assert_eq!(turbo.tags, &[ModelTag::Multilingual]);
        let medium = find("medium-q5_0").unwrap().facts;
        assert_eq!(medium.display_name, "Medium (multilingual)");
        assert_eq!(medium.tags, &[ModelTag::Multilingual]);
        let large = find("large-v3-q5_0").unwrap().facts;
        assert_eq!(large.display_name, "Large (multilingual)");
        assert_eq!(
            large.tags,
            &[
                ModelTag::MostAccurate,
                ModelTag::Multilingual,
                ModelTag::Slower
            ]
        );
        // Only Large carries the Slower chip.
        for spec in MODELS {
            assert_eq!(
                spec.facts.tags.contains(&ModelTag::Slower),
                spec.id == "large-v3-q5_0",
                "{}",
                spec.id
            );
        }
        // Only Large claims to be the most accurate.
        for spec in MODELS {
            assert_eq!(
                spec.facts.tags.contains(&ModelTag::MostAccurate),
                spec.id == "large-v3-q5_0",
                "{}",
                spec.id
            );
        }
    }

    #[test]
    fn the_models_are_listed_in_order_with_their_pinned_files() {
        let ids: Vec<_> = MODELS.iter().map(|spec| spec.id).collect();
        assert_eq!(
            ids,
            [
                "small.en-q5_1",
                "medium-q5_0",
                "large-v3-turbo-q5_0",
                "large-v3-q5_0"
            ]
        );
        for spec in MODELS {
            assert_eq!(spec.filename, format!("ggml-{}.bin", spec.id));
            assert!(spec.url.ends_with(spec.filename), "{}", spec.id);
        }
        assert_eq!(find("medium-q5_0").unwrap().bytes, 539_212_467);
        assert_eq!(find("large-v3-q5_0").unwrap().bytes, 1_081_140_203);
    }

    #[test]
    fn english_only_models_get_en_and_multilingual_ones_auto_detect() {
        for spec in MODELS {
            let expected = spec.id.contains(".en").then_some("en");
            assert_eq!(spec.whisper_language(), expected, "{}", spec.id);
            let on_disk = Path::new("/models").join(spec.filename);
            assert_eq!(whisper_language_for_file(&on_disk), expected, "{}", spec.id);
        }
        assert_eq!(
            find("small.en-q5_1").unwrap().whisper_language(),
            Some("en")
        );
        assert_eq!(
            find("large-v3-turbo-q5_0").unwrap().whisper_language(),
            None
        );
        // Files outside the catalogue go by whisper.cpp's naming.
        assert_eq!(
            whisper_language_for_file(Path::new("/x/ggml-base.en.bin")),
            Some("en")
        );
        assert_eq!(
            whisper_language_for_file(Path::new("/x/ggml-base.bin")),
            None
        );
    }

    #[test]
    fn every_model_has_the_language_list_its_id_implies() {
        for spec in MODELS {
            let languages = spec.facts.languages.languages();
            assert!(!languages.is_empty(), "{}", spec.id);
            let expected = if spec.id.contains(".en") {
                1
            } else if spec.id.starts_with("large-v3") {
                100
            } else {
                99
            };
            assert_eq!(languages.len(), expected, "{}", spec.id);
            // The good-for line states the same count the (i) list shows.
            if expected > 1 {
                assert!(
                    spec.facts
                        .good_for
                        .contains(&format!("{expected} languages")),
                    "{}",
                    spec.id
                );
            }
        }
        assert_eq!(
            find("small.en-q5_1").unwrap().facts.languages.languages(),
            &[("en", "English")]
        );
    }

    #[test]
    fn tags_reach_the_window_in_kebab_case() {
        assert_eq!(
            serde_json::to_string(&ModelTag::MostAccurate).unwrap(),
            "\"most-accurate\""
        );
        assert_eq!(
            serde_json::to_string(&ModelTag::EnglishOnly).unwrap(),
            "\"english-only\""
        );
    }

    const GB: u64 = 1024 * 1024 * 1024;

    fn machine(gpu: Gpu, gb: u64) -> Hardware {
        Hardware {
            gpu,
            total_memory_bytes: gb * GB,
        }
    }

    #[test]
    fn large_turbo_is_recommended_with_a_gpu_and_16_gb_or_more() {
        let pick = recommended(&machine(Gpu::Usable, 16));
        assert_eq!(pick.model_id, LARGE_MODEL);
        assert!(pick.reason.contains("16 GB"), "{}", pick.reason);
        assert!(
            pick.reason.contains(crate::platform::GPU_YES),
            "{}",
            pick.reason
        );
        assert_eq!(recommended(&machine(Gpu::Usable, 64)).model_id, LARGE_MODEL);
    }

    #[test]
    fn small_is_recommended_below_16_gb_or_without_a_gpu() {
        let pick = recommended(&machine(Gpu::Usable, 8));
        assert_eq!(pick.model_id, SMALL_MODEL);
        assert!(pick.reason.contains("8 GB"), "{}", pick.reason);
        // One byte short of the line is still under it.
        let short = Hardware {
            gpu: Gpu::Usable,
            total_memory_bytes: 16 * GB - 1,
        };
        assert_eq!(recommended(&short).model_id, SMALL_MODEL);
        let no_gpu = recommended(&machine(Gpu::None, 64));
        assert_eq!(no_gpu.model_id, SMALL_MODEL);
        assert!(
            no_gpu.reason.contains(crate::platform::GPU_NONE),
            "{}",
            no_gpu.reason
        );
        // Unknown memory (0) is not a reason to suggest the big one.
        assert_eq!(recommended(&machine(Gpu::Usable, 0)).model_id, SMALL_MODEL);
    }

    #[test]
    fn a_gpu_that_crashed_before_gets_the_small_model_and_says_why() {
        let pick = recommended(&machine(Gpu::CrashedBefore, 64));
        assert_eq!(pick.model_id, SMALL_MODEL);
        assert!(pick.reason.contains("crashed"), "{}", pick.reason);
        assert!(pick.reason.contains("processor"), "{}", pick.reason);
    }

    #[test]
    fn every_reason_names_the_machine_the_way_this_os_does() {
        for gpu in [Gpu::Usable, Gpu::None, Gpu::CrashedBefore] {
            for gb in [8, 32] {
                let reason = recommended(&machine(gpu, gb)).reason.to_lowercase();
                assert!(
                    reason.contains(&crate::platform::MACHINE.to_lowercase()),
                    "{reason}"
                );
                assert!(!reason.contains('\u{2014}'), "em dash in {reason}");
            }
        }
    }

    #[test]
    fn the_default_model_is_the_recommended_one_unless_only_others_are_here() {
        let none = |_: &ModelSpec| false;
        assert_eq!(default_model(LARGE_MODEL, none), LARGE_MODEL);
        assert_eq!(default_model(SMALL_MODEL, none), SMALL_MODEL);

        // Installed before the tiers: keep it rather than ask for a download.
        let turbo_only = |spec: &ModelSpec| spec.id == LARGE_MODEL;
        assert_eq!(default_model(SMALL_MODEL, turbo_only), LARGE_MODEL);

        // The recommended one is here: it wins over others that are too.
        let both = |spec: &ModelSpec| spec.id == LARGE_MODEL || spec.id == SMALL_MODEL;
        assert_eq!(default_model(LARGE_MODEL, both), LARGE_MODEL);
        assert_eq!(default_model(SMALL_MODEL, both), SMALL_MODEL);

        // Several others: the first in catalogue order.
        let medium_and_large =
            |spec: &ModelSpec| spec.id.starts_with("medium") || spec.id == "large-v3-q5_0";
        assert_eq!(default_model(SMALL_MODEL, medium_and_large), "medium-q5_0");
    }

    #[test]
    fn the_model_directory_is_under_whatever_root_it_is_given() {
        // A root the user picked in Settings, nowhere near `~/Meetings`. The
        // models must follow it, or `change_root` strands them (SPEC §3.1).
        let root = Path::new("/Volumes/Archive/Work meetings");
        assert_eq!(
            model_dir(root),
            Path::new("/Volumes/Archive/Work meetings/.app/models")
        );
    }

    #[test]
    fn the_fallback_root_prefers_the_env_override_over_home() {
        let home = Some(PathBuf::from("/Users/someone"));
        assert_eq!(
            fallback_meetings_root(Some("/tmp/fixture-root".into()), home.clone()).unwrap(),
            Path::new("/tmp/fixture-root")
        );
        assert_eq!(
            fallback_meetings_root(None, home.clone()).unwrap(),
            Path::new("/Users/someone/Meetings")
        );
        // An empty variable is "unset", not "the current directory".
        assert_eq!(
            fallback_meetings_root(Some("".into()), home).unwrap(),
            Path::new("/Users/someone/Meetings")
        );
        assert!(fallback_meetings_root(None, None).is_err());
    }

    #[test]
    fn the_fallback_model_directory_is_under_a_meetings_root() {
        let dir = default_model_dir().unwrap();
        assert!(dir.ends_with(".app/models"), "{}", dir.display());
    }
}
