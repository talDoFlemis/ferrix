use std::path::MAIN_SEPARATOR;
use std::sync::Arc;
use std::{num::ParseIntError, path::PathBuf};

use miette::{Result as MietteResult, Severity, SourceSpan};
use winnow::ascii::multispace0;
use winnow::combinator::{delimited, eof, not, opt, repeat_till, trace};
use winnow::stream::StreamIsPartial;
use winnow::{
    ascii::digit1,
    combinator::{alt, empty, fail, repeat},
    error::{AddContext, ErrorKind, FromExternalError, FromRecoverableError, ParserError},
    prelude::*,
    stream::{AsChar, Location, Recoverable, Stream},
    token::{any, literal, one_of, take_while},
    LocatingSlice,
};

use crate::error::{FerrixDiagnostic, FerrixError};

type Input<'a> = Recoverable<LocatingSlice<&'a str>, FerrixParserError>;
type ParserResult<T> = winnow::PResult<T, FerrixParserError>;

#[derive(Debug, Default, Clone, Eq, PartialEq)]
pub struct FerrixParserError {
    pub message: Option<String>,
    pub span: Option<SourceSpan>,
    pub label: Option<String>,
    pub help: Option<String>,
    pub severity: Option<Severity>,
}

#[derive(Debug, Clone, Default, Eq, PartialEq)]
struct FerrixParseContext {
    message: Option<String>,
    label: Option<String>,
    help: Option<String>,
    severity: Option<Severity>,
}

impl FerrixParseContext {
    fn msg(mut self, txt: impl AsRef<str>) -> Self {
        self.message = Some(txt.as_ref().to_string());
        self
    }

    fn lbl(mut self, txt: impl AsRef<str>) -> Self {
        self.label = Some(txt.as_ref().to_string());
        self
    }
}

fn cx() -> FerrixParseContext {
    Default::default()
}

impl<I: Stream> ParserError<I> for FerrixParserError {
    fn from_error_kind(_input: &I, _kind: ErrorKind) -> Self {
        Self {
            message: None,
            span: None,
            label: None,
            help: None,
            severity: None,
        }
    }

    fn append(
        self,
        _input: &I,
        _token_start: &<I as Stream>::Checkpoint,
        _kind: ErrorKind,
    ) -> Self {
        self
    }
}

impl<I: Stream> AddContext<I, FerrixParseContext> for FerrixParserError {
    fn add_context(
        mut self,
        _input: &I,
        _token_start: &<I as Stream>::Checkpoint,
        ctx: FerrixParseContext,
    ) -> Self {
        self.message = ctx.message.or(self.message);
        self.label = ctx.label.or(self.label);
        self.help = ctx.help.or(self.help);
        self.severity = ctx.severity.or(self.severity);
        self
    }
}

impl<I: Stream + Location> FromRecoverableError<I, Self> for FerrixParserError {
    #[inline]
    fn from_recoverable_error(
        token_start: &<I as Stream>::Checkpoint,
        _err_start: &<I as Stream>::Checkpoint,
        input: &I,
        mut e: Self,
    ) -> Self {
        e.span = e
            .span
            .or_else(|| Some(span_from_checkpoint(input, token_start)));
        e
    }
}

impl<'a> FromExternalError<Input<'a>, ParseIntError> for FerrixParserError {
    fn from_external_error(_: &Input<'a>, _kind: ErrorKind, e: ParseIntError) -> Self {
        FerrixParserError {
            span: None,
            message: Some(format!("{e}")),
            label: Some("invalid integer".into()),
            help: None,
            severity: Some(Severity::Error),
        }
    }
}

fn span_from_checkpoint<I: Stream + Location>(
    input: &I,
    start: &<I as Stream>::Checkpoint,
) -> SourceSpan {
    let offset = input.offset_from(start);
    ((input.location() - offset)..input.location()).into()
}

/// The complete set of commands that can be parsed by the Ferrix parser
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum CompleteCommand {
    /// Creates a new file with a given amount of integers
    Touch {
        file: PathBuf,
        number_of_integers: u32,
    },
    /// Move a file from one location to another
    Move { from: PathBuf, to: PathBuf },
    /// Create a new directory
    /// If parents is true, create all parent directories if they don't exist
    MkDir { dir: PathBuf, parents: bool },
    /// Remove a given file from the ferrix fs
    Remove { file: PathBuf, recursive: bool },
    /// Read the content of a file and output it to stdout
    Head { file: PathBuf, start: u32, end: u32 },
    /// List directory contents with each file and dir with their size on the right size and system
    /// storage info at the bottom
    List { dir: Option<PathBuf>, all: bool },
    /// Sort a given inline integer vector file
    Sort { file: PathBuf, inverse_order: bool },
    /// Concat a given list of files into a stream and output it's content to a output file or
    /// fd
    Cat {
        files: Vec<PathBuf>,
        output_file: Option<PathBuf>,
    },
}

