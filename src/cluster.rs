//! Groups articles about the same event into stories and ranks them for the
//! Briefing. Plain statistics over titles and summaries: word prefixes,
//! TF-IDF and cosine similarity, plus shared names to join Czech and foreign
//! coverage of one event. The constants were tuned on real feeds; the tests
//! replay that data.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use crate::store::ArticleRow;

/// Similarity an article needs to join a story from other outlets.
const JOIN_SIM: f32 = 0.27;
/// An outlet rarely covers one event twice, so its own articles need more.
const SAME_SOURCE_SIM: f32 = 0.45;
/// Words an article must share with a story, besides the similarity.
const MIN_SHARED: usize = 3;
/// A Czech and a foreign story sharing one distinctive name join from here.
const CROSS_LANGUAGE_SIM: f32 = 0.10;
/// Articles further apart in time than this never join.
const WINDOW_SECS: i64 = 36 * 3600;
/// Score halves every this many hours since the story's newest article.
const HALF_LIFE_HOURS: f32 = 8.0;
/// Words are compared by their first letters, which absorbs most Czech
/// inflection (Ukrajina, Ukrajině, Ukrajinu).
const PREFIX: usize = 5;

pub struct Story {
    /// Indexes into the articles given to `cluster`, newest first.
    pub members: Vec<usize>,
    /// The article whose title stands for the story.
    pub head: usize,
    /// Distinct outlets covering it.
    pub sources: usize,
    pub category: String,
    pub score: f32,
}

/// Groups `articles` into stories, best first. `priority` maps a source
/// name to its weight (1.0 when missing).
pub fn cluster(articles: &[ArticleRow], now: i64, priority: &HashMap<String, f32>) -> Vec<Story> {
    let docs: Vec<Doc> = articles
        .iter()
        .map(|a| analyze(&a.title, &a.summary))
        .collect();
    let vectors = tf_idf(&docs);
    let czech_source = czech_sources(articles);

    let mut groups = group_by_similarity(articles, &vectors);
    join_across_languages(&mut groups, articles, &docs, &czech_source);

    let mut stories: Vec<Story> = groups
        .into_iter()
        .map(|g| to_story(g, articles, now, priority, &czech_source))
        .collect();
    stories.sort_by(|a, b| b.score.total_cmp(&a.score));
    stories
}

/// Oldest first, each article joins the closest story or starts one.
fn group_by_similarity(articles: &[ArticleRow], vectors: &[HashMap<u32, f32>]) -> Vec<Group> {
    let mut groups: Vec<Group> = Vec::new();
    let mut order: Vec<usize> = (0..articles.len()).collect();
    order.sort_by_key(|&i| articles[i].published);
    for i in order {
        let a = &articles[i];
        let mut best: Option<(usize, f32)> = None;
        for (g, group) in groups.iter().enumerate() {
            if a.published - group.newest > WINDOW_SECS {
                continue;
            }
            let shared = vectors[i]
                .keys()
                .filter(|k| group.centroid.contains_key(k))
                .count();
            if shared < MIN_SHARED {
                continue;
            }
            let own_only = group
                .members
                .iter()
                .all(|&m| articles[m].source == a.source);
            let need = if own_only { SAME_SOURCE_SIM } else { JOIN_SIM };
            let margin = group.similarity(&vectors[i]) - need;
            if margin >= 0.0 && best.is_none_or(|(_, b)| margin > b) {
                best = Some((g, margin));
            }
        }
        match best {
            Some((g, _)) => groups[g].add(i, &vectors[i], a.published),
            None => {
                let mut group = Group::default();
                group.add(i, &vectors[i], a.published);
                groups.push(group);
            }
        }
    }
    groups
}

