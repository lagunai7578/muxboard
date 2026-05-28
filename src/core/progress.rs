#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProgressSemantics {
    normalized: NormalizedSummary,
    phase: ProgressPhase,
    noun: SignalNoun,
}

impl ProgressSemantics {
    pub fn priority(&self) -> u8 {
        if self.normalized.is_generic_state() {
            10
        } else {
            35 + self.noun.priority() + self.phase.priority()
        }
    }

    pub fn normalized(&self) -> &str {
        self.normalized.as_str()
    }

    pub fn into_normalized(self) -> String {
        self.normalized.into_string()
    }

    #[cfg(test)]
    pub fn phase(&self) -> ProgressPhase {
        self.phase
    }

    #[cfg(test)]
    pub fn noun(&self) -> SignalNoun {
        self.noun
    }

    pub fn signal_phrase(&self) -> Option<&'static str> {
        self.noun.phrase()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NormalizedSummary(String);

impl NormalizedSummary {
    fn new(summary: &str) -> Self {
        let trimmed = summary.trim();
        if trimmed.ends_with("...")
            && !trimmed.contains('?')
            && !looks_like_prompt_ellipsis(trimmed)
            && let Some(stripped) = trimmed.strip_suffix("...")
        {
            let stripped = stripped.trim_end();
            if !stripped.is_empty() {
                return Self(stripped.to_owned());
            }
        }

        Self(trimmed.to_owned())
    }

    fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn into_string(self) -> String {
        self.0
    }

    fn is_generic_state(&self) -> bool {
        matches!(
            self.0.as_str(),
            value
                if value.eq_ignore_ascii_case("running")
                    || value.eq_ignore_ascii_case("done")
                    || value.eq_ignore_ascii_case("completed")
                    || value.eq_ignore_ascii_case("error")
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProgressPhase {
    Complete,
    Validate,
    Build,
    Prepare,
    Unknown,
}

impl ProgressPhase {
    fn priority(self) -> u8 {
        match self {
            Self::Complete => 14,
            Self::Validate => 10,
            Self::Build => 7,
            Self::Prepare => 4,
            Self::Unknown => 0,
        }
    }

    fn from_summary(summary: &str) -> Self {
        let normalized = summary.to_ascii_lowercase();

        if contains_any(
            &normalized,
            &[
                "handoff",
                "completed ",
                "complete ",
                "finished ",
                "released ",
                "shipping ",
                "ship ",
            ],
        ) {
            Self::Complete
        } else if contains_any(
            &normalized,
            &[
                "validating",
                "validate ",
                "checking ",
                "checksums",
                "testing ",
                "verified ",
            ],
        ) {
            Self::Validate
        } else if contains_any(
            &normalized,
            &[
                "building",
                "build ",
                "compiling",
                "compile ",
                "installing",
                "install ",
                "syncing",
                "sync ",
            ],
        ) {
            Self::Build
        } else if contains_any(
            &normalized,
            &[
                "preparing",
                "prep ",
                "loading",
                "load ",
                "writing",
                "write ",
                "reading",
                "read ",
                "searching",
                "search ",
                "analyzing",
                "analyse ",
                "analyze ",
            ],
        ) {
            Self::Prepare
        } else {
            Self::Unknown
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SignalNoun {
    NetworkAccess,
    StagingHandoff,
    Handoff,
    ReleaseArtifacts,
    Checksums,
    ArtifactMirrors,
    ReleaseImage,
    ShellAliases,
    Staging,
    Artifacts,
    Unknown,
}

impl SignalNoun {
    fn priority(self) -> u8 {
        match self {
            Self::NetworkAccess | Self::StagingHandoff => 12,
            Self::Handoff => 11,
            Self::ReleaseArtifacts => 10,
            Self::Checksums | Self::ArtifactMirrors => 9,
            Self::ReleaseImage | Self::ShellAliases => 8,
            Self::Staging | Self::Artifacts => 6,
            Self::Unknown => 0,
        }
    }

    pub fn phrase(self) -> Option<&'static str> {
        match self {
            Self::NetworkAccess => Some("network access"),
            Self::StagingHandoff => Some("staging handoff"),
            Self::Handoff => Some("handoff"),
            Self::ReleaseArtifacts => Some("release artifacts"),
            Self::Checksums => Some("checksums"),
            Self::ArtifactMirrors => Some("artifact mirrors"),
            Self::ReleaseImage => Some("release image"),
            Self::ShellAliases => Some("shell aliases"),
            Self::Staging => Some("staging"),
            Self::Artifacts => Some("artifacts"),
            Self::Unknown => None,
        }
    }

    fn from_summary(summary: &str) -> Self {
        let normalized = summary.to_ascii_lowercase();

        [
            Self::NetworkAccess,
            Self::StagingHandoff,
            Self::Handoff,
            Self::ReleaseArtifacts,
            Self::Checksums,
            Self::ArtifactMirrors,
            Self::ReleaseImage,
            Self::ShellAliases,
            Self::Staging,
            Self::Artifacts,
        ]
        .into_iter()
        .find_map(|noun| {
            noun.phrase()
                .and_then(|phrase| normalized.contains(phrase).then_some(noun))
        })
        .unwrap_or(Self::Unknown)
    }
}

pub(crate) fn classify_fallback_summary(summary: &str) -> ProgressSemantics {
    let normalized = NormalizedSummary::new(summary);
    let phrase = normalized.as_str();

    ProgressSemantics {
        phase: ProgressPhase::from_summary(phrase),
        noun: SignalNoun::from_summary(phrase),
        normalized,
    }
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

fn looks_like_prompt_ellipsis(summary: &str) -> bool {
    let normalized = summary.to_ascii_lowercase();
    normalized.starts_with("type your answer")
        || normalized.starts_with("reply in exactly")
        || normalized.starts_with("press enter")
}

#[cfg(test)]
mod tests {
    use super::{ProgressPhase, SignalNoun, classify_fallback_summary};

    #[test]
    fn generic_state_tokens_rank_low() {
        assert_eq!(classify_fallback_summary("running").priority(), 10);
        assert_eq!(classify_fallback_summary("completed").priority(), 10);
    }

    #[test]
    fn stronger_phase_and_signal_nouns_rank_higher() {
        assert!(
            classify_fallback_summary("completed staging handoff").priority()
                > classify_fallback_summary("preparing release image").priority()
        );
        assert!(
            classify_fallback_summary("building release artifacts").priority()
                > classify_fallback_summary("writing logs").priority()
        );
    }

    #[test]
    fn classifies_phase_and_signal_noun_explicitly() {
        let summary = classify_fallback_summary("validating checksums across mirrors");
        assert_eq!(summary.phase(), ProgressPhase::Validate);
        assert_eq!(summary.noun(), SignalNoun::Checksums);

        let summary = classify_fallback_summary("completed staging handoff");
        assert_eq!(summary.phase(), ProgressPhase::Complete);
        assert_eq!(summary.noun(), SignalNoun::StagingHandoff);
        assert_eq!(summary.signal_phrase(), Some("staging handoff"));
    }

    #[test]
    fn visual_ellipsis_is_trimmed_without_touching_prompts() {
        assert_eq!(
            classify_fallback_summary("building...").into_normalized(),
            "building"
        );
        assert_eq!(classify_fallback_summary("...").into_normalized(), "...");
        assert_eq!(
            classify_fallback_summary("Type your answer...").into_normalized(),
            "Type your answer..."
        );
    }
}