pub fn try_parse<'a, P, T>(mut parser: P, input: &'a str) -> Result<T, FerrixError>
where
    P: Parser<Input<'a>, T, FerrixParserError>,
{
    let (_, maybe_val, errs) = parser.recoverable_parse(LocatingSlice::new(input));
    if let (Some(v), true) = (maybe_val, errs.is_empty()) {
        Ok(v)
    } else {
        Err(failure_from_errs(errs, input))
    }
}

pub fn failure_from_errs(errs: Vec<FerrixParserError>, input: &str) -> FerrixError {
    let src = Arc::new(String::from(input));
    FerrixError {
        input: src.clone(),
        diagnostics: errs
            .into_iter()
            .map(|e| FerrixDiagnostic {
                input: src.clone(),
                span: e.span.unwrap_or_else(|| (0usize..0usize).into()),
                message: e
                    .message
                    .or_else(|| e.label.clone().map(|l| format!("Expected {l}"))),
                label: e.label.map(|l| format!("not {l}")),
                help: e.help,
                severity: Severity::Error,
            })
            .collect(),
    }
}

/// A parser for the Winnow Ferrix language
/// This parser is used to parse a given input string into a list of commands
/// that can be executed by the Ferrix file system
/// The parser is based on the [Winnow](https://docs.rs/winnow) parser combinator library
pub struct WinnowFerrixParser<'a> {
    input: &'a str,
    commands: Vec<CompleteCommand>,
}

impl<'a> WinnowFerrixParser<'a> {
    /// Create a new parser for the given input
    pub fn new(input: &'a str) -> Self {
        WinnowFerrixParser {
            input,
            commands: Vec::new(),
        }
    }

    /// Parse the input and return a list of commands
    /// If there are any errors, return a FerrixError
    pub fn get_commands(&mut self) -> MietteResult<&[CompleteCommand]> {
        match try_parse(Self::parse_commands, self.input) {
            Ok(cmds) => self.commands = cmds,
            Err(err) => return Err(err.into()),
        };

        Ok(&self.commands)
    }

    fn parse_commands(input: &mut Input<'_>) -> ParserResult<Vec<CompleteCommand>> {
        (repeat(1.., Self::parse_complete_command), multispace0)
            .map(|(cmds, _): (Vec<CompleteCommand>, _)| cmds)
            .parse_next(input)
    }

    /// Parse a complete command from the input
    /// This function will parse a complete command from the input
    /// and return a CompleteCommand enum
    ///
    /// # Grammar
    ///
    /// ```md
    /// complete_command := touch_command
    ///                 | move_command
    ///                 | mkdir_command
    ///                 | remove_command
    ///                 | head_command
    ///                 | list_command
    ///                 | sort_command
    ///                 | cat_command;
    /// ```
    fn parse_complete_command(input: &mut Input<'_>) -> ParserResult<CompleteCommand> {
        let command = delimited(
            multispace0,
            alt((
                Self::parse_touch_command,
                Self::parse_move_command,
                Self::parse_mkdir_command,
                Self::parse_remove_command,
                Self::parse_head_command,
                Self::parse_list_command,
                Self::parse_sort_command,
                Self::parse_cat_command,
            )),
            Self::newline,
        )
        .parse_next(input)?;

        Ok(command)
    }

    /// Parse a touch command from the input
    ///
    /// # Grammar
    ///
    /// ```md
    /// touch_command := "touch" path_buffer number_of_integers;
    /// number_of_integers := integer;
    /// ```
    fn parse_touch_command(input: &mut Input<'_>) -> ParserResult<CompleteCommand> {
        Self::wss.parse_next(input)?;

        "touch".parse_next(input)?;
        let path_buffer = Self::parse_path_buffer(input).map_err(|e| {
            e.add_context(
                input,
                &input.checkpoint(),
                cx().msg("Expected a path buffer for touch command"),
            )
        })?;
        let number_of_integers = Self::parse_unsigned_integer(input).map_err(|e| {
            e.add_context(
                input,
                &input.checkpoint(),
                cx().msg("Expected a number of integers for touch command"),
            )
        })?;

        Ok(CompleteCommand::Touch {
            file: path_buffer,
            number_of_integers,
        })
    }