/// Joins a Czech and a foreign story about the same event. Names carry
/// across languages where other words don't, but only names found in at
/// most two stories mark an event (not "Zelensky", who is in every Ukraine
/// story). Each story joins at most one other.
fn join_across_languages(
    groups: &mut Vec<Group>,
    articles: &[ArticleRow],
    docs: &[Doc],
    czech_source: &HashMap<&str, bool>,
) {
    let names: Vec<HashSet<&str>> = groups
        .iter()
        .map(|g| {
            g.members
                .iter()
                .flat_map(|&m| docs[m].names.iter().map(String::as_str))
                .collect()
        })
        .collect();
    let mut spread: HashMap<&str, usize> = HashMap::new();
    for name in names.iter().flatten() {
        *spread.entry(name).or_default() += 1;
    }
    let distinct: Vec<HashSet<&str>> = names
        .iter()
        .map(|ns| ns.iter().copied().filter(|n| spread[n] <= 2).collect())
        .collect();
    let czech: Vec<bool> = groups
        .iter()
        .map(|g| {
            let n = g
                .members
                .iter()
                .filter(|&&m| czech_source[articles[m].source.as_str()])
                .count();
            n * 2 > g.members.len()
        })
        .collect();

    let mut pairs: Vec<(f32, usize, usize)> = Vec::new();
    for i in 0..groups.len() {
        for j in i + 1..groups.len() {
            if czech[i] == czech[j] || (groups[i].newest - groups[j].newest).abs() > WINDOW_SECS {
                continue;
            }
            let shared = distinct[i].intersection(&distinct[j]).count();
            if shared == 0 {
                continue;
            }
            let sim = groups[i].similarity_to(&groups[j]);
            if shared >= 2 || sim >= CROSS_LANGUAGE_SIM {
                pairs.push((shared as f32 + sim, i, j));
            }
        }
    }
    pairs.sort_by(|a, b| b.0.total_cmp(&a.0));

    let mut used = HashSet::new();
    for (_, i, j) in pairs {
        if used.contains(&i) || used.contains(&j) {
            continue;
        }
        used.extend([i, j]);
        let moved = std::mem::take(&mut groups[j].members);
        groups[i].members.extend(moved);
        groups[i].newest = groups[i].newest.max(groups[j].newest);
    }
    groups.retain(|g| !g.members.is_empty());
}

/// Score: distinct outlets, times the best source priority, halving every
/// `HALF_LIFE_HOURS`, with a bonus when Czech and foreign outlets both
/// cover it. The headline comes from the highest priority source.
fn to_story(
    group: Group,
    articles: &[ArticleRow],
    now: i64,
    priority: &HashMap<String, f32>,
    czech_source: &HashMap<&str, bool>,
) -> Story {
    let weight = |m: usize| priority.get(&articles[m].source).copied().unwrap_or(1.0);
    let mut members = group.members;
    members.sort_by_key(|&m| std::cmp::Reverse(articles[m].published));
    let sources: HashSet<&str> = members
        .iter()
        .map(|&m| articles[m].source.as_str())
        .collect();
    let languages: HashSet<bool> = members
        .iter()
        .map(|&m| czech_source[articles[m].source.as_str()])
        .collect();
    let head = *members
        .iter()
        .max_by(|&&x, &&y| {
            weight(x)
                .total_cmp(&weight(y))
                .then(articles[x].published.cmp(&articles[y].published))
        })
        .expect("a story has members");
    let hours = (now - group.newest).max(0) as f32 / 3600.0;
    let score = sources.len() as f32
        * members.iter().map(|&m| weight(m)).fold(0.0, f32::max)
        * 0.5f32.powf(hours / HALF_LIFE_HOURS)
        * if languages.len() > 1 { 1.3 } else { 1.0 };
    Story {
        category: main_category(articles, &members, head),
        sources: sources.len(),
        head,
        members,
        score,
    }
}

/// The category most members are filed under; the head's on a tie.
fn main_category(articles: &[ArticleRow], members: &[usize], head: usize) -> String {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for &m in members {
        *counts.entry(articles[m].category.as_str()).or_default() += 1;
    }
    let head_category = articles[head].category.as_str();
    counts
        .into_iter()
        .max_by_key(|&(c, n)| (n, c == head_category))
        .map(|(c, _)| c.to_string())
        .unwrap_or_default()
}

#[derive(Default)]
struct Group {
    members: Vec<usize>,
    /// Sum of the members' vectors, and its length.
    centroid: HashMap<u32, f32>,
    norm: f32,
    newest: i64,
}

impl Group {
    fn add(&mut self, member: usize, vector: &HashMap<u32, f32>, published: i64) {
        self.members.push(member);
        for (&k, &x) in vector {
            *self.centroid.entry(k).or_default() += x;
        }
        self.norm = self.centroid.values().map(|x| x * x).sum::<f32>().sqrt();
        self.newest = self.newest.max(published);
    }

    /// Cosine similarity of a unit vector to the centroid.
    fn similarity(&self, vector: &HashMap<u32, f32>) -> f32 {
        if self.norm == 0.0 {
            return 0.0;
        }
        let dot: f32 = vector
            .iter()
            .filter_map(|(k, x)| self.centroid.get(k).map(|c| c * x))
            .sum();
        dot / self.norm
    }

