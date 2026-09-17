//! Which skills a message is about (docs/plan/12 §A4).
//!
//! Deterministic keyword scoring, on purpose. A local embedding model would mean shipping an
//! inference runtime and a model file to rank a few dozen short descriptions — and a
//! description written to say *when to use this* already carries the words a matching message
//! uses. The model pulling a skill on demand with `gantry__load_skill` covers what keywords
//! miss, which is the half of the design that scales past a hundred skills.

use std::collections::HashSet;

use gantry_core::SkillDto;

/// Words that carry no signal. Kept short: this is about not scoring "the" against a
/// description that happens to contain it, not about linguistics.
const STOPWORDS: &[&str] = &[
    "a", "about", "after", "all", "also", "am", "an", "and", "any", "are", "as", "at", "be",
    "been", "before", "being", "but", "by", "can", "could", "did", "do", "does", "doing", "done",
    "for", "from", "get", "give", "had", "has", "have", "he", "her", "here", "him", "his", "how",
    "i", "if", "in", "into", "is", "it", "its", "just", "make", "may", "me", "might", "more",
    "most", "must", "my", "need", "no", "not", "now", "of", "on", "one", "only", "or", "other",
    "our", "out", "over", "please", "said", "same", "see", "she", "should", "so", "some", "still",
    "such", "take", "than", "that", "the", "their", "them", "then", "there", "these", "they",
    "this", "those", "through", "to", "too", "under", "up", "us", "use", "used", "using", "very",
    "want", "was", "way", "we", "well", "were", "what", "when", "where", "which", "while", "who",
    "why", "will", "with", "would", "you", "your",
];

/// A score of this much means the message is about the skill: one trigger hit, or a name hit
/// plus a description hit (12 §A4 rule 2).
pub const QUALIFYING_SCORE: u32 = 3;
/// How many matched skills one message may pull in.
pub const TOP_N: usize = 3;

/// The message, reduced to what is worth matching on.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Terms {
    /// Stemmed, stopword-free single words.
    pub words: HashSet<String>,
    /// Adjacent pairs, **including** stopwords, so a trigger phrase like "borrow checker" or
    /// "look over" can hit and only hit when the phrase is really there.
    pub bigrams: HashSet<String>,
    /// The same words unstemmed, for the full-text query the memory selector makes (12 §B4).
    pub raw: Vec<String>,
}

impl Terms {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }
}

/// One token of a message or a trigger: what it looked like, what it stems to, and whether it
/// is a word that carries no signal on its own.
struct Token {
    lower: String,
    stem: String,
    stop: bool,
}

/// Lowercase, split on anything that is not a letter or digit, stem lightly. Stopwords are
/// kept, in place, because a phrase is a phrase: dropping them here is what turned the trigger
/// "look over" into the single word "look", which then matched "very good looking nature" and
/// sent a code-review playbook with a request for a picture (found live, 2026-09-17).
fn tokens(text: &str) -> Vec<Token> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(str::to_lowercase)
        .filter(|lower| lower.len() >= 2)
        .map(|lower| Token {
            stem: stem(&lower),
            stop: STOPWORDS.contains(&lower.as_str()),
            lower,
        })
        .collect()
}

/// Lowercase, split on anything that is not a letter or digit, drop stopwords, stem lightly.
///
/// The stemming is a handful of suffix rules rather than a stemmer crate. 12 §A4 named
/// `rust-stemmers`; what the scoring actually needs is that "commits", "committing" and
/// "commit" land on the same token, and three rules do that without a dependency whose whole
/// job is a heuristic inside another heuristic.
#[must_use]
pub fn terms(message: &str) -> Terms {
    let tokens = tokens(message);
    let mut words = HashSet::new();
    let mut raw = Vec::new();
    for token in &tokens {
        if token.stop {
            continue;
        }
        if !raw.contains(&token.lower) {
            raw.push(token.lower.clone());
        }
        words.insert(token.stem.clone());
    }
    let bigrams = tokens
        .windows(2)
        .map(|w| format!("{} {}", w[0].stem, w[1].stem))
        .collect();
    Terms {
        words,
        bigrams,
        raw,
    }
}

/// Light suffix stripping. Deliberately conservative: a wrong stem costs a match, and a missed
/// stem only costs the plural.
#[must_use]
pub fn stem(word: &str) -> String {
    for suffix in ["ings", "ing", "ies", "edly", "ed", "es", "s"] {
        if let Some(root) = word.strip_suffix(suffix)
            && root.len() >= 4
        {
            // "ies" → "y" keeps "libraries" and "library" together.
            if suffix == "ies" {
                return format!("{root}y");
            }
            // A doubled consonant before -ing or -ed: "committing" → "commit".
            let mut chars = root.chars().rev();
            if matches!(suffix, "ing" | "ings" | "ed")
                && let (Some(a), Some(b)) = (chars.next(), chars.next())
                && a == b
                && !"aeiou".contains(a)
            {
                return root[..root.len() - a.len_utf8()].to_owned();
            }
            return root.to_owned();
        }
    }
    word.to_owned()
}