    /// Parse a move command from the input
    ///
    /// # Grammar
    /// ```md
    /// move_command := "move" path_buffer path_buffer;
    /// ```
    fn parse_move_command(input: &mut Input<'_>) -> ParserResult<CompleteCommand> {
        Self::wss.parse_next(input)?;

        "move".parse_next(input)?;

        let from = Self::parse_path_buffer(input).map_err(|e| {
            e.add_context(
                input,
                &input.checkpoint(),
                cx().msg("Expected a 'from' path buffer"),
            )
        })?;
        let to = Self::parse_path_buffer(input).map_err(|e| {
            e.add_context(
                input,
                &input.checkpoint(),
                cx().msg("Expected a 'to' path buffer"),
            )
        })?;

        Ok(CompleteCommand::Move { from, to })
    }

    /// Parse a mkdir command from the input
    ///
    /// # Grammar
    /// ```md
    /// mkdir_command := "mkdir" path_buffer (("-p" | "--parents")? line_space*);
    /// ```
    fn parse_mkdir_command(input: &mut Input<'_>) -> ParserResult<CompleteCommand> {
        Self::wss.parse_next(input)?;

        "mkdir".parse_next(input)?;

        let dir = Self::parse_path_buffer(input).map_err(|e| {
            e.add_context(
                input,
                &input.checkpoint(),
                cx().msg("Expected a path buffer for mkdir command"),
            )
        })?;

        let parents = alt((
            "-p".value(true),
            "--parents".value(true),
            empty.value(false),
        ))
        .parse_next(input)?;

        repeat(0.., Self::line_space)
            .map(|_: ()| ())
            .take()
            .parse_next(input)?;

        Ok(CompleteCommand::MkDir { dir, parents })
    }

    /// Parse a remove command from the input
    /// # Grammar
    /// ```md
    /// remove_command := "remove" path_buffer ("-r" | "--recursive")? line_space*;
    /// ```
    fn parse_remove_command(input: &mut Input<'_>) -> ParserResult<CompleteCommand> {
        Self::wss.parse_next(input)?;

        "remove".parse_next(input)?;

        let file = Self::parse_path_buffer(input).map_err(|e| {
            e.add_context(
                input,
                &input.checkpoint(),
                cx().msg("Expected a path buffer for remove command"),
            )
        })?;

        let recursive = opt(alt(("-r".value(true), "--recursive".value(true))))
            .map(|opt| opt.unwrap_or(false))
            .parse_next(input)?;

        repeat(0.., Self::line_space)
            .map(|_: ()| ())
            .take()
            .parse_next(input)?;

        Ok(CompleteCommand::Remove { file, recursive })
    }

    /// Parse a head command from the input
    /// # Grammar
    /// ```md
    /// head_command := "head" path_buffer integer integer line_space*;
    /// ```
    fn parse_head_command(input: &mut Input<'_>) -> ParserResult<CompleteCommand> {
        Self::wss.parse_next(input)?;

        "head".parse_next(input)?;

        let file = Self::parse_path_buffer(input).map_err(|e| {
            e.add_context(
                input,
                &input.checkpoint(),
                cx().msg("Expected a path buffer for head command"),
            )
        })?;

        let start = Self::parse_unsigned_integer(input).map_err(|e| {
            e.add_context(
                input,
                &input.checkpoint(),
                cx().msg("Expected a start integer for head command"),
            )
        })?;

        let end = Self::parse_unsigned_integer(input).map_err(|e| {
            e.add_context(
                input,
                &input.checkpoint(),
                cx().msg("Expected an end integer for head command"),
            )
        })?;

        repeat(0.., Self::line_space)
            .map(|_: ()| ())
            .take()
            .parse_next(input)?;

        Ok(CompleteCommand::Head { file, start, end })
    }

    /// Parse a list command from the input
    ///
    /// # Grammar
    /// ```md
    /// list_command := "ls" ws* (path_buffer | "-a" | "--all")? line_space*;
    /// ```
    fn parse_list_command(input: &mut Input<'_>) -> ParserResult<CompleteCommand> {
        Self::wss.parse_next(input)?;

        ("ls", Self::wss).parse_next(input)?;

        let dir = opt(Self::parse_path_buffer).parse_next(input)?;

        let all = opt(alt(("-a".value(true), "--all".value(true))))
            .map(|opt| opt.unwrap_or(false))
            .parse_next(input)?;

        repeat(0.., Self::line_space)
            .map(|_: ()| ())
            .take()
            .parse_next(input)?;

        Ok(CompleteCommand::List { dir, all })
    }

