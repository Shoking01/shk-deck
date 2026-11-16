//! The four pages the rail offers, and the words each one is introduced with.
//!
//! The mapping lives here rather than inside `render` so it can be tested with
//! no window: a page whose title is empty, or two pages that share a title,
//! would leave the rail and the page header disagreeing.

/// A page of the controller window.
///
/// The order of [`Page::ALL`] is the order of the rail, so the first page is
/// also the one the window opens on.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) enum Page {
    /// The key grid, the paint controls and the selected key.
    #[default]
    Deck,
    /// What each key runs when it is pressed.
    Actions,
    /// The pictures a key can show.
    Artwork,
    /// How the controller behaves away from the window.
    Settings,
}

impl Page {
    /// Every page, in the order the rail lists them.
    pub(crate) const ALL: [Page; 4] = [Page::Deck, Page::Actions, Page::Artwork, Page::Settings];

    /// The rail label: one word, so four rows stay a column and not a table.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Page::Deck => "Deck",
            Page::Actions => "Actions",
            Page::Artwork => "Artwork",
            Page::Settings => "Settings",
        }
    }

    /// The heading of the page, at the top of the content area.
    pub(crate) fn title(self) -> &'static str {
        match self {
            Page::Deck => "Deck",
            Page::Actions => "Actions",
            Page::Artwork => "Artwork",
            Page::Settings => "Settings",
        }
    }

    /// One sentence under the heading, saying what the page is for.
    pub(crate) fn description(self) -> &'static str {
        match self {
            Page::Deck => {
                "What every key shows right now, and the editor for the key you pick. Touching \
                 a key never changes it."
            }
            Page::Actions => {
                "Every key as a chip, the editor for the selected one, and what is bound right \
                 now. A key with no action still lights up and still says so."
            }
            Page::Artwork => {
                "The pictures a key can show, and what one looks like at the size the deck asks \
                 for."
            }
            Page::Settings => {
                "How the controller behaves when the window is closed and the deck is left to it."
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Page;
    use std::collections::BTreeSet;

    #[test]
    fn every_page_has_a_title_and_a_sentence() {
        for page in Page::ALL {
            assert!(!page.title().trim().is_empty(), "{page:?} has no title");
            assert!(
                !page.description().trim().is_empty(),
                "{page:?} has no description"
            );
            assert!(!page.label().trim().is_empty(), "{page:?} has no nav label");
        }
    }

    #[test]
    fn the_four_pages_are_all_different() {
        let titles: BTreeSet<&str> = Page::ALL.iter().map(|page| page.title()).collect();
        let descriptions: BTreeSet<&str> =
            Page::ALL.iter().map(|page| page.description()).collect();
        let labels: BTreeSet<&str> = Page::ALL.iter().map(|page| page.label()).collect();

        assert_eq!(titles.len(), Page::ALL.len(), "two pages share a title");
        assert_eq!(
            descriptions.len(),
            Page::ALL.len(),
            "two pages share a description"
        );
        assert_eq!(labels.len(), Page::ALL.len(), "two pages share a label");
    }

    #[test]
    fn the_rail_starts_on_the_deck_and_lists_the_pages_in_order() {
        assert_eq!(Page::default(), Page::Deck);
        assert_eq!(
            Page::ALL,
            [Page::Deck, Page::Actions, Page::Artwork, Page::Settings]
        );
        assert_eq!(Page::Deck.label(), "Deck");
        assert_eq!(Page::Actions.label(), "Actions");
        assert_eq!(Page::Artwork.label(), "Artwork");
        assert_eq!(Page::Settings.label(), "Settings");
    }
}
