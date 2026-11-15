//! The sidebar, in order: the same six pages as the other desktop clients.

/// One entry of the sidebar and the page behind it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Section {
    /// Stable id: what `--page` takes, the same on every platform.
    pub id: &'static str,
    /// Wording id of the name shown in the sidebar and as the page heading.
    pub title: &'static str,
    /// Wording id of the line under the heading.
    pub summary: &'static str,
    /// Name of a symbolic icon in the system's icon theme.
    pub icon: &'static str,
    /// Whether the entry sits at the bottom of the sidebar.
    pub footer: bool,
}

pub const OVERVIEW: &str = "overview";
pub const PROXIES: &str = "proxies";
pub const RULES: &str = "rules";
pub const EXTENSIONS: &str = "extensions";
pub const ACTIVITY: &str = "activity";
pub const SETTINGS: &str = "settings";

/// Every section, in the order of the sidebar.
pub const ALL: [Section; 6] = [
    Section {
        id: OVERVIEW,
        title: "nav.overview",
        summary: "page.overview.summary",
        icon: "go-home-symbolic",
        footer: false,
    },
    Section {
        id: PROXIES,
        title: "nav.proxies",
        summary: "page.proxies.summary",
        icon: "network-server-symbolic",
        footer: false,
    },
    Section {
        id: RULES,
        title: "nav.rules",
        summary: "page.rules.summary",
        icon: "view-list-symbolic",
        footer: false,
    },
    Section {
        id: EXTENSIONS,
        title: "nav.extensions",
        summary: "page.extensions.summary",
        icon: "application-x-addon-symbolic",
        footer: false,
    },
    Section {
        id: ACTIVITY,
        title: "nav.activity",
        summary: "page.activity.summary",
        icon: "network-transmit-receive-symbolic",
        footer: false,
    },
    Section {
        id: SETTINGS,
        title: "nav.settings",
        summary: "page.settings.summary",
        icon: "emblem-system-symbolic",
        footer: true,
    },
];

/// The place of the section with this id in [`ALL`].
pub fn position(id: &str) -> Option<usize> {
    ALL.iter().position(|section| section.id == id)
}

/// The section with this id.
pub fn find(id: &str) -> Option<&'static Section> {
    position(id).map(|index| &ALL[index])
}

#[cfg(test)]
mod tests {
    use super::*;
    use client_strings::{find as wording, Language};

    #[test]
    fn every_section_has_wording_in_every_language() {
        for section in ALL {
            for language in Language::ALL {
                for id in [section.title, section.summary] {
                    let sentence = wording(language, id);
                    assert!(
                        sentence.is_some_and(|sentence| !sentence.is_empty()),
                        "{id} in {}",
                        language.id()
                    );
                }
            }
        }
    }

    #[test]
    fn ids_are_unique() {
        for (index, section) in ALL.iter().enumerate() {
            assert_eq!(position(section.id), Some(index));
            assert!(!section.icon.is_empty());
        }
    }

    #[test]
    fn settings_sits_at_the_bottom() {
        let footers: Vec<_> = ALL.iter().filter(|section| section.footer).collect();
        assert_eq!(footers.len(), 1);
        assert_eq!(footers[0].id, SETTINGS);
        assert_eq!(ALL[ALL.len() - 1].id, SETTINGS);
    }

    #[test]
    fn find_knows_every_id_and_nothing_else() {
        for section in &ALL {
            assert_eq!(find(section.id), Some(section));
        }
        assert_eq!(find("nowhere"), None);
        assert_eq!(find(""), None);
    }

    #[test]
    fn the_pages_are_the_ones_the_windows_window_has() {
        // apps/windows-native/src/AnixOps.Net.Presentation/Section.cs lists the same pages in the same order.
        let source = include_str!("../../windows-native/src/AnixOps.Net.Presentation/Section.cs");
        let mut theirs = Vec::new();
        for line in source.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("new(") {
                let fields: Vec<&str> = rest.split('"').collect();
                // new(Name, "title", "summary", "glyph"…
                theirs.push((
                    fields[1].to_owned(),
                    fields[3].to_owned(),
                    line.contains("Footer: true"),
                ));
            }
        }
        let ours: Vec<_> = ALL
            .iter()
            .map(|section| {
                (
                    section.title.to_owned(),
                    section.summary.to_owned(),
                    section.footer,
                )
            })
            .collect();
        assert_eq!(ours, theirs);
    }
}