/// One skill's score against a message, and why it got it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Score {
    pub id: String,
    pub score: u32,
    /// The words that matched, for the Test match box in the editor (12 §A5).
    pub hits: Vec<String>,
}

/// Scores every candidate: `3 × trigger hits + 2 × name hits + 1 × description hits`.
#[must_use]
pub fn score(terms: &Terms, skill: &SkillDto) -> Score {
    let mut score = 0;
    let mut hits = Vec::new();
    for trigger in &skill.triggers {
        if hit(terms, trigger) {
            score += 3;
            hits.push(trigger.clone());
        }
    }
    for word in words_of(&skill.name) {
        if terms.words.contains(&word) {
            score += 2;
            hits.push(word);
        }
    }
    for word in words_of(&skill.description) {
        if terms.words.contains(&word) && !hits.contains(&word) {
            score += 1;
            hits.push(word);
        }
    }
    Score {
        id: skill.id.clone(),
        score,
        hits,
    }
}

/// The matched skills for a message, best first, already cut to `TOP_N` and to the skills the
/// caller says are still worth injecting.
///
/// `skip` is what the last few turns already injected (12 §A4 rule 3) together with anything
/// pinned or always-on, which lives in the frozen prompt instead.
#[must_use]
pub fn matches(message: &str, skills: &[SkillDto], skip: &HashSet<String>) -> Vec<Score> {
    let terms = terms(message);
    if terms.is_empty() {
        return Vec::new();
    }
    let mut scored: Vec<Score> = skills
        .iter()
        .filter(|s| s.enabled && !s.always_include && !skip.contains(&s.id))
        .map(|s| score(&terms, s))
        .filter(|s| s.score >= QUALIFYING_SCORE)
        .collect();
    // Highest first; ties by name, so the same message always picks the same skills.
    scored.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.id.cmp(&b.id)));
    scored.truncate(TOP_N);
    scored
}

/// Whether a trigger appears in the message: a phrase against the bigrams, a word against the
/// words.
///
/// Tokenised exactly like the message, stopwords and all, so the two sides agree about what a
/// phrase is. A one-word trigger that is itself a stopword matches nothing, because `words`
/// never holds one.
fn hit(terms: &Terms, trigger: &str) -> bool {
    let parts: Vec<String> = tokens(trigger).into_iter().map(|t| t.stem).collect();
    match parts.len() {
        0 => false,
        1 => terms.words.contains(&parts[0]),
        // A longer phrase is checked pair by pair: every adjacent pair must be in the message,
        // which is as close to "the phrase is there" as bigrams get.
        _ => parts
            .windows(2)
            .all(|w| terms.bigrams.contains(&format!("{} {}", w[0], w[1]))),
    }
}