    fn similarity_to(&self, other: &Group) -> f32 {
        if other.norm == 0.0 {
            return 0.0;
        }
        self.similarity(&other.centroid) / other.norm
    }
}

struct Doc {
    /// Weighted word prefixes.
    words: HashMap<String, f32>,
    /// One entry per run of capitalised words ("Volodymyr Zelensky"), keyed
    /// by the run's last word.
    names: HashSet<String>,
}

/// Title words count double; names (capitalised mid-sentence) 1.5 times.
fn analyze(title: &str, summary: &str) -> Doc {
    let summary_start: String = summary
        .split_whitespace()
        .take(40)
        .collect::<Vec<_>>()
        .join(" ");
    let mut words: HashMap<String, f32> = HashMap::new();
    let mut names = HashSet::new();
    for (text, weight) in [(title, 2.0), (summary_start.as_str(), 1.0)] {
        let tokens: Vec<&str> = text
            .split(|c: char| !(c.is_alphanumeric() || c == '\'' || c == '_'))
            .filter(|t| !t.is_empty())
            .collect();
        let mut run: Option<String> = None;
        for (i, word) in tokens.iter().enumerate() {
            let folded: String = word.chars().map(fold).collect::<String>().to_lowercase();
            let folded = folded.trim_matches('\'');
            let is_name = i > 0 && word.chars().next().is_some_and(char::is_uppercase);
            if !is_name && let Some(name) = run.take() {
                names.insert(name);
            }
            let length = folded.chars().count();
            let is_number = length > 0 && folded.chars().all(|c| c.is_ascii_digit());
            if (length < 3 && !is_number) || STOP_WORDS.contains(folded) {
                continue;
            }
            let key: String = folded.chars().take(PREFIX).collect();
            if is_name && length >= 4 {
                run = Some(key.clone());
            }
            *words.entry(key).or_default() += weight * if is_name { 1.5 } else { 1.0 };
        }
        names.extend(run);
    }
    Doc { words, names }
}

/// Unit-length TF-IDF vectors over the given documents. Words found in only
/// one document can't link anything and are left out.
fn tf_idf(docs: &[Doc]) -> Vec<HashMap<u32, f32>> {
    let mut ids: HashMap<&str, u32> = HashMap::new();
    let mut df: Vec<usize> = Vec::new();
    for doc in docs {
        for word in doc.words.keys() {
            let next = ids.len() as u32;
            let id = *ids.entry(word.as_str()).or_insert(next);
            if id as usize == df.len() {
                df.push(0);
            }
            df[id as usize] += 1;
        }
    }
    let n = docs.len() as f32;
    docs.iter()
        .map(|doc| {
            let mut vector: HashMap<u32, f32> = doc
                .words
                .iter()
                .map(|(word, &count)| (ids[word.as_str()], count))
                .filter(|&(id, _)| df[id as usize] > 1)
                .map(|(id, count)| (id, (1.0 + count.ln()) * (n / df[id as usize] as f32).ln()))
                .collect();
            let norm = vector.values().map(|x| x * x).sum::<f32>().sqrt();
            if norm > 0.0 {
                vector.values_mut().for_each(|x| *x /= norm);
            }
            vector
        })
        .collect()
}

/// Whether each source writes in Czech, by the majority of its articles
/// (a single headline can lack the telltale letters).
fn czech_sources(articles: &[ArticleRow]) -> HashMap<&str, bool> {
    let mut votes: HashMap<&str, (usize, usize)> = HashMap::new();
    for a in articles {
        let czech = format!("{} {}", a.title, a.summary)
            .chars()
            .any(|c| "ěščřžůĚŠČŘŽŮ".contains(c));
        let v = votes.entry(a.source.as_str()).or_default();
        v.0 += usize::from(czech);
        v.1 += 1;
    }
    votes
        .into_iter()
        .map(|(source, (czech, total))| (source, czech * 2 > total))
        .collect()
}