    /// Parse a sort command from the input
    ///
    /// # Grammar
    /// ```md
    /// sort_command := "sort" path_buffer ("-r" | "--reverse")? line_space*;
    /// ```
    fn parse_sort_command(input: &mut Input<'_>) -> ParserResult<CompleteCommand> {
        Self::wss.parse_next(input)?;

        "sort".parse_next(input)?;

        let file = Self::parse_path_buffer(input).map_err(|e| {
            e.add_context(
                input,
                &input.checkpoint(),
                cx().msg("Expected a path buffer for sort command"),
            )
        })?;

        let inverse_order = opt(alt(("-r".value(true), "--reverse".value(true))))
            .map(|opt| opt.unwrap_or(false))
            .parse_next(input)?;

        repeat(0.., Self::line_space)
            .map(|_: ()| ())
            .take()
            .parse_next(input)?;

        Ok(CompleteCommand::Sort {
            file,
            inverse_order,
        })
    }

    /// Parse a cat command from the input
    ///
    /// # Grammar
    /// ```md
    /// cat_command := "cat" path_buffer path_buffer+ ( ">" path_buffer)? line_space*;
    /// ```
    fn parse_cat_command(input: &mut Input<'_>) -> ParserResult<CompleteCommand> {
        Self::wss.parse_next(input)?;

        "cat".parse_next(input)?;

        let files: Vec<PathBuf> = repeat(1.., Self::parse_path_buffer)
            .fold(Vec::new, |mut acc, item| {
                acc.push(item);
                acc
            })
            .parse_next(input)?;

        let output_file = opt(delimited(
            ">",
            Self::parse_path_buffer,
            repeat(0.., Self::line_space).map(|_: ()| ()).take(),
        ))
        .parse_next(input)?;

        Ok(CompleteCommand::Cat { files, output_file })
    }

    /// Parse a path buffer from the input
    ///
    /// # Grammar
    ///
    /// ```md
    /// path_buffer := wsp? string line_space;
    /// ```
    fn parse_path_buffer(input: &mut Input<'_>) -> ParserResult<PathBuf> {
        delimited(
            repeat(0.., Self::wsp).map(|_: ()| ()).take(),
            take_while(1.., |c: char| {
                c.is_ascii_alphanumeric() || c == MAIN_SEPARATOR || c == '.'
            }),
            repeat(0.., Self::line_space).map(|_: ()| ()).take(),
        )
        .map(|s: &str| PathBuf::from(s))
        .parse_next(input)
        .map_err(|e| {
            e.add_context(
                input,
                &input.checkpoint(),
                cx().msg("Expected a path buffer"),
            )
        })
    }

    /// Parse an unsigned integer from the input
    ///
    /// # Grammar
    ///
    /// ```md
    /// integer := line_space* digit1 (node_space | line_space)*;
    /// ```
    fn parse_unsigned_integer(input: &mut Input<'_>) -> ParserResult<u32> {
        delimited(
            repeat(0.., Self::wsp).map(|_: ()| ()).take(),
            trace(
                "parse_unsigned_integer",
                (
                    digit1,
                    repeat(
                        0..,
                        alt(("_", take_while(1.., AsChar::is_dec_digit).take())),
                    ),
                )
                    .try_map(|(l, r): (&str, Vec<&str>)| {
                        u32::from_str_radix(
                            &format!("{l}{}", str::replace(&r.join(""), "_", "")),
                            10,
                        )
                    }),
            ),
            repeat(0.., Self::line_space).map(|_: ()| ()).take(),
        )
        .parse_next(input)
        .map_err(|e| {
            e.add_context(
                input,
                &input.checkpoint(),
                cx().msg("Expected an unsigned integer"),
            )
        })
    }

    /// Parse a line space from the input
    ///
    /// # Grammar
    ///
    /// ```md
    /// line_space := node_space | single_line_comment;
    /// ```
    fn line_space(input: &mut Input<'_>) -> ParserResult<()> {
        alt((Self::wsp, Self::single_line_comment)).parse_next(input)
    }

    /// Parse a single line comment from the input
    /// Single line comments start with a `#` character and end with a newline
    ///
    /// # Grammar
    ///
    /// ```md
    /// single_line_comment := "#" ^newline* (newline | eof);
    /// ```
    fn single_line_comment(input: &mut Input<'_>) -> ParserResult<()> {
        "#".parse_next(input)?;
        repeat_till(
            0..,
            (not(alt((Self::newline, eof.void()))), any),
            alt((Self::newline, eof.void())),
        )
        .map(|(_, _): ((), _)| ())
        .parse_next(input)
    }

