//! A short memory of pages already read, so turning a page costs nothing.
//!
//! `fetch_url` returns a window of a document, and the model asks for the next window by
//! offset. Without a memory here every page turn is a second download, a second parse and a
//! second chance for the site to have changed underneath the offsets — which would make the
//! paging arithmetic quietly wrong rather than merely slow. So a fetched document is kept, and
//! the windows after the first are cut from the copy the first one came from.
//!
//! The same memory answers the other half of it: a chat where the model reads one page twice
//! ("Used Web 2 times") pays for it once.
//!
//! What is kept is public content fetched anonymously — no cookies, no credentials, the same
//! user agent every time — so two chats asking for the same URL would have got the same bytes,
//! and one cache across the connector leaks nothing between them. The bounds are small on
//! purpose: this is a memory of what the current conversation is reading, not a web cache.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::extract::Format;

/// How long a page is still worth serving from memory. Long enough that paging through a long
/// document never re-downloads it, short enough that "read it again" on a page that changes
/// gets the change. A model reading the same URL after five minutes has usually moved on and
/// come back, which is exactly when a fresh copy is the right answer.
pub const TTL: Duration = Duration::from_secs(300);

/// Documents kept at once.
pub const MAX_PAGES: usize = 8;

/// Characters kept across all of them. A cap in characters rather than pages, because one
/// 5 MB page and eight small ones are the same problem.
pub const MAX_CHARS: usize = 4_000_000;

/// A document as it was read, before any window was cut from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    /// After redirects — what the content is actually the content *of*.
    pub final_url: String,
    pub status: u16,
    pub content_type: String,
    pub title: Option<String>,
    pub redirects: Vec<String>,
    /// The 5 MB response cap was hit, so the document itself is short of the real page.
    pub response_truncated: bool,
    pub body: String,
    /// Characters in `body`, counted once when the page was read.
    pub chars: usize,
}

impl Page {
    #[must_use]
    pub fn new(
        final_url: String,
        status: u16,
        content_type: String,
        title: Option<String>,
        redirects: Vec<String>,
        response_truncated: bool,
        body: String,
    ) -> Self {
        Self {
            chars: body.chars().count(),
            final_url,
            status,
            content_type,
            title,
            redirects,
            response_truncated,
            body,
        }
    }
}

/// The URL as the model asked for it, and the shape it asked for it in. Keyed on the request
/// rather than on the final URL: a second call spelling the URL the same way is the one that
/// should be free, and re-extracting the same document as Markdown and as HTML really are two
/// different documents.
type Key = (String, Format);

struct Entry {
    key: Key,
    at: Instant,
    page: Arc<Page>,
}

/// The pages this connector currently remembers.
///
/// The lock is held only around the `Vec` — never across a fetch or an extraction — so a poisoned
/// lock is not a thing that can happen here. It is handled anyway, by forgetting everything:
/// this is a cache, and the correct response to not knowing its state is to have no state.
#[derive(Default)]
pub struct Cache {
    entries: Mutex<Vec<Entry>>,
}

impl Cache {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The page, and how long ago it was read. `None` when it was never read or has expired.
    #[must_use]
    pub fn get(&self, url: &str, format: Format) -> Option<(Arc<Page>, Duration)> {
        self.get_at(url, format, Instant::now())
    }

    /// Remember a page, evicting whatever has to go to keep the bounds.
    pub fn put(&self, url: &str, format: Format, page: Arc<Page>) {
        self.put_at(url, format, page, Instant::now());
    }

    fn get_at(&self, url: &str, format: Format, now: Instant) -> Option<(Arc<Page>, Duration)> {
        let entries = self.entries.lock().ok()?;
        let entry = entries
            .iter()
            .find(|entry| entry.key.0 == url && entry.key.1 == format)?;
        let age = now.saturating_duration_since(entry.at);
        (age < TTL).then(|| (Arc::clone(&entry.page), age))
    }