/// Drops diacritics from Latin letters (č → c, ü → u).
fn fold(c: char) -> char {
    match c {
        'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' | 'ą' => 'a',
        'Á' | 'À' | 'Â' | 'Ä' | 'Ã' | 'Å' | 'Ą' => 'A',
        'č' | 'ć' | 'ç' => 'c',
        'Č' | 'Ć' | 'Ç' => 'C',
        'ď' => 'd',
        'Ď' => 'D',
        'é' | 'è' | 'ê' | 'ë' | 'ě' | 'ę' => 'e',
        'É' | 'È' | 'Ê' | 'Ë' | 'Ě' | 'Ę' => 'E',
        'í' | 'ì' | 'î' | 'ï' => 'i',
        'Í' | 'Ì' | 'Î' | 'Ï' => 'I',
        'ľ' | 'ĺ' | 'ł' => 'l',
        'Ľ' | 'Ĺ' | 'Ł' => 'L',
        'ň' | 'ń' | 'ñ' => 'n',
        'Ň' | 'Ń' | 'Ñ' => 'N',
        'ó' | 'ò' | 'ô' | 'ö' | 'õ' | 'ő' => 'o',
        'Ó' | 'Ò' | 'Ô' | 'Ö' | 'Õ' | 'Ő' => 'O',
        'ř' | 'ŕ' => 'r',
        'Ř' | 'Ŕ' => 'R',
        'š' | 'ś' | 'ș' => 's',
        'Š' | 'Ś' | 'Ș' => 'S',
        'ť' | 'ț' => 't',
        'Ť' | 'Ț' => 'T',
        'ú' | 'ù' | 'û' | 'ü' | 'ů' | 'ű' => 'u',
        'Ú' | 'Ù' | 'Û' | 'Ü' | 'Ů' | 'Ű' => 'U',
        'ý' | 'ÿ' => 'y',
        'Ý' => 'Y',
        'ž' | 'ź' | 'ż' => 'z',
        'Ž' | 'Ź' | 'Ż' => 'Z',
        other => other,
    }
}