fn words_of(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() >= 2 && !STOPWORDS.contains(&t.to_lowercase().as_str()))
        .map(|t| stem(&t.to_lowercase()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gantry_core::SkillSource;

    fn skill(id: &str, description: &str, triggers: &[&str]) -> SkillDto {
        SkillDto {
            id: id.to_owned(),
            source: SkillSource::Bundled,
            path: None,
            name: id.to_owned(),
            description: description.to_owned(),
            triggers: triggers.iter().map(|t| (*t).to_owned()).collect(),
            always_include: false,
            enabled: true,
            content_hash: String::new(),
            size: 0,
            version: 1,
            author: None,
            license: None,
            references: Vec::new(),
            installed_at: 0,
            updated_at: 0,
            last_used_at: None,
            use_count: 0,
            pinned_count: 0,
        }
    }

    #[test]
    fn a_plural_and_its_singular_are_the_same_word() {
        assert_eq!(stem("commits"), "commit");
        assert_eq!(stem("committing"), "commit");
        assert_eq!(stem("libraries"), "library");
        assert_eq!(stem("reviewed"), "review");
        // Short words are left alone rather than stemmed into nonsense.
        assert_eq!(stem("is"), "is");
        assert_eq!(stem("css"), "css");
    }

    #[test]
    fn one_trigger_hit_is_enough_and_a_phrase_needs_both_words() {
        let s = skill(
            "rust-idioms",
            "Idiomatic Rust for this codebase.",
            &["borrow checker", "tokio"],
        );
        assert!(score(&terms("the borrow checker is angry"), &s).score >= QUALIFYING_SCORE);
        assert!(score(&terms("we use tokio here"), &s).score >= QUALIFYING_SCORE);
        // "checker" alone is not the phrase.
        assert!(score(&terms("the checker refused"), &s).score < QUALIFYING_SCORE);
    }

    #[test]
    fn a_message_about_nothing_in_particular_matches_nothing() {
        let skills = [
            skill(
                "commit-messages",
                "Write a good commit message.",
                &["commit"],
            ),
            skill("code-review", "Review a change carefully.", &["review"]),
        ];
        assert!(matches("hello, how are you today?", &skills, &HashSet::new()).is_empty());
    }

    /// The bundled skills, as they actually ship, against a message that is about none of them.
    ///
    /// From a live run on 2026-09-17: "Could you please create a image of very good looking
    /// nature?" pulled in `code-review` and sent its whole playbook with the turn. A skill is
    /// several thousand tokens of instruction the user pays for and the model then tries to
    /// follow, so a false match is worse than a missed one.
    #[test]
    fn an_image_request_matches_none_of_the_bundled_skills() {
        let bundled = [
            skill(
                "artifact-authoring",
                "Build a Gantry artifact that renders the first time: choosing between markdown, code, html, svg, mermaid and react, the React component contract and its six importable modules, what the sandbox does not have, and how to answer a render error. Use when creating or editing an artifact, building a component, page, diagram, chart or interactive demo, or when an artifact failed to render.",
                &[
                    "artifact",
                    "component",
                    "react",
                    "chart",
                    "diagram",
                    "mermaid",
                    "svg",
                    "html page",
                    "interactive",
                    "render error",
                ],
            ),
            skill(
                "code-review",
                "Review a change the way a careful colleague would: correctness first, then the failure the author cannot see, then the cost of maintaining it, with every comment naming a concrete scenario rather than a preference. Use when reviewing code, a diff, a pull request or a patch, when asked what is wrong with a piece of code, or before merging.",
                &[
                    "code review",
                    "review",
                    "pull request",
                    "diff",
                    "patch",
                    "merge",
                    "critique",
                    "look over",
                ],
            ),
            skill(
                "commit-messages",
                "Write a commit message that says what changed and why, in the imperative present tense, with a subject under 72 characters and a body that explains the reasoning rather than restating the diff. Use when committing, writing or rewriting a commit message, preparing a pull request description, or when the user mentions git, a commit, staging or a changelog entry.",
                &[
                    "commit",
                    "commit message",
                    "git",
                    "pull request",
                    "changelog",
                    "squash",
                    "amend",
                ],
            ),
            skill(
                "writing-a-plan",
                "Turn a vague piece of work into a plan somebody can act on: the decision and why, the scope as a short list of concrete deliverables, the order of the work, what will be verified, and what was deliberately left out. Use when asked for a plan, a design, an approach, an implementation strategy or a proposal, or before starting a change large enough to need one.",
                &[
                    "plan",
                    "design doc",
                    "proposal",
                    "approach",
                    "strategy",
                    "roadmap",
                    "break this down",
                    "how should we",
                ],
            ),
        ];
        let got = matches(
            "Could you please create a image of very good looking nature?",
            &bundled,
            &HashSet::new(),
        );
        assert!(
            got.is_empty(),
            "nothing here is about any of these: {got:?}"
        );
    }

    /// The other half of the same fix: a phrase trigger whose words include a stopword works,
    /// rather than quietly becoming a one-word trigger or nothing at all. `how should we` and
    /// `break this down` had never matched anything before this.
    #[test]
    fn a_phrase_trigger_matches_the_phrase_and_nothing_looser() {
        let review = skill("code-review", "Review a change.", &["look over"]);
        assert!(score(&terms("can you look over this diff"), &review).score >= QUALIFYING_SCORE);
        assert!(
            score(&terms("a very good looking nature scene"), &review).score < QUALIFYING_SCORE
        );

        let plan = skill(
            "writing-a-plan",
            "Write a plan.",
            &["how should we", "break this down"],
        );
        assert!(score(&terms("how should we do the migration"), &plan).score >= QUALIFYING_SCORE);
        assert!(score(&terms("break this down for me"), &plan).score >= QUALIFYING_SCORE);
        // The words on their own are not the phrase.
        assert!(score(&terms("we broke down the door"), &plan).score < QUALIFYING_SCORE);
    }

    #[test]
    fn the_best_three_win_and_the_order_is_stable() {
        let skills = [
            skill("commit-messages", "Write a commit message.", &["commit"]),
            skill(
                "code-review",
                "Review a change.",
                &["review", "pull request"],
            ),
            skill("writing-a-plan", "Write a plan.", &["plan"]),
            skill("rust-idioms", "Rust.", &["rust"]),
        ];
        let terms = "reviewing this pull request before I commit the plan in rust";
        let got = matches(terms, &skills, &HashSet::new());
        assert_eq!(got.len(), TOP_N);
        assert_eq!(got[0].id, "code-review", "two triggers beat one: {got:?}");
        assert_eq!(matches(terms, &skills, &HashSet::new()), got, "stable");
    }

    #[test]
    fn what_the_last_turns_already_sent_is_not_sent_again() {
        let skills = [skill("code-review", "Review a change.", &["review"])];
        let skip: HashSet<String> = ["code-review".to_owned()].into_iter().collect();
        assert!(matches("review this", &skills, &skip).is_empty());
    }

    #[test]
    fn an_always_on_skill_is_never_matched_because_it_is_already_there() {
        let mut s = skill("house-style", "House style.", &["style"]);
        s.always_include = true;
        assert!(matches("what is our style", &[s], &HashSet::new()).is_empty());
    }
}
