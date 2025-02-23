use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use clean_path::Clean;

use miette::Result;

use clap_repl::reedline::{Prompt, PromptHistorySearchStatus};
use clap_repl::ClapEditor;

use crate::complete_command::CompleteCommand;
use crate::system::System;

static DEFAULT_PROMPT_INDICATOR: &str = "$ ";
static DEFAULT_MULTILINE_INDICATOR: &str = "::: ";

#[derive(Clone)]
pub enum FerrixPromptSegment {
    /// A basic user-defined prompt (i.e. just text)
    Basic(String),
    /// The path of the current working directory
    WorkingDirectory,
    /// An empty prompt segment
    Empty,
}

pub struct FerrixPrompt {
    segment: FerrixPromptSegment,
    current_working_dir: Arc<RwLock<PathBuf>>,
}

impl FerrixPrompt {
    pub fn new(current_working_dir: Arc<RwLock<PathBuf>>, segment: FerrixPromptSegment) -> Self {
        Self {
            segment,
            current_working_dir,
        }
    }
}

impl FerrixPrompt {
    fn render_prompt_segment(&self) -> Cow<str> {
        match &self.segment {
            FerrixPromptSegment::Basic(s) => s.into(),
            FerrixPromptSegment::WorkingDirectory => Cow::Owned(format!(
                "{}{}",
                self.current_working_dir
                    .read()
                    .expect("Failed to read current working directory")
                    .display(),
                "@ferrix",
            )),
            FerrixPromptSegment::Empty => Cow::Borrowed(""),
        }
    }
}

impl Prompt for FerrixPrompt {
    fn render_prompt_left(&self) -> std::borrow::Cow<str> {
        self.render_prompt_segment()
    }

    fn render_prompt_right(&self) -> std::borrow::Cow<str> {
        Cow::Borrowed("")
    }

    fn render_prompt_indicator(
        &self,
        _prompt_mode: clap_repl::reedline::PromptEditMode,
    ) -> std::borrow::Cow<str> {
        DEFAULT_PROMPT_INDICATOR.into()
    }

    fn render_prompt_multiline_indicator(&self) -> std::borrow::Cow<str> {
        Cow::Borrowed(DEFAULT_MULTILINE_INDICATOR)
    }

    fn render_prompt_history_search_indicator(
        &self,
        history_search: clap_repl::reedline::PromptHistorySearch,
    ) -> std::borrow::Cow<str> {
        let prefix = match history_search.status {
            PromptHistorySearchStatus::Passing => "",
            PromptHistorySearchStatus::Failing => "failing ",
        };
        Cow::Owned(format!(
            "({}reverse-search: {}) ",
            prefix, history_search.term
        ))
    }
}

pub struct ReplV2 {}

#[cfg(target_family = "unix")]
pub const DEFAULT_CURRENT_WORKING_DIR: &str = "/";

#[cfg(target_family = "windows")]
pub const DEFAULT_CURRENT_WORKING_DIR: &str = "C:\\";

impl ReplV2 {
    pub fn run<S>(system: S, segment: FerrixPromptSegment) -> anyhow::Result<()>
    where
        S: System + Send + Sync + 'static,
    {
        let shared_path = Arc::new(RwLock::new(PathBuf::from(DEFAULT_CURRENT_WORKING_DIR)));

        let prompt = FerrixPrompt::new(shared_path.clone(), segment);
        let rl = ClapEditor::<CompleteCommand>::builder()
            .with_prompt(Box::new(prompt))
            .build();

        rl.repl(|cmd| match cmd {
            CompleteCommand::Exit(cmd) => {
                if let Err(e) = system.exit(&cmd) {
                    eprintln!("Error exiting: {:?}", e);
                }
            }
            CompleteCommand::ChangeDir(cmd) => {
                let mut guard = shared_path
                    .write()
                    .expect("Failed to write current working directory");
                let new_path = PathBuf::from(
                    cmd.path
                        .unwrap_or(DEFAULT_CURRENT_WORKING_DIR.into())
                        .clone(),
                );
                guard.push(new_path);
                let cleared_path = guard.clean();
                guard.clear();
                guard.push(cleared_path);
            }
            _ => eprintln!("Command not implemented: {:?}", cmd),
        });

        Ok(())
    }
}
