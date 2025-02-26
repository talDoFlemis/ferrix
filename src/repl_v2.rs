use clean_path::Clean;
use tabled::Table;
use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use clap_repl::reedline::{Prompt, PromptHistorySearchStatus};
use clap_repl::ClapEditor;

use crate::complete_command::{
    CatCommand, CompleteCommand, HeadCommand, ListCommand, MakeDirCommand, MoveCommand,
    RemoveCommand, SortCommand, TouchCommand,
};
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
    pub fn run<S>(system: &mut S, segment: FerrixPromptSegment) -> anyhow::Result<()>
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
            CompleteCommand::List(cmd) => {
                let mut dir = shared_path
                    .read()
                    .expect("Failed to read current working directory")
                    .clone()
                    .into_os_string()
                    .to_os_string();

                if cmd.dir.is_some() {
                    let path = PathBuf::from(cmd.dir.as_ref().unwrap());
                    let cwd = PathBuf::from(dir);
                    dir = cwd.join(path).clean().into_os_string().to_os_string();
                };

                let cmd = ListCommand {
                    dir: Some(dir),
                    all: cmd.all,
                };
                match system.list(&cmd) {
                    Ok(output) => {
                        let len = output.nodes.len();
                        let total_size = output.total_disk_space_in_bytes;
                        let remaining_size = output.remaining_disk_space_in_bytes;
                        let table = Table::new(output.nodes).to_string();
                        println!("{table}");
                        println!("Total: {len} nodes");
                        println!("Total disk size: {total_size} bytes");
                        println!("Remaining disk size: {remaining_size} bytes");
                    }
                    Err(e) => eprintln!("Error listing: {:?}", e),
                }
            }
            CompleteCommand::Touch(cmd) => {
                let mut cwd = shared_path
                    .read()
                    .expect("Failed to read current working directory")
                    .clone();

                cwd.push(PathBuf::from(cmd.file));
                let cwd = cwd.clean();

                let cmd = TouchCommand {
                    file: cwd.into_os_string().to_os_string(),
                    number_of_integers: cmd.number_of_integers,
                };

                if let Err(e) = system.touch(&cmd) {
                    eprintln!("Error touching: {:?}", e);
                }
            }
            CompleteCommand::MakeDir(cmd) => {
                let mut cwd = shared_path
                    .read()
                    .expect("Failed to read current working directory")
                    .clone();

                cwd.push(PathBuf::from(cmd.dir));
                let cwd = cwd.clean();

                let cmd = MakeDirCommand {
                    dir: cwd.into_os_string().to_os_string(),
                    parents: cmd.parents,
                };
                if let Err(e) = system.make_dir(&cmd) {
                    eprintln!("Error making directory: {:?}", e);
                }
            }
            CompleteCommand::Head(cmd) => {
                let mut cwd = shared_path
                    .read()
                    .expect("Failed to read current working directory")
                    .clone();

                cwd.push(PathBuf::from(cmd.file));
                let cwd = cwd.clean();

                let cmd = HeadCommand {
                    file: cwd.into_os_string().to_os_string(),
                    start: cmd.start,
                    end: cmd.end,
                };
                match system.head(&cmd) {
                    Ok(numbers) => {
                        for number in &numbers {
                            println!("{}", number);
                        }
                    }
                    Err(e) => eprintln!("Error heading: {:?}", e),
                }
            }
            CompleteCommand::Cat(cmd) => {
                let cwd = shared_path
                    .read()
                    .expect("Failed to read current working directory")
                    .clone();

                let mut files = Vec::new();

                for file in cmd.files {
                    let file = cwd.join(PathBuf::from(file));
                    files.push(file.into_os_string().to_os_string());
                }

                let cmd = CatCommand {
                    files: files,
                    output_file: cmd.output_file,
                };

                if let Err(e) = system.cat(&cmd) {
                    eprintln!("Error catting: {:?}", e);
                }
            }
            CompleteCommand::Remove(cmd) => {
                let mut cwd = shared_path
                    .read()
                    .expect("Failed to read current working directory")
                    .clone();

                cwd.push(PathBuf::from(cmd.file_or_dir));
                let cwd = cwd.clean();

                let cmd = RemoveCommand {
                    file_or_dir: cwd.into_os_string().to_os_string(),
                    recursive: cmd.recursive,
                };
                if let Err(e) = system.remove(&cmd) {
                    eprintln!("Error removing: {:?}", e);
                }
            }
            CompleteCommand::Move(cmd) => {
                let cwd = shared_path
                    .read()
                    .expect("Failed to read current working directory")
                    .clone();

                let to = cwd
                    .join(PathBuf::from(cmd.from))
                    .clean()
                    .into_os_string()
                    .to_os_string();
                let from = cwd
                    .join(PathBuf::from(cmd.to))
                    .clean()
                    .into_os_string()
                    .to_os_string();

                let cmd = MoveCommand { from, to };

                if let Err(e) = system.mv(&cmd) {
                    eprintln!("Error moving: {:?}", e);
                }
            }
            CompleteCommand::Sort(cmd) => {
                let cwd = shared_path
                    .read()
                    .expect("Failed to read current working directory")
                    .clone();

                let file = cwd
                    .join(PathBuf::from(cmd.file))
                    .clean()
                    .into_os_string()
                    .to_os_string();

                let cmd = SortCommand {
                    file,
                    inverse_order: cmd.inverse_order,
                };
                if let Err(e) = system.sort(&cmd) {
                    eprintln!("Error sorting: {:?}", e);
                }
            }
        });

        Ok(())
    }
}
