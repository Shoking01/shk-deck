//! The three pages the rail offers, and the words each one is introduced with.
//!
//! The mapping lives here rather than inside `render` so it can be tested with
//! no window: a page whose title is empty, or two pages that share a title,
//! would leave the rail and the page header disagreeing.

use gpui_kit::assets::IconName;

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
    /// What every measurement is doing, and what it has been doing.
    Stats,
    /// How the controller behaves away from the window.
    Settings,
}

impl Page {
    /// Every page, in the order the rail lists them.
    pub(crate) const ALL: [Page; 4] = [Page::Deck, Page::Actions, Page::Stats, Page::Settings];

    /// The rail label: one word, so the rows stay a column and not a table.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Page::Deck => "Deck",
            Page::Actions => "Actions",
            Page::Stats => "Stats",
            Page::Settings => "Settings",
        }
    }

    /// The heading of the page, at the top of the content area.
    pub(crate) fn title(self) -> &'static str {
        match self {
            Page::Deck => "Deck",
            Page::Actions => "Actions",
            Page::Stats => "Stats",
            Page::Settings => "Settings",
        }
    }

    /// The icon the rail and the page heading wear.
    ///
    /// One icon per page, chosen for what the page is about rather than for what
    /// it contains: the deck is the grid of keys, actions are what the keys run,
    /// stats are the readings, settings are the settings.
    ///
    /// Only names in `gpui_kit`'s bundled set are drawn: the enum lists every
    /// Lucide icon, but the asset source carries the curated subset in
    /// `crates/assets/default-icons.txt`, and a name outside it paints nothing.
    pub(crate) fn icon(self) -> IconName {
        match self {
            Page::Deck => IconName::LayoutDashboard,
            Page::Actions => IconName::SquareTerminal,
            Page::Stats => IconName::ChartPie,
            Page::Settings => IconName::Settings,
        }
    }

    /// One sentence under the heading, saying what the page is for.
    pub(crate) fn description(self) -> &'static str {
        match self {
            Page::Deck => {
                "What every key shows right now, and the editor for the key you pick. Touching \
                 a key never changes it; dragging one onto another trades their places."
            }
            Page::Actions => {
                "Every key as a chip, the editor for the selected one, and what is bound right \
                 now. A key with no action still lights up and still says so."
            }
            Page::Stats => {
                "What every measurement is doing, and what it has been doing for the last few \
                 minutes. The deck's sampler does the measuring; nothing here asks the machine."
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
    fn no_two_pages_wear_the_same_icon() {
        // Two rows of the rail that look alike are two rows the eye cannot tell
        // apart, which is the one thing an icon must not do.
        let icons: BTreeSet<String> = Page::ALL
            .iter()
            .map(|page| format!("{:?}", page.icon()))
            .collect();

        assert_eq!(icons.len(), Page::ALL.len(), "two pages share an icon");
    }

    #[test]
    fn the_three_pages_are_all_different() {
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
            [Page::Deck, Page::Actions, Page::Stats, Page::Settings]
        );
        assert_eq!(Page::Deck.label(), "Deck");
        assert_eq!(Page::Actions.label(), "Actions");
        assert_eq!(Page::Settings.label(), "Settings");
    }

    #[test]
    fn there_is_no_page_of_its_own_for_the_picture() {
        // The picture a key shows belongs to the key, and the key is edited on
        // the `Deck` page and summarised on `Actions`. A fourth page for it was
        // retired, and a rail row reappearing there would be the regression.
        assert_eq!(Page::ALL.len(), 4, "the rail lists exactly four pages");
        for page in Page::ALL {
            assert_ne!(page.label(), "Artwork");
            assert_ne!(page.title(), "Artwork");
        }
    }
}