    /// Parse a newline character from the input
    fn newline(input: &mut Input<'_>) -> ParserResult<()> {
        alt(NEWLINES)
            .void()
            .context(cx().lbl("newline"))
            .parse_next(input)
    }

    /// Parse a whitespace character from the input
    fn ws(input: &mut Input<'_>) -> ParserResult<()> {
        one_of(UNICODE_SPACES).void().parse_next(input)
    }

    /// Parse zero or more whitespace characters from the input
    fn wss(input: &mut Input<'_>) -> ParserResult<()> {
        repeat(0.., Self::ws).parse_next(input)
    }

    /// Parse one or more whitespace characters from the input
    fn wsp(input: &mut Input<'_>) -> ParserResult<()> {
        repeat(1.., Self::ws).parse_next(input)
    }
}

trait SpaceAround<I, O, E>: Parser<I, O, E> + Sized
where
    I: StreamIsPartial + Stream,
    E: ParserError<I>,
    I::Token: AsChar + Clone,
{
    fn space_around(self) -> impl Parser<I, O, E> {
        delimited(
            multispace0,
            trace("spaced around parser", self),
            multispace0,
        )
    }
}

// Implement for all parsers
impl<I, O, E, T> SpaceAround<I, O, E> for T
where
    I: StreamIsPartial + Stream,
    E: ParserError<I>,
    I::Token: AsChar + Clone,
    T: Parser<I, O, E>,
{
}

static UNICODE_SPACES: [char; 18] = [
    '\u{0009}', '\u{0020}', '\u{00A0}', '\u{1680}', '\u{2000}', '\u{2001}', '\u{2002}', '\u{2003}',
    '\u{2004}', '\u{2005}', '\u{2006}', '\u{2007}', '\u{2008}', '\u{2009}', '\u{200A}', '\u{202F}',
    '\u{205F}', '\u{3000}',
];

