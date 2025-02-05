use std::borrow::{Borrow, Cow};
use std::cell::RefCell;
use std::sync::{Arc, RwLock};

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

pub struct FerrixPrompt<S: System + Send + Sync> {
    segment: FerrixPromptSegment,
    system: S,
}

impl<S: System + Send + Sync> FerrixPrompt<S> {
    pub fn new(system: S, segment: FerrixPromptSegment) -> Self {
        Self { segment, system }
    }
}

impl<S: System + Send + Sync> FerrixPrompt<S> {
    fn render_prompt_segment(&self) -> Cow<str> {
        match &self.segment {
            FerrixPromptSegment::Basic(s) => s.into(),
            FerrixPromptSegment::WorkingDirectory => Cow::Owned(format!(
                "{}{}",
                self.system.get_cwd().unwrap_or_default().to_string_lossy(),
                "@ferrix",
            )),
            FerrixPromptSegment::Empty => Cow::Borrowed(""),
        }
    }
}

impl<S: System + Send + Sync> Prompt for FerrixPrompt<S> {
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

impl ReplV2 {
    pub fn run<S>(system: S, prompt: FerrixPrompt<S>) -> Result<()>
    where
        S: System + Send + Sync + 'static,
    {
        let rl = ClapEditor::<CompleteCommand>::builder()
            .with_prompt(Box::new(prompt))
            .build();

        rl.repl(|cmd| match cmd {
            CompleteCommand::Exit(cmd) => {
                if let Err(e) = system.exit(&cmd) {
                    eprintln!("Error exiting: {:?}", e);
                }
            }
            _ => eprintln!("Command not implemented: {:?}", cmd),
        });

        Ok(())
    }
}
