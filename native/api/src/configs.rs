//! The config file on Horizon: `config/hempdustry.json`, the Java mod's own file, registered as a
//! `Common` config. Horizon reads it when the game (or a dedicated server) starts and again
//! whenever the file changes; `/hempdustry reload` re-reads it as the Java mod's command does.
//!
//! A file read whole is written back complete (every knob, the comments), as
//! `HempdustryConfig.load` does; one that can't be read is left as it is, and the settings
//! already running stay.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use horizon_client::mod_api::{NativeCommand, NativeConfig, NativeConfigFile, NativeConfigKind};
use horizon_client::text::{RED, Text};

use crate::config::{self, Live, Settings};

const MOD_ID: &str = "hempdustry";
/// The log target: Horizon's logger keeps warnings and notes only from targets that start with
/// `horizon` (anything else logs its errors alone), and a clamped value is a warning.
const LOG: &str = "horizon_mod::hempdustry";
/// `/hempdustry reload`, as the command hook hears it (no slash).
pub(crate) const RELOAD: &str = "hempdustry reload";
/// Vanilla's `/reload` and `/gamerule` bar: this changes the mod for everyone on the server.
const RELOAD_LEVEL: u8 = 2;

#[derive(Debug, Default)]
pub(crate) struct Configs {
    settings: Live,
    /// Where the file was last read, for the command.
    path: Mutex<Option<PathBuf>>,
}

impl Configs {
    pub(crate) fn new(settings: Live) -> Self {
        Self {
            settings,
            path: Mutex::default(),
        }
    }

    /// The file and the text Horizon writes when it is missing: the defaults, complete.
    pub(crate) fn register(configs: &mut Vec<NativeConfig>) {
        let defaults = Settings::default().to_json();
        configs.push(
            NativeConfig::new(MOD_ID, NativeConfigKind::Common, &defaults).file(config::FILE),
        );
    }

    pub(crate) fn register_commands(commands: &mut Vec<NativeCommand>) {
        commands.push(NativeCommand::op(RELOAD, RELOAD_LEVEL));
    }

    /// Horizon read the file (at start, or because it changed). Every mod hears every config.
    pub(crate) fn loaded(&self, file: &NativeConfigFile) {
        if file.mod_id != MOD_ID {
            return;
        }
        *self.path.lock().unwrap_or_else(PoisonError::into_inner) = Some(file.path.clone());
        self.apply(&file.path, &file.text);
    }

    /// `/hempdustry reload`: the file read again. The answer is the command's feedback.
    pub(crate) fn reload(&self) -> Text {
        let path = self
            .path
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let read = path.as_deref().map(|path| match fs::read_to_string(path) {
            // A deleted file is a fresh start, as in Java: the defaults, written out again.
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok((path, String::new())),
            other => other.map(|text| (path, text)),
        });
        let applied = match read {
            Some(Ok((path, text))) => self.apply(path, &text),
            Some(Err(e)) => {
                log::error!(target: LOG, "could not read the hempdustry config: {e}; the settings running stay");
                false
            }
            None => false,
        };
        if applied {
            Text::translate("commands.hempdustry.reload", &[])
        } else {
            // Said, or a typo looks exactly like a reload that took.
            Text::translate("commands.hempdustry.reload.failed", &[]).with_color(RED)
        }
    }

    /// `text` put in force and written back complete; false when it couldn't be read.
    fn apply(&self, path: &Path, text: &str) -> bool {
        let (settings, clamped) = match Settings::parse(text) {
            Ok(read) => read,
            Err(e) => {
                log::error!(
                    target: LOG,
                    "{}: {e}; nothing in it was applied, and it is left as it is",
                    path.display()
                );
                return false;
            }
        };
        for c in &clamped {
            log::warn!(target: LOG, "{c}");
        }
        self.settings.set(settings);
        let complete = settings.to_json();
        if complete != text
            && let Err(e) = fs::write(path, complete)
        {
            log::error!(target: LOG, "could not write {}: {e}", path.display());
        }
        true
    }
}