static NEWLINES: [&str; 8] = [
    "\u{000D}\u{000A}",
    "\u{000D}",
    "\u{000A}",
    "\u{0085}",
    "\u{000B}",
    "\u{000C}",
    "\u{2028}",
    "\u{2029}",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_unsigned_integer() {
        // Arrange
        let inputs = [
            "123",
            "1_000",
            "1_000_000",
            "1_000_000_000",
            "1_000_000_000_",
            "   123",
            "123   ",
            "   123   ",
            "123   # comment",
        ];
        let outputs = [
            123,
            1000,
            1_000_000,
            1_000_000_000,
            1_000_000_000,
            123,
            123,
            123,
            123,
        ];

        // Arrange
        for (input, output) in inputs.iter().zip(outputs.iter()) {
            let result = try_parse(WinnowFerrixParser::parse_unsigned_integer, input);

            // Assert
            assert_eq!(result.unwrap(), *output);
        }
    }

    #[test]
    fn test_parse_path_buffer() {
        // Arrange
        let mut inputs = vec![];

        let mut outputs = vec![];

        #[cfg(target_os = "linux")]
        {
            inputs.extend(vec![
                "test.txt",
                "test.txt   ",
                "   test.txt",
                "   test.txt   ",
                "./test.txt",
                "/test.txt",
                "/tmp/test.txt",
            ]);
            outputs.extend(vec![
                PathBuf::from("test.txt"),
                PathBuf::from("test.txt"),
                PathBuf::from("test.txt"),
                PathBuf::from("test.txt"),
                PathBuf::from("./test.txt"),
                PathBuf::from("/test.txt"),
                PathBuf::from("/tmp/test.txt"),
            ]);
        }

        #[cfg(target_os = "windows")]
        {
            inputs.extend(vec![
                "C:\\test.txt",
                "C:\\Windows\\test.txt",
                "..\\test.txt",
                ".\\test.txt",
                "D:\\Program Files\\test.txt",
            ]);
            outputs.extend(vec![
                PathBuf::from("C:\\test.txt"),
                PathBuf::from("C:\\Windows\\test.txt"),
                PathBuf::from("..\\test.txt"),
                PathBuf::from(".\\test.txt"),
                PathBuf::from("D:\\Program Files\\test.txt"),
            ]);
        }

        // Arrange
        for (input, output) in inputs.iter().zip(outputs.iter()) {
            let result = try_parse(WinnowFerrixParser::parse_path_buffer, input);

            // Assert
            assert_eq!(result.unwrap(), *output);
        }
    }

    #[test]
    fn test_touch_command() {
        // Arrange
        let inputs = [
            "touch test.txt 100",
            "touch test.txt 100   ",
            "   touch test.txt 100",
            "   touch test.txt 100   ",
        ];

        let outputs = [
            CompleteCommand::Touch {
                file: PathBuf::from("test.txt"),
                number_of_integers: 100,
            },
            CompleteCommand::Touch {
                file: PathBuf::from("test.txt"),
                number_of_integers: 100,
            },
            CompleteCommand::Touch {
                file: PathBuf::from("test.txt"),
                number_of_integers: 100,
            },
            CompleteCommand::Touch {
                file: PathBuf::from("test.txt"),
                number_of_integers: 100,
            },
        ];

        // Arrange
        for (input, output) in inputs.iter().zip(outputs.iter()) {
            let result = try_parse(WinnowFerrixParser::parse_touch_command, input);

            // Assert
            assert_eq!(result.unwrap(), *output);
        }
    }

    #[test]
    fn test_move_command() {
        // Arrange
        let inputs = [
            "move test.txt test2.txt",
            "move test.txt test2.txt   ",
            "   move test.txt test2.txt",
            "   move test.txt test2.txt   ",
        ];

        let outputs = [
            CompleteCommand::Move {
                from: PathBuf::from("test.txt"),
                to: PathBuf::from("test2.txt"),
            },
            CompleteCommand::Move {
                from: PathBuf::from("test.txt"),
                to: PathBuf::from("test2.txt"),
            },
            CompleteCommand::Move {
                from: PathBuf::from("test.txt"),
                to: PathBuf::from("test2.txt"),
            },
            CompleteCommand::Move {
                from: PathBuf::from("test.txt"),
                to: PathBuf::from("test2.txt"),
            },
        ];

        // Arrange
        for (input, output) in inputs.iter().zip(outputs.iter()) {
            let result = try_parse(WinnowFerrixParser::parse_move_command, input);

            // Assert
            assert_eq!(result.unwrap(), *output);
        }
    }

    #[test]
    fn test_mkdir_command() {
        // Arrange
        let inputs = [
            "mkdir test",
            "mkdir test   ",
            "   mkdir test",
            "   mkdir test   ",
            "mkdir test -p",
            "mkdir test -p   ",
            "   mkdir test -p",
            "   mkdir test -p   ",
            "mkdir test --parents",
            "mkdir test --parents   ",
            "   mkdir test --parents",
            "   mkdir test --parents   ",
        ];

        let outputs = [
            CompleteCommand::MkDir {
                dir: PathBuf::from("test"),
                parents: false,
            },
            CompleteCommand::MkDir {
                dir: PathBuf::from("test"),
                parents: false,
            },
            CompleteCommand::MkDir {
                dir: PathBuf::from("test"),
                parents: false,
            },
            CompleteCommand::MkDir {
                dir: PathBuf::from("test"),
                parents: false,
            },
            CompleteCommand::MkDir {
                dir: PathBuf::from("test"),
                parents: true,
            },
            CompleteCommand::MkDir {
                dir: PathBuf::from("test"),
                parents: true,
            },
            CompleteCommand::MkDir {
                dir: PathBuf::from("test"),
                parents: true,
            },
            CompleteCommand::MkDir {
                dir: PathBuf::from("test"),
                parents: true,
            },
            CompleteCommand::MkDir {
                dir: PathBuf::from("test"),
                parents: true,
            },
            CompleteCommand::MkDir {
                dir: PathBuf::from("test"),
                parents: true,
            },
            CompleteCommand::MkDir {
                dir: PathBuf::from("test"),
                parents: true,
            },
            CompleteCommand::MkDir {
                dir: PathBuf::from("test"),
                parents: true,
            },
        ];

        // Arrange
        for (input, output) in inputs.iter().zip(outputs.iter()) {
            let result = try_parse(WinnowFerrixParser::parse_mkdir_command, input);

            // Assert
            assert_eq!(result.unwrap(), *output);
        }
    }

    #[test]
    fn test_remove_command() {
        // Arrange
        let inputs = [
            "remove test.txt",
            "remove test.txt   ",
            "   remove test.txt",
            "   remove test.txt   ",
            "remove test.txt -r",
            "remove test.txt -r   ",
            "   remove test.txt -r",
            "   remove test.txt -r   ",
            "remove test.txt --recursive",
            "remove test.txt --recursive   ",
            "   remove test.txt --recursive",
            "   remove test.txt --recursive   ",
        ];

        let outputs = [
            CompleteCommand::Remove {
                file: PathBuf::from("test.txt"),
                recursive: false,
            },
            CompleteCommand::Remove {
                file: PathBuf::from("test.txt"),
                recursive: false,
            },
            CompleteCommand::Remove {
                file: PathBuf::from("test.txt"),
                recursive: false,
            },
            CompleteCommand::Remove {
                file: PathBuf::from("test.txt"),
                recursive: false,
            },
            CompleteCommand::Remove {
                file: PathBuf::from("test.txt"),
                recursive: true,
            },
            CompleteCommand::Remove {
                file: PathBuf::from("test.txt"),
                recursive: true,
            },
            CompleteCommand::Remove {
                file: PathBuf::from("test.txt"),
                recursive: true,
            },
            CompleteCommand::Remove {
                file: PathBuf::from("test.txt"),
                recursive: true,
            },
            CompleteCommand::Remove {
                file: PathBuf::from("test.txt"),
                recursive: true,
            },
            CompleteCommand::Remove {
                file: PathBuf::from("test.txt"),
                recursive: true,
            },
            CompleteCommand::Remove {
                file: PathBuf::from("test.txt"),
                recursive: true,
            },
            CompleteCommand::Remove {
                file: PathBuf::from("test.txt"),
                recursive: true,
            },
        ];

        // Arrange
        for (input, output) in inputs.iter().zip(outputs.iter()) {
            let result = try_parse(WinnowFerrixParser::parse_remove_command, input);

            // Assert
            assert_eq!(result.unwrap(), *output);
        }
    }

    #[test]
    fn test_head_command() {
        // Arrange
        let inputs = [
            "head test.txt 0 100",
            "   head test.txt 0 100",
            "head test.txt 0 100   ",
            "   head test.txt 0 100   ",
        ];

        let outputs = [
            CompleteCommand::Head {
                file: PathBuf::from("test.txt"),
                start: 0,
                end: 100,
            },
            CompleteCommand::Head {
                file: PathBuf::from("test.txt"),
                start: 0,
                end: 100,
            },
            CompleteCommand::Head {
                file: PathBuf::from("test.txt"),
                start: 0,
                end: 100,
            },
            CompleteCommand::Head {
                file: PathBuf::from("test.txt"),
                start: 0,
                end: 100,
            },
        ];

        // Arrange
        for (input, output) in inputs.iter().zip(outputs.iter()) {
            let result = try_parse(WinnowFerrixParser::parse_head_command, input);

            // Assert
            assert_eq!(result.unwrap(), *output);
        }
    }

    #[test]
    fn test_list_command() {
        // Arrange
        let inputs = [
            "ls",
            "    ls",
            "    ls  ",
            "ls test",
            "ls -a",
            "ls --all",
            "ls test -a",
            "ls test --all",
        ];

        let outputs = [
            CompleteCommand::List {
                dir: None,
                all: false,
            },
            CompleteCommand::List {
                dir: None,
                all: false,
            },
            CompleteCommand::List {
                dir: None,
                all: false,
            },
            CompleteCommand::List {
                dir: Some(PathBuf::from("test")),
                all: false,
            },
            CompleteCommand::List {
                dir: None,
                all: true,
            },
            CompleteCommand::List {
                dir: None,
                all: true,
            },
            CompleteCommand::List {
                dir: Some(PathBuf::from("test")),
                all: true,
            },
            CompleteCommand::List {
                dir: Some(PathBuf::from("test")),
                all: true,
            },
        ];

        // Arrange
        for (input, output) in inputs.iter().zip(outputs.iter()) {
            let result = try_parse(WinnowFerrixParser::parse_list_command, input);

            // Assert
            assert_eq!(result.unwrap(), *output);
        }
    }

    #[test]
    fn test_sort_command() {
        // Arrange
        let inputs = [
            "sort      test.txt",
            "   sort     test.txt",
            "sort    test.txt   ",
            "   sort     test.txt   ",
            "sort test.txt -r",
            "sort test.txt      --reverse",
        ];
        let outputs = [
            CompleteCommand::Sort {
                file: PathBuf::from("test.txt"),
                inverse_order: false,
            },
            CompleteCommand::Sort {
                file: PathBuf::from("test.txt"),
                inverse_order: false,
            },
            CompleteCommand::Sort {
                file: PathBuf::from("test.txt"),
                inverse_order: false,
            },
            CompleteCommand::Sort {
                file: PathBuf::from("test.txt"),
                inverse_order: false,
            },
            CompleteCommand::Sort {
                file: PathBuf::from("test.txt"),
                inverse_order: true,
            },
            CompleteCommand::Sort {
                file: PathBuf::from("test.txt"),
                inverse_order: true,
            },
        ];

        // Arrange
        for (input, output) in inputs.iter().zip(outputs.iter()) {
            let result = try_parse(WinnowFerrixParser::parse_sort_command, input);

            // Assert
            assert_eq!(result.unwrap(), *output);
        }
    }

    #[test]
    fn test_cat_command() {
        // Arrange
        let inputs = [
            "cat test.txt test2.txt",
            "cat test.txt test2.txt   ",
            "   cat test.txt test2.txt",
            "   cat test.txt test2.txt   ",
            "cat test.txt test2.txt > output.txt",
            "cat test.txt test2.txt > output.txt   ",
            "   cat test.txt test2.txt > output.txt",
            "   cat test.txt test2.txt > output.txt   ",
        ];

        let outputs = [
            CompleteCommand::Cat {
                files: vec![PathBuf::from("test.txt"), PathBuf::from("test2.txt")],
                output_file: None,
            },
            CompleteCommand::Cat {
                files: vec![PathBuf::from("test.txt"), PathBuf::from("test2.txt")],
                output_file: None,
            },
            CompleteCommand::Cat {
                files: vec![PathBuf::from("test.txt"), PathBuf::from("test2.txt")],
                output_file: None,
            },
            CompleteCommand::Cat {
                files: vec![PathBuf::from("test.txt"), PathBuf::from("test2.txt")],
                output_file: None,
            },
            CompleteCommand::Cat {
                files: vec![PathBuf::from("test.txt"), PathBuf::from("test2.txt")],
                output_file: Some(PathBuf::from("output.txt")),
            },
            CompleteCommand::Cat {
                files: vec![PathBuf::from("test.txt"), PathBuf::from("test2.txt")],
                output_file: Some(PathBuf::from("output.txt")),
            },
            CompleteCommand::Cat {
                files: vec![PathBuf::from("test.txt"), PathBuf::from("test2.txt")],
                output_file: Some(PathBuf::from("output.txt")),
            },
            CompleteCommand::Cat {
                files: vec![PathBuf::from("test.txt"), PathBuf::from("test2.txt")],
                output_file: Some(PathBuf::from("output.txt")),
            },
        ];

        // Arrange
        for (input, output) in inputs.iter().zip(outputs.iter()) {
            let result = try_parse(WinnowFerrixParser::parse_cat_command, input);

            // Assert
            assert_eq!(result.unwrap(), *output);
        }
    }

    #[test]
    fn test_parse_all_commands() {
        // Arrange
        let input = r#"
            touch test.txt 100
            move test.txt test2.txt
            mkdir test
            remove test.txt
            head test.txt 0 100
            ls
            sort test.txt
            cat test.txt test2.txt > output.txt
        "#;

        let outputs = [
            CompleteCommand::Touch {
                file: PathBuf::from("test.txt"),
                number_of_integers: 100,
            },
            CompleteCommand::Move {
                from: PathBuf::from("test.txt"),
                to: PathBuf::from("test2.txt"),
            },
            CompleteCommand::MkDir {
                dir: PathBuf::from("test"),
                parents: false,
            },
            CompleteCommand::Remove {
                file: PathBuf::from("test.txt"),
                recursive: false,
            },
            CompleteCommand::Head {
                file: PathBuf::from("test.txt"),
                start: 0,
                end: 100,
            },
            CompleteCommand::List {
                dir: None,
                all: false,
            },
            CompleteCommand::Sort {
                file: PathBuf::from("test.txt"),
                inverse_order: false,
            },
            CompleteCommand::Cat {
                files: vec![PathBuf::from("test.txt"), PathBuf::from("test2.txt")],
                output_file: Some(PathBuf::from("output.txt")),
            },
        ];

        // Arrange
        let mut parser = WinnowFerrixParser::new(input);
        let result = parser.get_commands().unwrap();

        // Assert
        assert_eq!(result, outputs);
    }

    #[test]
    fn test_single_line_comment() {
        // Arrange
        let inputs = ["# this is a comment # asdfa sdf", "# this is a comment\n"];

        // Arrange
        for input in inputs.iter() {
            let result = try_parse(WinnowFerrixParser::single_line_comment, input);

            // Assert
            assert!(result.is_ok());
        }
    }
}
