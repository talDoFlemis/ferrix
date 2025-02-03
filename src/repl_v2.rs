use std::borrow::Cow;

use miette::Result;

use clap_repl::reedline::{Prompt, PromptHistorySearchStatus};
use clap_repl::ClapEditor;

use crate::complete_command::CompleteCommand;
use crate::fs::Filesystem;
use crate::system::System;

static DEFAULT_PROMPT_INDICATOR: &str = ">> ";
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

struct FerrixPrompt {
    segment: FerrixPromptSegment,
}

impl FerrixPrompt {
    pub fn new(segment: FerrixPromptSegment) -> Self {
        Self { segment }
    }
}

impl FerrixPrompt {
    fn render_prompt_segment(&self) -> Cow<str> {
        match &self.segment {
            FerrixPromptSegment::Basic(s) => s.into(),
            FerrixPromptSegment::WorkingDirectory => {
                Cow::Owned("/path/to/currentdir/@ferrix".to_string())
            }
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

pub struct ReplV2<F>
where
    F: Filesystem,
{
    system: System<F>,
}

impl<F> ReplV2<F>
where
    F: Filesystem,
{
    pub fn new(system: System<F>) -> Self {
        Self { system }
    }

    pub fn run(&mut self) -> Result<()> {
        let prompt = FerrixPrompt::new(FerrixPromptSegment::Basic("ferrix".into()));

        let rl = ClapEditor::<CompleteCommand>::builder()
            .with_prompt(Box::new(prompt))
            .build();

        rl.repl(|cmd| {
            self.system
                .process_command(cmd)
                .expect("expected to process command")
        });

        Ok(())
    }
}