    fn put_at(&self, url: &str, format: Format, page: Arc<Page>, now: Instant) {
        // One page larger than the whole cache would evict everything else and then itself.
        // Not remembering it is the honest outcome: it is still returned, just not kept.
        if page.chars > MAX_CHARS {
            return;
        }
        let Ok(mut entries) = self.entries.lock() else {
            return;
        };
        let key = (url.to_owned(), format);
        entries.retain(|entry| entry.key != key && now.saturating_duration_since(entry.at) < TTL);
        entries.push(Entry { key, at: now, page });

        // Oldest first, which for a document being paged through is the one nobody is reading.
        while entries.len() > MAX_PAGES
            || entries.iter().map(|entry| entry.page.chars).sum::<usize>() > MAX_CHARS
        {
            let oldest = entries
                .iter()
                .enumerate()
                .min_by_key(|(_, entry)| entry.at)
                .map(|(at, _)| at);
            match oldest {
                Some(at) => drop(entries.remove(at)),
                None => break,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(body: &str) -> Arc<Page> {
        Arc::new(Page::new(
            "https://example.com/a".to_owned(),
            200,
            "text/html".to_owned(),
            Some("A".to_owned()),
            Vec::new(),
            false,
            body.to_owned(),
        ))
    }

    #[test]
    fn a_page_read_once_is_there_for_the_next_window() {
        let cache = Cache::new();
        assert!(
            cache
                .get("https://example.com/a", Format::Markdown)
                .is_none()
        );
        cache.put("https://example.com/a", Format::Markdown, page("hello"));

        let (found, age) = cache
            .get("https://example.com/a", Format::Markdown)
            .expect("the page just read");
        assert_eq!(found.body, "hello");
        assert!(age < Duration::from_secs(1));

        // A different URL, and the same URL in a different shape, are different documents.
        assert!(
            cache
                .get("https://example.com/b", Format::Markdown)
                .is_none()
        );
        assert!(cache.get("https://example.com/a", Format::Html).is_none());
    }

    #[test]
    fn a_page_older_than_the_ttl_is_read_again() {
        let cache = Cache::new();
        let long_ago = Instant::now();
        cache.put_at(
            "https://example.com/a",
            Format::Markdown,
            page("old"),
            long_ago,
        );

        // Just inside the window, and just outside it.
        let nearly = long_ago + TTL - Duration::from_secs(1);
        assert!(
            cache
                .get_at("https://example.com/a", Format::Markdown, nearly)
                .is_some()
        );
        let after = long_ago + TTL + Duration::from_secs(1);
        assert!(
            cache
                .get_at("https://example.com/a", Format::Markdown, after)
                .is_none()
        );
    }

    #[test]
    fn reading_a_page_again_replaces_the_copy_rather_than_keeping_two() {
        let cache = Cache::new();
        cache.put("https://example.com/a", Format::Markdown, page("first"));
        cache.put("https://example.com/a", Format::Markdown, page("second"));
        assert_eq!(cache.entries.lock().unwrap().len(), 1);
        let (found, _) = cache
            .get("https://example.com/a", Format::Markdown)
            .unwrap();
        assert_eq!(found.body, "second");
    }

    #[test]
    fn the_oldest_page_goes_when_there_is_no_room() {
        let cache = Cache::new();
        let start = Instant::now();
        for n in 0..MAX_PAGES + 3 {
            cache.put_at(
                &format!("https://example.com/{n}"),
                Format::Markdown,
                page("x"),
                start + Duration::from_secs(n as u64),
            );
        }
        let now = start + Duration::from_secs(MAX_PAGES as u64 + 3);
        assert_eq!(cache.entries.lock().unwrap().len(), MAX_PAGES);
        // The first three are gone; the last one read is still there.
        assert!(
            cache
                .get_at("https://example.com/0", Format::Markdown, now)
                .is_none()
        );
        assert!(
            cache
                .get_at(
                    &format!("https://example.com/{}", MAX_PAGES + 2),
                    Format::Markdown,
                    now
                )
                .is_some()
        );
    }

    #[test]
    fn one_huge_page_does_not_evict_everything_to_make_room_for_itself() {
        let cache = Cache::new();
        cache.put("https://example.com/small", Format::Markdown, page("x"));
        let huge = "y".repeat(MAX_CHARS + 1);
        cache.put("https://example.com/huge", Format::Markdown, page(&huge));

        assert!(
            cache
                .get("https://example.com/huge", Format::Markdown)
                .is_none(),
            "too large to keep"
        );
        assert!(
            cache
                .get("https://example.com/small", Format::Markdown)
                .is_some(),
            "and it should not have taken the others down with it"
        );
    }

    #[test]
    fn the_character_bound_holds_across_several_pages() {
        let cache = Cache::new();
        let big = "z".repeat(MAX_CHARS / 2 + 1);
        for n in 0..4 {
            cache.put(
                &format!("https://example.com/{n}"),
                Format::Markdown,
                page(&big),
            );
        }
        let kept: usize = cache
            .entries
            .lock()
            .unwrap()
            .iter()
            .map(|entry| entry.page.chars)
            .sum();
        assert!(kept <= MAX_CHARS, "{kept} characters kept");
    }
}
