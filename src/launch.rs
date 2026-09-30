// SPDX-License-Identifier: GPL-3.0-only

//! What a launch asks for, and how a second launch hands it to the first.
//!
//! Pocket is one window. The first process owns the application's name on the
//! session bus; `pocket flight.pkpass` run while that window is up — which is
//! what "Open with Pocket" in a file manager does — does not start a second
//! one. libcosmic's `run_single_instance` sends the running window this
//! launch's [`Flags`] and exits, and the pass is shown there.
//!
//! What crosses the bus is a list of strings, so the paths travel as absolute
//! `file://` URLs: the second process's working directory is not the first's,
//! and a file name is bytes, not necessarily UTF-8.

use std::ffi::OsString;
use std::path::PathBuf;

/// The action a second launch sends when it has files to show. A launch with
/// none sends no action, and the running window is only raised.
pub const OPEN: &str = "open";

/// What the application starts with.
#[derive(Clone, Debug, Default)]
pub struct Flags {
    /// The `.pkpass` files to show, as absolute `file://` URLs.
    open: Vec<String>,
    /// [`OPEN`] when there is something to show. Decided here rather than by
    /// the running window because libcosmic picks the bus method from it.
    action: Option<String>,
}

impl Flags {
    /// Flags for the arguments after the program's name: every one is a file,
    /// resolved against this process's working directory.
    #[must_use]
    pub fn new(arguments: impl IntoIterator<Item = OsString>) -> Self {
        let open: Vec<String> = arguments
            .into_iter()
            .map(PathBuf::from)
            .filter_map(|path| {
                let path = std::path::absolute(&path).unwrap_or(path);
                // Only a path that is not absolute has no URL, and the only
                // one `absolute` leaves that way is the empty argument, which
                // names nothing.
                url::Url::from_file_path(path).ok().map(String::from)
            })
            .collect();
        Self {
            action: (!open.is_empty()).then(|| OPEN.to_owned()),
            open,
        }
    }

    /// The files to show.
    #[must_use]
    pub fn paths(&self) -> Vec<PathBuf> {
        paths(&self.open)
    }
}

/// The paths a list of `file://` URLs names. Anything else in the list is not
/// a file on this computer and is left out.
pub fn paths<S: AsRef<str>>(urls: &[S]) -> Vec<PathBuf> {
    urls.iter()
        .filter_map(|url| url::Url::parse(url.as_ref()).ok())
        .filter_map(|url| url.to_file_path().ok())
        .collect()
}

impl cosmic::app::CosmicFlags for Flags {
    type SubCommand = String;
    type Args = Vec<String>;

    fn action(&self) -> Option<&Self::SubCommand> {
        self.action.as_ref()
    }

    fn args(&self) -> Vec<&str> {
        self.open.iter().map(String::as_str).collect()
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::ffi::OsStringExt as _;

    use cosmic::app::CosmicFlags as _;

    use super::*;

    fn flags(arguments: &[&str]) -> Flags {
        Flags::new(arguments.iter().map(OsString::from))
    }

    /// Nothing to show: the second launch sends a plain activation, which
    /// raises the running window and nothing more.
    #[test]
    fn a_bare_launch_asks_the_running_window_for_nothing() {
        let flags = flags(&[]);
        assert!(flags.action().is_none());
        assert!(flags.args().is_empty());
    }

    /// The files arrive in the running window as the files they named here,
    /// whatever directory that window was started in.
    #[test]
    fn files_are_handed_over_absolute() {
        let flags = flags(&["flight.pkpass", "/srv/passes/gym card.pkpass"]);
        assert_eq!(flags.action().map(String::as_str), Some(OPEN));

        let here = std::env::current_dir().unwrap();
        // What the running window does with what it is sent.
        let received: Vec<String> = flags.args().into_iter().map(str::to_owned).collect();
        assert_eq!(
            paths(&received),
            [
                here.join("flight.pkpass"),
                PathBuf::from("/srv/passes/gym card.pkpass")
            ]
        );
        assert_eq!(flags.paths(), paths(&received));
    }

    /// A file name is bytes. One that is not UTF-8, or that holds a newline,
    /// still crosses the bus — which carries only strings — and comes out the
    /// same name.
    #[test]
    fn a_name_that_is_not_text_survives_the_handover() {
        let odd = OsString::from_vec(b"/tmp/caf\xe9\n100%.pkpass".to_vec());
        let flags = Flags::new([odd.clone()]);
        assert_eq!(flags.args().len(), 1);
        assert_eq!(flags.paths(), [PathBuf::from(odd)]);
    }

    /// What is not a local file is left out rather than guessed at.
    #[test]
    fn what_is_not_a_local_file_is_not_shown() {
        assert!(paths(&["https://example.test/a.pkpass", "flight.pkpass", ""]).is_empty());
        assert!(flags(&[""]).action().is_none());
    }
}