/// English and Czech (without diacritics) words that say nothing about the
/// event, plus newsroom filler ("live", "update", "řekl").
static STOP_WORDS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    "about aby after again against ale all also amid and ani ano any are asi back because been
     before being bez bude budou but byl byla byli bylo byt can could coz dal dalsi day did
     dnes does doing down during each few first for from further get gets got had has have
     having her here hers him his how into its jak jako jeho jej jeji jejich jen jeste jiz jsem
     jsi jsme jsou jste just kam kde kdo kdy kdyz know ktera ktere kteri kterou ktery last
     latest let letos lide lidi live made maji make makes may mezi might mit more most must nad
     nam nas nasi nebo need neni new news nez nic nich nim nor not now off ona once one oni
     only ono other our out over own pak people photos pictures pod podle pokud pouze pred pri
     pro proc proti protoze rekl rekla report reports rika rikaji rok roku said same say says
     she should some still such sve svych tak take takze tam tedy ten tento teto than that the
     their them then there these they this those three through tim time today tohle toho tom
     tomu too tuto two tyden tyto under until update updates uvedl uvedla vam vas vcera very
     via vice video vsak vsechno want was watch week were what when where which while who whom
     why will with would year years you your zda zde"
        .split_whitespace()
        .collect()
});

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// 357 real articles from the 48 hours to 31 Oct 2026, the data the
    /// constants were tuned on.
    pub(crate) fn snapshot() -> (Vec<ArticleRow>, i64) {
        let json: serde_json::Value =
            serde_json::from_str(include_str!("../testdata/news-48h.json")).unwrap();
        let text = |a: &serde_json::Value, k: &str| a[k].as_str().unwrap_or_default().to_string();
        let articles = json["articles"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| ArticleRow {
                source: text(a, "source"),
                category: text(a, "category"),
                title: text(a, "title"),
                summary: text(a, "summary"),
                link: text(a, "link"),
                published: a["published"].as_i64().unwrap(),
                ..Default::default()
            })
            .collect();
        (articles, json["now"].as_i64().unwrap())
    }

    fn priorities() -> HashMap<String, f32> {
        [
            ("ČT24 Hlavní zprávy", 1.5),
            ("BBC World", 1.5),
            ("iRozhlas Hlavní zprávy", 1.5),
            ("Hacker News", 0.5),
        ]
        .into_iter()
        .map(|(s, p)| (s.to_string(), p))
        .collect()
    }

    /// Clusters the snapshot and returns each story as its member titles.
    fn stories() -> Vec<Vec<String>> {
        let (articles, now) = snapshot();
        cluster(&articles, now, &priorities())
            .into_iter()
            .map(|s| {
                s.members
                    .iter()
                    .map(|&m| articles[m].title.clone())
                    .collect()
            })
            .collect()
    }

    /// Czech headlines use non-breaking spaces; compare with plain ones.
    fn starts(title: &str, start: &str) -> bool {
        title.replace('\u{a0}', " ").starts_with(start)
    }

    fn story_with<'a>(stories: &'a [Vec<String>], title_start: &str) -> &'a Vec<String> {
        stories
            .iter()
            .find(|s| s.iter().any(|t| starts(t, title_start)))
            .unwrap_or_else(|| panic!("no article titled {title_start:?}"))
    }

    /// Whether the articles starting with `a` and `b` share a story. Both
    /// must exist, so a typo can't make a "stay apart" check pass.
    fn together(stories: &[Vec<String>], a: &str, b: &str) -> bool {
        story_with(stories, b);
        story_with(stories, a).iter().any(|t| starts(t, b))
    }

    #[test]
    fn the_same_event_is_one_story_across_outlets_and_languages() {
        let s = stories();
        // Poland's constitutional court: English and Czech coverage.
        assert!(together(
            &s,
            "Riot police deployed to Poland",
            "Police deployed during stand-off"
        ));
        assert!(together(
            &s,
            "Riot police deployed to Poland",
            "Spor o polský ústavní soud"
        ));
        // The Binance heist, from BBC to ČT24 and Patria.
        assert!(together(
            &s,
            "What we know about Binance",
            "Hackers steal $6.3 billion"
        ));
        assert!(together(
            &s,
            "What we know about Binance",
            "„Největší krádež"
        ));
        assert!(together(
            &s,
            "What we know about Binance",
            "Binance zmrazila aktiva"
        ));
        // Worded completely differently, still one story.
        assert!(together(
            &s,
            "Ukraine’s Zelenskyy praises US over $2.1bn",
            "US approves sale of 500 Patriot"
        ));
        assert!(together(
            &s,
            "G20 'family photo' cancelled",
            "Evropané odmítli pózovat s Lavrovem"
        ));
    }

    #[test]
    fn related_but_different_events_stay_apart() {
        let s = stories();
        // Only "Volodymyr Zelensky" in common: too common to mark one event.
        assert!(!together(
            &s,
            "Trump admin approves potential Patriot",
            "Evakuovaní z okupovaného"
        ));
        // "New York Times" is one name, not two.
        assert!(!together(
            &s,
            "China denies allegations of spy",
            "„Bitvu“ dvou aljašských"
        ));
        // A Czech minister in Poland is not the Polish court dispute.
        assert!(!together(
            &s,
            "Havlíček v Polsku jednal",
            "Polská vláda přebírá ústavní soud"
        ));
        // One outlet, two unrelated pieces sharing a word.
        assert!(!together(
            &s,
            "V Česku posiluje nový přístup ke smrti",
            "Auto na leasing"
        ));
        assert!(!together(
            &s,
            "The oldest ice disappears",
            "Prospero: A new coding harness"
        ));
    }

    #[test]
    fn stories_covered_widely_rank_first() {
        let (articles, now) = snapshot();
        let stories = cluster(&articles, now, &priorities());
        // The headline may be a Czech one (ČT24 has priority), so look at
        // all titles of each top story.
        let top: Vec<String> = stories[..3]
            .iter()
            .map(|s| {
                s.members
                    .iter()
                    .map(|&m| articles[m].title.as_str())
                    .collect()
            })
            .collect();
        for expected in ["Binance", "Poland", "Alaska"] {
            assert!(
                top.iter().any(|t| t.contains(expected)),
                "{expected} not in the top three"
            );
        }
        assert!(stories.windows(2).all(|w| w[0].score >= w[1].score));
        // Every article lands in exactly one story.
        let mut seen: Vec<usize> = stories.iter().flat_map(|s| s.members.clone()).collect();
        seen.sort();
        assert_eq!(seen, (0..articles.len()).collect::<Vec<_>>());
    }

    #[test]
    fn names_are_runs_of_capitalised_words_keyed_by_the_last_one() {
        let doc = analyze(
            "Ukraine’s Volodymyr Zelensky meets Mark Carney in New York",
            "",
        );
        let mut names: Vec<&str> = doc.names.iter().map(String::as_str).collect();
        names.sort();
        assert_eq!(names, ["carne", "york", "zelen"]);
        assert!(doc.words.contains_key("ukrai"), "{:?}", doc.words.keys());
        assert!(!doc.words.contains_key("the"));
    }

    #[test]
    fn czech_inflections_share_a_prefix() {
        let a = analyze("Válka na Ukrajině", "");
        let b = analyze("Pomoc pro Ukrajinu", "");
        assert!(a.words.contains_key("ukraj") && b.words.contains_key("ukraj"));
    }
}
